use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseKind {
    MySql,
    Postgres,
    MongoDb,
}

impl DatabaseKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MySql => "mysql",
            Self::Postgres => "postgres",
            Self::MongoDb => "mongodb",
        }
    }

    pub fn default_charset(self) -> &'static str {
        match self {
            Self::MySql => "utf8mb4",
            Self::Postgres => "UTF8",
            Self::MongoDb => "",
        }
    }

    pub fn default_collation(self) -> &'static str {
        match self {
            Self::MySql => "utf8mb4_unicode_ci",
            Self::Postgres => "",
            Self::MongoDb => "",
        }
    }

    pub fn from_db_value(value: &str) -> Result<Self, AppError> {
        match value {
            "mysql" => Ok(Self::MySql),
            "postgres" => Ok(Self::Postgres),
            "mongodb" => Ok(Self::MongoDb),
            _ => Err(AppError::Validation(format!("unsupported database kind: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
}

impl SslMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
        }
    }

    pub fn from_db_value(value: &str) -> Result<Self, AppError> {
        match value {
            "disable" => Ok(Self::Disable),
            "prefer" => Ok(Self::Prefer),
            "require" => Ok(Self::Require),
            _ => Err(AppError::Validation(format!("unsupported ssl mode: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfileInput {
    pub name: String,
    pub kind: DatabaseKind,
    pub group_name: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub default_database: Option<String>,
    pub save_password: bool,
    pub ssl_mode: SslMode,
    pub direct_connection: bool,
    pub replica_set: Option<String>,
    pub connection_uri: Option<String>,
}

impl Default for ConnectionProfileInput {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: DatabaseKind::MySql,
            group_name: None,
            host: "127.0.0.1".into(),
            port: 3306,
            username: String::new(),
            password: None,
            default_database: None,
            save_password: true,
            ssl_mode: SslMode::Prefer,
            direct_connection: false,
            replica_set: None,
            connection_uri: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub kind: DatabaseKind,
    pub group_name: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub default_database: Option<String>,
    pub password_saved: bool,
    pub ssl_mode: SslMode,
    pub sort_order: i64,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub direct_connection: bool,
    pub replica_set: Option<String>,
    pub connection_uri: Option<String>,
}

impl ConnectionProfile {
    pub fn from_input(id: String, input: ConnectionProfileInput) -> Self {
        let now = Utc::now();
        Self {
            id,
            name: input.name,
            kind: input.kind,
            group_name: input.group_name,
            host: input.host,
            port: input.port,
            username: input.username,
            default_database: input.default_database,
            password_saved: input.save_password,
            ssl_mode: input.ssl_mode,
            sort_order: 0,
            last_used_at: None,
            created_at: now,
            updated_at: now,
            direct_connection: input.direct_connection,
            replica_set: input.replica_set,
            connection_uri: input.connection_uri,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Connected,
    Failed,
    Reconnecting,
}

#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub keepalive_interval_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 500,
            max_delay_ms: 5000,
            keepalive_interval_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExplorerNodeType {
    Connection,
    Database,
    Schema,
    Table,
    View,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerNode {
    pub id: String,
    pub connection_id: String,
    pub name: String,
    pub node_type: ExplorerNodeType,
    pub parent_id: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub expandable: bool,
    pub loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRef {
    pub connection_id: String,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub table: String,
    pub is_view: bool,
}

impl TableRef {
    pub fn label(&self) -> String {
        match (&self.schema, &self.database) {
            (Some(schema), _) => format!("{schema}.{}", self.table),
            (None, Some(database)) => format!("{database}.{}", self.table),
            _ => self.table.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub auto_increment: bool,
    pub default_value: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDefinition {
    pub columns: Vec<ColumnDefinition>,
    pub create_sql: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryExecution {
    pub connection_id: String,
    pub database: Option<String>,
    pub sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryCellValue {
    Null,
    Text(String),
}

impl QueryCellValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Null => None,
            Self::Text(value) => Some(value.as_str()),
        }
    }

    pub fn display_text(&self) -> &str {
        match self {
            Self::Null => "(NULL)",
            Self::Text(value) => value.as_str(),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_empty_text(&self) -> bool {
        matches!(self, Self::Text(value) if value.is_empty())
    }
}

impl Default for QueryCellValue {
    fn default() -> Self {
        Self::Null
    }
}

impl From<String> for QueryCellValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for QueryCellValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

/// MongoDB BSON 类型，用于在导出/复制时保留值的真实类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MongoValue {
    ObjectId,
    DateTime,
    Timestamp,
    Decimal128,
    Binary,
    RegularExpression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<BTreeMap<String, QueryCellValue>>,
    pub affected_rows: Option<u64>,
    pub elapsed_ms: u128,
    pub message: Option<String>,
    /// MongoDB 单元格的真实 BSON 类型，按 (行号, 列名) 索引。
    #[serde(default)]
    pub mongo_types: std::collections::HashMap<(usize, String), MongoValue>,
}

impl QueryResult {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: None,
            elapsed_ms: 0,
            message: Some(message.into()),
            mongo_types: std::collections::HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    pub id: String,
    pub connection_id: String,
    pub sql_text: String,
    pub executed_at: DateTime<Utc>,
}

impl QueryHistoryEntry {
    pub fn new(connection_id: impl Into<String>, sql_text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            connection_id: connection_id.into(),
            sql_text: sql_text.into(),
            executed_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQueryEntry {
    pub id: String,
    pub connection_id: String,
    pub database: Option<String>,
    pub title: String,
    pub sql_text: String,
    pub saved_at: DateTime<Utc>,
    pub sort_order: i32,
}

impl SavedQueryEntry {
    pub fn new(
        connection_id: impl Into<String>,
        database: Option<String>,
        title: impl Into<String>,
        sql_text: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            connection_id: connection_id.into(),
            database,
            title: title.into(),
            sql_text: sql_text.into(),
            saved_at: Utc::now(),
            sort_order: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCellChange {
    pub row_index: usize,
    pub column: String,
    pub new_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableChangeSet {
    pub table: TableRef,
    pub changes: Vec<PendingCellChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiStateValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation error: {0}")]
    Validation(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("query error: {0}")]
    Query(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
    #[error("not found: {0}")]
    NotFound(String),
}

impl AppError {
    /// 判断错误是否为临时性错误（适合重试，如连接断开、超时）
    pub fn is_transient(&self) -> bool {
        match self {
            AppError::Connection(msg) => {
                let lower = msg.to_ascii_lowercase();
                lower.contains("timeout")
                    || lower.contains("connection closed")
                    || lower.contains("connection refused")
                    || lower.contains("broken pipe")
                    || lower.contains("connection reset")
                    || lower.contains("tls")
                    || lower.contains("dns")
                    || lower.contains("server terminated")
                    || lower.contains("unexpectedly")
                    || lower.contains("lost connection")
            }
            _ => false,
        }
    }

    pub fn clone_error(&self) -> Self {
        match self {
            Self::Validation(s) => Self::Validation(s.clone()),
            Self::Connection(s) => Self::Connection(s.clone()),
            Self::Query(s) => Self::Query(s.clone()),
            Self::Storage(s) => Self::Storage(s.clone()),
            Self::Unsupported(s) => Self::Unsupported(s.clone()),
            Self::NotFound(s) => Self::NotFound(s.clone()),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_profile_from_input() {
        let profile = ConnectionProfile::from_input(
            "id-1".into(),
            ConnectionProfileInput {
                name: "local".into(),
                username: "root".into(),
                ..ConnectionProfileInput::default()
            },
        );

        assert_eq!(profile.name, "local");
        assert_eq!(profile.username, "root");
        assert!(profile.password_saved);
    }

    #[test]
    fn table_ref_label_prefers_schema() {
        let table = TableRef {
            connection_id: "c".into(),
            database: Some("db1".into()),
            schema: Some("public".into()),
            table: "users".into(),
            is_view: false,
        };

        assert_eq!(table.label(), "public.users");
    }
}
