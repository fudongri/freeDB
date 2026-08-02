use anyhow::Context;
use async_trait::async_trait;
use core_domain::{
    AppError, AppResult, ColumnDefinition, ConnectionProfile, ExplorerNode, ExplorerNodeType,
    QueryCellValue, QueryExecution, QueryResult, SslMode, TableChangeSet, TableDefinition, TableRef,
};
use driver_api::{ConnectionHandle, ConnectionProvider, DatabaseDriver};
use i18n::tr;
use mysql_async::{prelude::Queryable, Conn, OptsBuilder, Row, SslOpts, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

#[derive(Clone, Default)]
pub struct MySqlDriver;

#[async_trait]
impl ConnectionProvider for MySqlDriver {
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        _database: Option<&str>,
    ) -> AppResult<ConnectionHandle> {
        let conn = open_conn(profile, password, None).await?;
        Ok(ConnectionHandle::MySql { conn })
    }

    async fn ping(&self, handle: &mut ConnectionHandle) -> AppResult<()> {
        match handle {
            ConnectionHandle::MySql { conn } => {
                conn.ping().await.map_err(map_mysql_error)?;
                Ok(())
            }
            _ => Err(AppError::Validation("expected mysql handle".into())),
        }
    }
}

#[async_trait]
impl DatabaseDriver for MySqlDriver {
    async fn test_connection(&self, profile: &ConnectionProfile, password: &str) -> AppResult<()> {
        let mut conn = open_conn(profile, password, profile.default_database.as_deref()).await?;
        conn.ping().await.map_err(map_mysql_error)?;
        disconnect(conn).await;
        Ok(())
    }

    async fn list_roots(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
    ) -> AppResult<Vec<ExplorerNode>> {
        let conn = mysql_conn_mut(handle)?;
        let dbs: Vec<String> = conn
            .query_map("SHOW DATABASES", |name: String| name)
            .await
            .map_err(map_mysql_error)?;
        Ok(dbs
            .into_iter()
            .map(|db| ExplorerNode {
                id: format!("mysql-db:{connection_id}:{db}"),
                connection_id: connection_id.to_string(),
                name: db.clone(),
                node_type: ExplorerNodeType::Database,
                parent_id: None,
                database: Some(db),
                schema: None,
                expandable: true,
                loaded: false,
            })
            .collect())
    }

