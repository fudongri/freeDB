use core_domain::{AppResult, ConnectionProfile, DatabaseKind};
use driver_api::{ConnectionHandle, ConnectionProvider};
use driver_mongodb::MongoDbDriver;
use driver_mysql::MySqlDriver;
use driver_postgres::PostgresDriver;
use driver_sqlite::SqliteDriver;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{sleep, timeout, Duration};

const POOL_MAX_PER_KEY: usize = 5;
const PING_TIMEOUT_SECS: u64 = 3;
/// 空闲超过该时长（毫秒）的连接在复用前先验活。
/// 合盖休眠、切换网络（公司 Wi-Fi ↔ 家庭网络）后，池里的 TCP 连接会变成半开死连接，
/// 此时写进 socket 不会有任何报错，读则永远等不到回包。复用前 ping 一次即可发现并重建。
const IDLE_REVERIFY_MS: u64 = 10_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn pool_key(profile_id: &str, kind: DatabaseKind, database: Option<&str>) -> String {
    match kind {
        DatabaseKind::MySql => format!("{profile_id}::mysql"),
        DatabaseKind::Postgres | DatabaseKind::MongoDb => match database {
            Some(db) => format!("{profile_id}::{db}"),
            None => format!("{profile_id}::__no_db__"),
        },
        DatabaseKind::Sqlite => match database {
            Some(db) => format!("{profile_id}::sqlite:{db}"),
            None => format!("{profile_id}::sqlite:__no_db__"),
        },
    }
}

type ConnHandle = Arc<AsyncMutex<ConnectionHandle>>;

/// 池中一条连接：句柄 + 最近一次确认可用的时间戳（毫秒）。
/// 使用墙钟而非 `Instant`，因为 macOS/Linux 的单调时钟在系统休眠期间不推进，
/// 睡一夜后算出的空闲时长会偏小，正好漏掉需要验活的场景。
struct PooledConn {
    handle: ConnHandle,
    verified_at_ms: AtomicU64,
}

impl PooledConn {
    fn new(handle: ConnHandle) -> Self {
        Self { handle, verified_at_ms: AtomicU64::new(now_ms()) }
    }

    /// 已确认可用的最后时刻，验活成功后调用
    fn touch(&self) {
        self.verified_at_ms.store(now_ms(), Ordering::Relaxed);
    }

    fn idle_ms(&self) -> u64 {
        now_ms().saturating_sub(self.verified_at_ms.load(Ordering::Relaxed))
    }
}

pub struct ConnectionPool {
    entries: Arc<std::sync::Mutex<HashMap<String, Vec<Arc<PooledConn>>>>>,
    pub postgres: PostgresDriver,
    pub mysql: MySqlDriver,
    pub mongodb: MongoDbDriver,
    pub sqlite: SqliteDriver,
    keepalive_secs: u64,
}

impl ConnectionPool {
    pub fn new(keepalive_secs: u64) -> Self {
        Self {
            entries: Arc::new(std::sync::Mutex::new(HashMap::new())),
            postgres: PostgresDriver,
            mysql: MySqlDriver,
            mongodb: MongoDbDriver,
            sqlite: SqliteDriver,
            keepalive_secs,
        }
    }

    fn provider(&self, kind: DatabaseKind) -> &dyn ConnectionProvider {
        match kind {
            DatabaseKind::Postgres => &self.postgres,
            DatabaseKind::MySql => &self.mysql,
            DatabaseKind::MongoDb => &self.mongodb,
            DatabaseKind::Sqlite => &self.sqlite,
        }
    }

