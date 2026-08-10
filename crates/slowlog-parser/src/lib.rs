use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static TIME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^# Time:\s+(.+)$").unwrap());
static USER_HOST_RE: LazyLock<Regex> = LazyLock::new(|| {
    // MySQL 慢日志账号格式为 "user[权限用户]"，括号内通常与 user 相同，只取括号前部分。
    // host 可能是裸主机名/ IP（`@ localhost []`），也可能是中括号包裹（`@ [192.168.1.1]`），
    // 两种都需支持，否则整条正则失配会连 user 一起丢失。
    Regex::new(r"^# User@Host:\s+([^\s\[@]+)(?:\[[^\]]*\])?\s*@\s*(?:\[([^\]]+)\]|([^\s\[@]+))").unwrap()
});
static STATS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^# Query_time:\s+([\d.]+)\s+Lock_time:\s+([\d.]+)\s+Rows_sent:\s+(\d+)\s+Rows_examined:\s+(\d+)").unwrap()
});
// MySQL 8 慢日志在 Query_time 行携带权威的 Schema 字段（use 语句不一定存在）
static SCHEMA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Schema:\s*(\S+)").unwrap());
static SET_TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^SET timestamp=\d+;?\s*$").unwrap());
static MYSQLD_BANNER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^/usr/local/mysql/bin/mysqld, Version:|^Tcp port:|^Time\s+Id\s+Command\s+Argument").unwrap()
});
static USE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^use\s+([^\s;]+)\s*;?\s*$").unwrap());
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static IN_CLAUSE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\?\s*(?:,\s*\?)+").unwrap());

/// 单条慢查询记录
#[derive(Debug, Clone)]
pub struct SlowQueryEntry {
    pub timestamp: Option<String>,
    pub user: Option<String>,
    pub host: Option<String>,
    pub database: Option<String>,
    pub query_time_secs: f64,
    pub lock_time_secs: f64,
    pub rows_sent: u64,
    pub rows_examined: u64,
    pub sql: String,
}

/// 指纹聚合统计
#[derive(Debug, Clone)]
pub struct FingerprintStats {
    pub fingerprint: String,
    pub count: usize,
    pub total_time: f64,
    pub min_time: f64,
    pub max_time: f64,
    pub avg_time: f64,
    pub total_lock_time: f64,
    pub avg_lock_time: f64,
    pub total_rows_examined: u64,
    pub total_rows_sent: u64,
    pub avg_rows_examined: f64,
    pub avg_rows_sent: f64,
    pub example_sql: String,
    pub database: Option<String>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
}

/// 排序维度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Count,
    TotalTime,
    AvgTime,
    MaxTime,
    AvgLockTime,
    AvgRowsSent,
    AvgRowsExamined,
}


#[derive(Debug)]
pub enum SlowLogError {
    ParseError(String),
}

impl std::fmt::Display for SlowLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlowLogError::ParseError(msg) => write!(f, "{}", i18n::tr!("解析错误: {}", msg)),
        }
    }
}