    async fn list_children(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
        parent: &ExplorerNode,
    ) -> AppResult<Vec<ExplorerNode>> {
        if matches!(parent.node_type, ExplorerNodeType::Connection) {
            return self.list_roots(handle, connection_id).await;
        }
        let db = parent
            .database
            .as_ref()
            .ok_or_else(|| AppError::Validation("missing database".into()))?;
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("USE {}", quote_mysql(db)))
            .await
            .map_err(map_mysql_error)?;
        let sql = format!("SHOW FULL TABLES FROM {}", quote_mysql(db));
        let rows: Vec<Row> = conn.query(sql).await.map_err(map_mysql_error)?;
        let mut nodes: Vec<ExplorerNode> = rows
            .into_iter()
            .map(|row| {
                let name = row.get::<String, _>(0).unwrap_or_default();
                let kind = row
                    .get::<String, _>(1)
                    .unwrap_or_else(|| "BASE TABLE".into());
                let is_view = kind.to_ascii_uppercase().contains("VIEW");
                ExplorerNode {
                    id: format!("mysql-table:{connection_id}:{db}:{name}"),
                    connection_id: connection_id.to_string(),
                    name: name.clone(),
                    node_type: if is_view {
                        ExplorerNodeType::View
                    } else {
                        ExplorerNodeType::Table
                    },
                    parent_id: Some(parent.id.clone()),
                    database: Some(db.clone()),
                    schema: None,
                    expandable: false,
                    loaded: true,
                }
            })
            .collect();
        // 查询存储过程和函数
        let routine_sql = format!(
            "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = '{}' ORDER BY ROUTINE_NAME",
            db.replace('\'', "''")
        );
        let routine_rows: Vec<Row> = conn.query(routine_sql).await.unwrap_or_default();
        for row in routine_rows {
            let name: Option<String> = row.get(0);
            let name = name.unwrap_or_default();
            let kind: Option<String> = row.get(1);
            let kind = kind.unwrap_or_else(|| "PROCEDURE".into());
            let is_proc = kind.eq_ignore_ascii_case("PROCEDURE");
            nodes.push(ExplorerNode {
                id: format!("mysql-routine:{connection_id}:{db}:{name}:{kind}"),
                connection_id: connection_id.to_string(),
                name: name.clone(),
                node_type: if is_proc {
                    ExplorerNodeType::Procedure
                } else {
                    ExplorerNodeType::Function
                },
                parent_id: Some(parent.id.clone()),
                database: Some(db.clone()),
                schema: None,
                expandable: false,
                loaded: true,
            });
        }
        Ok(nodes)
    }

    async fn load_table_definition(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
    ) -> AppResult<TableDefinition> {
        let db = table
            .database
            .as_ref()
            .ok_or_else(|| AppError::Validation("mysql table requires database".into()))?;
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("USE {}", quote_mysql(db)))
            .await
            .map_err(map_mysql_error)?;
        // 加载表注释、引擎、字符集
        let table_info_sql = format!(
            "SELECT t.TABLE_COMMENT, t.ENGINE, c.CHARACTER_SET_NAME FROM INFORMATION_SCHEMA.TABLES t LEFT JOIN INFORMATION_SCHEMA.COLLATIONS c ON t.TABLE_COLLATION = c.COLLATION_NAME WHERE t.TABLE_SCHEMA = '{}' AND t.TABLE_NAME = '{}'",
            escape_mysql_literal(db),
            escape_mysql_literal(&table.table),
        );
        let (table_comment, engine, charset) = conn.query(table_info_sql)
            .await
            .ok()
            .and_then(|rows: Vec<Row>| rows.into_iter().next())
            .map(|row| {
                let comment: Option<String> = row.get::<Option<String>, _>(0).flatten().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                let engine: Option<String> = row.get::<Option<String>, _>(1).flatten().filter(|s| !s.is_empty());
                let charset: Option<String> = row.get::<Option<String>, _>(2).flatten().filter(|s| !s.is_empty());
                (comment, engine, charset)
            })
            .unwrap_or((None, None, None));
        let sql = format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, COLUMN_COMMENT, EXTRA FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = '{}' AND TABLE_NAME = '{}' ORDER BY ORDINAL_POSITION",
            escape_mysql_literal(db),
            escape_mysql_literal(&table.table),
        );
        let rows: Vec<Row> = conn.query(sql).await.map_err(map_mysql_error)?;
        // 表不存在时 INFORMATION_SCHEMA.COLUMNS 返回空，仍会走到这里。
        // 校验表存在性，避免把用户 SQL 里随意输入的表名缓存进智能提示。
        if rows.is_empty() {
            let exists_rows: Vec<Row> = conn
                .query(format!(
                    "SELECT 1 FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = '{}' AND TABLE_NAME = '{}' LIMIT 1",
                    escape_mysql_literal(db),
                    escape_mysql_literal(&table.table),
                ))
                .await
                .map_err(map_mysql_error)?;
            if exists_rows.is_empty() {
                return Err(AppError::NotFound(format!("table {} not found", table.table)));
            }
        }
        let columns = rows
            .into_iter()
            .map(|row| {
                let extra: String = row.get::<String, _>(6).unwrap_or_default();
                let extra_lower = extra.to_ascii_lowercase();
                ColumnDefinition {
                    name: row.get::<String, _>(0).unwrap_or_default(),
                    data_type: row.get::<String, _>(1).unwrap_or_default(),
                    nullable: row
                        .get::<String, _>(2)
                        .map(|v| v.eq_ignore_ascii_case("YES"))
                        .unwrap_or(true),
                    primary_key: row
                        .get::<String, _>(3)
                        .map(|v| v.eq_ignore_ascii_case("PRI"))
                        .unwrap_or(false),
                    unique: row
                        .get::<String, _>(3)
                        .map(|v| v.eq_ignore_ascii_case("UNI"))
                        .unwrap_or(false),
                    auto_increment: extra_lower.contains("auto_increment"),
                    on_update_current_timestamp: extra_lower.contains("on update current_timestamp"),
                    default_value: row
                        .get::<Option<String>, _>(4)
                        .flatten()
                        .filter(|v| !v.is_empty()),
                    comment: row
                        .get::<Option<String>, _>(5)
                        .flatten()
                        .filter(|v| !v.is_empty()),
                }
            })
            .collect();

        let create_sql = if table.is_view {
            let sql = format!(
                "SHOW CREATE VIEW {}.{}",
                quote_mysql(db),
                quote_mysql(&table.table)
            );
            conn.query(sql)
                .await
                .map_err(map_mysql_error)
                .ok()
                .and_then(|rows: Vec<Row>| rows.into_iter().next())
                .and_then(|row| row.get::<Option<String>, _>(1).flatten().or_else(|| row.get::<Option<String>, _>(0).flatten()))
        } else {
            let sql = format!(
                "SHOW CREATE TABLE {}.{}",
                quote_mysql(db),
                quote_mysql(&table.table)
            );
            conn.query(sql)
                .await
                .map_err(map_mysql_error)
                .ok()
                .and_then(|rows: Vec<Row>| rows.into_iter().next())
                .and_then(|row| row.get::<Option<String>, _>(1).flatten().or_else(|| row.get::<Option<String>, _>(0).flatten()))
        };
        Ok(TableDefinition { columns, create_sql, table_comment, engine, charset })
    }

    async fn load_routine_definition(
        &self,
        handle: &mut ConnectionHandle,
        routine: &core_domain::RoutineRef,
    ) -> AppResult<core_domain::RoutineDefinition> {
        let db = routine
            .database
            .as_ref()
            .ok_or_else(|| AppError::Validation("mysql routine requires database".into()))?;
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("USE {}", quote_mysql(db)))
            .await
            .map_err(map_mysql_error)?;
        let kind = if routine.is_procedure { "PROCEDURE" } else { "FUNCTION" };
        let sql = format!(
            "SHOW CREATE {} {}",
            kind,
            quote_mysql(&routine.name),
        );
        let create_sql: Option<String> = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            async {
                conn.query(sql)
                    .await
                    .ok()
                    .and_then(|rows: Vec<Row>| rows.into_iter().next())
                    .and_then(|row| row.get::<Option<String>, _>(2))
                    .flatten()
            },
        )
        .await
        .unwrap_or(None);
        Ok(core_domain::RoutineDefinition { create_sql })
    }

    async fn preview_table(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
        limit: u32,
    ) -> AppResult<QueryResult> {
        let db = table
            .database
            .as_ref()
            .ok_or_else(|| AppError::Validation("mysql table requires database".into()))?;
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("USE {}", quote_mysql(db)))
            .await
            .map_err(map_mysql_error)?;
        let sql = format!(
            "SELECT * FROM {}.{} LIMIT {}",
            quote_mysql(db),
            quote_mysql(&table.table),
            limit
        );
        query_rows(conn, &sql).await
    }

    async fn execute_sql(
        &self,
        handle: &mut ConnectionHandle,
        execution: QueryExecution,
    ) -> AppResult<QueryResult> {
        match handle {
            ConnectionHandle::MySql { conn } => exec_on_conn(conn, execution).await,
            _ => Err(AppError::Validation("expected mysql handle".into())),
        }
    }

    async fn apply_table_changes(
        &self,
        _handle: &mut ConnectionHandle,
        _changes: TableChangeSet,
    ) -> AppResult<QueryResult> {
        Err(AppError::Unsupported(
            tr!("MySQL 表格编辑将在后续迭代中补全").to_string(),
        ))
    }

    async fn create_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
        charset: Option<&str>,
        collation: Option<&str>,
    ) -> AppResult<()> {
        let conn = mysql_conn_mut(handle)?;
        let mut sql = format!("CREATE DATABASE IF NOT EXISTS {}", quote_mysql(name));
        if let Some(cs) = charset {
            sql.push_str(&format!(" CHARACTER SET {}", cs));
        }
        if let Some(col) = collation {
            sql.push_str(&format!(" COLLATE {}", col));
        }
        conn.query_drop(sql).await.map_err(map_mysql_error)?;
        Ok(())
    }

    async fn rename_database(
        &self,
        _handle: &mut ConnectionHandle,
        _old_name: &str,
        _new_name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("MySQL 不支持重命名数据库").to_string()))
    }

    async fn drop_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
    ) -> AppResult<()> {
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("DROP DATABASE IF EXISTS {}", quote_mysql(name)))
            .await
            .map_err(map_mysql_error)?;
        Ok(())
    }

    async fn create_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("MySQL 不支持 Schema").to_string()))
    }

    async fn rename_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _old_name: &str,
        _new_name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("MySQL 不支持 Schema").to_string()))
    }

    async fn drop_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("MySQL 不支持 Schema").to_string()))
    }

    async fn rename_table(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        _schema: Option<&str>,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!(
            "RENAME TABLE {}.{} TO {}.{}",
            quote_mysql(database),
            quote_mysql(old_name),
            quote_mysql(database),
            quote_mysql(new_name)
        ))
        .await
        .map_err(map_mysql_error)?;
        Ok(())
    }

    async fn dump_table_all_data(
        &self,
        handle: &mut ConnectionHandle,
        table: &core_domain::TableRef,
    ) -> AppResult<QueryResult> {
        let db = table
            .database
            .as_ref()
            .ok_or_else(|| AppError::Validation("mysql table requires database".into()))?;
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("USE {}", quote_mysql(db)))
            .await
            .map_err(map_mysql_error)?;
        let sql = format!(
            "SELECT * FROM {}.{}",
            quote_mysql(db),
            quote_mysql(&table.table)
        );
        query_rows(conn, &sql).await
    }

    async fn load_tables_summary(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        _schema: Option<&str>,
    ) -> AppResult<Vec<driver_api::TableSummary>> {
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("USE {}", quote_mysql(database)))
            .await
            .map_err(map_mysql_error)?;

        // SHOW TABLE STATUS 获取表元数据
        let sql = format!("SHOW TABLE STATUS FROM {}", quote_mysql(database));
        let rows: Vec<Row> = conn.query(sql).await.map_err(map_mysql_error)?;
        let mut summaries: Vec<driver_api::TableSummary> = rows
            .into_iter()
            .map(|row| {
                let name: String = row.get::<String, _>(0).unwrap_or_default();
                let engine: Option<String> = row.get::<Option<String>, _>(1).flatten();
                let rows_est: Option<i64> = row.get::<Option<i64>, _>(4).flatten();
                let data_len: Option<i64> = row.get::<Option<i64>, _>(6).flatten();
                let index_len: Option<i64> = row.get::<Option<i64>, _>(8).flatten();
                let collation: Option<String> = row.get::<Option<String>, _>(14).flatten();
                let comment: Option<String> = row.get::<Option<String>, _>(17).flatten();
                let create_time: Option<String> = row
                    .get::<Option<mysql_async::Value>, _>(11)
                    .flatten()
                    .and_then(|v| match v {
                        mysql_async::Value::Bytes(b) => Some(String::from_utf8_lossy(&b).to_string()),
                        mysql_async::Value::Date(y, m, d, hh, mm, ss, _) => {
                            Some(format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}"))
                        }
                        _ => None,
                    });

                let total = match (data_len, index_len) {
                    (Some(d), Some(i)) => Some(d + i),
                    (Some(d), None) => Some(d),
                    (None, Some(i)) => Some(i),
                    _ => None,
                };

                driver_api::TableSummary {
                    name,
                    table_type: "TABLE".into(),
                    row_count: rows_est,
                    total_size: total,
                    data_size: data_len,
                    index_size: index_len,
                    engine,
                    collation,
                    primary_keys: Vec::new(),
                    comment: comment.filter(|c| !c.is_empty()),
                    create_time: create_time.filter(|c| !c.is_empty()),
                }
            })
            .collect();

        // 查询主键列
        let pk_sql = format!(
            "SELECT TABLE_NAME, COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = '{}' AND COLUMN_KEY = 'PRI' ORDER BY ORDINAL_POSITION",
            escape_mysql_literal(database),
        );
        let pk_rows: Vec<Row> = conn.query(pk_sql).await.map_err(map_mysql_error)?;
        let mut pk_map: HashMap<String, Vec<String>> = HashMap::new();
        for row in pk_rows {
            let tbl: String = row.get::<String, _>(0).unwrap_or_default();
            let col: String = row.get::<String, _>(1).unwrap_or_default();
            pk_map.entry(tbl).or_default().push(col);
        }
        // 同时更新 table_type（检测 VIEW）
        let type_sql = format!(
            "SELECT TABLE_NAME, TABLE_TYPE FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = '{}'",
            escape_mysql_literal(database),
        );
        let type_rows: Vec<Row> = conn.query(type_sql).await.map_err(map_mysql_error)?;
        let mut type_map: HashMap<String, String> = HashMap::new();
        for row in type_rows {
            let tbl: String = row.get::<String, _>(0).unwrap_or_default();
            let ttype: String = row.get::<String, _>(1).unwrap_or_default();
            type_map.insert(tbl, ttype);
        }

        for s in &mut summaries {
            if let Some(pks) = pk_map.remove(&s.name) {
                s.primary_keys = pks;
            }
            if let Some(ttype) = type_map.get(&s.name) {
                if ttype.contains("VIEW") {
                    s.table_type = "VIEW".into();
                }
            }
        }

        Ok(summaries)
    }

    async fn load_routines_summary(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        _schema: Option<&str>,
    ) -> AppResult<Vec<driver_api::TableSummary>> {
        let conn = mysql_conn_mut(handle)?;
        conn.query_drop(format!("USE {}", quote_mysql(database)))
            .await
            .map_err(map_mysql_error)?;

        let sql = format!(
            "SELECT ROUTINE_NAME, ROUTINE_TYPE, ROUTINE_COMMENT, CREATED, LAST_ALTERED \
             FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = '{}' ORDER BY ROUTINE_NAME",
            escape_mysql_literal(database),
        );
        let rows: Vec<Row> = conn.query(sql).await.map_err(map_mysql_error)?;
        let summaries = rows
            .into_iter()
            .map(|row| {
                let name: String = row.get::<String, _>(0).unwrap_or_default();
                let routine_type: String = row.get::<String, _>(1).unwrap_or_default();
                let comment: Option<String> = row.get::<Option<String>, _>(2).flatten();
                let create_time: Option<String> = row
                    .get::<Option<mysql_async::Value>, _>(3)
                    .flatten()
                    .and_then(|v| match v {
                        mysql_async::Value::Date(y, m, d, hh, mm, ss, _) => {
                            Some(format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}"))
                        }
                        _ => None,
                    });

                driver_api::TableSummary {
                    name,
                    table_type: routine_type,
                    row_count: None,
                    total_size: None,
                    data_size: None,
                    index_size: None,
                    engine: None,
                    collation: None,
                    primary_keys: Vec::new(),
                    comment: comment.filter(|c| !c.is_empty()),
                    create_time: create_time.filter(|c| !c.is_empty()),
                }
            })
            .collect();
        Ok(summaries)
    }

    async fn show_processlist(
        &self,
        handle: &mut ConnectionHandle,
    ) -> AppResult<Vec<core_domain::ProcessInfo>> {
        let conn = mysql_conn_mut(handle)?;
        let sql = "SHOW FULL PROCESSLIST";
        let rows: Vec<Row> = conn.query(sql).await.map_err(map_mysql_error)?;
        let mut result = Vec::new();
        for row in rows {
            let id: u64 = row.get::<u64, _>(0).unwrap_or(0);
            let user: String = row.get::<String, _>(1).unwrap_or_default();
            let host: String = row.get::<String, _>(2).unwrap_or_default();
            let db: Option<String> = row.get::<Option<String>, _>(3).flatten();
            let command: String = row.get::<String, _>(4).unwrap_or_default();
            let time_secs: u64 = row.get::<u64, _>(5).unwrap_or(0);
            let state: Option<String> = row.get::<Option<String>, _>(6).flatten();
            let info: Option<String> = row.get::<Option<String>, _>(7).flatten();
            result.push(core_domain::ProcessInfo {
                id, user, host, db, command, time_secs, state, info,
            });
        }
        Ok(result)
    }
}

