use async_trait::async_trait;
use core_domain::{
    AppError, AppResult, ColumnDefinition, ConnectionProfile, ExplorerNode, ExplorerNodeType,
    QueryCellValue, QueryExecution, QueryResult, TableChangeSet, TableDefinition, TableRef,
};
use driver_api::{ConnectionHandle, ConnectionProvider, DatabaseDriver, TableSummary};
use i18n::tr;
use rusqlite::{Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct SqliteDriver;

type SharedConn = Arc<Mutex<Connection>>;

/// 同步 rusqlite 调用封装：spawn_blocking + blocking_lock，避免阻塞 async runtime。
async fn run_blocking<T: Send + 'static>(
    conn: SharedConn,
    f: impl FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(move || {
        let mut conn = conn.blocking_lock();
        f(&mut conn).map_err(map_sqlite_error)
    })
    .await
    .map_err(|e| AppError::Query(format!("sqlite task panicked: {e}")))?
}

fn sqlite_conn(handle: &mut ConnectionHandle) -> AppResult<SharedConn> {
    match handle {
        ConnectionHandle::Sqlite { conn } => Ok(conn.clone()),
        _ => Err(AppError::Validation("expected sqlite handle".into())),
    }
}

/// 展开 `~` 为家目录；SQLite 不会自动展开波浪号。
fn expand_home_with(path: &str, home: Option<OsString>) -> String {
    if path == "~" {
        if let Some(home) = home {
            return home.to_string_lossy().to_string();
        }
    } else if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home {
            return std::path::Path::new(&home)
                .join(rest)
                .to_string_lossy()
                .to_string();
        }
    }
    path.to_string()
}

fn expand_home(path: &str) -> String {
    expand_home_with(path, std::env::var_os("HOME"))
}

fn file_path(profile: &ConnectionProfile) -> AppResult<String> {
    let raw = profile
        .file_path
        .as_deref()
        .ok_or_else(|| AppError::Validation(tr!("SQLite 需要文件路径").to_string()))?;
    Ok(expand_home(raw))
}

#[async_trait]
impl ConnectionProvider for SqliteDriver {
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        _password: &str,
        _database: Option<&str>,
    ) -> AppResult<ConnectionHandle> {
        let path = file_path(profile)?;
        let conn = tokio::task::spawn_blocking(move || Connection::open(&path))
            .await
            .map_err(|e| AppError::connection(format!("sqlite task panicked: {e}")))?
            .map_err(map_sqlite_error)?;
        Ok(ConnectionHandle::Sqlite { conn: Arc::new(Mutex::new(conn)) })
    }

    async fn ping(&self, handle: &mut ConnectionHandle) -> AppResult<()> {
        let conn = sqlite_conn(handle)?;
        run_blocking(conn, |c| c.query_row("SELECT 1", [], |_| Ok(()))).await
    }
}

#[async_trait]
impl DatabaseDriver for SqliteDriver {
    async fn test_connection(&self, profile: &ConnectionProfile, _password: &str) -> AppResult<()> {
        let path = file_path(profile)?;
        if !std::path::Path::new(&path).exists() {
            return Err(AppError::Validation(
                tr!("SQLite 文件不存在: {}", path),
            ));
        }
        // 打开 + 验证可读都在阻塞闭包内执行，避免阻塞 async runtime
        tokio::task::spawn_blocking(move || {
            let conn = Connection::open(&path).map_err(map_sqlite_error)?;
            conn.query_row("SELECT 1", [], |_| Ok(()))
                .map_err(map_sqlite_error)?;
            Ok::<(), AppError>(())
        })
        .await
        .map_err(|e| AppError::connection(format!("sqlite task panicked: {e}")))?
    }

