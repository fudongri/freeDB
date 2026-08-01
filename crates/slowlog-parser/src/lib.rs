use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static TIME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^# Time:\s+(.+)$").unwrap());
static USER_HOST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^# User@Host:\s+(\S+)\s*@\s*(\S*)").unwrap());
static STATS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^# Query_time:\s+([\d.]+)\s+Lock_time:\s+([\d.]+)\s+Rows_sent:\s+(\d+)\s+Rows_examined:\s+(\d+)").unwrap()
});
static SET_TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^SET timestamp=\d+;?\s*$").unwrap());
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static IN_CLAUSE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\?\s*(?:,\s*\?)+").unwrap());

/// 单条慢查询记录
#[derive(Debug, Clone)]
pub struct SlowQueryEntry {
    pub timestamp: Option<String>,
    pub user: Option<String>,
    pub host: Option<String>,
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
    pub total_rows_examined: u64,
    pub total_rows_sent: u64,
    pub example_sql: String,
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
            continue;
        }

        if let Some(caps) = USER_HOST_RE.captures(line) {
            current_user = caps.get(1).map(|m| m.as_str().to_string());
            current_host = caps.get(2).map(|m| m.as_str().to_string());
            in_entry = true;
            continue;
        }

        if let Some(caps) = STATS_RE.captures(line) {
            let qt = caps.get(1).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
            let lt = caps.get(2).unwrap().as_str().parse::<f64>().unwrap_or(0.0);
            let rs = caps.get(3).unwrap().as_str().parse::<u64>().unwrap_or(0);
            let re = caps.get(4).unwrap().as_str().parse::<u64>().unwrap_or(0);
            current_stats = Some((qt, lt, rs, re));
            in_entry = true;
            continue;
        }

        if SET_TIMESTAMP_RE.is_match(line) {
            continue;
        }

        if in_entry {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                sql_lines.push(trimmed.to_string());
            }
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
            total_rows_examined: 0,
            total_rows_sent: 0,
            example_sql: entry.sql.clone(),
            first_seen: entry.timestamp.clone(),
            last_seen: entry.timestamp.clone(),
        });

        stats.count += 1;
        stats.total_time += entry.query_time_secs;
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
        }
        if s.min_time == f64::MAX {
            s.min_time = 0.0;
        }
    }
    result
}

/// 对聚合结果排序
pub fn sort_stats(stats: &mut [FingerprintStats], sort_by: SortBy) {
    match sort_by {
        SortBy::Count => stats.sort_by(|a, b| b.count.cmp(&a.count)),
        SortBy::TotalTime => stats.sort_by(|a, b| b.total_time.partial_cmp(&a.total_time).unwrap_or(std::cmp::Ordering::Equal)),
        SortBy::AvgTime => stats.sort_by(|a, b| b.avg_time.partial_cmp(&a.avg_time).unwrap_or(std::cmp::Ordering::Equal)),
        SortBy::MaxTime => stats.sort_by(|a, b| b.max_time.partial_cmp(&a.max_time).unwrap_or(std::cmp::Ordering::Equal)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOG: &str = r#"# Time: 2024-01-15T10:30:00.000000+08:00
# User@Host: root[root] @ localhost []  Id:    42
# Query_time: 1.234567  Lock_time: 0.000100  Rows_sent: 100  Rows_examined: 50000
SET timestamp=1705285800;
SELECT * FROM users WHERE status = 'active' AND created_at > '2024-01-01';
# Time: 2024-01-15T10:31:00.000000+08:00
# User@Host: root[root] @ localhost []  Id:    43
# Query_time: 0.500000  Lock_time: 0.000050  Rows_sent: 10  Rows_examined: 1000
SET timestamp=1705285860;
SELECT * FROM users WHERE id IN (1, 2, 3, 4, 5);
# Time: 2024-01-15T10:32:00.000000+08:00
# User@Host: app[app] @ 192.168.1.100 []  Id:    44
# Query_time: 2.500000  Lock_time: 0.000200  Rows_sent: 1  Rows_examined: 100000
SET timestamp=1705285920;
SELECT * FROM users WHERE status = 'inactive' AND created_at > '2023-12-01';
"#;

    #[test]
    fn test_parse_slow_log() {
        let entries = parse_slow_log(SAMPLE_LOG).unwrap();
        assert_eq!(entries.len(), 3);
        assert!((entries[0].query_time_secs - 1.234567).abs() < 0.001);
        assert_eq!(entries[0].rows_examined, 50000);
        assert_eq!(entries[1].rows_sent, 10);
        assert!(entries[2].sql.contains("inactive"));
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
    }

    #[test]
    fn test_sort_stats() {
        let entries = parse_slow_log(SAMPLE_LOG).unwrap();
        let mut stats = aggregate(&entries);
        sort_stats(&mut stats, SortBy::TotalTime);
        assert!(stats[0].total_time >= stats[1].total_time);
    }

    #[test]
    fn test_empty_log() {
        let entries = parse_slow_log("").unwrap();
        assert!(entries.is_empty());
        let stats = aggregate(&entries);
        assert!(stats.is_empty());
    }
}
