use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::{fs, path::PathBuf, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub id: String,
    pub connection_id: String,
    pub sql_text: String,
    pub executed_at: DateTime<Utc>,
    pub elapsed_ms: u128,
    pub success: bool,
}

#[derive(Clone, Debug)]
pub struct SavedQueryRecord {
    pub id: String,
    pub connection_id: String,
    pub database: Option<String>,
    pub title: String,
    pub sql_text: String,
    pub saved_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct HistoryStore {
    connection: Arc<Mutex<Connection>>,
}

impl HistoryStore {
    pub fn new() -> Result<Self> {
        let path = database_path()?;
        let connection = Connection::open(path)?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.init()?;
        Ok(store)
    }

    pub fn append(&self, connection_id: &str, sql_text: &str, elapsed_ms: u128, success: bool) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "INSERT INTO query_history (id, connection_id, sql_text, executed_at, elapsed_ms, success)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![Uuid::new_v4().to_string(), connection_id, sql_text, Utc::now().to_rfc3339(), elapsed_ms as i64, success as i64],
        )?;
        Ok(())
    }

    pub fn list_by_connection(&self, connection_id: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, connection_id, sql_text, executed_at, elapsed_ms, success
             FROM query_history
             WHERE connection_id = ?1
             ORDER BY executed_at DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![connection_id, limit as i64], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                connection_id: row.get(1)?,
                sql_text: row.get(2)?,
                executed_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                    .map_err(to_sql_error)?
                    .with_timezone(&Utc),
                elapsed_ms: row.get::<_, i64>(4).unwrap_or(0) as u128,
                success: row.get::<_, i64>(5).unwrap_or(1) != 0,
            })
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn clear_by_connection(&self, connection_id: &str) -> Result<usize> {
        let connection = self.connection.lock();
        let deleted = connection.execute(
            "DELETE FROM query_history WHERE connection_id = ?1",
            params![connection_id],
        )?;
        Ok(deleted)
    }

    pub fn save_query(
        &self,
        connection_id: &str,
        database: Option<&str>,
        title: &str,
        sql_text: &str,
    ) -> Result<SavedQueryRecord> {
        let connection = self.connection.lock();
        let existing = connection
            .query_row(
                "SELECT id FROM saved_queries WHERE connection_id = ?1 AND sql_text = ?2",
                params![connection_id, sql_text],
                |row| row.get::<_, String>(0),
            )
            .ok();
        let saved_at = Utc::now();
        let id = existing.unwrap_or_else(|| Uuid::new_v4().to_string());
        connection.execute(
            "INSERT INTO saved_queries (id, connection_id, database, title, sql_text, saved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                database = excluded.database,
                title = excluded.title,
                sql_text = excluded.sql_text,
                saved_at = excluded.saved_at",
            params![id, connection_id, database, title, sql_text, saved_at.to_rfc3339()],
        )?;
        Ok(SavedQueryRecord {
            id,
            connection_id: connection_id.to_string(),
            database: database.map(String::from),
            title: title.to_string(),
            sql_text: sql_text.to_string(),
            saved_at,
        })
    }

    pub fn rename_saved_query(&self, id: &str, title: &str) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE saved_queries
             SET title = ?2
             WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    pub fn update_saved_query(&self, id: &str, sql_text: &str, connection_id: &str, database: Option<&str>) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE saved_queries
             SET sql_text = ?2, connection_id = ?3, database = ?4
             WHERE id = ?1",
            params![id, sql_text, connection_id, database],
        )?;
        Ok(())
    }

    pub fn delete_saved_query(&self, id: &str) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute("DELETE FROM saved_queries WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_saved_queries(
        &self,
        connection_id: &str,
        limit: usize,
    ) -> Result<Vec<SavedQueryRecord>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, connection_id, database, title, sql_text, saved_at
             FROM saved_queries
             WHERE connection_id = ?1
             ORDER BY saved_at DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![connection_id, limit as i64], |row| {
            Ok(SavedQueryRecord {
                id: row.get(0)?,
                connection_id: row.get(1)?,
                database: row.get(2)?,
                title: row.get(3)?,
                sql_text: row.get(4)?,
                saved_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .map_err(to_sql_error)?
                    .with_timezone(&Utc),
            })
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_all_saved_queries(&self, limit: usize) -> Result<Vec<SavedQueryRecord>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, connection_id, database, title, sql_text, saved_at
             FROM saved_queries
             ORDER BY saved_at DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok(SavedQueryRecord {
                id: row.get(0)?,
                connection_id: row.get(1)?,
                database: row.get(2)?,
                title: row.get(3)?,
                sql_text: row.get(4)?,
                saved_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .map_err(to_sql_error)?
                    .with_timezone(&Utc),
            })
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn init(&self) -> Result<()> {
        let connection = self.connection.lock();
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS query_history (
                id TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                sql_text TEXT NOT NULL,
                executed_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_query_history_connection_id
                ON query_history(connection_id, executed_at DESC);
            CREATE TABLE IF NOT EXISTS saved_queries (
                id TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                database TEXT,
                title TEXT NOT NULL,
                sql_text TEXT NOT NULL,
                saved_at TEXT NOT NULL,
                UNIQUE(connection_id, sql_text)
            );
            CREATE INDEX IF NOT EXISTS idx_saved_queries_connection_id
                ON saved_queries(connection_id, saved_at DESC);",
        )?;

        // Migrate: add database column if it doesn't exist (for databases created before v2)
        let has_database_column: bool = connection
            .prepare("SELECT database FROM saved_queries LIMIT 0")
            .is_ok();
        if !has_database_column {
            connection.execute_batch(
                "ALTER TABLE saved_queries ADD COLUMN database TEXT;",
            )?;
        }

        // Migrate: add elapsed_ms column to query_history if missing
        let has_elapsed_column: bool = connection
            .prepare("SELECT elapsed_ms FROM query_history LIMIT 0")
            .is_ok();
        if !has_elapsed_column {
            connection.execute_batch(
                "ALTER TABLE query_history ADD COLUMN elapsed_ms INTEGER NOT NULL DEFAULT 0;",
            )?;
        }

        // Migrate: add success column to query_history if missing
        let has_success_column: bool = connection
            .prepare("SELECT success FROM query_history LIMIT 0")
            .is_ok();
        if !has_success_column {
            connection.execute_batch(
                "ALTER TABLE query_history ADD COLUMN success INTEGER NOT NULL DEFAULT 1;",
            )?;
        }

        Ok(())
    }
}

fn database_path() -> Result<PathBuf> {
    for dir in candidate_data_dirs() {
        if ensure_dir_writable(&dir).is_ok() {
            return Ok(dir.join("freedb.sqlite3"));
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

fn ensure_dir_writable(dir: &PathBuf) -> Result<()> {
    fs::create_dir_all(dir)?;
    let probe_path = dir.join(".write-test");
    fs::write(&probe_path, b"ok")?;
    fs::remove_file(probe_path)?;
    Ok(())
}

fn to_sql_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(error),
    )
}
