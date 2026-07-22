/// SessionManager —— 管理数据库会话，内置连接池、自动重试、后台 keepalive。
///
/// 每次数据库操作都通过 acquire → 复用缓存连接 → ping 健康检查 流程。
/// 所有 DatabaseDriver 方法都通过池化 handle 执行，不再自行建连。
/// 遇到 transient 错误（连接断开、超时等）自动 exponential backoff 重试（最多 3 次）。
/// 后台每 60s 对所有缓存连接做 ping，清理死连接。
use connection_pool::ConnectionPool;
use core_domain::{
    AppError, AppResult, ConnectionProfile, ConnectionState, DatabaseKind, ExplorerNode,
    QueryExecution, QueryResult, RetryConfig, TableChangeSet, TableDefinition, TableRef,
};
use driver_api::DatabaseDriver;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// 内联重试+连接池复用循环。
///
/// $call 是一个表达式，其中 `$h` 绑定到 `&mut ConnectionHandle`，`$d` 绑定到 `&dyn DatabaseDriver`。
macro_rules! with_pool {
    ($self:expr, $profile:expr, $password:expr, $db:expr, $h:ident, $d:ident => $call:expr) => {{
        let mut last_err: Option<AppError> = None;
        #[allow(unused_assignments)]
        let mut result = Err(AppError::connection("retry exhausted"));
        for attempt in 0..=$self.retry.max_retries {
            if attempt > 0 {
                let backoff = $self.backoff_ms(attempt);
                tracing::info!(connection_id = %$profile.id, attempt, backoff_ms = backoff, "重试数据库操作");
                sleep(Duration::from_millis(backoff)).await;
                $self.set_reconnecting(&$profile.id, last_err.as_ref());
            }
            let handle = match $self.pool.acquire($profile, $password, $db).await {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(connection_id = %$profile.id, error = %e, "获取连接失败");
                    last_err = Some(e); continue;
                }
            };
            let mut guard = handle.lock().await;
            let $h: &mut driver_api::ConnectionHandle = &mut *guard;
            let $d: &dyn DatabaseDriver = match $profile.kind {
                DatabaseKind::Postgres => &$self.pool.postgres,
                DatabaseKind::MySql => &$self.pool.mysql,
                DatabaseKind::MongoDb => &$self.pool.mongodb,
            };
            match $call.await {
                Ok(val) => {
                    drop(guard);
                    $self.set_connected(&$profile.id);
                    result = Ok(val);
                    break;
                }
                Err(e) if $self.is_retryable(&e) => {
                    drop(guard);
                    tracing::warn!(connection_id = %$profile.id, error = %e, "操作失败（可重试），驱逐连接");
                    $self.pool.evict(&$profile.id);
                    last_err = Some(e);
                }
                Err(e) => {
                    drop(guard);
                    tracing::error!(connection_id = %$profile.id, error = %e, "操作失败（不可重试）");
                    $self.set_failed(&$profile.id, &e);
                    result = Err(e);
                    break;
                }
            }
        }
        if result.is_err() {
            let err = last_err.unwrap_or_else(|| AppError::connection("retry exhausted"));
            tracing::error!(connection_id = %$profile.id, error = %err, "重试耗尽，操作最终失败");
            $self.set_failed(&$profile.id, &err);
        }
        result
    }};
}


#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub state: ConnectionState,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct SessionManager {
    pool: Arc<ConnectionPool>,
    statuses: Arc<RwLock<HashMap<String, SessionStatus>>>,
    disconnected_by_user: Arc<RwLock<HashSet<String>>>,
    retry: RetryConfig,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::with_retry_config(RetryConfig::default())
    }
}

impl SessionManager {
    pub fn with_retry_config(retry: RetryConfig) -> Self {
        let pool = Arc::new(ConnectionPool::new(retry.keepalive_interval_secs));
        Self {
            pool,
            statuses: Arc::new(RwLock::new(HashMap::new())),
            disconnected_by_user: Arc::new(RwLock::new(HashSet::new())),
            retry,
        }
    }