/// 解析 MySQL slow query log 文本
pub fn parse_slow_log(input: &str) -> Result<Vec<SlowQueryEntry>, SlowLogError> {
    let mut entries = Vec::new();
    let mut current_time: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_host: Option<String> = None;
    let mut current_db: Option<String> = None;
    let mut current_stats: Option<(f64, f64, u64, u64)> = None;
    let mut sql_lines: Vec<String> = Vec::new();
    let mut in_entry = false;

    for line in input.lines() {
        // 新条目开始：遇到 # Time: 或 # User@Host:
        if TIME_RE.is_match(line) {
            // 保存上一条
            if in_entry && !sql_lines.is_empty() {
                let sql = sql_lines.join("\n").trim().to_string();
                if let Some((qt, lt, rs, re)) = current_stats {
                    if !sql.is_empty() {
                        entries.push(SlowQueryEntry {
                            timestamp: current_time.clone(),
                            user: current_user.clone(),
                            host: current_host.clone(),
                            database: current_db.clone(),
                            query_time_secs: qt,
                            lock_time_secs: lt,
                            rows_sent: rs,
                            rows_examined: re,
                            sql,
                        });
                    }
                }
            }
            current_time = TIME_RE.captures(line).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
            sql_lines.clear();
            in_entry = true;
            current_stats = None;
            current_user = None;
            current_host = None;
            current_db = None;
            continue;
        }

        if let Some(caps) = USER_HOST_RE.captures(line) {
            current_user = caps.get(1).map(|m| m.as_str().to_string());
            current_host = caps.get(2).or_else(|| caps.get(3)).map(|m| m.as_str().to_string());
            in_entry = true;
            continue;
        }

        if let Some(caps) = STATS_RE.captures(line) {
            let qt = caps.get(1).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
            let lt = caps.get(2).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
            let rs = caps.get(3).unwrap().as_str().parse::<u64>().unwrap_or(0);
            let re = caps.get(4).unwrap().as_str().parse::<u64>().unwrap_or(0);
            current_stats = Some((qt, lt, rs, re));
            // MySQL 8 的 Query_time 行同时携带 Schema 字段，是库名的权威来源
            if let Some(schema) = SCHEMA_RE.captures(line).and_then(|c| c.get(1)) {
                current_db = Some(schema.as_str().to_string());
            }
            in_entry = true;
            continue;
        }

        if SET_TIMESTAMP_RE.is_match(line) {
            continue;
        }

        // MySQL 5.7 启动横幅行（不以 # 开头，否则会被当成 SQL 收进条目）
        if MYSQLD_BANNER_RE.is_match(line) {
            continue;
        }

        // use <db>; 语句单独提取库名（Query_time 行的 Schema 字段优先，use 仅兜底），不进入 SQL 文本
        if in_entry {
            if let Some(caps) = USE_RE.captures(line.trim()) {
                if current_db.is_none() {
                    current_db = Some(caps.get(1).unwrap().as_str().to_string());
                }
                continue;
            }
        }

        if in_entry {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // MySQL 慢日志里的元数据注释行（如 "# QC_Hit: No  Full_scan: No ..."），
            // 不匹配已知表头，须跳过以免污染 SQL 文本
            if trimmed.starts_with('#') {
                continue;
            }
            sql_lines.push(trimmed.to_string());
        }
    }

    // 保存最后一条
    if in_entry && !sql_lines.is_empty() {
        let sql = sql_lines.join("\n").trim().to_string();
        if let Some((qt, lt, rs, re)) = current_stats {
            if !sql.is_empty() {
                entries.push(SlowQueryEntry {
                    timestamp: current_time,
                    user: current_user,
                    host: current_host,
                    database: current_db,
                    query_time_secs: qt,
                    lock_time_secs: lt,
                    rows_sent: rs,
                    rows_examined: re,
                    sql,
                });
            }
        }
    }

    Ok(entries)
}

