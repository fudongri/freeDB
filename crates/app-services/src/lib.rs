use anyhow::{anyhow, Result};
use base64::Engine;
use chrono::Utc;
use connection_store::ConnectionStore;
use core_domain::{
    AppError, ConnectionProfile, ConnectionProfileInput, DatabaseKind, ExplorerNode,
    ExplorerNodeType, QueryExecution, QueryResult, SavedQueryEntry, SslMode, TableChangeSet,
    TableDefinition, TableRef, UiStateValue,
};
use export_service::ExportService;
use history_store::HistoryStore;
use i18n::tr;
use metadata_cache::{CachedEntry, MetadataCache};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use secure_store::SecureStore;
use serde::Deserialize;
use session_manager::{SessionManager, SessionStatus};
use std::io::Cursor;
use std::path::Path;
use tracing::warn;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppServices {
    connection_store: ConnectionStore,
    history_store: HistoryStore,
    secure_store: SecureStore,
    metadata_cache: MetadataCache,
    export_service: ExportService,
    session_manager: SessionManager,
}

impl AppServices {
    pub fn new() -> Result<Self> {
        let connection_store = ConnectionStore::new()?;
        let shared_conn = connection_store.shared_conn();
        Ok(Self {
            metadata_cache: MetadataCache::new(shared_conn)?,
            connection_store,
            history_store: HistoryStore::new()?,
            secure_store: SecureStore::new()?,
            export_service: ExportService,
            session_manager: SessionManager::default(),
        })
    }

    pub fn list_connections(&self) -> Result<Vec<ConnectionProfile>> {
        let mut profiles = self.connection_store.list_connections()?;
        for profile in &mut profiles {
            profile.password_saved = self
                .secure_store
                .load_password(&profile.id)?
                .is_some();
        }
        Ok(profiles)
    }

    pub fn save_connection(&self, input: ConnectionProfileInput) -> Result<ConnectionProfile> {
        validate_connection_input(&input)?;
        self.ensure_unique_connection_name(&input.name, None)?;
        let password = input.password.clone();
        let profile = ConnectionProfile::from_input(Uuid::new_v4().to_string(), input);
        if profile.password_saved {
            if let Some(password) = password {
                self.secure_store.save_password(&profile.id, &password)?;
            }
        } else {
            self.secure_store.delete_password(&profile.id)?;
        }
        self.connection_store.save_connection(&profile)?;
        Ok(profile)
    }

    pub fn update_connection(
        &self,
        connection_id: &str,
        input: ConnectionProfileInput,
    ) -> Result<ConnectionProfile> {
        validate_connection_input(&input)?;
        self.ensure_unique_connection_name(&input.name, Some(connection_id))?;
        let mut profile = self
            .connection_store
            .get_connection(connection_id)?
            .ok_or_else(|| anyhow!("connection not found"))?;
        profile.name = input.name;
        profile.kind = input.kind;
        profile.group_name = input.group_name;
        profile.host = input.host;
        profile.port = input.port;
        profile.username = input.username;
        profile.default_database = input.default_database;
        profile.password_saved = input.save_password;
        profile.ssl_mode = input.ssl_mode;
        profile.direct_connection = input.direct_connection;
        profile.replica_set = input.replica_set;
        profile.connection_uri = input.connection_uri;
        profile.file_path = input.file_path;
        profile.updated_at = Utc::now();

        if profile.password_saved {
            if let Some(password) = input.password {
                self.secure_store.save_password(&connection_id, &password)?;
            }
        } else {
            self.secure_store.delete_password(&connection_id)?;
        }
        self.connection_store.update_connection(&profile)?;
        Ok(profile)
    }

    pub fn delete_connection(&self, connection_id: &str) -> Result<()> {
        self.connection_store.delete_connection(connection_id)?;
        self.secure_store.delete_password(connection_id)?;
        self.session_manager.disconnect_connection(connection_id);
        Ok(())
    }

    fn ensure_unique_connection_name(&self, name: &str, exclude_id: Option<&str>) -> Result<()> {
        let existing = self.connection_store.list_connections()?;
        let duplicate = existing.iter().any(|c| {
            c.name == name && exclude_id.map_or(true, |id| c.id != id)
        });
        if duplicate {
            return Err(anyhow!("{}", tr!("连接名称 \"{}\" 已存在", name)));
        }
        Ok(())
    }

