use core_domain::{AppError, AppResult, ConnectionProfile, DatabaseKind};
use driver_api::{ConnectionHandle, ConnectionProvider};
use driver_mongodb::MongoDbDriver;
use driver_mysql::MySqlDriver;
use driver_postgres::PostgresDriver;
use ssh_tunnel::SshTunnelManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{sleep, timeout, Duration};

const POOL_MAX_PER_KEY: usize = 5;
const PING_TIMEOUT_SECS: u64 = 3;

fn pool_key(profile_id: &str, kind: DatabaseKind, database: Option<&str>) -> String {
    match kind {
        DatabaseKind::MySql => format!("{profile_id}::mysql"),
        DatabaseKind::Postgres | DatabaseKind::MongoDb => match database {
            Some(db) => format!("{profile_id}::{db}"),
            None => format!("{profile_id}::__no_db__"),
        },
    }
}

type ConnHandle = Arc<AsyncMutex<ConnectionHandle>>;

pub struct ConnectionPool {
    entries: Arc<std::sync::Mutex<HashMap<String, Vec<ConnHandle>>>>,
    pub postgres: PostgresDriver,
    pub mysql: MySqlDriver,
    pub mongodb: MongoDbDriver,
    ssh_tunnel: SshTunnelManager,
    keepalive_secs: u64,
}

impl ConnectionPool {
    pub fn new(keepalive_secs: u64) -> Self {
        Self {
            entries: Arc::new(std::sync::Mutex::new(HashMap::new())),
            postgres: PostgresDriver,
            mysql: MySqlDriver,
            mongodb: MongoDbDriver,
            ssh_tunnel: SshTunnelManager,
            keepalive_secs,
        }
    }

    fn provider(&self, kind: DatabaseKind) -> &dyn ConnectionProvider {
        match kind {
            DatabaseKind::Postgres => &self.postgres,
            DatabaseKind::MySql => &self.mysql,
            DatabaseKind::MongoDb => &self.mongodb,
        }
    }

    /// 带 3 秒超时的 ping，避免 TCP 超时等待过长
    async fn timed_ping(&self, handle: &mut ConnectionHandle) -> bool {
        let provider: &dyn ConnectionProvider = if handle.is_postgres() {
            &self.postgres
        } else if handle.is_mongodb() {
            &self.mongodb
        } else {
            &self.mysql
        };
        matches!(
            timeout(Duration::from_secs(PING_TIMEOUT_SECS), provider.ping(handle)).await,
            Ok(Ok(()))
        )
    }

    /// 获取一个健康的连接。优先复用空闲连接，池未满则新建，池满则排队等待。
    pub async fn acquire(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: Option<&str>,
    ) -> AppResult<Arc<AsyncMutex<ConnectionHandle>>> {
        self.ssh_tunnel
            .validate(profile.ssh_tunnel.as_ref())
            .map_err(|e| AppError::Validation(e.to_string()))?;

        let key = pool_key(&profile.id, profile.kind, database);

        // 找一个空闲健康连接；一个 ping 失败则全部清掉（链路断开）
        let snapshot = self.snapshot(&key);
        for handle in &snapshot {
            if let Ok(mut guard) = handle.try_lock() {
                if self.timed_ping(&mut guard).await {
                    tracing::debug!(key = %key, "连接池命中，复用空闲连接");
                    return Ok(handle.clone());
                }
                drop(guard);
                tracing::info!(key = %key, "ping 失败，清空该 key 下所有连接并重建");
                self.clear_key(&key);
                break;
            }
        }

        // 建新连接
        tracing::info!(key = %key, host = %profile.host, port = profile.port, "连接池新建连接");
        let new = self.provider(profile.kind).connect(profile, password, database).await?;
        let handle = Arc::new(AsyncMutex::new(new));
        self.push(&key, handle.clone());
        Ok(handle)
    }

    // ── 内部操作 ──

    fn snapshot(&self, key: &str) -> Vec<ConnHandle> {
        self.entries
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    fn push(&self, key: &str, handle: ConnHandle) {
        self.entries
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_default()
            .push(handle);
    }

    fn remove(&self, key: &str, target: &ConnHandle) {
        let mut map = self.entries.lock().unwrap();
        if let Some(vec) = map.get_mut(key) {
            vec.retain(|h| !Arc::ptr_eq(h, target));
        }
    }

    fn clear_key(&self, key: &str) {
        self.entries.lock().unwrap().remove(key);
    }

    /// 驱逐某 connection 的所有缓存
    pub fn evict(&self, connection_id: &str) {
        let mut map = self.entries.lock().unwrap();
        let count: usize = map.values().map(|v| v.len()).sum();
        map.retain(|k, _| !k.starts_with(connection_id));
        let remaining: usize = map.values().map(|v| v.len()).sum();
        let removed = count - remaining;
        if removed > 0 {
            tracing::info!(connection_id, removed, "驱逐连接缓存");
        }
    }

    pub fn disconnect_all(&self) {
        self.entries.lock().unwrap().clear();
    }

    pub fn start_keepalive(self: &Arc<Self>) {
        if tokio::runtime::Handle::try_current().is_ok() {
            let pool = Arc::downgrade(self);
            let interval = self.keepalive_secs;
            tokio::spawn(async move {
                loop {
                    sleep(std::time::Duration::from_secs(interval)).await;
                    let Some(pool) = pool.upgrade() else { break };
                    pool.keepalive_pass().await;
                }
            });
        }
    }

    async fn keepalive_pass(&self) {
        let snapshot: Vec<_> = {
            self.entries
                .lock()
                .unwrap()
                .iter()
                .flat_map(|(k, v)| v.iter().map(|h| (k.clone(), h.clone())))
                .collect()
        };
        for (key, handle) in snapshot {
            if let Ok(mut guard) = handle.try_lock() {
                if self.timed_ping(&mut guard).await { continue; }
                drop(guard);
                tracing::info!(key = %key, "keepalive ping 失败，清理死连接");
                self.remove(&key, &handle);
            }
        }
    }
}