/// SQL 指纹归一化：将具体参数替换为 ?
pub fn normalize_sql(sql: &str) -> String {
    let bytes = sql.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len);
    let mut i = 0;
    let mut in_token = false;

    while i < len {
        // 单行注释
        if i + 1 < len && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            break;
        }
        // 多行注释
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            in_token = false;
            continue;
        }
        // 单引号字符串
        if bytes[i] == b'\'' {
            if !in_token && !result.is_empty() && *result.last().unwrap() != b' ' && *result.last().unwrap() != b'(' && *result.last().unwrap() != b'[' {
                result.push(b' ');
            }
            result.push(b'?');
            i += 1;
            while i < len {
                if bytes[i] == b'\'' {
                    i += 1;
                    if i < len && bytes[i] == b'\'' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            in_token = false;
            continue;
        }
        // 双引号字符串
        if bytes[i] == b'"' {
            if !in_token && !result.is_empty() && *result.last().unwrap() != b' ' && *result.last().unwrap() != b'(' && *result.last().unwrap() != b'[' {
                result.push(b' ');
            }
            result.push(b'?');
            i += 1;
            while i < len {
                if bytes[i] == b'"' {
                    i += 1;
                    if i < len && bytes[i] == b'"' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            in_token = false;
            continue;
        }
        // 数字
        if bytes[i].is_ascii_digit()
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'))
        {
            if !in_token && !result.is_empty() && *result.last().unwrap() != b' ' && *result.last().unwrap() != b'(' && *result.last().unwrap() != b'[' {
                result.push(b' ');
            }
            result.push(b'?');
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            in_token = false;
            continue;
        }
        // 空白：保留至多一个空格，退出 token
        if bytes[i].is_ascii_whitespace() {
            if !result.is_empty() && *result.last().unwrap() != b' ' {
                result.push(b' ');
            }
            in_token = false;
            i += 1;
            continue;
        }
        // ASCII 标识符字符（字母、数字、下划线）
        if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            if !in_token && !result.is_empty() && *result.last().unwrap() != b' ' && *result.last().unwrap() != b'(' && *result.last().unwrap() != b'[' {
                result.push(b' ');
            }
            result.push(bytes[i]);
            in_token = true;
            i += 1;
            continue;
        }
        // 多字节 UTF-8 字符，原样保留
        if bytes[i] > 0x7F {
            let char_len = if bytes[i] & 0xE0 == 0xC0 { 2 }
                else if bytes[i] & 0xF0 == 0xE0 { 3 }
                else { 4 };
            let end = (i + char_len).min(len);
            result.extend_from_slice(&bytes[i..end]);
            i = end;
            in_token = false;
            continue;
        }
        // 其他 ASCII 符号（括号、逗号、运算符等）
        result.push(bytes[i]);
        in_token = false;
        i += 1;
    }

    // 合并连续空白为单个空格
    let normalized = WHITESPACE_RE.replace_all(
        std::str::from_utf8(&result).unwrap_or("").trim(),
        " ",
    );
    // 合并连续的 ? 为单个 ?（处理 IN 子句）
    IN_CLAUSE_RE.replace_all(&normalized, "?").to_string()
}

/// 对慢查询条目进行指纹聚合
pub fn aggregate(entries: &[SlowQueryEntry]) -> Vec<FingerprintStats> {
    let mut map: HashMap<String, FingerprintStats> = HashMap::new();

    for entry in entries {
        let fingerprint = normalize_sql(&entry.sql);
        let stats = map.entry(fingerprint.clone()).or_insert_with(|| FingerprintStats {
            fingerprint,
            count: 0,
            total_time: 0.0,
            min_time: f64::MAX,
            max_time: 0.0,
            avg_time: 0.0,
            total_lock_time: 0.0,
            avg_lock_time: 0.0,
            total_rows_examined: 0,
            total_rows_sent: 0,
            avg_rows_examined: 0.0,
            avg_rows_sent: 0.0,
            example_sql: entry.sql.clone(),
            database: entry.database.clone(),
            first_seen: entry.timestamp.clone(),
            last_seen: entry.timestamp.clone(),
        });

        stats.count += 1;
        stats.total_time += entry.query_time_secs;
        stats.total_lock_time += entry.lock_time_secs;
        // 库名取组内首个非空值：5.7 旧格式条目无 Schema 字段，可能只有首条为空
        if stats.database.is_none() {
            stats.database = entry.database.clone();
        }
        if entry.query_time_secs < stats.min_time {
            stats.min_time = entry.query_time_secs;
        }
        if entry.query_time_secs > stats.max_time {
            stats.max_time = entry.query_time_secs;
        }
        stats.total_rows_examined += entry.rows_examined;
        stats.total_rows_sent += entry.rows_sent;
        if stats.last_seen.as_ref().map_or(true, |l| {
            entry.timestamp.as_ref().map_or(false, |t| t > l)
        }) {
            stats.last_seen = entry.timestamp.clone();
        }
    }

    let mut result: Vec<FingerprintStats> = map.into_values().collect();
    for s in &mut result {
        if s.count > 0 {
            s.avg_time = s.total_time / s.count as f64;
            s.avg_lock_time = s.total_lock_time / s.count as f64;
            s.avg_rows_examined = s.total_rows_examined as f64 / s.count as f64;
            s.avg_rows_sent = s.total_rows_sent as f64 / s.count as f64;
        }
        if s.min_time == f64::MAX {
            s.min_time = 0.0;
        }
    }
    result
}

/// 对聚合结果排序（默认降序，便于展示最大值在前）
pub fn sort_stats(stats: &mut [FingerprintStats], sort_by: SortBy) {
    sort_stats_with_direction(stats, sort_by, false);
}

/// 对聚合结果排序（支持升降序）
pub fn sort_stats_with_direction(stats: &mut [FingerprintStats], sort_by: SortBy, ascending: bool) {
    let cmp: fn(&FingerprintStats, &FingerprintStats) -> std::cmp::Ordering = match sort_by {
        SortBy::Count => |a, b| a.count.cmp(&b.count),
        SortBy::TotalTime => |a, b| a.total_time.partial_cmp(&b.total_time).unwrap_or(std::cmp::Ordering::Equal),
        SortBy::AvgTime => |a, b| a.avg_time.partial_cmp(&b.avg_time).unwrap_or(std::cmp::Ordering::Equal),
        SortBy::MaxTime => |a, b| a.max_time.partial_cmp(&b.max_time).unwrap_or(std::cmp::Ordering::Equal),
        SortBy::AvgLockTime => |a, b| a.avg_lock_time.partial_cmp(&b.avg_lock_time).unwrap_or(std::cmp::Ordering::Equal),
        SortBy::AvgRowsSent => |a, b| a.avg_rows_sent.partial_cmp(&b.avg_rows_sent).unwrap_or(std::cmp::Ordering::Equal),
        SortBy::AvgRowsExamined => |a, b| a.avg_rows_examined.partial_cmp(&b.avg_rows_examined).unwrap_or(std::cmp::Ordering::Equal),
    };
    if ascending {
        stats.sort_by(cmp);
    } else {
        stats.sort_by(|a, b| cmp(b, a));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOG: &str = r#"# Time: 2024-01-15T10:30:00.000000+08:00
# User@Host: root[root] @ localhost []  Id:    42
# Query_time: 1.234567  Lock_time: 0.000100  Rows_sent: 100  Rows_examined: 50000
SET timestamp=1705285800;
use plutus_8;
SELECT * FROM users WHERE status = 'active' AND created_at > '2024-01-01';
# Time: 2024-01-15T10:31:00.000000+08:00
# User@Host: root[root] @ localhost []  Id:    43
# Query_time: 0.500000  Lock_time: 0.000050  Rows_sent: 10  Rows_examined: 1000
SET timestamp=1705285860;
# QC_Hit: No  Full_scan: No  Full_join: No  Tmp_table: No  Tmp_table_on_disk: No  Filesort: No  Filesort_on_disk: No
SELECT * FROM users WHERE id IN (1, 2, 3, 4, 5);
# Time: 2024-01-15T10:32:00.000000+08:00
# User@Host: app[app] @ 192.168.1.100 []  Id:    44
# Query_time: 2.500000  Lock_time: 0.000200  Rows_sent: 1  Rows_examined: 100000
SET timestamp=1705285920;
use order_db;
SELECT * FROM users WHERE status = 'inactive' AND created_at > '2023-12-01';
# Time: 2024-01-15T10:33:00.000000+08:00
# User@Host: zeuscore[zeuscore] @ 10.0.0.5 []  Id:    45
# Query_time: 0.300000  Lock_time: 0.000010  Rows_sent: 2  Rows_examined: 200
SET timestamp=1705285980;
SELECT COUNT(*) FROM orders;
"#;

    #[test]
    fn test_parse_slow_log() {
        let entries = parse_slow_log(SAMPLE_LOG).unwrap();
        assert_eq!(entries.len(), 4);
        assert!((entries[0].query_time_secs - 1.234567).abs() < 0.001);
        assert_eq!(entries[0].rows_examined, 50000);
        assert_eq!(entries[1].rows_sent, 10);
        assert!(entries[2].sql.contains("inactive"));
    }

    #[test]
    fn test_parse_metadata_and_use() {
        let entries = parse_slow_log(SAMPLE_LOG).unwrap();
        // use 语句提取为数据库名，不出现在 SQL 文本里
        assert_eq!(entries[0].database.as_deref(), Some("plutus_8"));
        assert!(!entries[0].sql.contains("plutus"));
        assert!(entries[0].sql.starts_with("SELECT"));
        // 元数据注释行被跳过，SQL 以真实语句开头
        assert!(entries[1].sql.starts_with("SELECT"));
        assert!(!entries[1].sql.contains("QC_Hit"));
        assert_eq!(entries[2].database.as_deref(), Some("order_db"));
        // 账号形如 zeuscore[zeuscore] 时只保留 zeuscore，不重复展示
        assert_eq!(entries[3].user.as_deref(), Some("zeuscore"));
        assert_eq!(entries[3].host.as_deref(), Some("10.0.0.5"));
    }

    #[test]
    fn test_bracketed_host() {
        // MySQL 8 慢日志可能把 host 用中括号包裹：`@ [192.168.1.1]`
        let entries = parse_slow_log(
            r#"# Time: 2024-01-15T10:34:00.000000+08:00
# User@Host: worker[worker] @ [192.168.1.88]  Id:    46
# Query_time: 0.100000  Lock_time: 0.000010  Rows_sent: 1  Rows_examined: 10
SET timestamp=1705286040;
SELECT 1;
"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].user.as_deref(), Some("worker"));
        assert_eq!(entries[0].host.as_deref(), Some("192.168.1.88"));
    }

    #[test]
    fn test_mysql8_schema_field() {
        // MySQL 8 慢日志在 Query_time 行携带 Schema 字段，库名以此为准（use 语句不一定存在）
        let entries = parse_slow_log(
            r#"# Time: 2024-06-01T08:00:00.000000Z
# User@Host: zeuscore[zeuscore] @  [10.198.7.40]  Id: 175315741
# Query_time: 1.133039  Lock_time: 0.000077 Rows_sent: 0  Rows_examined: 60821 Thread_id: 175315741 Schema: plutus_8 Errno: 0
SET timestamp=1717228800;
SELECT * FROM large_table WHERE id > 100;
# Time: 2024-06-01T08:00:01.000000Z
# User@Host: zeuscore[zeuscore] @  [10.198.7.40]  Id: 175315742
# Query_time: 0.500000  Lock_time: 0.000050 Rows_sent: 1  Rows_examined: 500 Thread_id: 175315742 Schema: analytics_db Errno: 0
SET timestamp=1717228801;
use plutus_8;
SELECT * FROM small_table;
"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        // 无 use 语句，库名取自 Schema 字段
        assert_eq!(entries[0].database.as_deref(), Some("plutus_8"));
        // 同时存在 use 语句时，Schema 字段优先（权威来源）
        assert_eq!(entries[1].database.as_deref(), Some("analytics_db"));
        // 两个库都来自 Query_time 行，SQL 不含 use 语句
        assert!(entries[1].sql.starts_with("SELECT"));
    }

    #[test]
    fn test_mysql57_banner_and_aggregate_db_fill() {
        // 5.7 旧格式条目（无 Thread_id/Schema 字段）库名为空，
        // 且其后的 mysqld 版本横幅不能被当成 SQL 收进条目
        let entries = parse_slow_log(
            r#"# Time: 2024-06-01T08:00:00.000000Z
# User@Host: zeuscoreread[zeuscoreread] @  [10.198.13.148]  Id: 175523465
# Query_time: 1.711819  Lock_time: 0.000108 Rows_sent: 8963  Rows_examined: 8963 Launch_time: 0.000000
# QC_Hit: No  Full_scan: No  Full_join: No  Tmp_table: No  Tmp_table_on_disk: No  Filesort: No  Filesort_on_disk: No
SET timestamp=1717228800;
SELECT * FROM `bind_card_order_info` WHERE bank_card_no in ('a','b');
/usr/local/mysql/bin/mysqld, Version: 5.7.44-240900-log (MySQL Community Server - (GPL)). started with:
Tcp port: 3306  Unix socket: /tmp/mysql.sock
Time                 Id Command    Argument
# Time: 2024-06-01T08:00:01.000000Z
# User@Host: zeuscore[zeuscore] @  [10.198.13.61]  Id: 175513374
# Query_time: 1.015290  Lock_time: 0.000033 Rows_sent: 0  Rows_examined: 1694675 Thread_id: 175513374 Schema: plutus_0 Errno: 0
SET timestamp=1717228801;
SELECT * FROM `bind_card_order_info` WHERE bank_card_no in ('a','b');
"#,
        )
        .unwrap();
        // 横幅行未污染条目
        assert_eq!(entries.len(), 2);
        assert!(!entries[0].sql.contains("mysqld"));
        assert!(!entries[0].sql.contains("Tcp port"));
        assert!(entries[0].sql.starts_with("SELECT"));
        // 5.7 旧格式条目库名为空，8.0 条目有 Schema
        assert_eq!(entries[0].database, None);
        assert_eq!(entries[1].database.as_deref(), Some("plutus_0"));
        // 聚合时库名取组内首个非空值，不因首条为空而整组为空
        let stats = aggregate(&entries);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].database.as_deref(), Some("plutus_0"));
    }

    #[test]
    fn test_normalize_sql() {
        assert_eq!(
            normalize_sql("SELECT * FROM users WHERE id = 42"),
            "SELECT * FROM users WHERE id = ?"
        );
        assert_eq!(
            normalize_sql("SELECT * FROM users WHERE id IN (1, 2, 3)"),
            "SELECT * FROM users WHERE id IN (?)"
        );
        assert_eq!(
            normalize_sql("SELECT * FROM users WHERE name = 'hello'"),
            "SELECT * FROM users WHERE name = ?"
        );
    }

    #[test]
    fn test_aggregate() {
        let entries = parse_slow_log(SAMPLE_LOG).unwrap();
        let stats = aggregate(&entries);
        // 第一条和第三条的归一化指纹相同（都是 SELECT * FROM users WHERE status = ? AND created_at > ?）
        let main_stat = stats.iter().find(|s| s.fingerprint.contains("status")).unwrap();
        assert_eq!(main_stat.count, 2);
        assert!((main_stat.total_time - 3.734567).abs() < 0.01);
        // 平均字段
        assert!((main_stat.avg_time - 3.734567 / 2.0).abs() < 0.01);
        assert!((main_stat.avg_lock_time - (0.000100 + 0.000200) / 2.0).abs() < 1e-6);
        assert_eq!(main_stat.avg_rows_sent as u64, (100 + 1) / 2);
        assert_eq!(main_stat.avg_rows_examined as u64, (50000 + 100000) / 2);
        // 所属数据库取首个出现条目的库名
        assert_eq!(main_stat.database.as_deref(), Some("plutus_8"));
    }

    #[test]
    fn test_sort_stats() {
        let entries = parse_slow_log(SAMPLE_LOG).unwrap();
        let mut stats = aggregate(&entries);
        sort_stats(&mut stats, SortBy::TotalTime);
        assert!(stats[0].total_time >= stats[1].total_time);
        // 新维度：平均锁时间 / 平均发送行 / 平均扫描行，升降序
        sort_stats(&mut stats, SortBy::AvgLockTime);
        assert!(stats[0].avg_lock_time >= stats[1].avg_lock_time);
        sort_stats_with_direction(&mut stats, SortBy::AvgRowsSent, true);
        assert!(stats[0].avg_rows_sent <= stats.last().unwrap().avg_rows_sent);
        sort_stats_with_direction(&mut stats, SortBy::AvgRowsExamined, false);
        assert!(stats[0].avg_rows_examined >= stats.last().unwrap().avg_rows_examined);
        // 聚合字段完整性
        let s = &stats[0];
        assert!(s.avg_time >= 0.0 && s.avg_lock_time >= 0.0 && s.avg_rows_sent >= 0.0 && s.avg_rows_examined >= 0.0);
    }

    #[test]
    fn test_empty_log() {
        let entries = parse_slow_log("").unwrap();
        assert!(entries.is_empty());
        let stats = aggregate(&entries);
        assert!(stats.is_empty());
    }
}