    pub async fn test_connection(&self, input: ConnectionProfileInput) -> Result<()> {
        validate_connection_input(&input)?;
        let password = input
            .password
            .clone()
            .ok_or_else(|| anyhow!("{}", tr!("测试连接需要密码")))?;
        let mut profile = ConnectionProfile::from_input("test-connection".into(), input);
        profile.password_saved = false;
        self.session_manager
            .test_connection(&profile, &password)
            .await
            .map_err(into_anyhow)
    }

    pub async fn load_connection_tree(&self, connection_id: &str) -> Result<Vec<ExplorerNode>> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        let nodes = self
            .session_manager
            .load_connection_tree(&profile, &password)
            .await
            .map_err(into_anyhow)?;
        if let Err(error) = self.connection_store.set_last_used_at(connection_id, Utc::now()) {
            warn!(
                connection_id = connection_id,
                error = %error,
                "failed to persist connection last_used_at"
            );
        }
        Ok(nodes)
    }

    pub async fn list_databases(&self, connection_id: &str) -> Result<Vec<String>> {
        let nodes = self.load_connection_tree(connection_id).await?;
        let databases: Vec<String> = nodes
            .into_iter()
            .filter(|n| n.node_type == core_domain::ExplorerNodeType::Database)
            .map(|n| n.name)
            .collect();
        Ok(databases)
    }

    /// Recursively load all Table/View nodes for a connection (does not rely on GUI cache).
    pub async fn load_all_schema_tables(
        &self,
        connection_id: &str,
    ) -> Result<Vec<ExplorerNode>> {
        let roots = self.load_connection_tree(connection_id).await?;
        let mut result = Vec::new();
        // BFS: Database → Schema (PG) → Table/View
        let mut queue: Vec<ExplorerNode> = roots;
        while let Some(node) = queue.pop() {
            match node.node_type {
                core_domain::ExplorerNodeType::Table | core_domain::ExplorerNodeType::View => {
                    result.push(node);
                }
                _ => {
                    let children = self
                        .load_node_children(connection_id, &node)
                        .await
                        .unwrap_or_default();
                    queue.extend(children);
                }
            }
        }
        Ok(result)
    }

    pub async fn load_node_children(
        &self,
        connection_id: &str,
        node: &ExplorerNode,
    ) -> Result<Vec<ExplorerNode>> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager
            .load_node_children(&profile, &password, node)
            .await
            .map_err(into_anyhow)
    }

    pub fn load_cached_metadata(&self, connection_id: &str) -> Vec<CachedEntry> {
        self.metadata_cache
            .load_for_connection(connection_id)
            .unwrap_or_default()
    }

    pub fn save_metadata_cache(&self, connection_id: &str, entries: &[CachedEntry]) {
        if let Err(error) = self.metadata_cache.save_for_connection(connection_id, entries) {
            warn!(error = %error, "failed to save metadata cache");
        }
    }

    pub fn merge_metadata_cache(&self, connection_id: &str, entries: &[CachedEntry]) {
        if let Err(error) = self.metadata_cache.merge_for_connection(connection_id, entries) {
            warn!(error = %error, "failed to merge metadata cache");
        }
    }

    pub async fn search_objects(
        &self,
        connection_id: &str,
        keyword: &str,
    ) -> Result<Vec<ExplorerNode>> {
        let roots = self.load_connection_tree(connection_id).await?;
        let mut matches = Vec::new();
        for root in roots {
            if root.name.to_ascii_lowercase().contains(&keyword.to_ascii_lowercase()) {
                matches.push(root.clone());
            }
            let children = self.load_node_children(connection_id, &root).await.unwrap_or_default();
            for child in children {
                if child.name.to_ascii_lowercase().contains(&keyword.to_ascii_lowercase()) {
                    matches.push(child.clone());
                }
                if child.expandable {
                    let grandchildren = self
                        .load_node_children(connection_id, &child)
                        .await
                        .unwrap_or_default();
                    for grandchild in grandchildren {
                        if grandchild
                            .name
                            .to_ascii_lowercase()
                            .contains(&keyword.to_ascii_lowercase())
                        {
                            matches.push(grandchild);
                        }
                    }
                }
            }
        }
        Ok(matches)
    }

    pub async fn load_table_definition(&self, table: &TableRef) -> Result<TableDefinition> {
        let profile = self.require_connection(&table.connection_id)?;
        let password = self.require_saved_password(&table.connection_id)?;
        self.session_manager
            .load_table_definition(&profile, &password, table)
            .await
            .map_err(into_anyhow)
    }

    pub async fn load_routine_definition(&self, routine: &core_domain::RoutineRef) -> Result<core_domain::RoutineDefinition> {
        let profile = self.require_connection(&routine.connection_id)?;
        let password = self.require_saved_password(&routine.connection_id)?;
        self.session_manager
            .load_routine_definition(&profile, &password, routine)
            .await
            .map_err(into_anyhow)
    }

    pub async fn open_table_preview(&self, table: &TableRef, limit: u32) -> Result<QueryResult> {
        let profile = self.require_connection(&table.connection_id)?;
        let password = self.require_saved_password(&table.connection_id)?;
        self.session_manager
            .preview_table(&profile, &password, table, limit)
            .await
            .map_err(into_anyhow)
    }

    pub async fn execute_sql(&self, execution: QueryExecution) -> Result<QueryResult> {
        let profile = self.require_connection(&execution.connection_id)?;
        let password = self.require_saved_password(&execution.connection_id)?;
        let result = self
            .session_manager
            .execute_sql(&profile, &password, execution.clone())
            .await;
        let (elapsed_ms, success) = match &result {
            Ok(r) => (r.elapsed_ms, true),
            Err(_) => (0, false),
        };
        if let Err(error) = self.history_store.append(&execution.connection_id, &execution.sql, elapsed_ms, success) {
            warn!(
                connection_id = execution.connection_id.as_str(),
                error = %error,
                "failed to persist query history"
            );
        }
        result.map_err(into_anyhow)
    }

    /// 批量执行 SQL 语句，所有语句在同一个连接上按顺序执行
    pub async fn execute_sql_batch(
        &self,
        connection_id: &str,
        database: Option<&str>,
        statements: Vec<String>,
    ) -> Vec<Result<QueryResult>> {
        let profile = match self.require_connection(connection_id) {
            Ok(p) => p,
            Err(e) => return statements.into_iter().map(|_| Err(anyhow::anyhow!("{}", e))).collect(),
        };
        let password = match self.require_saved_password(connection_id) {
            Ok(p) => p,
            Err(e) => return statements.into_iter().map(|_| Err(anyhow::anyhow!("{}", e))).collect(),
        };
        let results = self
            .session_manager
            .execute_sql_batch(&profile, &password, connection_id, database, statements.clone())
            .await;
        // 记录历史
        for (stmt, result) in statements.iter().zip(results.iter()) {
            let (elapsed_ms, success) = match result {
                Ok(r) => (r.elapsed_ms, true),
                Err(_) => (0, false),
            };
            if let Err(error) = self.history_store.append(connection_id, stmt, elapsed_ms, success) {
                warn!(
                    connection_id = connection_id,
                    error = %error,
                    "failed to persist query history"
                );
            }
        }
        results.into_iter().map(|r| r.map_err(into_anyhow)).collect()
    }

    /// 逐条执行 SQL，每条完成即通过回调返回结果（用于实时推送消息）
    pub async fn execute_sql_batch_streaming(
        &self,
        connection_id: &str,
        database: Option<&str>,
        statements: Vec<String>,
        mut on_result: impl FnMut(String, Result<QueryResult>) -> bool,
    ) {
        let profile = match self.require_connection(connection_id) {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                for stmt in statements { if !on_result(stmt, Err(anyhow::anyhow!("{}", msg))) { break; } }
                return;
            }
        };
        let password = match self.require_saved_password(connection_id) {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                for stmt in statements { if !on_result(stmt, Err(anyhow::anyhow!("{}", msg))) { break; } }
                return;
            }
        };
        let history_store = self.history_store.clone();
        let conn_id = connection_id.to_string();
        self.session_manager
            .execute_sql_batch_streaming(&profile, &password, connection_id, database, statements, move |stmt, result| {
                let (elapsed_ms, success) = match &result {
                    Ok(r) => (r.elapsed_ms, true),
                    Err(_) => (0, false),
                };
                if let Err(error) = history_store.append(&conn_id, &stmt, elapsed_ms, success) {
                    warn!(connection_id = %conn_id, error = %error, "failed to persist query history");
                }
                on_result(stmt, result.map_err(into_anyhow))
            })
            .await;
    }

    pub async fn apply_table_changes(&self, changes: TableChangeSet) -> Result<QueryResult> {
        let profile = self.require_connection(&changes.table.connection_id)?;
        let password = self.require_saved_password(&changes.table.connection_id)?;
        self.session_manager
            .apply_table_changes(&profile, &password, changes)
            .await
            .map_err(into_anyhow)
    }

    pub async fn show_processlist(&self, connection_id: &str) -> Result<Vec<core_domain::ProcessInfo>> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager
            .show_processlist(&profile, &password)
            .await
            .map_err(into_anyhow)
    }

    pub fn list_query_history(&self, connection_id: &str, limit: usize) -> Result<Vec<history_store::HistoryEntry>> {
        Ok(self
            .history_store
            .list_by_connection(connection_id, limit)?)
    }

    pub fn clear_query_history(&self, connection_id: &str) -> Result<usize> {
        Ok(self.history_store.clear_by_connection(connection_id)?)
    }

    pub fn save_query(
        &self,
        connection_id: &str,
        database: Option<&str>,
        title: &str,
        sql_text: &str,
    ) -> Result<SavedQueryEntry> {
        let title = title.trim();
        let sql_text = sql_text.trim();
        if sql_text.is_empty() {
            return Err(anyhow!("{}", tr!("没有可保存的语句")));
        }
        let title = if title.is_empty() {
            build_saved_query_title(sql_text)
        } else {
            title.to_string()
        };
        let record = self.history_store.save_query(connection_id, database, &title, sql_text)?;
        Ok(SavedQueryEntry {
            id: record.id,
            connection_id: record.connection_id,
            database: record.database,
            title: record.title,
            sql_text: record.sql_text,
            saved_at: record.saved_at,
            sort_order: record.sort_order,
        })
    }

    pub fn list_saved_queries(&self, connection_id: &str) -> Result<Vec<SavedQueryEntry>> {
        Ok(self
            .history_store
            .list_saved_queries(connection_id, 100)?
            .into_iter()
            .map(|record| SavedQueryEntry {
                id: record.id,
                connection_id: record.connection_id,
                database: record.database,
                title: record.title,
                sql_text: record.sql_text,
                saved_at: record.saved_at,
                sort_order: record.sort_order,
            })
            .collect())
    }

    pub fn list_all_saved_queries(&self) -> Result<Vec<SavedQueryEntry>> {
        Ok(self
            .history_store
            .list_all_saved_queries(200)?
            .into_iter()
            .map(|record| SavedQueryEntry {
                id: record.id,
                connection_id: record.connection_id,
                database: record.database,
                title: record.title,
                sql_text: record.sql_text,
                saved_at: record.saved_at,
                sort_order: record.sort_order,
            })
            .collect())
    }

    pub fn rename_saved_query(&self, id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            return Err(anyhow!("{}", tr!("查询名称不能为空")));
        }
        self.history_store.rename_saved_query(id, title)
    }

    pub fn update_saved_query(&self, id: &str, sql_text: &str, connection_id: &str, database: Option<&str>) -> Result<()> {
        let sql_text = sql_text.trim();
        if sql_text.is_empty() {
            return Err(anyhow!("{}", tr!("语句内容不能为空")));
        }
        self.history_store.update_saved_query(id, sql_text, connection_id, database)
    }

    pub fn delete_saved_query(&self, id: &str) -> Result<()> {
        self.history_store.delete_saved_query(id)
    }

    pub fn update_saved_query_sort_orders(&self, updates: &[(String, i32)]) -> Result<()> {
        self.history_store.update_saved_query_sort_orders(updates)
    }

    pub fn export_query_result_csv(
        &self,
        result: &QueryResult,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        self.export_service.export_query_result_csv(result, path)
    }

    pub fn export_query_result_xlsx(
        &self,
        result: &QueryResult,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        self.export_service.export_query_result_xlsx(result, path)
    }

    pub fn export_query_result_sql(
        &self,
        result: &QueryResult,
        table_name: &str,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        self.export_service.export_query_result_sql(result, table_name, path)
    }

    pub fn export_query_result_mongo(
        &self,
        result: &QueryResult,
        collection: &str,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        self.export_service.export_query_result_mongo(result, collection, path)
    }

    /// Dump a single table's structure (and optionally data) as SQL.
    pub async fn dump_table_sql(
        &self,
        table: &TableRef,
        include_data: bool,
        db_kind: DatabaseKind,
    ) -> Result<String> {
        let profile = self.require_connection(&table.connection_id)?;
        let password = self.require_saved_password(&table.connection_id)?;

        let table_def = self.session_manager.load_table_definition(&profile, &password, table).await.map_err(into_anyhow)?;

        let data = if include_data {
            Some(self.session_manager.dump_table_all_data(&profile, &password, table).await.map_err(into_anyhow)?)
        } else {
            None
        };

        let qualified_name = match db_kind {
            DatabaseKind::Postgres => {
                let schema = table.schema.as_deref().unwrap_or("public");
                format!("{schema}.{}", table.table)
            }
            _ => table.table.clone(),
        };

        Ok(export_service::sql_dump::dump_table_sql(
            &qualified_name,
            &table_def,
            data.as_ref(),
            db_kind,
            include_data,
        ))
    }

    /// Dump all tables in a database as SQL.
    pub async fn dump_database_sql(
        &self,
        connection_id: &str,
        database: &str,
        schema: Option<&str>,
        include_data: bool,
        db_kind: DatabaseKind,
    ) -> Result<String> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;

        // Build a parent node to list children (tables/views)
        let parent = ExplorerNode {
            id: String::new(),
            connection_id: connection_id.to_string(),
            name: database.to_string(),
            node_type: ExplorerNodeType::Database,
            parent_id: None,
            database: Some(database.to_string()),
            schema: schema.map(|s| s.to_string()),
            expandable: true,
            loaded: false,
        };

        let children = self.session_manager.load_node_children(&profile, &password, &parent).await.map_err(into_anyhow)?;

        let mut tables = Vec::new();
        for child in &children {
            if !matches!(child.node_type, ExplorerNodeType::Table) {
                continue; // skip views for now
            }
            let table_ref = TableRef {
                connection_id: connection_id.to_string(),
                database: Some(database.to_string()),
                schema: child.schema.clone().or_else(|| schema.map(|s| s.to_string())),
                table: child.name.clone(),
                is_view: false,
            };
            let table_def = self.session_manager.load_table_definition(&profile, &password, &table_ref).await.map_err(into_anyhow)?;

            let data = if include_data {
                Some(self.session_manager.dump_table_all_data(&profile, &password, &table_ref).await.map_err(into_anyhow)?)
            } else {
                None
            };

            let qualified_name = match db_kind {
                DatabaseKind::Postgres => {
                    let s = table_ref.schema.as_deref().unwrap_or("public");
                    format!("{s}.{}", table_ref.table)
                }
                _ => table_ref.table.clone(),
            };

            tables.push((qualified_name, table_def, data));
        }

        Ok(export_service::sql_dump::dump_database_sql(tables, db_kind, include_data))
    }

    pub fn disconnect_connection(&self, connection_id: &str) {
        self.session_manager.disconnect_connection(connection_id);
    }

    /// 在 tokio runtime 启动后调用
    pub fn start_keepalive(&self) {
        self.session_manager.start_keepalive();
    }

    /// 预热连接池：后台建立一个额外连接，供并发操作复用。
    pub async fn prewarm_connection(&self, connection_id: &str) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        let db = profile.default_database.clone();
        self.session_manager.prewarm_connection(&profile, &password, db.as_deref()).await;
        Ok(())
    }

    pub fn connection_status(&self, connection_id: &str) -> SessionStatus {
        self.session_manager.connection_status(connection_id)
    }

    pub fn clear_user_disconnect(&self, connection_id: &str) {
        self.session_manager.clear_user_disconnect(connection_id);
    }

    // ── DDL 操作 ──

    pub async fn create_database(&self, connection_id: &str, name: &str, charset: Option<&str>, collation: Option<&str>) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager.create_database(&profile, &password, name, charset, collation).await.map_err(into_anyhow)
    }

    pub async fn rename_database(&self, connection_id: &str, old_name: &str, new_name: &str) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager.rename_database(&profile, &password, old_name, new_name).await.map_err(into_anyhow)
    }

    pub async fn drop_database(&self, connection_id: &str, name: &str) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager.drop_database(&profile, &password, name).await.map_err(into_anyhow)
    }

    pub async fn create_schema(&self, connection_id: &str, database: &str, name: &str) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager.create_schema(&profile, &password, database, name).await.map_err(into_anyhow)
    }

    pub async fn rename_schema(&self, connection_id: &str, database: &str, old_name: &str, new_name: &str) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager.rename_schema(&profile, &password, database, old_name, new_name).await.map_err(into_anyhow)
    }

    pub async fn drop_schema(&self, connection_id: &str, database: &str, name: &str) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager.drop_schema(&profile, &password, database, name).await.map_err(into_anyhow)
    }

    pub async fn rename_table(&self, connection_id: &str, database: &str, schema: Option<&str>, old_name: &str, new_name: &str) -> Result<()> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager.rename_table(&profile, &password, database, schema, old_name, new_name).await.map_err(into_anyhow)
    }

    pub async fn load_tables_summary(&self, connection_id: &str, database: &str, schema: Option<&str>) -> Result<Vec<driver_api::TableSummary>> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager
            .load_tables_summary(&profile, &password, database, schema)
            .await
            .map_err(into_anyhow)
    }

    pub async fn load_routines_summary(&self, connection_id: &str, database: &str, schema: Option<&str>) -> Result<Vec<driver_api::TableSummary>> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager
            .load_routines_summary(&profile, &password, database, schema)
            .await
            .map_err(into_anyhow)
    }

    pub async fn load_schemas_summary(&self, connection_id: &str, database: &str) -> Result<Vec<driver_api::SchemaSummary>> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager
            .load_schemas_summary(&profile, &password, database)
            .await
            .map_err(into_anyhow)
    }

    pub async fn load_collection_stats(&self, connection_id: &str, database: &str, collection: &str) -> Result<Option<(Option<i64>, Option<i64>, Option<i64>, Option<i64>)>> {
        let profile = self.require_connection(connection_id)?;
        let password = self.require_saved_password(connection_id)?;
        self.session_manager
            .load_collection_stats(&profile, &password, database, collection)
            .await
            .map_err(into_anyhow)
    }

    pub fn save_ui_state(&self, key: &str, value: &str) -> Result<()> {
        if let Err(error) = self.connection_store.save_ui_state(UiStateValue {
            key: key.to_string(),
            value: value.to_string(),
        }) {
            warn!(key = key, error = %error, "failed to persist ui state");
        }
        Ok(())
    }

    pub fn load_ui_state(&self, key: &str) -> Result<Option<String>> {
        self.connection_store.load_ui_state(key)
    }

    pub fn update_sort_orders(&self, orders: &[(String, i64)]) -> Result<()> {
        self.connection_store.update_sort_orders(orders)
    }

    pub fn load_password(&self, connection_id: &str) -> Result<Option<String>> {
        self.secure_store.load_password(connection_id)
    }

    pub fn save_password(&self, id: &str, password: &str) -> Result<()> {
        self.secure_store.save_password(id, password)?;
        Ok(())
    }

    fn require_connection(&self, connection_id: &str) -> Result<ConnectionProfile> {
        self.connection_store
            .get_connection(connection_id)?
            .ok_or_else(|| anyhow!("connection not found"))
    }

    fn require_saved_password(&self, connection_id: &str) -> Result<String> {
        self.secure_store
            .load_password(connection_id)?
            .ok_or_else(|| anyhow!("{}", tr!("该连接未保存密码，请重新编辑连接后保存密码")))
    }

    pub fn export_config(&self, path: &Path) -> Result<()> {
        let connections = self.connection_store.list_connections()?;
        let all_queries = self.history_store.list_all_saved_queries(10_000)?;

        let mut w = Writer::new(Cursor::new(Vec::new()));
        w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        write_start(&mut w, "FreeDBConfig")?;

        // Connections（直接作为 FreeDBConfig 的子元素，不使用包装元素）
        for conn in &connections {
            let mut tag = BytesStart::new("Connection");
            tag.push_attribute(("originalId", conn.id.as_str()));
            w.write_event(Event::Start(tag))?;
            write_elem(&mut w, "name", &conn.name)?;
            write_elem(&mut w, "kind", conn.kind.as_str())?;
            if let Some(ref g) = conn.group_name {
                write_elem(&mut w, "groupName", g)?;
            }
            write_elem(&mut w, "host", &conn.host)?;
            write_elem(&mut w, "port", &conn.port.to_string())?;
            write_elem(&mut w, "username", &conn.username)?;
            if let Ok(Some(encrypted)) = self.secure_store.load_encrypted_password(&conn.id) {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&encrypted);
                write_elem(&mut w, "password", &encoded)?;
            }
            if let Some(ref db) = conn.default_database {
                write_elem(&mut w, "defaultDatabase", db)?;
            }
            write_elem(&mut w, "sslMode", conn.ssl_mode.as_str())?;
            write_elem(&mut w, "directConnection", if conn.direct_connection { "true" } else { "false" })?;
            if let Some(ref rs) = conn.replica_set {
                write_elem(&mut w, "replicaSet", rs)?;
            }
            if let Some(ref uri) = conn.connection_uri {
                write_elem(&mut w, "connectionUri", uri)?;
            }
            if let Some(ref fp) = conn.file_path {
                write_elem(&mut w, "filePath", fp)?;
            }
            write_end(&mut w, "Connection")?;
        }

        // SavedQueries（直接作为 FreeDBConfig 的子元素）
        for q in &all_queries {
            write_start(&mut w, "Query")?;
            write_elem(&mut w, "connectionId", &q.connection_id)?;
            if let Some(ref db) = q.database {
                write_elem(&mut w, "database", db)?;
            }
            write_elem(&mut w, "title", &q.title)?;
            write_elem(&mut w, "sqlText", &q.sql_text)?;
            write_end(&mut w, "Query")?;
        }

        write_end(&mut w, "FreeDBConfig")?;

        let xml = String::from_utf8(w.into_inner().into_inner())
            .map_err(|e| anyhow!("xml encoding error: {e}"))?;
        std::fs::write(path, xml)?;
        Ok(())
    }

    pub fn import_config(&self, path: &Path) -> Result<ImportResult> {
        let content = std::fs::read_to_string(path)?;
        let bundle: ConfigBundle = quick_xml::de::from_str(&content)?;

        if bundle.connections.is_empty() && bundle.saved_queries.is_empty() {
            return Err(anyhow!("{}", tr!("配置文件中没有数据")));
        }

        let mut id_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        for conn in &bundle.connections {
            let new_id = Uuid::new_v4().to_string();
            let kind = match conn.kind.as_deref() {
                Some("mysql") => DatabaseKind::MySql,
                Some("postgres") => DatabaseKind::Postgres,
                Some("mongodb") => DatabaseKind::MongoDb,
                _ => DatabaseKind::MySql,
            };
            let ssl_mode = match conn.ssl_mode.as_deref() {
                Some("disable") => SslMode::Disable,
                Some("require") => SslMode::Require,
                _ => SslMode::Prefer,
            };
            let port: u16 = conn.port.as_deref().unwrap_or("3306").parse().unwrap_or(3306);
            let profile = ConnectionProfile {
                id: new_id.clone(),
                name: conn.name.clone().unwrap_or_default(),
                kind,
                group_name: conn.group_name.clone(),
                host: conn.host.clone().unwrap_or_else(|| "127.0.0.1".into()),
                port,
                username: conn.username.clone().unwrap_or_default(),
                default_database: conn.default_database.clone(),
                password_saved: conn.password.is_some(),
                ssl_mode,
                sort_order: 0,
                last_used_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                direct_connection: conn
                    .direct_connection
                    .as_deref()
                    .map(|s| s == "true")
                    .unwrap_or(false),
                replica_set: conn.replica_set.clone(),
                connection_uri: conn.connection_uri.clone(),
                file_path: conn.file_path.clone(),
            };
            if let Some(ref pw) = conn.password {
                match base64::engine::general_purpose::STANDARD.decode(pw) {
                    Ok(encrypted) => {
                        self.secure_store.save_encrypted_password(&new_id, &encrypted)?;
                    }
                    Err(_) => {
                        // 兼容旧格式（明文密码）
                        self.secure_store.save_password(&new_id, pw)?;
                    }
                }
            }
            self.connection_store.save_connection(&profile)?;
            // 记录旧 ID → 新 ID 映射（savedQueries 引用旧 connectionId）
            // 由于导出时 connectionId 是原始 ID，需要通过匹配来映射
            // 这里用顺序索引：第 i 个连接对应原始 ID
            id_map.insert(conn.original_id.clone().unwrap_or_default(), new_id);
        }

        // 为保存的查询重新映射 connection_id
        for q in &bundle.saved_queries {
            let new_conn_id = id_map
                .get(q.connection_id.as_deref().unwrap_or(""))
                .cloned()
                .or_else(|| id_map.values().next().cloned());
            if let Some(conn_id) = new_conn_id {
                self.history_store.save_query(
                    &conn_id,
                    q.database.as_deref(),
                    q.title.as_deref().unwrap_or(""),
                    q.sql_text.as_deref().unwrap_or(""),
                )?;
            }
        }

        Ok(ImportResult {
            connections_added: bundle.connections.len(),
            queries_added: bundle.saved_queries.len(),
        })
    }
}