    async fn list_roots(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
    ) -> AppResult<Vec<ExplorerNode>> {
        // SQLite 单文件单库：返回一个数据库节点，name 为文件 basename。
        // 全部走 run_blocking，禁止在 async 上下文直接 blocking_lock。
        let conn = sqlite_conn(handle)?;
        let name = run_blocking(conn, |c| {
            let raw = c.path().unwrap_or_default().to_string();
            let base = std::path::Path::new(&raw)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "sqlite".to_string());
            Ok(base)
        })
        .await?;
        Ok(vec![ExplorerNode {
            id: format!("sqlite-db:{connection_id}:{name}"),
            connection_id: connection_id.to_string(),
            name: name.clone(),
            node_type: ExplorerNodeType::Database,
            parent_id: None,
            database: Some(name),
            schema: None,
            expandable: true,
            loaded: false,
        }])
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
        let conn = sqlite_conn(handle)?;
        let sql = "SELECT name, type FROM sqlite_master \
                   WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
                   ORDER BY name";
        let nodes = run_blocking(conn, move |c| {
            let mut stmt = c.prepare(sql)?;
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(0)?;
                let ty: String = row.get(1)?;
                Ok((name, ty))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await?;
        Ok(nodes
            .into_iter()
            .map(|(name, ty)| {
                let is_view = ty == "view";
                ExplorerNode {
                    id: format!("sqlite-table:{connection_id}:{name}"),
                    connection_id: connection_id.to_string(),
                    name: name.clone(),
                    node_type: if is_view {
                        ExplorerNodeType::View
                    } else {
                        ExplorerNodeType::Table
                    },
                    parent_id: Some(parent.id.clone()),
                    database: parent.database.clone(),
                    schema: None,
                    expandable: false,
                    loaded: true,
                }
            })
            .collect())
    }

    async fn load_table_definition(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
    ) -> AppResult<TableDefinition> {
        let conn = sqlite_conn(handle)?;
        let table_name = table.table.clone();
        let (cols, uniq_sqls, create_sql) = run_blocking(conn, move |c| {
            // 列信息：PRAGMA table_info → (name, type, notnull, dflt_value, pk)
            let mut stmt = c.prepare(&format!("PRAGMA table_info({})", quote_ident(&table_name)))?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,          // name
                    row.get::<_, String>(2)?,          // type
                    row.get::<_, i64>(3)?,             // notnull
                    row.get::<_, Option<String>>(4)?,  // dflt_value
                    row.get::<_, i64>(5)?,             // pk
                ))
            })?;
            let cols = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            // 索引检测：sqlite_master 没有 unique 列，须用 PRAGMA index_list 的 unique 字段
            // 收集所有索引 (name, unique)，auto 索引的 sql 为 NULL 直接跳过
            let mut ilstmt = c.prepare(&format!("PRAGMA index_list({})", quote_ident(&table_name)))?;
            let index_entries: Vec<(String, bool)> = ilstmt
                .query_map([], |row| Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)? != 0)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            // 取这些索引的 CREATE 声明；uniq_sqls 仅保留唯一索引用于列级 unique 标记
            let mut uniq_sqls: Vec<String> = Vec::new();
            let mut all_index_sqls: Vec<String> = Vec::new();
            for (name, unique) in index_entries {
                if let Some(sql) = c
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE type='index' AND name=?1 AND sql IS NOT NULL",
                        [&name],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                {
                    all_index_sqls.push(sql.clone());
                    if unique {
                        uniq_sqls.push(sql);
                    }
                }
            }
            // 建表 SQL（含视图的 sqlite_master.sql）
            let mut create_sql: Option<String> = c
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type IN ('table','view') AND name=?1",
                    [&table_name],
                    |row| row.get(0),
                )
                .unwrap_or(None);
            // 追加独立 CREATE INDEX，供 UI 索引页解析展示（与 PostgreSQL 驱动的做法一致）
            if let Some(sql) = create_sql.as_mut() {
                for idx_sql in &all_index_sqls {
                    sql.push('\n');
                    sql.push_str(idx_sql);
                    sql.push(';');
                }
            }
            Ok((cols, uniq_sqls, create_sql))
        })
        .await?;

        let unique_col_names = collect_unique_columns(&uniq_sqls);
        let columns = cols
            .into_iter()
            .map(|(name, data_type, notnull, dflt, pk)| {
                let auto_increment = create_sql
                    .as_deref()
                    .map(|sql| column_decl_contains(sql, &name, "AUTOINCREMENT"))
                    .unwrap_or(false);
                ColumnDefinition {
                    auto_increment,
                    name: name.clone(),
                    data_type,
                    nullable: notnull == 0,
                    primary_key: pk > 0,
                    unique: unique_col_names.contains(&name),
                    on_update_current_timestamp: false,
                    default_value: dflt,
                    comment: None,
                }
            })
            .collect();

        Ok(TableDefinition {
            columns,
            create_sql,
            table_comment: None,
            engine: None,
            charset: None,
        })
    }

    async fn preview_table(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
        limit: u32,
    ) -> AppResult<QueryResult> {
        let conn = sqlite_conn(handle)?;
        let sql = format!(
            "SELECT * FROM {} LIMIT {}",
            quote_ident(&table.table),
            limit
        );
        query_rows(conn, &sql).await
    }

    async fn execute_sql(
        &self,
        handle: &mut ConnectionHandle,
        execution: QueryExecution,
    ) -> AppResult<QueryResult> {
        let conn = sqlite_conn(handle)?;
        let sql = execution.sql.trim().to_string();
        let lower = sql.to_ascii_lowercase();
        if lower.starts_with("select")
            || lower.starts_with("pragma")
            || lower.starts_with("explain")
            || lower.starts_with("with")
        {
            query_rows(conn, &sql).await
        } else {
            // INSERT/UPDATE/DELETE/DDL
            let conn2 = conn.clone();
            let sql2 = sql.clone();
            let start = Instant::now();
            let affected = tokio::task::spawn_blocking(move || {
                let c = conn2.blocking_lock();
                let n = c.execute(&sql2, [])?;
                Ok::<u64, rusqlite::Error>(n as u64)
            })
            .await
            .map_err(|e| AppError::Query(format!("sqlite task panicked: {e}")))?
            .map_err(map_sqlite_error)?;
            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                affected_rows: Some(affected),
                elapsed_ms: start.elapsed().as_millis(),
                message: Some(tr!("语句执行成功").to_string()),
                mongo_types: HashMap::new(),
            })
        }
    }

    async fn apply_table_changes(
        &self,
        _handle: &mut ConnectionHandle,
        _changes: TableChangeSet,
    ) -> AppResult<QueryResult> {
        Err(AppError::Unsupported(
            tr!("SQLite 表格编辑请在数据视图中直接修改后保存").to_string(),
        ))
    }

    async fn create_database(
        &self,
        _handle: &mut ConnectionHandle,
        _name: &str,
        _charset: Option<&str>,
        _collation: Option<&str>,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("SQLite 是单文件数据库，无需创建数据库").to_string()))
    }

    async fn rename_database(
        &self,
        _handle: &mut ConnectionHandle,
        _old_name: &str,
        _new_name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("SQLite 是单文件数据库，无需重命名数据库").to_string()))
    }

    async fn drop_database(
        &self,
        _handle: &mut ConnectionHandle,
        _name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("SQLite 是单文件数据库，无需删除数据库").to_string()))
    }

    async fn create_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("SQLite 不支持 Schema").to_string()))
    }

    async fn rename_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _old_name: &str,
        _new_name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("SQLite 不支持 Schema").to_string()))
    }

    async fn drop_schema(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _name: &str,
    ) -> AppResult<()> {
        Err(AppError::Unsupported(tr!("SQLite 不支持 Schema").to_string()))
    }

    async fn rename_table(
        &self,
        handle: &mut ConnectionHandle,
        _database: &str,
        _schema: Option<&str>,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        let conn = sqlite_conn(handle)?;
        let sql = format!(
            "ALTER TABLE {} RENAME TO {}",
            quote_ident(old_name),
            quote_ident(new_name)
        );
        run_blocking(conn, move |c| c.execute_batch(&sql)).await
    }

    async fn dump_table_all_data(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
    ) -> AppResult<QueryResult> {
        let conn = sqlite_conn(handle)?;
        let sql = format!("SELECT * FROM {}", quote_ident(&table.table));
        query_rows(conn, &sql).await
    }

    async fn load_tables_summary(
        &self,
        handle: &mut ConnectionHandle,
        _database: &str,
        _schema: Option<&str>,
    ) -> AppResult<Vec<TableSummary>> {
        let conn = sqlite_conn(handle)?;
        let sql = "SELECT name, type FROM sqlite_master \
                   WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
                   ORDER BY name";
        run_blocking(conn, move |c| {
            let mut stmt = c.prepare(sql)?;
            let rows = stmt.query_map([], |row| {
                let name: String = row.get(0)?;
                let ty: String = row.get(1)?;
                Ok((name, ty))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .map(|nodes| {
            nodes
                .into_iter()
                .map(|(name, ty)| TableSummary {
                    name,
                    table_type: if ty == "view" { "VIEW".into() } else { "TABLE".into() },
                    row_count: None,
                    total_size: None,
                    data_size: None,
                    index_size: None,
                    engine: None,
                    collation: None,
                    primary_keys: Vec::new(),
                    comment: None,
                    create_time: None,
                })
                .collect()
        })
    }
}

// ── helpers ──

async fn query_rows(conn: SharedConn, sql: &str) -> AppResult<QueryResult> {
    let start = Instant::now();
    let sql = sql.to_string();
    run_blocking(conn, move |c| {
        let mut stmt = c.prepare(&sql)?;
        let columns: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        let rows = stmt.query_map([], |row| {
            let mut m = BTreeMap::new();
            for (i, col) in columns.iter().enumerate() {
                m.insert(col.clone(), sqlite_cell(row.get_ref(i)?));
            }
            Ok(m)
        })?;
        let rows = rows.collect::<rusqlite::Result<Vec<BTreeMap<String, QueryCellValue>>>>()?;
        Ok((columns, rows))
    })
    .await
    .map(|(columns, rows)| QueryResult {
        columns,
        rows,
        affected_rows: None,
        elapsed_ms: start.elapsed().as_millis(),
        message: None,
        mongo_types: HashMap::new(),
    })
}

/// 从唯一索引的 CREATE 声明中解析涉及的列名
/// sql 形如：CREATE UNIQUE INDEX ... ON table (col1, col2)
fn collect_unique_columns(index_sqls: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for sql in index_sqls {
        if let Some(open) = sql.find('(') {
            if let Some(close) = sql.find(')') {
                let inner = &sql[open + 1..close];
                for part in inner.split(',') {
                    let col = part.trim().trim_matches('"').trim_matches('`');
                    if !col.is_empty() {
                        result.push(col.to_string());
                    }
                }
            }
        }
    }
    result
}

/// 检查建表 SQL 中某列的声明是否包含关键字（如 AUTOINCREMENT）。
/// 匹配列名（兼容 "col"、`col`、[col]、裸 col 写法）后到下一个逗号/右括号的片段。
fn column_decl_contains(create_sql: &str, column: &str, keyword: &str) -> bool {
    let keyword = keyword.to_ascii_uppercase();
    let lower_sql = create_sql.to_ascii_lowercase();
    let lower_col = column.to_ascii_lowercase();
    // 扫描列名出现的每个位置，要求左右是标识符边界，再检查该列声明片段
    let mut from = 0;
    while let Some(rel) = lower_sql[from..].find(&lower_col) {
        let pos = from + rel;
        let prev = lower_sql.as_bytes().get(pos.wrapping_sub(1)).copied();
        let next = lower_sql.as_bytes().get(pos + lower_col.len()).copied();
        let start_ok = matches!(
            prev,
            None | Some(b'(' | b',' | b'"' | b'`' | b'[' | b' ' | b'\t' | b'\n')
        );
        let end_ok = matches!(
            next,
            None | Some(b' ' | b'\t' | b'\n' | b',' | b')' | b'"' | b'`' | b']')
        );
        if start_ok && end_ok {
            let rest = &create_sql[pos + lower_col.len()..];
            let end = rest
                .find(',')
                .or_else(|| rest.find(')'))
                .unwrap_or(rest.len());
            if rest[..end].to_ascii_uppercase().contains(&keyword) {
                return true;
            }
        }
        from = pos + 1;
    }
    false
}

fn sqlite_cell(value: rusqlite::types::ValueRef<'_>) -> QueryCellValue {
    match value {
        rusqlite::types::ValueRef::Null => QueryCellValue::Null,
        rusqlite::types::ValueRef::Integer(i) => i.to_string().into(),
        rusqlite::types::ValueRef::Real(f) => f.to_string().into(),
        rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).to_string().into(),
        rusqlite::types::ValueRef::Blob(b) => String::from_utf8_lossy(b).to_string().into(),
    }
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn map_sqlite_error(e: rusqlite::Error) -> AppError {
    let msg = e.to_string();
    if msg.contains("locked") {
        AppError::transient_connection(msg)
    } else {
        AppError::Query(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_domain::{ConnectionProfile, ConnectionProfileInput, DatabaseKind};

    fn test_profile(path: &str) -> ConnectionProfile {
        let input = ConnectionProfileInput {
            name: "test".into(),
            kind: DatabaseKind::Sqlite,
            host: String::new(),
            port: 0,
            username: String::new(),
            file_path: Some(path.to_string()),
            ..ConnectionProfileInput::default()
        };
        ConnectionProfile::from_input("c1".into(), input)
    }

    fn memory_profile() -> ConnectionProfile {
        test_profile(":memory:")
    }

    #[test]
    fn expand_home_resolves_tilde() {
        let home = Some(std::ffi::OsString::from("/home/user"));
        assert_eq!(expand_home_with("~/db.sqlite", home.clone()), "/home/user/db.sqlite");
        assert_eq!(expand_home_with("~", home), "/home/user");
        // 非 ~ 路径原样返回
        assert_eq!(expand_home_with("/abs/path.db", Some(std::ffi::OsString::from("/h"))), "/abs/path.db");
        assert_eq!(expand_home_with(":memory:", None), ":memory:");
        // 无 HOME 时保持原样
        assert_eq!(expand_home_with("~/x.db", None), "~/x.db");
    }

    #[tokio::test]
    async fn connect_and_ping() {
        let driver = SqliteDriver;
        let profile = memory_profile();
        let mut handle = driver.connect(&profile, "", None).await.unwrap();
        driver.ping(&mut handle).await.unwrap();
    }

    #[tokio::test]
    async fn list_children_returns_tables() {
        let driver = SqliteDriver;
        let profile = memory_profile();
        let mut handle = driver.connect(&profile, "", None).await.unwrap();
        // 建两张表
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "CREATE TABLE t1 (id INTEGER PRIMARY KEY, name TEXT)".into(),
                },
            )
            .await
            .unwrap();
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "CREATE VIEW v1 AS SELECT 1".into(),
                },
            )
            .await
            .unwrap();
        let parent = ExplorerNode {
            id: "db".into(),
            connection_id: "c1".into(),
            name: "db".into(),
            node_type: ExplorerNodeType::Database,
            parent_id: None,
            database: Some("db".into()),
            schema: None,
            expandable: true,
            loaded: true,
        };
        let nodes = driver.list_children(&mut handle, "c1", &parent).await.unwrap();
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["t1", "v1"]);
        assert!(nodes[0].node_type == ExplorerNodeType::Table);
        assert!(nodes[1].node_type == ExplorerNodeType::View);
    }

    #[tokio::test]
    async fn execute_select_returns_rows() {
        let driver = SqliteDriver;
        let profile = memory_profile();
        let mut handle = driver.connect(&profile, "", None).await.unwrap();
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "CREATE TABLE t (a INTEGER, b TEXT)".into(),
                },
            )
            .await
            .unwrap();
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "INSERT INTO t VALUES (1, 'x'), (2, NULL)".into(),
                },
            )
            .await
            .unwrap();
        let result = driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "SELECT * FROM t ORDER BY a".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.columns, vec!["a", "b"]);
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0]["a"].as_text(), Some("1"));
        assert_eq!(result.rows[0]["b"].as_text(), Some("x"));
        assert!(result.rows[1]["b"].is_null());
    }

    #[tokio::test]
    async fn load_definition_maps_columns() {
        let driver = SqliteDriver;
        let profile = memory_profile();
        let mut handle = driver.connect(&profile, "", None).await.unwrap();
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email TEXT)".into(),
                },
            )
            .await
            .unwrap();
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "CREATE UNIQUE INDEX idx_email ON t (email)".into(),
                },
            )
            .await
            .unwrap();
        let def = driver
            .load_table_definition(
                &mut handle,
                &TableRef {
                    connection_id: "c1".into(),
                    database: None,
                    schema: None,
                    table: "t".into(),
                    is_view: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(def.columns.len(), 3);
        assert!(def.columns[0].primary_key);
        assert!(def.columns[0].auto_increment);
        assert!(!def.columns[1].nullable);
        // email 列由显式 CREATE UNIQUE INDEX 唯一索引覆盖
        assert!(def.columns[2].unique);
        // name 列无唯一索引，不应被误判为 unique
        assert!(!def.columns[1].unique);
        assert!(def.create_sql.is_some());
        // 独立 CREATE INDEX 应追加到 create_sql，供 UI 索引页解析展示
        let create_sql = def.create_sql.unwrap();
        assert!(create_sql.contains("CREATE UNIQUE INDEX idx_email ON t (email)"), "create_sql 应包含独立索引 SQL: {create_sql}");
    }

    #[tokio::test]
    async fn list_children_filters_internal_and_maps_cells() {
        let driver = SqliteDriver;
        let profile = memory_profile();
        let mut handle = driver.connect(&profile, "", None).await.unwrap();
        // AUTOINCREMENT 会生成 sqlite_sequence 内部表，list_children 必须过滤掉
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, v REAL, s TEXT)".into(),
                },
            )
            .await
            .unwrap();
        driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "INSERT INTO t (v, s) VALUES (1.5, 'hi'), (NULL, NULL)".into(),
                },
            )
            .await
            .unwrap();
        let parent = ExplorerNode {
            id: "db".into(),
            connection_id: "c1".into(),
            name: "db".into(),
            node_type: ExplorerNodeType::Database,
            parent_id: None,
            database: Some("db".into()),
            schema: None,
            expandable: true,
            loaded: true,
        };
        let nodes = driver.list_children(&mut handle, "c1", &parent).await.unwrap();
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["t"], "sqlite_% 内部表应被过滤");
        // 浮点/文本/NULL 单元格转换
        let result = driver
            .execute_sql(
                &mut handle,
                QueryExecution {
                    connection_id: "c1".into(),
                    database: None,
                    sql: "SELECT * FROM t ORDER BY id".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.rows[0]["v"].as_text(), Some("1.5"));
        assert_eq!(result.rows[0]["s"].as_text(), Some("hi"));
        assert!(result.rows[1]["v"].is_null());
        assert!(result.rows[1]["s"].is_null());
    }
}
