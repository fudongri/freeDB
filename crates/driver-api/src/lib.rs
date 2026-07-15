use async_trait::async_trait;
use core_domain::{AppResult, ConnectionProfile};

/// 持久化连接句柄——被连接池缓存
pub enum ConnectionHandle {
    Postgres {
        client: tokio_postgres::Client,
        connection: tokio::task::JoinHandle<()>,
    },
    MySql {
        conn: mysql_async::Conn,
    },
    MongoDb {
        client: mongodb::Client,
        database: Option<String>,
    },
}

impl ConnectionHandle {
    pub fn is_postgres(&self) -> bool {
        matches!(self, Self::Postgres { .. })
    }

    pub fn is_mongodb(&self) -> bool {
        matches!(self, Self::MongoDb { .. })
    }
}

/// 连接池需要的基础操作
#[async_trait]
pub trait ConnectionProvider: Send + Sync {
    async fn connect(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: Option<&str>,
    ) -> AppResult<ConnectionHandle>;

    async fn ping(&self, handle: &mut ConnectionHandle) -> AppResult<()>;
}

/// 表/集合的汇总统计信息
#[derive(Clone, Debug)]
pub struct TableSummary {
    pub name: String,
    pub table_type: String,
    pub row_count: Option<i64>,
    pub total_size: Option<i64>,
    pub data_size: Option<i64>,
    pub index_size: Option<i64>,
    pub engine: Option<String>,
    pub collation: Option<String>,
    pub primary_keys: Vec<String>,
    pub comment: Option<String>,
    pub create_time: Option<String>,
}

/// 数据库操作 trait —— 所有方法接收池化的 `&mut ConnectionHandle`，
/// 不再自行建立连接。
#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// 一次性测试连接（不走连接池）
    async fn test_connection(
        &self,
        profile: &ConnectionProfile,
        password: &str,
    ) -> AppResult<()>;

    async fn list_roots(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
    ) -> AppResult<Vec<core_domain::ExplorerNode>>;

    async fn list_children(
        &self,
        handle: &mut ConnectionHandle,
        connection_id: &str,
        parent: &core_domain::ExplorerNode,
    ) -> AppResult<Vec<core_domain::ExplorerNode>>;

    async fn load_table_definition(
        &self,
        handle: &mut ConnectionHandle,
        table: &core_domain::TableRef,
    ) -> AppResult<core_domain::TableDefinition>;

    async fn preview_table(
        &self,
        handle: &mut ConnectionHandle,
        table: &core_domain::TableRef,
        limit: u32,
    ) -> AppResult<core_domain::QueryResult>;

    async fn execute_sql(
        &self,
        handle: &mut ConnectionHandle,
        execution: core_domain::QueryExecution,
    ) -> AppResult<core_domain::QueryResult>;

    async fn apply_table_changes(
        &self,
        handle: &mut ConnectionHandle,
        changes: core_domain::TableChangeSet,
    ) -> AppResult<core_domain::QueryResult>;

    // ── DDL ──

    async fn create_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
        charset: Option<&str>,
        collation: Option<&str>,
    ) -> AppResult<()>;

    async fn rename_database(
        &self,
        handle: &mut ConnectionHandle,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()>;

    async fn drop_database(
        &self,
        handle: &mut ConnectionHandle,
        name: &str,
    ) -> AppResult<()>;

    async fn create_schema(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        name: &str,
    ) -> AppResult<()>;

    async fn rename_schema(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()>;

    async fn drop_schema(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        name: &str,
    ) -> AppResult<()>;

    async fn rename_table(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        schema: Option<&str>,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<()>;

    async fn dump_table_all_data(
        &self,
        handle: &mut ConnectionHandle,
        table: &core_domain::TableRef,
    ) -> AppResult<core_domain::QueryResult>;

    async fn load_tables_summary(
        &self,
        handle: &mut ConnectionHandle,
        database: &str,
        schema: Option<&str>,
    ) -> AppResult<Vec<TableSummary>>;

    /// 懒加载单个表/集合的统计信息（行数、大小）。默认不支持。
    async fn load_collection_stats(
        &self,
        _handle: &mut ConnectionHandle,
        _database: &str,
        _collection: &str,
    ) -> AppResult<Option<(Option<i64>, Option<i64>, Option<i64>, Option<i64>)>> {
        Ok(None)
    }
}
