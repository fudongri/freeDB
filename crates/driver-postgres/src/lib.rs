use async_trait::async_trait;
use core_domain::{
    AppError, AppResult, ColumnDefinition, ConnectionProfile, ExplorerNode, ExplorerNodeType,
    QueryCellValue, QueryExecution, QueryResult, SslMode, TableChangeSet, TableDefinition, TableRef,
};
use driver_api::{ConnectionHandle, ConnectionProvider, DatabaseDriver};
use i18n::tr;
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Clone, Default)]
pub struct PostgresDriver;

#[async_trait]
impl ConnectionProvider for PostgresDriver {
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: Option<&str>,
    ) -> AppResult<ConnectionHandle> {
        let db = profile.default_database.as_deref().unwrap_or("postgres");
        let conn_str = format!(
            "host={} port={} user={} password={} dbname={}",
            profile.host, profile.port, profile.username, password, db
        );
        let (client, connection) = match profile.ssl_mode {
            SslMode::Disable => {
                let (c, conn) = tokio_postgres::connect(&conn_str, NoTls)
                    .await
                    .map_err(map_pg_error)?;
                let h: BoxFuture<()> = Box::pin(async move { let _ = conn.await; });
                (c, tokio::spawn(h))
            }
            SslMode::Prefer => {
                let tls = TlsConnector::builder()
                    .danger_accept_invalid_certs(true)
                    .build()
                    .map_err(|e| AppError::Connection(format!("TLS init failed: {e}")))?;
                match tokio_postgres::connect(&conn_str, MakeTlsConnector::new(tls)).await {
                    Ok((c, conn)) => {
                        let h: BoxFuture<()> = Box::pin(async move { let _ = conn.await; });
                        (c, tokio::spawn(h))
                    }
                    Err(e) => {
                        let msg = e.to_string().to_ascii_lowercase();
                        if msg.contains("tls") || msg.contains("ssl") {
                            let (c, conn) = tokio_postgres::connect(&conn_str, NoTls)
                                .await
                                .map_err(map_pg_error)?;
                            let h: BoxFuture<()> = Box::pin(async move { let _ = conn.await; });
                            (c, tokio::spawn(h))
                        } else {
                            return Err(map_pg_error(e));
                        }
                    }
                }
            }
            SslMode::Require => {
                let tls = TlsConnector::new()
                    .map_err(|e| AppError::Connection(format!("TLS init failed: {e}")))?;
                let (c, conn) = tokio_postgres::connect(&conn_str, MakeTlsConnector::new(tls))
                    .await
                    .map_err(map_pg_error)?;
                let h: BoxFuture<()> = Box::pin(async move { let _ = conn.await; });
                (c, tokio::spawn(h))
            }
        };
        Ok(ConnectionHandle::Postgres { client, connection })
    }

    async fn ping(&self, handle: &mut ConnectionHandle) -> AppResult<()> {
        match handle {
            ConnectionHandle::Postgres { client, .. } => {
                client
                    .simple_query("SELECT 1")
                    .await
                    .map_err(map_pg_error)?;
                Ok(())
            }
            _ => Err(AppError::Validation("expected postgres handle".into())),
        }
    }
}

#[async_trait]
impl DatabaseDriver for PostgresDriver {
    async fn test_connection(
        &self,
        profile: &ConnectionProfile,
        password: &str,
    ) -> AppResult<()> {
        let mut handle = self.connect(profile, password, None).await?;
        self.ping(&mut handle).await
    }