// ── helpers ──

fn apply_ssl(builder: OptsBuilder, ssl_mode: SslMode) -> OptsBuilder {
    match ssl_mode {
        SslMode::Disable => builder,
        SslMode::Prefer => builder.ssl_opts(
            SslOpts::default().with_danger_accept_invalid_certs(true),
        ),
        SslMode::Require => builder.ssl_opts(SslOpts::default()),
    }
}

fn mysql_conn_mut(handle: &mut ConnectionHandle) -> AppResult<&mut Conn> {
    match handle {
        ConnectionHandle::MySql { conn } => Ok(conn),
        _ => Err(AppError::Validation("expected mysql handle".into())),
    }
}

fn open_conn(
    profile: &ConnectionProfile,
    password: &str,
    database: Option<&str>,
) -> impl std::future::Future<Output = AppResult<Conn>> {
    let mut builder = OptsBuilder::default();
    builder = builder
        .ip_or_hostname(profile.host.clone())
        .tcp_port(profile.port)
        .user(Some(profile.username.clone()))
        .pass(Some(password.to_string()));
    builder = apply_ssl(builder, profile.ssl_mode);
    let prefer_fallback = profile.ssl_mode == SslMode::Prefer;
    if let Some(db) = database {
        builder = builder.db_name(Some(db.to_string()));
    }
    async move {
        if prefer_fallback {
            match Conn::new(builder.clone()).await {
                Ok(conn) => return Ok(conn),
                Err(ref e) if is_ssl_error(e) => {
                    let mut plain = OptsBuilder::default();
                    plain = plain
                        .ip_or_hostname(profile.host.clone())
                        .tcp_port(profile.port)
                        .user(Some(profile.username.clone()))
                        .pass(Some(password.to_string()));
                    if let Some(db) = database {
                        plain = plain.db_name(Some(db.to_string()));
                    }
                    return Conn::new(plain).await.map_err(map_mysql_error);
                }
                Err(e) => return Err(map_mysql_error(e)),
            }
        }
        Conn::new(builder).await.map_err(map_mysql_error)
    }
}

