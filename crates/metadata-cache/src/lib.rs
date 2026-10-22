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

    /// 内存数据库实例（测试/集成验证用，不落盘）
    pub fn in_memory() -> Result<Self> {
        Self::new(Arc::new(Mutex::new(Connection::open_in_memory()?)))
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
        // 保留 columns_json 为空的条目：它们只记录了表/视图名，是表名补全的数据来源。
        // 旧版本这里会删掉无列条目，导致重启后表名补全彻底失效。
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
        let mut conn = self.conn.lock();
        // 用事务包裹，避免上千行逐条提交造成长时间持锁、拖住 UI 线程。
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM metadata_cache WHERE connection_id = ?1", params![connection_id])?;
        let now = Utc::now().to_rfc3339();
        {
            let mut stmt = tx.prepare(
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
        }
        tx.commit()?;
        tracing::debug!(
            connection_id = connection_id,
            count = entries.len(),
            "已保存元数据缓存"
        );
        Ok(())
    }

    /// 只更新表清单（表名/视图标记），保留已缓存的列信息。
    ///
    /// 连接树加载完会拿到该连接的全部表，这一步不应把之前辛苦预取到的列缓存覆盖成空。
    /// 同时清理掉本次清单里已经不存在的“无列”旧条目（表被删除/改名）。
    pub fn save_table_list(&self, connection_id: &str, entries: &[CachedEntry]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        // 先清掉该连接所有「只有表名、没有列」的旧条目：这次拿到的表清单是最新全量，
        // 旧的无列条目里可能包含已删除/改名的表。带列信息的条目保留，避免覆盖预取成果。
        tx.execute(
            "DELETE FROM metadata_cache
             WHERE connection_id IS ?1
               AND columns_json IS NULL",
            params![connection_id],
        )?;
        {
            let mut update = tx.prepare(
                "UPDATE metadata_cache
                 SET is_view = ?5, updated_at = ?6
                 WHERE connection_id IS ?1
                   AND database_name IS ?2
                   AND schema_name IS ?3
                   AND table_name IS ?4",
            )?;
            let mut insert = tx.prepare(
                "INSERT INTO metadata_cache (connection_id, database_name, schema_name, table_name, is_view, columns_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
            )?;
            for entry in entries {
                let updated = update.execute(params![
                    connection_id,
                    entry.database,
                    entry.schema,
                    entry.table,
                    entry.is_view as i64,
                    now,
                ])?;
                if updated == 0 {
                    insert.execute(params![
                        connection_id,
                        entry.database,
                        entry.schema,
                        entry.table,
                        entry.is_view as i64,
                        now,
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn merge_for_connection(&self, connection_id: &str, entries: &[CachedEntry]) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        // SQLite 复合主键对 NULL 列不强制唯一（MySQL 连接 schema_name 为 NULL），
        // 因此 INSERT OR REPLACE 每次都会插入新行而非替换，导致缓存无限膨胀。
        // 改为先按 `IS` 匹配删除旧行再插入，`IS` 能正确匹配 NULL 值。
        {
            let mut del = tx.prepare(
                "DELETE FROM metadata_cache
                 WHERE connection_id IS ?1
                   AND database_name IS ?2
                   AND schema_name IS ?3
                   AND table_name IS ?4",
            )?;
            let mut ins = tx.prepare(
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
        }
        tx.commit()?;
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
    /// 重新构造 MetadataCache（等价于应用重启）时应自动清理为一个表。
    /// 无列条目（只记录表名）必须保留——它们是表名补全的数据来源。
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
        assert_eq!(loaded.len(), 1, "重复行去重后应保留一条表名缓存");
    }

    /// 有列缓存的行不受 Fix B 清理影响，去重逻辑仍应保留最新一条。
    #[test]
    fn init_dedups_rows_with_columns() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
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
            for i in 0..10 {
                c.execute(
                    "INSERT INTO metadata_cache (connection_id, database_name, schema_name, table_name, is_view, columns_json, updated_at)
                     VALUES ('conn-any', 'mydb', NULL, 'users', 0, '[{\"name\":\"id\"}]', ?1)",
                    [format!("2026-01-0{}T00:00:00+00:00", i % 9 + 1)],
                )
                .unwrap();
            }
        }
        let cache = MetadataCache::new(conn).unwrap();
        let loaded = cache.load_for_connection("conn-any").unwrap();
        assert_eq!(loaded.len(), 1, "有列重复行去重后应保留最新一条");
    }

    fn column(name: &str) -> ColumnDefinition {
        ColumnDefinition {
            name: name.to_string(),
            data_type: "text".into(),
            nullable: true,
            primary_key: false,
            unique: false,
            auto_increment: false,
            on_update_current_timestamp: false,
            default_value: None,
            comment: None,
        }
    }

    /// 连接树加载后只更新表清单，不能把已缓存的列覆盖成空。
    #[test]
    fn save_table_list_keeps_existing_columns() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let cache = MetadataCache::new(conn).unwrap();
        cache
            .merge_for_connection(
                "conn-1",
                &[CachedEntry {
                    database: Some("mydb".into()),
                    schema: None,
                    table: "users".into(),
                    is_view: false,
                    columns: vec![column("id"), column("name")],
                }],
            )
            .unwrap();

        // 表清单里 users 仍在，另新增 orders（另一张有列的表）
        cache
            .save_table_list(
                "conn-1",
                &[
                    CachedEntry {
                        database: Some("mydb".into()),
                        schema: None,
                        table: "users".into(),
                        is_view: false,
                        columns: vec![],
                    },
                    CachedEntry {
                        database: Some("mydb".into()),
                        schema: None,
                        table: "orders".into(),
                        is_view: false,
                        columns: vec![],
                    },
                ],
            )
            .unwrap();

        let loaded = cache.load_for_connection("conn-1").unwrap();
        assert_eq!(loaded.len(), 2, "两表都应存在: {loaded:?}");
        let users = loaded.iter().find(|e| e.table == "users").unwrap();
        assert_eq!(users.columns.len(), 2, "已有列缓存不能被表清单覆盖");
        let orders = loaded.iter().find(|e| e.table == "orders").unwrap();
        assert!(orders.columns.is_empty());
    }

    /// 表清单刷新时，已删除的「仅表名」条目应被清理，但带列缓存的行保留。
    #[test]
    fn save_table_list_prunes_stale_table_only_rows() {
        let conn = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let cache = MetadataCache::new(conn).unwrap();
        cache
            .save_table_list(
                "conn-1",
                &[CachedEntry {
                    database: Some("mydb".into()),
                    schema: None,
                    table: "dropped_table".into(),
                    is_view: false,
                    columns: vec![],
                }],
            )
            .unwrap();
        cache
            .merge_for_connection(
                "conn-1",
                &[CachedEntry {
                    database: Some("mydb".into()),
                    schema: None,
                    table: "users".into(),
                    is_view: false,
                    columns: vec![column("id")],
                }],
            )
            .unwrap();

        cache
            .save_table_list(
                "conn-1",
                &[CachedEntry {
                    database: Some("mydb".into()),
                    schema: None,
                    table: "users".into(),
                    is_view: false,
                    columns: vec![],
                }],
            )
            .unwrap();

        let loaded = cache.load_for_connection("conn-1").unwrap();
        assert!(loaded.iter().all(|e| e.table != "dropped_table"), "过期表名应清理");
        let users = loaded.iter().find(|e| e.table == "users").unwrap();
        assert_eq!(users.columns.len(), 1, "带列缓存的行必须保留");
    }

    /// 集成场景：连接 → 写入表清单与列 → 重启（重新打开同一个 SQLite 文件）。
    /// 重启后表名（无列条目）与列缓存都必须还在，补全才有数据可用。
    #[test]
    fn persists_tables_and_columns_across_restart() {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(format!("freedb-metadata-test-{unique}.sqlite"));

        {
            let conn = Arc::new(Mutex::new(Connection::open(&path).unwrap()));
            let cache = MetadataCache::new(conn.clone()).unwrap();
            cache
                .save_table_list(
                    "conn-1",
                    &[
                        CachedEntry {
                            database: Some("shop".into()),
                            schema: None,
                            table: "users".into(),
                            is_view: false,
                            columns: vec![],
                        },
                        CachedEntry {
                            database: Some("shop".into()),
                            schema: None,
                            table: "orders".into(),
                            is_view: false,
                            columns: vec![],
                        },
                    ],
                )
                .unwrap();
            cache
                .merge_for_connection(
                    "conn-1",
                    &[CachedEntry {
                        database: Some("shop".into()),
                        schema: None,
                        table: "orders".into(),
                        is_view: false,
                        columns: vec![column("id"), column("total")],
                    }],
                )
                .unwrap();
        }

        // 模拟应用重启：新连接 + 新的 MetadataCache（会跑 init 自愈）
        {
            let conn = Arc::new(Mutex::new(Connection::open(&path).unwrap()));
            let cache = MetadataCache::new(conn).unwrap();
            let loaded = cache.load_for_connection("conn-1").unwrap();
            assert_eq!(loaded.len(), 2, "重启后表名缓存应保留: {loaded:?}");
            let orders = loaded.iter().find(|e| e.table == "orders").unwrap();
            assert_eq!(orders.columns.len(), 2, "重启后列缓存应保留");
            let users = loaded.iter().find(|e| e.table == "users").unwrap();
            assert!(users.columns.is_empty(), "无列的表名条目也必须保留");
        }

        let _ = std::fs::remove_file(&path);
    }
}