    pub fn start_keepalive(&self) {
        self.pool.start_keepalive();
    }

    /// 预热连接池：后台建立一个额外连接，供并发操作复用，避免首次打开表时两个查询争抢同一个连接。
    pub async fn prewarm_connection(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: Option<&str>,
    ) {
        if let Err(e) = self.pool.prewarm(profile, password, database).await {
            tracing::warn!(connection_id = %profile.id, error = %e, "连接池预热失败（不影响正常使用）");
        }
    }

    fn driver(&self, kind: DatabaseKind) -> &dyn DatabaseDriver {
        match kind {
            DatabaseKind::MySql => &self.pool.mysql,
            DatabaseKind::Postgres => &self.pool.postgres,
            DatabaseKind::MongoDb => &self.pool.mongodb,
        }
    }

    fn backoff_ms(&self, attempt: u32) -> u64 {
        self.retry
            .base_delay_ms
            .saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1)))
            .min(self.retry.max_delay_ms)
    }

    fn is_retryable(&self, error: &AppError) -> bool {
        error.is_transient()
    }

    // ── 公共 API ──

    pub async fn test_connection(
        &self,
        profile: &ConnectionProfile,
        password: &str,
    ) -> AppResult<()> {
        let result = self
            .driver(profile.kind)
            .test_connection(profile, password)
            .await;
        self.store_status(profile.id.clone(), &result);
        result
    }

    pub async fn load_connection_tree(
        &self,
        profile: &ConnectionProfile,
        password: &str,
    ) -> AppResult<Vec<ExplorerNode>> {
        let db = profile.default_database.as_deref();
        with_pool!(self, profile, password, db, h, d => d.list_roots(h, &profile.id))
    }

    pub async fn load_node_children(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        node: &ExplorerNode,
    ) -> AppResult<Vec<ExplorerNode>> {
        let db = node.database.as_deref().or(profile.default_database.as_deref());
        with_pool!(self, profile, password, db, h, d => d.list_children(h, &profile.id, node))
    }

    pub async fn load_table_definition(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        table: &TableRef,
    ) -> AppResult<TableDefinition> {
        let db = table.database.as_deref().or(profile.default_database.as_deref());
        with_pool!(self, profile, password, db, h, d => d.load_table_definition(h, table))
    }

    pub async fn load_routine_definition(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        routine: &core_domain::RoutineRef,
    ) -> AppResult<core_domain::RoutineDefinition> {
        let db = routine.database.as_deref().or(profile.default_database.as_deref());
        with_pool!(self, profile, password, db, h, d => d.load_routine_definition(h, routine))
    }

    pub async fn preview_table(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        table: &TableRef,
        limit: u32,
    ) -> AppResult<QueryResult> {
        let db = table.database.as_deref().or(profile.default_database.as_deref());
        with_pool!(self, profile, password, db, h, d => d.preview_table(h, table, limit))
    }

    pub async fn execute_sql(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        execution: QueryExecution,
    ) -> AppResult<QueryResult> {
        let db_owned = execution.database.clone().or_else(|| profile.default_database.clone());
        let db: Option<&str> = db_owned.as_deref();
        let exec = execution;
        with_pool!(self, profile, password, db, h, d => d.execute_sql(h, exec.clone()))
    }

    /// 批量执行 SQL 语句，所有语句在同一个连接上按顺序执行。
    /// 用于保持 MySQL 用户变量（@var）在语句间的持久性。
    pub async fn execute_sql_batch(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        connection_id: &str,
        database: Option<&str>,
        statements: Vec<String>,
    ) -> Vec<AppResult<QueryResult>> {
        let db_owned = database.or(profile.default_database.as_deref());
        let db: Option<&str> = db_owned;
        let handle = match self.pool.acquire(profile, password, db).await {
            Ok(h) => h,
            Err(e) => {
                // 所有语句都返回同一个错误
                return statements.into_iter().map(|_| Err(e.clone_error())).collect();
            }
        };
        let mut guard = handle.lock().await;
        let h: &mut driver_api::ConnectionHandle = &mut *guard;
        let d: &dyn DatabaseDriver = match profile.kind {
            DatabaseKind::Postgres => &self.pool.postgres,
            DatabaseKind::MySql => &self.pool.mysql,
            DatabaseKind::MongoDb => &self.pool.mongodb,
        };
        let mut results = Vec::with_capacity(statements.len());
        for stmt in statements {
            let exec = QueryExecution {
                connection_id: connection_id.to_string(),
                database: db.map(|s| s.to_string()),
                sql: stmt,
            };
            match d.execute_sql(h, exec).await {
                Ok(r) => results.push(Ok(r)),
                Err(e) => {
                    results.push(Err(e));
                    break; // 出错后停止执行后续语句
                }
            }
        }
        drop(guard);
        self.set_connected(connection_id);
        results
    }

    /// 逐条执行 SQL，每条完成即通过回调返回结果（用于实时推送消息）
    pub async fn execute_sql_batch_streaming(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        connection_id: &str,
        database: Option<&str>,
        statements: Vec<String>,
        mut on_result: impl FnMut(String, AppResult<QueryResult>) -> bool,
    ) {
        let db_owned = database.or(profile.default_database.as_deref());
        let db: Option<&str> = db_owned;
        let handle = match self.pool.acquire(profile, password, db).await {
            Ok(h) => h,
            Err(e) => {
                for stmt in statements {
                    if !on_result(stmt, Err(e.clone_error())) { break; }
                }
                return;
            }
        };
        let mut guard = handle.lock().await;
        let h: &mut driver_api::ConnectionHandle = &mut *guard;
        let d: &dyn DatabaseDriver = match profile.kind {
            DatabaseKind::Postgres => &self.pool.postgres,
            DatabaseKind::MySql => &self.pool.mysql,
            DatabaseKind::MongoDb => &self.pool.mongodb,
        };
        for stmt in statements {
            let exec = QueryExecution {
                connection_id: connection_id.to_string(),
                database: db.map(|s| s.to_string()),
                sql: stmt.clone(),
            };
            let result = d.execute_sql(h, exec).await;
            let is_err = result.is_err();
            let cont = on_result(stmt, result);
            if is_err || !cont { break; }
        }
        drop(guard);
        self.set_connected(connection_id);
    }

    pub async fn apply_table_changes(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        changes: TableChangeSet,
    ) -> AppResult<QueryResult> {
        let db = changes.table.database.as_deref().or(profile.default_database.as_deref());
        with_pool!(self, profile, password, db, h, d => d.apply_table_changes(h, changes.clone()))
    }

    pub async fn dump_table_all_data(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        table: &TableRef,
    ) -> AppResult<QueryResult> {
        let db = table.database.as_deref().or(profile.default_database.as_deref());
        with_pool!(self, profile, password, db, h, d => d.dump_table_all_data(h, table))
    }

    pub async fn show_processlist(
        &self,
        profile: &ConnectionProfile,
        password: &str,
    ) -> AppResult<Vec<core_domain::ProcessInfo>> {
        let db = profile.default_database.as_deref();
        with_pool!(self, profile, password, db, h, d => d.show_processlist(h))
    }

    // ── DDL 操作 ──

    pub async fn create_database(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        name: &str,
        charset: Option<&str>,
        collation: Option<&str>,
    ) -> AppResult<()> {
        let db = profile.default_database.as_deref();
        with_pool!(self, profile, password, db, h, d => d.create_database(h, name, charset, collation))
    }

    pub async fn rename_database(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        let db = profile.default_database.as_deref();
        with_pool!(self, profile, password, db, h, d => d.rename_database(h, old_name, new_name))
    }

    pub async fn drop_database(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        name: &str,
    ) -> AppResult<()> {
        let db = profile.default_database.as_deref();
        with_pool!(self, profile, password, db, h, d => d.drop_database(h, name))
    }

    pub async fn create_schema(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
        name: &str,
    ) -> AppResult<()> {
        with_pool!(self, profile, password, Some(database), h, d => d.create_schema(h, database, name))
    }

    pub async fn rename_schema(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        with_pool!(self, profile, password, Some(database), h, d => d.rename_schema(h, database, old_name, new_name))
    }

    pub async fn drop_schema(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
        name: &str,
    ) -> AppResult<()> {
        with_pool!(self, profile, password, Some(database), h, d => d.drop_schema(h, database, name))
    }

    pub async fn rename_table(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
        schema: Option<&str>,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        with_pool!(self, profile, password, Some(database), h, d => d.rename_table(h, database, schema, old_name, new_name))
    }

    pub async fn load_tables_summary(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
        schema: Option<&str>,
    ) -> AppResult<Vec<driver_api::TableSummary>> {
        with_pool!(self, profile, password, Some(database), h, d => d.load_tables_summary(h, database, schema))
    }

    pub async fn load_routines_summary(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
        schema: Option<&str>,
    ) -> AppResult<Vec<driver_api::TableSummary>> {
        with_pool!(self, profile, password, Some(database), h, d => d.load_routines_summary(h, database, schema))
    }

    pub async fn load_schemas_summary(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
    ) -> AppResult<Vec<driver_api::SchemaSummary>> {
        with_pool!(self, profile, password, Some(database), h, d => d.load_schemas_summary(h, database))
    }

    pub async fn load_collection_stats(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: &str,
        collection: &str,
    ) -> AppResult<Option<(Option<i64>, Option<i64>, Option<i64>, Option<i64>)>> {
        with_pool!(self, profile, password, Some(database), h, d => d.load_collection_stats(h, database, collection))
    }

    // ── 连接生命周期 ──

    pub fn disconnect_connection(&self, connection_id: &str) {
        self.pool.evict(connection_id);
        self.disconnected_by_user
            .write()
            .insert(connection_id.to_string());
        self.statuses.write().insert(
            connection_id.to_string(),
            SessionStatus {
                state: ConnectionState::Disconnected,
                last_error: None,
            },
        );
    }

    pub fn disconnect_all(&self) {
        self.pool.disconnect_all();
        let ids: Vec<String> = self.statuses.read().keys().cloned().collect();
        self.disconnected_by_user.write().extend(ids);
        self.statuses.write().clear();
    }

    pub fn clear_user_disconnect(&self, connection_id: &str) {
        self.disconnected_by_user.write().remove(connection_id);
    }

    pub fn connection_status(&self, connection_id: &str) -> SessionStatus {
        self.statuses
            .read()
            .get(connection_id)
            .cloned()
            .unwrap_or(SessionStatus {
                state: ConnectionState::Disconnected,
                last_error: None,
            })
    }

    fn store_status<T>(&self, id: String, result: &AppResult<T>) {
        self.statuses.write().insert(id, session_from_result(result));
    }

    fn set_connected(&self, id: &str) {
        if self.disconnected_by_user.read().contains(id) {
            return;
        }
        self.statuses.write().insert(
            id.to_string(),
            SessionStatus {
                state: ConnectionState::Connected,
                last_error: None,
            },
        );
    }

    fn set_failed(&self, id: &str, error: &AppError) {
        self.statuses.write().insert(
            id.to_string(),
            SessionStatus {
                state: ConnectionState::Failed,
                last_error: Some(error.to_string()),
            },
        );
    }

    fn set_reconnecting(&self, id: &str, error: Option<&AppError>) {
        if self.disconnected_by_user.read().contains(id) {
            return;
        }
        self.statuses.write().insert(
            id.to_string(),
            SessionStatus {
                state: ConnectionState::Reconnecting,
                last_error: error.map(|e| e.to_string()),
            },
        );
    }
}

fn session_from_result<T>(result: &AppResult<T>) -> SessionStatus {
    match result {
        Ok(_) => SessionStatus {
            state: ConnectionState::Connected,
            last_error: None,
        },
        Err(e) => SessionStatus {
            state: ConnectionState::Failed,
            last_error: Some(e.to_string()),
        },
    }
}