fn validate_connection_input(input: &ConnectionProfileInput) -> Result<()> {
    if input.name.trim().is_empty() {
        return Err(anyhow!("{}", tr!("连接名称不能为空")));
    }
    match input.kind {
        DatabaseKind::Sqlite => {
            if input.file_path.as_deref().map(str::trim).unwrap_or("").is_empty() {
                return Err(anyhow!("{}", tr!("SQLite 文件路径不能为空")));
            }
        }
        _ => {
            if input.host.trim().is_empty() {
                return Err(anyhow!("{}", tr!("主机地址不能为空")));
            }
            if input.username.trim().is_empty() {
                return Err(anyhow!("{}", tr!("用户名不能为空")));
            }
        }
    }
    Ok(())
}

fn into_anyhow(error: AppError) -> anyhow::Error {
    anyhow!(error.to_string())
}

fn build_saved_query_title(sql_text: &str) -> String {
    let first_line = sql_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let compact = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    let char_count = compact.chars().count();
    if char_count > 36 {
        format!("{}...", compact.chars().take(36).collect::<String>())
    } else if compact.is_empty() {
        tr!("未命名查询").to_string()
    } else {
        compact
    }
}

// ── 配置导入导出辅助类型 ──

pub struct ImportResult {
    pub connections_added: usize,
    pub queries_added: usize,
}