async fn exec_on_conn(conn: &mut Conn, execution: QueryExecution) -> AppResult<QueryResult> {
    let start = Instant::now();
    let sql = execution.sql.trim().to_string();
    if let Some(ref db) = execution.database {
        let lower = sql.to_ascii_lowercase();
        if !lower.starts_with("use ") {
            conn.query_drop(format!("USE {}", quote_mysql(db)))
                .await
                .map_err(map_mysql_error)?;
        }
    }
    let lower = sql.to_ascii_lowercase();
    if lower.starts_with("select")
        || lower.starts_with("show")
        || lower.starts_with("desc")
        || lower.starts_with("describe")
        || lower.starts_with("explain")
        || lower.starts_with("execute")
        || lower.starts_with("call")
    {
        query_rows(conn, &sql).await
    } else {
        {
            let mut rs = conn.query_iter(&sql).await.map_err(map_mysql_error)?;
            while let Some(_) = rs.next().await.map_err(map_mysql_error)? {}
        }
        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: Some(conn.affected_rows()),
            elapsed_ms: start.elapsed().as_millis(),
            message: Some(tr!("语句执行成功").to_string()),
            mongo_types: HashMap::new(),
        })
    }
}

async fn query_rows(conn: &mut Conn, sql: &str) -> AppResult<QueryResult> {
    let start = Instant::now();
    let mut result_set = conn.query_iter(sql).await.map_err(map_mysql_error)?;
    let columns: Vec<String> = result_set
        .columns_ref()
        .iter()
        .map(|c| c.name_str().to_string())
        .collect();
    let rows: Vec<Row> = result_set.collect().await.map_err(map_mysql_error)?;
    let mapped = rows
        .iter()
        .map(|row| {
            let mut m = BTreeMap::new();
            for (i, col) in columns.iter().enumerate() {
                m.insert(
                    col.clone(),
                    row.as_ref(i).map(mysql_cell).unwrap_or(QueryCellValue::Null),
                );
            }
            m
        })
        .collect();
    Ok(QueryResult {
        columns,
        rows: mapped,
        affected_rows: None,
        elapsed_ms: start.elapsed().as_millis(),
        message: None,
        mongo_types: HashMap::new(),
    })
}