    /// 带 3 秒超时的 ping，避免 TCP 超时等待过长
    async fn timed_ping(&self, handle: &mut ConnectionHandle) -> bool {
        let provider: &dyn ConnectionProvider = if handle.is_postgres() {
            &self.postgres
        } else if handle.is_mongodb() {
            &self.mongodb
        } else if handle.is_sqlite() {
            &self.sqlite
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
        // PostgreSQL / MongoDB 连接绑定到固定数据库，忽略调用方传入的 database 参数，
        // 始终使用连接配置中的 default_database。Hologres 等代理下 pg_database
        // 列出的逻辑库不能单独作为 dbname 连接，必须连配置的默认库；MongoDB 同理，
        // 建连接时只用表单填的「验证数据库」。
        let effective_db = if matches!(profile.kind, DatabaseKind::Postgres | DatabaseKind::MongoDb) {
            profile.default_database.as_deref()
        } else {
            database
        };
        let key = pool_key(&profile.id, profile.kind, effective_db);

        let mut backoff_ms = 50u64;
        loop {
            // 复用空闲连接。频繁使用的连接不做逐次 ping；只有久未确认的连接才先验活，
            // 避免写进休眠/换网后已失效的 socket 导致查询永久挂起。
            let snapshot = self.snapshot(&key);
            let mut dropped_dead = false;
            for entry in &snapshot {
                let Ok(mut guard) = entry.handle.try_lock() else { continue };
                if entry.idle_ms() > IDLE_REVERIFY_MS && !self.timed_ping(&mut guard).await {
                    drop(guard);
                    tracing::info!(key = %key, "空闲连接已失效，丢弃后重建");
                    self.remove(&key, entry);
                    dropped_dead = true;
                    continue;
                }
                entry.touch();
                drop(guard);
                tracing::debug!(key = %key, "连接池命中，复用空闲连接");
                return Ok(entry.handle.clone());
            }
            // 刚丢弃过死连接，重新取快照，避免用过期计数误判池已满
            if dropped_dead {
                continue;
            }

            // 池已满，等待连接释放
            let size = self.len(&key);
            if size >= POOL_MAX_PER_KEY {
                tracing::debug!(key = %key, pool_size = size, "连接池已满，等待空闲连接");
                sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(5000);
                continue;
            }

            // 建新连接
            tracing::info!(key = %key, host = %profile.host, port = profile.port, "连接池新建连接");
            let new = self.provider(profile.kind).connect(profile, password, effective_db).await?;
            let entry = Arc::new(PooledConn::new(Arc::new(AsyncMutex::new(new))));
            self.push(&key, entry.clone());
            return Ok(entry.handle.clone());
        }
    }

    // ── 内部操作 ──

    fn snapshot(&self, key: &str) -> Vec<Arc<PooledConn>> {
        self.entries
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    fn len(&self, key: &str) -> usize {
        self.entries.lock().unwrap().get(key).map_or(0, |v| v.len())
    }

    fn push(&self, key: &str, entry: Arc<PooledConn>) {
        self.entries
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_default()
            .push(entry);
    }

    fn remove(&self, key: &str, target: &Arc<PooledConn>) {
        let mut map = self.entries.lock().unwrap();
        if let Some(vec) = map.get_mut(key) {
            vec.retain(|e| !Arc::ptr_eq(e, target));
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

    /// 预热连接池：建立一个新连接并放入池中，供后续并发操作复用。
    pub async fn prewarm(
        &self,
        profile: &ConnectionProfile,
        password: &str,
        database: Option<&str>,
    ) -> AppResult<()> {
        let effective_db = if matches!(profile.kind, DatabaseKind::Postgres | DatabaseKind::MongoDb) {
            profile.default_database.as_deref()
        } else {
            database
        };
        let key = pool_key(&profile.id, profile.kind, effective_db);
        if self.len(&key) >= 2 {
            return Ok(());
        }
        tracing::info!(key = %key, "预热连接池");
        let conn = self.provider(profile.kind).connect(profile, password, effective_db).await?;
        self.push(&key, Arc::new(PooledConn::new(Arc::new(AsyncMutex::new(conn)))));
        Ok(())
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
                .flat_map(|(k, v)| v.iter().map(|e| (k.clone(), e.clone())))
                .collect()
        };
        for (key, entry) in snapshot {
            // 正在被查询占用的连接 try_lock 会失败，跳过；这类连接由 acquire 的验活兜底
            if let Ok(mut guard) = entry.handle.try_lock() {
                if self.timed_ping(&mut guard).await {
                    entry.touch();
                    continue;
                }
                drop(guard);
                tracing::info!(key = %key, "keepalive ping 失败，清理死连接");
                self.remove(&key, &entry);
            }
        }
    }
}