#[derive(Deserialize)]
struct ConfigBundle {
    #[serde(rename = "Connection", default)]
    connections: Vec<ImportConnection>,
    #[serde(rename = "Query", default)]
    saved_queries: Vec<ImportQuery>,
}

#[derive(Deserialize)]
#[derive(Debug)]
struct ImportConnection {
    #[serde(rename = "@originalId")]
    original_id: Option<String>,
    #[serde(rename = "name")]
    name: Option<String>,
    #[serde(rename = "kind")]
    kind: Option<String>,
    #[serde(rename = "groupName")]
    group_name: Option<String>,
    #[serde(rename = "host")]
    host: Option<String>,
    #[serde(rename = "port")]
    port: Option<String>,
    #[serde(rename = "username")]
    username: Option<String>,
    #[serde(rename = "password")]
    password: Option<String>,
    #[serde(rename = "defaultDatabase")]
    default_database: Option<String>,
    #[serde(rename = "sslMode")]
    ssl_mode: Option<String>,
    #[serde(rename = "directConnection")]
    direct_connection: Option<String>,
    #[serde(rename = "replicaSet")]
    replica_set: Option<String>,
    #[serde(rename = "connectionUri")]
    connection_uri: Option<String>,
    #[serde(rename = "filePath")]
    file_path: Option<String>,
}

#[derive(Deserialize)]
#[derive(Debug)]
struct ImportQuery {
    #[serde(rename = "connectionId")]
    connection_id: Option<String>,
    #[serde(rename = "database")]
    database: Option<String>,
    #[serde(rename = "title")]
    title: Option<String>,
    #[serde(rename = "sqlText")]
    sql_text: Option<String>,
}

// ── XML 写入辅助函数 ──

fn write_start(w: &mut Writer<Cursor<Vec<u8>>>, tag: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))?;
    Ok(())
}

fn write_end(w: &mut Writer<Cursor<Vec<u8>>>, tag: &str) -> Result<()> {
    w.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}

fn write_elem(w: &mut Writer<Cursor<Vec<u8>>>, tag: &str, value: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(tag)))?;
    w.write_event(Event::Text(BytesText::new(value)))?;
    w.write_event(Event::End(BytesEnd::new(tag)))?;
    Ok(())
}
