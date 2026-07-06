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
        Ok(rows
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
            .collect())
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
        let sql = format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, COLUMN_COMMENT, EXTRA FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = '{}' AND TABLE_NAME = '{}' ORDER BY ORDINAL_POSITION",
            escape_mysql_literal(db),
            escape_mysql_literal(&table.table),
        );
        let rows: Vec<Row> = conn.query(sql).await.map_err(map_mysql_error)?;
        let columns = rows
            .into_iter()
            .map(|row| {
                let extra: String = row.get::<String, _>(6).unwrap_or_default();
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
                    auto_increment: extra.to_ascii_lowercase().contains("auto_increment"),
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
                .and_then(|row| row.get::<String, _>(1).or_else(|| row.get::<String, _>(0)))
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
                .and_then(|row| row.get::<String, _>(1).or_else(|| row.get::<String, _>(0)))
        };
        Ok(TableDefinition { columns, create_sql })
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
        _ => AppError::Connection(e.to_string()),
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