fn mysql_cell(value: &Value) -> QueryCellValue {
    match value {
        Value::NULL => QueryCellValue::Null,
        Value::Bytes(b) => String::from_utf8_lossy(b).to_string().into(),
        Value::Int(v) => v.to_string().into(),
        Value::UInt(v) => v.to_string().into(),
        Value::Float(v) => v.to_string().into(),
        Value::Double(v) => v.to_string().into(),
        Value::Date(y, m, d, hh, mm, ss, us) => {
            format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}.{us:06}").into()
        }
        Value::Time(neg, days, h, m, s, us) => format!(
            "{}{days} {h:02}:{m:02}:{s:02}.{us:06}",
            if *neg { "-" } else { "" }
        )
        .into(),
    }
}

fn quote_mysql(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

fn escape_mysql_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn map_mysql_error(e: mysql_async::Error) -> AppError {
    match &e {
        mysql_async::Error::Server(_) => AppError::Query(e.to_string()),
        mysql_async::Error::Url(_) => AppError::connection(e.to_string()),
        _ => AppError::transient_connection(e.to_string()),
    }
}

fn is_ssl_error(e: &mysql_async::Error) -> bool {
    let msg = e.to_string().to_ascii_lowercase();
    msg.contains("ssl")
        || msg.contains("tls")
        || msg.contains("does not have this capability")
}

async fn disconnect(conn: Conn) {
    let _ = conn.disconnect().await.context("disconnect mysql");
}
