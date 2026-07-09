use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use core_domain::{ConnectionProfile, DatabaseKind, SslMode, UiStateValue};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct ConnectionStore {
    connection: Arc<Mutex<Connection>>,
}

impl ConnectionStore {
    pub fn new() -> Result<Self> {
        let path = database_path()?;
        let connection = Connection::open(path)?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.init()?;
        Ok(store)
    }

    pub fn list_connections(&self) -> Result<Vec<ConnectionProfile>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, name, kind, group_name, host, port, username, default_database,
                    ssl_mode, sort_order, last_used_at, created_at, updated_at,
                    direct_connection, replica_set, connection_uri
             FROM connection_profiles
             ORDER BY sort_order ASC, name ASC",
        )?;
        let rows = statement.query_map([], map_profile)?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to list connections")
    }

    pub fn save_connection(&self, profile: &ConnectionProfile) -> Result<()> {
        let connection = self.connection.lock();
        let max_order: i64 = connection
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) FROM connection_profiles",
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1);
        let sort_order = max_order + 1;
        connection.execute(
            "INSERT INTO connection_profiles (
                id, name, kind, group_name, host, port, username, default_database,
                ssl_mode, sort_order, last_used_at, created_at, updated_at,
                direct_connection, replica_set, connection_uri
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                profile.id,
                profile.name,
                profile.kind.as_str(),
                profile.group_name,
                profile.host,
                i64::from(profile.port),
                profile.username,
                profile.default_database,
                profile.ssl_mode.as_str(),
                sort_order,
                profile.last_used_at.map(|item| item.to_rfc3339()),
                profile.created_at.to_rfc3339(),
                profile.updated_at.to_rfc3339(),
                profile.direct_connection,
                profile.replica_set,
                profile.connection_uri,
            ],
        )?;
        Ok(())
    }

    pub fn update_connection(&self, profile: &ConnectionProfile) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE connection_profiles
             SET name = ?2, kind = ?3, group_name = ?4, host = ?5, port = ?6,
                 username = ?7, default_database = ?8, ssl_mode = ?9,
                 last_used_at = ?10, updated_at = ?11,
                 direct_connection = ?12, replica_set = ?13, connection_uri = ?14
             WHERE id = ?1",
            params![
                profile.id,
                profile.name,
                profile.kind.as_str(),
                profile.group_name,
                profile.host,
                i64::from(profile.port),
                profile.username,
                profile.default_database,
                profile.ssl_mode.as_str(),
                profile.last_used_at.map(|item| item.to_rfc3339()),
                profile.updated_at.to_rfc3339(),
                profile.direct_connection,
                profile.replica_set,
                profile.connection_uri,
            ],
        )?;
        Ok(())
    }

    pub fn delete_connection(&self, connection_id: &str) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "DELETE FROM connection_profiles WHERE id = ?1",
            params![connection_id],
        )?;
        Ok(())
    }

    pub fn get_connection(&self, connection_id: &str) -> Result<Option<ConnectionProfile>> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT id, name, kind, group_name, host, port, username, default_database,
                        ssl_mode, sort_order, last_used_at, created_at, updated_at,
                        direct_connection, replica_set, connection_uri
                 FROM connection_profiles WHERE id = ?1",
                params![connection_id],
                map_profile,
            )
            .optional()
            .context("failed to query connection")
    }

    pub fn set_last_used_at(&self, connection_id: &str, used_at: DateTime<Utc>) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE connection_profiles SET last_used_at = ?2, updated_at = ?3 WHERE id = ?1",
            params![connection_id, used_at.to_rfc3339(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn save_ui_state(&self, value: UiStateValue) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "INSERT INTO ui_state (id, key, value, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![
                format!("ui-{}", value.key),
                value.key,
                value.value,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_ui_state(&self, key: &str) -> Result<Option<String>> {
        let connection = self.connection.lock();
        connection
            .query_row("SELECT value FROM ui_state WHERE key = ?1", params![key], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .context("failed to load ui state")
    }

    fn init(&self) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS connection_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                kind TEXT NOT NULL CHECK (kind IN ('mysql', 'postgres', 'mongodb')),
                group_name TEXT,
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                username TEXT NOT NULL,
                default_database TEXT,
                connect_timeout_secs INTEGER NOT NULL DEFAULT 5,
                ssl_mode TEXT NOT NULL DEFAULT 'prefer',
                sort_order INTEGER NOT NULL DEFAULT 0,
                last_used_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                direct_connection INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS ui_state (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        // 迁移：已有数据库添加 sort_order 列
        let _ = connection.execute(
            "ALTER TABLE connection_profiles ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // 迁移：已有数据库添加 direct_connection 列
        let _ = connection.execute(
            "ALTER TABLE connection_profiles ADD COLUMN direct_connection INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // 迁移：已有数据库添加 replica_set / connection_uri 列
        let _ = connection.execute(
            "ALTER TABLE connection_profiles ADD COLUMN replica_set TEXT",
            [],
        );
        let _ = connection.execute(
            "ALTER TABLE connection_profiles ADD COLUMN connection_uri TEXT",
            [],
        );
        // 迁移：更新 kind CHECK 约束以包含 'mongodb'
        let needs_check_migration = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='connection_profiles'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default();
        if !needs_check_migration.contains("'mongodb'") {
            let _ = connection.execute_batch(
                "BEGIN;
                 CREATE TABLE _connection_profiles_new (
                     id TEXT PRIMARY KEY, name TEXT NOT NULL,
                     kind TEXT NOT NULL CHECK (kind IN ('mysql', 'postgres', 'mongodb')),
                     group_name TEXT, host TEXT NOT NULL, port INTEGER NOT NULL,
                     username TEXT NOT NULL, default_database TEXT,
                     connect_timeout_secs INTEGER NOT NULL DEFAULT 5,
                     ssl_mode TEXT NOT NULL DEFAULT 'prefer',
                     ssh_tunnel_json TEXT NOT NULL DEFAULT 'null',
                     sort_order INTEGER NOT NULL DEFAULT 0,
                     last_used_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                     direct_connection INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT INTO _connection_profiles_new
                     (id, name, kind, group_name, host, port, username, default_database,
                      connect_timeout_secs, ssl_mode, sort_order,
                      last_used_at, created_at, updated_at)
                 SELECT id, name, kind, group_name, host, port, username, default_database,
                        connect_timeout_secs, ssl_mode, sort_order,
                        last_used_at, created_at, updated_at
                 FROM connection_profiles;
                 DROP TABLE connection_profiles;
                 ALTER TABLE _connection_profiles_new RENAME TO connection_profiles;
                 COMMIT;",
            );
        }
        Ok(())
    }

    pub fn update_sort_orders(&self, orders: &[(String, i64)]) -> Result<()> {
        let connection = self.connection.lock();
        for (id, order) in orders {
            connection.execute(
                "UPDATE connection_profiles SET sort_order = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, order, Utc::now().to_rfc3339()],
            )?;
        }
        Ok(())
    }
}

/// 返回主数据库路径，供 SecureStore 复用
pub fn database_path() -> Result<PathBuf> {
    let primary_dir = primary_data_dir()?;
    let target_path = primary_dir.join("freedb.sqlite3");
    migrate_legacy_database_if_needed(&target_path)?;
    Ok(target_path)
}

fn primary_data_dir() -> Result<PathBuf> {
    for dir in candidate_data_dirs() {
        if ensure_dir_writable(&dir).is_ok() {
            return Ok(dir);
        }
    }

    Err(anyhow!("unable to create application data directory"))
}

fn candidate_data_dirs() -> Vec<PathBuf> {
    [
        dirs::data_local_dir().map(|path| path.join("freedb")),
        std::env::current_dir().ok().map(|path| path.join(".freedb-data")),
        dirs::home_dir().map(|path| path.join(".freedb")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn legacy_data_dirs(target_path: &PathBuf) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = candidate_data_dirs()
        .into_iter()
        .filter(|dir| dir.join("freedb.sqlite3") != *target_path)
        .collect();

    // 从旧 uudb 目录迁移
    let uudb_dirs: Vec<PathBuf> = [
        dirs::data_local_dir().map(|path| path.join("uudb")),
        std::env::current_dir().ok().map(|path| path.join(".uudb-data")),
        dirs::home_dir().map(|path| path.join(".uudb")),
    ]
    .into_iter()
    .flatten()
    .collect();
    dirs.extend(uudb_dirs);

    dirs
}

fn migrate_legacy_database_if_needed(target_path: &PathBuf) -> Result<()> {
    if current_connection_count(target_path)? > 0 {
        return Ok(());
    }

    for legacy_dir in legacy_data_dirs(target_path) {
        let legacy_path = legacy_dir.join("freedb.sqlite3");
        if legacy_path == *target_path || !legacy_path.exists() {
            continue;
        }
        if current_connection_count(&legacy_path)? == 0 {
            continue;
        }

        fs::copy(&legacy_path, target_path)?;
        return Ok(());
    }

    Ok(())
}

fn ensure_dir_writable(dir: &PathBuf) -> Result<()> {
    fs::create_dir_all(dir)?;
    let probe_path = dir.join(".write-test");
    fs::write(&probe_path, b"ok")?;
    fs::remove_file(probe_path)?;
    Ok(())
}

fn current_connection_count(path: &PathBuf) -> Result<i64> {
    if !path.exists() {
        return Ok(0);
    }

    let connection = Connection::open(path)?;
    let table_exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'connection_profiles' LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some();

    if !table_exists {
        return Ok(0);
    }

    connection
        .query_row("SELECT COUNT(*) FROM connection_profiles", [], |row| row.get(0))
        .context("failed to count stored connections")
}

fn map_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionProfile> {
    let kind = DatabaseKind::from_db_value(&row.get::<_, String>(2)?).map_err(to_sql_error)?;
    let ssl_mode = SslMode::from_db_value(&row.get::<_, String>(8)?).map_err(to_sql_error)?;
    let sort_order = row.get::<_, i64>(9)?;
    let last_used_at = row
        .get::<_, Option<String>>(10)?
        .map(|value| parse_datetime(&value))
        .transpose()
        .map_err(to_sql_error)?;
    let created_at = parse_datetime(&row.get::<_, String>(11)?).map_err(to_sql_error)?;
    let updated_at = parse_datetime(&row.get::<_, String>(12)?).map_err(to_sql_error)?;
    let direct_connection: i64 = row.get(13)?;
    let replica_set: Option<String> = row.get(14)?;
    let connection_uri: Option<String> = row.get(15)?;

    Ok(ConnectionProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        kind,
        group_name: row.get(3)?,
        host: row.get(4)?,
        port: row.get::<_, u16>(5)?,
        username: row.get(6)?,
        default_database: row.get(7)?,
        password_saved: false,
        ssl_mode,
        sort_order,
        last_used_at,
        created_at,
        updated_at,
        direct_connection: direct_connection != 0,
        replica_set,
        connection_uri,
    })
}

fn parse_datetime(value: &str) -> std::result::Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc))
}

fn to_sql_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(error),
    )
}
