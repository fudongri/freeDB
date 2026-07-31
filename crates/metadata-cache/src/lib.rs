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
        // 自愈：SQLite 复合主键对 NULL 列不强制唯一，旧版本在 MySQL 连接
        // （schema_name 为 NULL）下用 INSERT OR REPLACE 会不断累积重复行，
        // 导致打开连接时主线程同步加载元数据而卡死。启动时对全部连接去重，
        // 每组保留有列信息或最新的一条。此逻辑对所有连接生效，与连接名无关。
        let removed = conn.execute(
            "DELETE FROM metadata_cache
             WHERE rowid IN (
                 SELECT rowid FROM (
                     SELECT rowid,
                            ROW_NUMBER() OVER (
                                PARTITION BY connection_id,
                                             IFNULL(database_name,''),
                                             IFNULL(schema_name,''),
                                             table_name
                                ORDER BY CASE WHEN columns_json IS NOT NULL THEN 0 ELSE 1 END,
                                         updated_at DESC
                            ) AS rn
                     FROM metadata_cache
                 ) WHERE rn > 1
             )",
            [],
        )?;
        if removed > 0 {
            tracing::info!(removed = removed, "已清理元数据缓存中的重复行");
        }
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
        // SQLite 复合主键对 NULL 列不强制唯一（MySQL 连接 schema_name 为 NULL），
        // 因此 INSERT OR REPLACE 每次都会插入新行而非替换，导致缓存无限膨胀。
        // 改为先按 `IS` 匹配删除旧行再插入，`IS` 能正确匹配 NULL 值。
        let mut del = conn.prepare(
            "DELETE FROM metadata_cache
             WHERE connection_id IS ?1
               AND database_name IS ?2
               AND schema_name IS ?3
               AND table_name IS ?4",
        )?;
        let mut ins = conn.prepare(
            "INSERT INTO metadata_cache (connection_id, database_name, schema_name, table_name, is_view, columns_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for entry in entries {
            let columns_json = if entry.columns.is_empty() {
                None
            } else {
                serde_json::to_string(&entry.columns).ok()
            };
            del.execute(params![
                connection_id,
                entry.database,
                entry.schema,
                entry.table,
            ])?;
            ins.execute(params![
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_dedups_null_schema_entries() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let cache = MetadataCache::new(conn).unwrap();
        let entry = CachedEntry {
            database: Some("mydb".to_string()),
            schema: None,
            table: "users".to_string(),
            is_view: false,
            columns: vec![],
        };
        // 多次 merge 同一 MySQL 表（schema 为 NULL），不应累积重复行
        for _ in 0..10 {
            cache.merge_for_connection("conn-1", &[entry.clone()]).unwrap();
        }
        let loaded = cache.load_for_connection("conn-1").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].table, "users");
    }

    #[test]
    fn merge_replaces_existing_non_null_schema() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let cache = MetadataCache::new(conn).unwrap();
        let entry = CachedEntry {
            database: Some("mydb".to_string()),
            schema: Some("public".to_string()),
            table: "users".to_string(),
            is_view: false,
            columns: vec![],
        };
        cache.merge_for_connection("conn-1", &[entry.clone()]).unwrap();
        cache.merge_for_connection("conn-1", &[entry.clone()]).unwrap();
        let loaded = cache.load_for_connection("conn-1").unwrap();
        assert_eq!(loaded.len(), 1);
    }

    /// 模拟老用户升级：旧版本已累积大量重复行（schema 为 NULL 的 MySQL 连接），
    /// 重新构造 MetadataCache（等价于应用重启）时应自动去重，无需依赖连接名。
    #[test]
    fn init_self_heals_legacy_duplicates_for_any_connection() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        // 手动建表后，直接插入 100 条旧版本累积的重复行（模拟升级前状态）
        {
            let c = conn.lock();
            c.execute_batch(
                "CREATE TABLE metadata_cache (
                    connection_id TEXT NOT NULL,
                    database_name TEXT,
                    schema_name   TEXT,
                    table_name    TEXT NOT NULL,
                    is_view       INTEGER NOT NULL DEFAULT 0,
                    columns_json  TEXT,
                    updated_at    TEXT NOT NULL,
                    PRIMARY KEY (connection_id, database_name, schema_name, table_name)
                );",
            )
            .unwrap();
            for _ in 0..100 {
                c.execute(
                    "INSERT INTO metadata_cache (connection_id, database_name, schema_name, table_name, is_view, columns_json, updated_at)
                     VALUES ('conn-any', 'mydb', NULL, 'users', 0, NULL, '2026-01-01T00:00:00+00:00')",
                    [],
                )
                .unwrap();
            }
        }
        // 构造 MetadataCache 触发 init 自愈
        let cache = MetadataCache::new(conn).unwrap();
        let loaded = cache.load_for_connection("conn-any").unwrap();
        assert_eq!(loaded.len(), 1, "启动时应自动清理历史重复行");
        assert_eq!(loaded[0].table, "users");
    }
}