    async fn list_roots(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
    ) -> AppResult<Vec<ExplorerNode>> {
        let client = pg_client(handle)?;
        let rows = client
            .query(
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
                &[],
            )
            .await
            .map_err(map_pg_error)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let db: String = row.get(0);
                ExplorerNode {
                    id: format!("pg-db:{connection_id}:{db}"),
                    connection_id: connection_id.to_string(),
                    name: db.clone(),
                    node_type: ExplorerNodeType::Database,
                    parent_id: None,
                    database: Some(db),
                    schema: None,
                    expandable: true,
                    loaded: false,
                }
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
        match parent.node_type {
            ExplorerNodeType::Database => {
                let db = parent
                    .database
                    .clone()
                    .ok_or_else(|| AppError::Validation("missing database".into()))?;
                let client = pg_client(handle)?;
                let rows = client
                    .query(
                        "SELECT schema_name FROM information_schema.schemata WHERE schema_name NOT IN ('information_schema', 'pg_catalog') ORDER BY schema_name",
                        &[],
                    )
                    .await
                    .map_err(map_pg_error)?;
                Ok(rows
                    .into_iter()
                    .map(|row| {
                        let schema: String = row.get(0);
                        ExplorerNode {
                            id: format!("pg-schema:{connection_id}:{db}:{schema}"),
                            connection_id: connection_id.to_string(),
                            name: schema.clone(),
                            node_type: ExplorerNodeType::Schema,
                            parent_id: Some(parent.id.clone()),
                            database: Some(db.clone()),
                            schema: Some(schema),
                            expandable: true,
                            loaded: false,
                        }
                    })
                    .collect())
            }
            ExplorerNodeType::Schema => {
                let db = parent
                    .database
                    .clone()
                    .ok_or_else(|| AppError::Validation("missing database".into()))?;
                let schema = parent
                    .schema
                    .clone()
                    .ok_or_else(|| AppError::Validation("missing schema".into()))?;
                let client = pg_client(handle)?;
                let rows = client
                    .query(
                        "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = $1 ORDER BY table_name",
                        &[&schema],
                    )
                    .await
                    .map_err(map_pg_error)?;
                Ok(rows
                    .into_iter()
                    .map(|row| {
                        let name: String = row.get(0);
                        let kind: String = row.get(1);
                        let is_view = kind.eq_ignore_ascii_case("VIEW");
                        ExplorerNode {
                            id: format!("pg-table:{connection_id}:{db}:{schema}:{name}"),
                            connection_id: connection_id.to_string(),
                            name: name.clone(),
                            node_type: if is_view {
                                ExplorerNodeType::View
                            } else {
                                ExplorerNodeType::Table
                            },
                            parent_id: Some(parent.id.clone()),
                            database: Some(db.clone()),
                            schema: Some(schema.clone()),
                            expandable: false,
                            loaded: true,
                        }
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn load_table_definition(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
    ) -> AppResult<TableDefinition> {
        let schema = table.schema.clone().unwrap_or_else(|| "public".into());
        let client = pg_client(handle)?;
        let rows = client
            .query(
                "SELECT c.column_name, c.data_type, c.is_nullable,
                        EXISTS (SELECT 1 FROM information_schema.table_constraints tc JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema WHERE tc.table_schema = c.table_schema AND tc.table_name = c.table_name AND tc.constraint_type = 'PRIMARY KEY' AND kcu.column_name = c.column_name) AS is_primary,
                        EXISTS (SELECT 1 FROM information_schema.table_constraints tc JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema WHERE tc.table_schema = c.table_schema AND tc.table_name = c.table_name AND tc.constraint_type = 'UNIQUE' AND kcu.column_name = c.column_name) AS is_unique,
                        c.column_default,
                        col_description((c.table_schema||'.'||c.table_name)::regclass::oid, c.ordinal_position::int) AS col_comment
                 FROM information_schema.columns c WHERE c.table_schema = $1 AND c.table_name = $2 ORDER BY c.ordinal_position",
                &[&schema, &table.table],
            )
            .await
            .map_err(map_pg_error)?;
        let columns = rows
            .into_iter()
            .map(|row| {
                let default_val: Option<String> =
                    row.try_get::<_, String>(5).ok().filter(|v| !v.is_empty());
                let is_auto = default_val
                    .as_deref()
                    .map(|d| d.starts_with("nextval("))
                    .unwrap_or(false);
                ColumnDefinition {
                    name: row.get(0),
                    data_type: row.get(1),
                    nullable: row.get::<_, String>(2).eq_ignore_ascii_case("YES"),
                    primary_key: row.get(3),
                    unique: row.get(4),
                    auto_increment: is_auto,
                    on_update_current_timestamp: false,
                    default_value: default_val,
                    comment: row.try_get::<_, String>(6).ok().filter(|v| !v.is_empty()),
                }
            })
            .collect::<Vec<_>>();
        let create_sql = if table.is_view {
            client
                .query_one(
                    "SELECT pg_get_viewdef($1::regclass, true)",
                    &[&format!("{schema}.{}", table.table)],
                )
                .await
                .ok()
                .map(|row| row.get::<_, String>(0))
        } else {
            let table_name = &table.table;
            let cols = columns.clone();
            let mut lines = Vec::new();
            let mut pk_cols = Vec::new();
            for col in &cols {
                let mut parts = vec![format!("    {} {}", quote_pg(&col.name), col.data_type)];
                if !col.nullable {
                    parts.push("NOT NULL".into());
                }
                if let Some(ref def) = col.default_value {
                    parts.push(format!("DEFAULT {}", def));
                }
                lines.push(parts.join(" "));
                if col.primary_key {
                    pk_cols.push(quote_pg(&col.name));
                }
            }
            if !pk_cols.is_empty() {
                lines.push(format!("    PRIMARY KEY ({})", pk_cols.join(", ")));
            }
            let mut ddl = format!(
                "CREATE TABLE {}.{} (\n{}\n);",
                quote_pg(&schema),
                quote_pg(table_name),
                lines.join(",\n")
            );
            for col in &cols {
                if let Some(ref comment) = col.comment {
                    ddl.push_str(&format!(
                        "\nCOMMENT ON COLUMN {}.{}.{} IS '{}';",
                        quote_pg(&schema),
                        quote_pg(table_name),
                        quote_pg(&col.name),
                        comment.replace('\'', "''")
                    ));
                }
            }
            // 查询索引（排除 PRIMARY KEY，已在 CREATE TABLE 中体现）
            if let Ok(idx_rows) = client
                .query(
                    "SELECT indexname, indexdef FROM pg_indexes WHERE schemaname = $1 AND tablename = $2 AND indexname NOT LIKE '%_pkey'",
                    &[&schema, &table.table],
                )
                .await
            {
                for row in &idx_rows {
                    let indexdef: String = row.get(1);
                    ddl.push_str(&format!("\n{};", indexdef));
                }
            }
            Some(ddl)
        };
        Ok(TableDefinition { columns, create_sql })
    }

    async fn preview_table(
        &self,
        handle: &mut ConnectionHandle,
        table: &TableRef,
        limit: u32,
    ) -> AppResult<QueryResult> {
        let schema = table.schema.clone().unwrap_or_else(|| "public".into());
        let sql = format!(
            "SELECT * FROM {}.{} LIMIT {}",
            quote_pg(&schema),
            quote_pg(&table.table),
            limit
        );
        let client = pg_client(handle)?;
        simple_query(client, &sql).await
    }

    async fn execute_sql(
        &self,
        handle: &mut ConnectionHandle,
        execution: QueryExecution,
    ) -> AppResult<QueryResult> {
        match handle {
            ConnectionHandle::Postgres { client, .. } => {
                simple_query(client, execution.sql.trim()).await
            }
            _ => Err(AppError::Validation("expected postgres handle".into())),
        }
    }

    async fn apply_table_changes(
        &self,
        _handle: &mut ConnectionHandle,
        _changes: TableChangeSet,
    ) -> AppResult<QueryResult> {
        Err(AppError::Unsupported(
            tr!("PostgreSQL 表格编辑将在后续迭代中补全").to_string(),
        ))
    }

    async fn create_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
        charset: Option<&str>,
        collation: Option<&str>,
    ) -> AppResult<()> {
        let client = pg_client(handle)?;
        let mut sql = format!("CREATE DATABASE {}", quote_pg(name));
        if let Some(cs) = charset {
            if !cs.is_empty() {
                sql.push_str(&format!(" ENCODING '{}'", cs));
            }
        }
        if let Some(col) = collation {
            if !col.is_empty() {
                sql.push_str(&format!(" LC_COLLATE '{}'", col));
            }
        }
        client.simple_query(&sql).await.map_err(map_pg_error)?;
        Ok(())
    }

    async fn rename_database(
        &self,
        handle: &mut ConnectionHandle,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        let client = pg_client(handle)?;
        client
            .simple_query(&format!(
                "ALTER DATABASE {} RENAME TO {}",
                quote_pg(old_name),
                quote_pg(new_name)
            ))
            .await
            .map_err(map_pg_error)?;
        Ok(())
    }

    async fn drop_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
    ) -> AppResult<()> {
        let client = pg_client(handle)?;
        client
            .simple_query(&format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}' AND pid != pg_backend_pid()",
                name.replace('\'', "''")
            ))
            .await
            .map_err(map_pg_error)?;
        client
            .simple_query(&format!("DROP DATABASE IF EXISTS {}", quote_pg(name)))
            .await
            .map_err(map_pg_error)?;
        Ok(())
    }

    async fn create_schema(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        name: &str,
    ) -> AppResult<()> {
        let client = pg_client(handle)?;
        client
            .simple_query(&format!(
                "CREATE SCHEMA IF NOT EXISTS {}",
                quote_pg(name)
            ))
            .await
            .map_err(map_pg_error)?;
        let _ = database;
        Ok(())
    }

    async fn rename_schema(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        let client = pg_client(handle)?;
        client
            .simple_query(&format!(
                "ALTER SCHEMA {} RENAME TO {}",
                quote_pg(old_name),
                quote_pg(new_name)
            ))
            .await
            .map_err(map_pg_error)?;
        let _ = database;
        Ok(())
    }

    async fn drop_schema(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        name: &str,
    ) -> AppResult<()> {
        let client = pg_client(handle)?;
        client
            .simple_query(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                quote_pg(name)
            ))
            .await
            .map_err(map_pg_error)?;
        let _ = database;
        Ok(())
    }

    async fn rename_table(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        schema: Option<&str>,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()> {
        let client = pg_client(handle)?;
        let qualified = match schema {
            Some(s) => format!(
                "ALTER TABLE {}.{} RENAME TO {}",
                quote_pg(s),
                quote_pg(old_name),
                quote_pg(new_name)
            ),
            None => format!(
                "ALTER TABLE {} RENAME TO {}",
                quote_pg(old_name),
                quote_pg(new_name)
            ),
        };
        let _ = database;
        client.simple_query(&qualified).await.map_err(map_pg_error)?;
        Ok(())
    }

    async fn dump_table_all_data(
        &self,
        handle: &mut ConnectionHandle,
        table: &core_domain::TableRef,
    ) -> AppResult<QueryResult> {
        let schema = table.schema.clone().unwrap_or_else(|| "public".into());
        let sql = format!(
            "SELECT * FROM {}.{}",
            quote_pg(&schema),
            quote_pg(&table.table)
        );
        let client = pg_client(handle)?;
        simple_query(client, &sql).await
    }
}

// ── helpers ──

fn pg_client(handle: &ConnectionHandle) -> AppResult<&Client> {
    match handle {
        ConnectionHandle::Postgres { client, .. } => Ok(client),
        _ => Err(AppError::Validation("expected postgres handle".into())),
    }
}

async fn simple_query(client: &Client, sql: &str) -> AppResult<QueryResult> {
    let start = Instant::now();
    let messages = client.simple_query(sql).await.map_err(map_pg_error)?;
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let mut affected_rows = None;
    let mut message = None;
    for item in messages {
        match item {
            SimpleQueryMessage::Row(row) => {
                if columns.is_empty() {
                    columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                }
                let mut mapped = BTreeMap::new();
                for (i, col) in columns.iter().enumerate() {
                    mapped.insert(col.clone(), pg_cell(row.get(i)));
                }
                rows.push(mapped);
            }
            SimpleQueryMessage::CommandComplete(n) => {
                affected_rows = Some(n);
                message = Some(tr!("语句执行成功").to_string());
            }
            _ => {}
        }
    }
    if columns.is_empty() && rows.is_empty() {
        if let Ok(stmt) = client.prepare(sql).await {
            columns = stmt.columns().iter().map(|c| c.name().to_string()).collect();
        }
    }
    Ok(QueryResult {
        columns,
        rows,
        affected_rows,
        elapsed_ms: start.elapsed().as_millis(),
        message,
        mongo_types: HashMap::new(),
    })
}

fn pg_cell(value: Option<&str>) -> QueryCellValue {
    match value {
        Some(t) => QueryCellValue::Text(t.to_string()),
        None => QueryCellValue::Null,
    }
}

fn quote_pg(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn map_pg_error(e: tokio_postgres::Error) -> AppError {
    if let Some(db_err) = e.as_db_error() {
        let mut msg = format!("{}: {}", db_err.severity(), db_err.message());
        if let Some(detail) = db_err.detail() {
            msg.push_str(&format!("\n{}: {}", tr!("详细"), detail));
        }
        if let Some(hint) = db_err.hint() {
            msg.push_str(&format!("\n{}: {}", tr!("提示"), hint));
        }
        if let Some(position) = db_err.position() {
            msg.push_str(&format!("\n{}: {:?}", tr!("位置"), position));
        }
        AppError::Query(msg)
    } else {
        AppError::Connection(e.to_string())
    }
}
