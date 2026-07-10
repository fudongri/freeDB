use anyhow::Result;
use chrono::Utc;
use core_domain::ColumnDefinition;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::sync::Arc;

#[derive(Clone)]
pub struct MetadataCache {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct CachedEntry {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub table: String,
    pub is_view: bool,
    pub columns: Vec<ColumnDefinition>,
}

impl MetadataCache {
    /// 使用共享的 SQLite 连接初始化元数据缓存
    pub fn new(conn: Arc<Mutex<Connection>>) -> Result<Self> {
        let cache = Self { conn };
        cache.init()?;
        Ok(cache)
    }

    fn init(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata_cache (
                connection_id TEXT NOT NULL,
                database_name TEXT,
                schema_name   TEXT,
                table_name    TEXT NOT NULL,
                is_view       INTEGER NOT NULL DEFAULT 0,
                columns_json  TEXT,
                updated_at    TEXT NOT NULL,
                PRIMARY KEY (connection_id, database_name, schema_name, table_name)
            );",
        )?;
        Ok(())
    }

    pub fn load_for_connection(&self, connection_id: &str) -> Result<Vec<CachedEntry>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT database_name, schema_name, table_name, is_view, columns_json
             FROM metadata_cache
             WHERE connection_id = ?1",
        )?;
        let entries = stmt
            .query_map(params![connection_id], |row| {
                let columns_json: Option<String> = row.get(4)?;
                let columns = columns_json
                    .and_then(|json| serde_json::from_str(&json).ok())
                    .unwrap_or_default();
                Ok(CachedEntry {
                    database: row.get(0)?,
                    schema: row.get(1)?,
                    table: row.get(2)?,
                    is_view: row.get::<_, i64>(3)? != 0,
                    columns,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if !entries.is_empty() {
            tracing::debug!(
                connection_id = connection_id,
                count = entries.len(),
                "已从缓存加载元数据"
            );
        }
        Ok(entries)
    }

    pub fn save_for_connection(&self, connection_id: &str, entries: &[CachedEntry]) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM metadata_cache WHERE connection_id = ?1", params![connection_id])?;
        let now = Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "INSERT INTO metadata_cache (connection_id, database_name, schema_name, table_name, is_view, columns_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for entry in entries {
            let columns_json = if entry.columns.is_empty() {
                None
            } else {
                serde_json::to_string(&entry.columns).ok()
            };
            stmt.execute(params![
                connection_id,
                entry.database,
                entry.schema,
                entry.table,
                entry.is_view as i64,
                columns_json,
                now,
            ])?;
        }
        tracing::debug!(
            connection_id = connection_id,
            count = entries.len(),
            "已保存元数据缓存"
        );
        Ok(())
    }

    pub fn merge_for_connection(&self, connection_id: &str, entries: &[CachedEntry]) -> Result<()> {
        let conn = self.conn.lock();
        let now = Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "INSERT OR REPLACE INTO metadata_cache (connection_id, database_name, schema_name, table_name, is_view, columns_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for entry in entries {
            let columns_json = if entry.columns.is_empty() {
                None
            } else {
                serde_json::to_string(&entry.columns).ok()
            };
            stmt.execute(params![
                connection_id,
                entry.database,
                entry.schema,
                entry.table,
                entry.is_view as i64,
                columns_json,
                now,
            ])?;
        }
        Ok(())
    }

    pub fn clear_connection(&self, connection_id: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM metadata_cache WHERE connection_id = ?1", params![connection_id])?;
        Ok(())
    }
}
