use core_domain::{ColumnDefinition, DatabaseKind};
use eframe::egui::{self, Align2, Area, Color32, FontFamily, FontId, Id, Order, ScrollArea, Sense, Stroke};
use eframe::egui::text::{LayoutJob, TextFormat};
use i18n::tr;
use std::collections::{HashMap, VecDeque};

/// Snap a byte index to the nearest preceding UTF-8 character boundary.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    let mut bound = index.min(s.len());
    while bound > 0 && !s.is_char_boundary(bound) {
        bound -= 1;
    }
    bound
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// 拖拽插入 SQL 时，判断光标相邻字符是否需要补空格分隔。
pub(crate) fn needs_space_padding(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '(' | ')' | ',' | '.' | ';' | '\'' | '"' | '`'))
}

fn compact_object_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

const WHERE_OPERATOR_SNIPPETS: &[(&str, &str, usize)] = &[
    ("IS NULL", "IS NULL", 7),
    ("IS NOT NULL", "IS NOT NULL", 11),
    ("IN", "IN ()", 4),
    ("NOT IN", "NOT IN ()", 8),
    ("LIKE", "LIKE ", 5),
    ("NOT LIKE", "NOT LIKE ", 9),
    ("BETWEEN", "BETWEEN  AND ", 8),
];

/// Check if `pattern` is a subsequence of `text` (case-insensitive).
/// Returns the matched character indices in `text` if found.
fn subsequence_match(text: &str, pattern: &[char]) -> Option<Vec<usize>> {
    if pattern.is_empty() {
        return Some(Vec::new());
    }
    let text_chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut indices = Vec::with_capacity(pattern.len());
    let mut pi = 0;
    for (ti, &tc) in text_chars.iter().enumerate() {
        if tc == pattern[pi] {
            indices.push(ti);
            pi += 1;
            if pi == pattern.len() {
                return Some(indices);
            }
        }
    }
    None
}

/// A single autocomplete suggestion item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutocompleteSuggestion {
    /// The text to insert.
    pub label: String,
    /// What kind of thing this is (affects icon + sort order).
    pub kind: SuggestionKind,
    /// Character indices in `label` that matched the prefix (for highlighting).
    #[doc(hidden)]
    pub matched_indices: Vec<usize>,
    /// Optional insertion text with parameter template (e.g. "find({})").
    /// When set, this is inserted instead of `label`.
    pub insertion_text: Option<String>,
    /// Cursor position (char offset from start of insertion_text) after inserting.
    pub cursor_offset: Option<usize>,
}

impl AutocompleteSuggestion {
    pub fn new(label: String, kind: SuggestionKind) -> Self {
        Self { label, kind, matched_indices: Vec::new(), insertion_text: None, cursor_offset: None }
    }
}

#[derive(Clone, Default)]
pub(crate) struct AutocompleteUsageMemory {
    order: VecDeque<String>,
}

impl AutocompleteUsageMemory {
    const MAX_ENTRIES: usize = 64;

    pub fn record(&mut self, label: &str) {
        let key = label.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        if let Some(pos) = self.order.iter().position(|item| item == &key) {
            self.order.remove(pos);
        }
        self.order.push_front(key);
        while self.order.len() > Self::MAX_ENTRIES {
            self.order.pop_back();
        }
    }

    pub fn score(&self, label: &str) -> i32 {
        let key = label.trim().to_ascii_lowercase();
        self.order
            .iter()
            .position(|item| item == &key)
            .map(|idx| (Self::MAX_ENTRIES.saturating_sub(idx)) as i32)
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SuggestionKind {
    Loading,
    Database,
    Schema,
    Table,
    View,
    Column { parent_table: String },
    Function,
    Keyword,
    /// 语句骨架片段（sel / ins / upd / del / joi / whr）
    Snippet,
    MongoKeyword,
}

/// 补全用的元数据缓存：连接 → 库 / schema → 表 / 视图 → 列。
///
/// 所有查询都按当前连接收敛，避免不同连接、不同库的同名表互相串扰。
/// 表名/视图来自连接树与元数据缓存，列来自按需加载的表结构。
/// 表/视图在缓存中的完整位置。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TableKey {
    connection: String,
    /// 空字符串表示未知（如 SQLite）
    database: String,
    /// 空字符串表示未知
    schema: String,
    table: String,
}

fn make_table_key(
    connection_id: &str,
    database: Option<&str>,
    schema: Option<&str>,
    table: &str,
) -> TableKey {
    TableKey {
        connection: connection_id.to_string(),
        database: database.unwrap_or("").to_string(),
        schema: schema.unwrap_or("").to_string(),
        table: table.to_string(),
    }
}

#[derive(Clone, Default)]
pub(crate) struct SchemaCache {
    /// connection_id → 数据库名（有序）
    databases: HashMap<String, Vec<String>>,
    /// connection_id → schema 名（有序）
    schemas: HashMap<String, Vec<String>>,
    /// (connection, database, schema, table) → (is_view, columns)
    tables: HashMap<TableKey, (bool, Vec<ColumnDefinition>)>,
}

impl SchemaCache {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            databases: HashMap::new(),
            schemas: HashMap::new(),
        }
    }

    /// 注册表（或视图）。幂等；同一表以「裸表名」和「库.表」两种路径注册时，
    /// 会各自保留一条记录，读取时会按短表名去重。
    pub fn add_table(
        &mut self,
        connection_id: &str,
        database: Option<&str>,
        schema: Option<&str>,
        table: String,
        is_view: bool,
    ) {
        let key = make_table_key(connection_id, database, schema, &table);
        let entry = self.tables.entry(key).or_insert((is_view, Vec::new()));
        // 稍后得知是视图时更新标记
        if is_view {
            entry.0 = true;
        }
    }

    /// Register a database name for a connection. Idempotent.
    pub fn add_database(&mut self, connection_id: &str, name: String) {
        let list = self.databases.entry(connection_id.to_string()).or_default();
        if !list.contains(&name) {
            list.push(name);
        }
    }

    /// Register a schema name for a connection. Idempotent.
    pub fn add_schema(&mut self, connection_id: &str, name: String) {
        let list = self.schemas.entry(connection_id.to_string()).or_default();
        if !list.contains(&name) {
            list.push(name);
        }
    }

    /// Return database names for a specific connection.
    pub fn database_names_for(&self, connection_id: &str) -> Vec<&str> {
        self.databases
            .get(connection_id)
            .map(|list| list.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Return schema names for a specific connection.
    pub fn schema_names_for(&self, connection_id: &str) -> Vec<&str> {
        self.schemas
            .get(connection_id)
            .map(|list| list.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Return table names for a given database within a connection, if known.
    pub fn tables_for_database(&self, connection_id: &str, database: &str) -> Vec<String> {
        self.collect_table_names(connection_id, |key| key.database == database)
    }

    /// Return table names for a given schema within a connection, if known.
    pub fn tables_for_schema(&self, connection_id: &str, schema: &str) -> Vec<String> {
        self.collect_table_names(connection_id, |key| key.schema == schema)
    }

    fn collect_table_names(
        &self,
        connection_id: &str,
        filter: impl Fn(&TableKey) -> bool,
    ) -> Vec<String> {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut names: Vec<String> = Vec::new();
        for key in self.tables.keys() {
            if key.connection != connection_id || !filter(key) {
                continue;
            }
            if seen.insert(key.table.as_str()) {
                names.push(key.table.clone());
            }
        }
        names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        names
    }

    /// Store column definitions for a table.
    pub fn add_columns(
        &mut self,
        connection_id: &str,
        database: Option<&str>,
        schema: Option<&str>,
        table: String,
        columns: Vec<ColumnDefinition>,
    ) {
        let key = make_table_key(connection_id, database, schema, &table);
        let entry = self.tables.entry(key).or_insert((false, Vec::new()));
        entry.1 = columns;
    }

    /// 该连接是否已缓存任意表/视图（用于判断是否需要后台补表清单，避免每帧构造列表）。
    pub fn has_tables(&self, connection_id: &str) -> bool {
        self.tables.keys().any(|key| key.connection == connection_id)
    }

    /// 当前连接（可选库）下的表/视图，按短表名去重，返回 (名称, 是否视图)。
    /// 指定了库但该库没有任何已知表时，回退到连接内全部表（库尚未加载的场景）。
    pub fn table_names_for<'a>(
        &'a self,
        connection_id: Option<&str>,
        database: Option<&str>,
    ) -> Vec<(&'a str, bool)> {
        let Some(connection_id) = connection_id else {
            return Vec::new();
        };
        let mut scoped: Vec<(&str, bool)> = Vec::new();
        let mut fallback: Vec<(&str, bool)> = Vec::new();
        let mut scoped_seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut fallback_seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (key, (is_view, _)) in &self.tables {
            if key.connection != connection_id {
                continue;
            }
            if fallback_seen.insert(key.table.as_str()) {
                fallback.push((key.table.as_str(), *is_view));
            }
            if database.is_some_and(|db| key.database == db)
                && scoped_seen.insert(key.table.as_str())
            {
                scoped.push((key.table.as_str(), *is_view));
            }
        }
        if scoped.is_empty() {
            scoped = fallback;
        }
        scoped.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        scoped
    }

    /// 连接内的全部表及其列，供 AI 工具快照使用。
    pub fn table_entries_for_connection(
        &self,
        connection_id: &str,
    ) -> Vec<(String, bool, Vec<ColumnDefinition>)> {
        let mut entries: Vec<(String, bool, Vec<ColumnDefinition>)> = Vec::new();
        for (key, (is_view, cols)) in &self.tables {
            if key.connection != connection_id {
                continue;
            }
            if entries.iter().any(|(name, _, existing)| {
                name == &key.table && (existing.is_empty() || cols.is_empty())
            }) {
                continue;
            }
            entries.push((key.table.clone(), *is_view, cols.clone()));
        }
        entries.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        entries
    }

    /// 当前连接（可选库）下的全部列，按短表名去重，返回 (表名, 列)。
    /// 单次遍历缓存，避免逐表调用查找造成 O(n²)。
    /// 结果有上限：超大 schema（上万列）下每次按键都全量收集/排序会明显卡顿。
    pub fn column_entries_for_scope<'a>(
        &'a self,
        connection_id: Option<&str>,
        database: Option<&str>,
    ) -> Vec<(&'a str, &'a ColumnDefinition)> {
        const MAX_COLUMN_ENTRIES: usize = 3000;
        let Some(connection_id) = connection_id else {
            return Vec::new();
        };
        let collect = |require_database: bool| -> Vec<(&'a str, &'a ColumnDefinition)> {
            let mut entries: Vec<(&'a str, &'a ColumnDefinition)> = Vec::new();
            let mut seen: std::collections::HashSet<&'a str> = std::collections::HashSet::new();
            for (key, (_, cols)) in &self.tables {
                if key.connection != connection_id || cols.is_empty() {
                    continue;
                }
                if require_database && database.is_some_and(|db| key.database != db) {
                    continue;
                }
                if !seen.insert(key.table.as_str()) {
                    continue;
                }
                for col in cols {
                    entries.push((key.table.as_str(), col));
                    if entries.len() >= MAX_COLUMN_ENTRIES {
                        return entries;
                    }
                }
            }
            entries
        };
        let entries = collect(database.is_some());
        if entries.is_empty() && database.is_some() {
            // 指定库尚未加载时回退到连接内全部列
            return collect(false);
        }
        entries
    }

    /// 当前连接（可选库）下的表位置，按 (schema, table) 去重。
    /// 返回 (database, schema, table, is_view)，供后台列预取构造 TableRef。
    /// 指定库时严格只看该库，不会回退到连接内其它库。
    pub fn table_locations(
        &self,
        connection_id: &str,
        database: Option<&str>,
    ) -> Vec<(Option<String>, Option<String>, String, bool)> {
        let mut locations: Vec<(Option<String>, Option<String>, String, bool)> = Vec::new();
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for (key, (is_view, _)) in &self.tables {
            if key.connection != connection_id {
                continue;
            }
            if database.is_some_and(|target| key.database != target) {
                continue;
            }
            let db = if key.database.is_empty() {
                None
            } else {
                Some(key.database.clone())
            };
            let schema = if key.schema.is_empty() {
                None
            } else {
                Some(key.schema.clone())
            };
            if seen.insert((key.schema.clone(), key.table.clone())) {
                locations.push((db, schema, key.table.clone(), *is_view));
            }
        }
        locations
    }

    /// Return column definitions for a given table, if cached.
    /// 优先匹配当前库；`table` 支持 `db.table` / `schema.table` 形式。
    pub fn columns_for_table(
        &self,
        connection_id: Option<&str>,
        database: Option<&str>,
        table: &str,
    ) -> Option<&[ColumnDefinition]> {
        let connection_id = connection_id?;
        let short = table.rsplit('.').next().unwrap_or(table);
        let mut fallback: Option<&[ColumnDefinition]> = None;
        for (key, (_, cols)) in &self.tables {
            if key.connection != connection_id
                || (key.table != short && key.table != table)
                || cols.is_empty()
            {
                continue;
            }
            if database.is_some_and(|db| key.database == db) {
                return Some(cols.as_slice());
            }
            if fallback.is_none() {
                fallback = Some(cols.as_slice());
            }
        }
        fallback
    }

    /// 表是否为视图（限定连接/库）。
    pub fn is_view_of(&self, connection_id: Option<&str>, database: Option<&str>, table: &str) -> bool {
        let Some(connection_id) = connection_id else {
            return false;
        };
        let short = table.rsplit('.').next().unwrap_or(table);
        for (key, (is_view, _)) in &self.tables {
            if key.connection == connection_id
                && key.table == short
                && *is_view
                && database.map_or(true, |db| key.database == db)
            {
                return true;
            }
        }
        false
    }

    /// 判断表是否已由连接树/元数据缓存确认存在，而非用户 SQL 里随意输入的未知名称。
    /// 智能提示只在确认存在的表上加载列，未知表名静默跳过，避免每次输入
    /// 都触发数据库查询并对不存在的表报错。
    pub fn is_known_table(
        &self,
        connection_id: Option<&str>,
        schema: Option<&str>,
        database: Option<&str>,
        table: &str,
    ) -> bool {
        if self.columns_for_table(connection_id, database, table).is_some() {
            return true;
        }
        let Some(connection_id) = connection_id else {
            return false;
        };
        let short = table.rsplit('.').next().unwrap_or(table);
        self.tables.keys().any(|key| {
            key.connection == connection_id
                && (key.table == short || key.table == table)
                && schema.map_or(true, |s| key.schema == s)
                && database.map_or(true, |db| key.database == db)
        })
    }

    /// Number of tables with cached column definitions.
    pub fn tables_with_columns_count(&self) -> usize {
        self.tables
            .values()
            .filter(|(_, cols)| !cols.is_empty())
            .count()
    }

    /// 缓存中的表/库/模式总数（诊断用）
    pub fn len(&self) -> usize {
        self.tables.len()
            + self.databases.values().map(|v| v.len()).sum::<usize>()
            + self.schemas.values().map(|v| v.len()).sum::<usize>()
    }

    /// 估算缓存占用的字节数（表名/库名/模式名/列定义名与类型，粗略求和，诊断用）
    pub fn estimated_bytes(&self) -> usize {
        use std::mem::size_of;

        let table_bytes: usize = self
            .tables
            .iter()
            .map(|(key, (_, cols))| {
                key.table.len()
                    + key.database.len()
                    + key.schema.len()
                    + cols.iter().map(|c| c.name.len() + c.data_type.len()).sum::<usize>()
                    + cols.len() * size_of::<ColumnDefinition>()
            })
            .sum();
        let db_bytes: usize = self
            .databases
            .iter()
            .map(|(conn, list)| {
                conn.len() + list.iter().map(|s| s.len()).sum::<usize>()
            })
            .sum();
        let schema_bytes: usize = self
            .schemas
            .iter()
            .map(|(conn, list)| {
                conn.len() + list.iter().map(|s| s.len()).sum::<usize>()
            })
            .sum();
        table_bytes + db_bytes + schema_bytes
    }

    /// Clear all cached tables.
    pub fn clear(&mut self) {
        self.tables.clear();
        self.databases.clear();
        self.schemas.clear();
    }
}

/// The SQL context around the cursor, used to filter suggestion types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SqlContext {
    /// 光标位于字符串字面量或注释内，不提供补全。
    Suppressed,
    /// Cursor is after a keyword like FROM, JOIN, etc. Suggests tables.
    AfterKeyword { keyword: String },
    /// Cursor is after `something.` — suggests columns of that table/alias.
    AfterColumnDot { parent: String },
    /// Inside SELECT column list (or after comma in select list). Suggests columns.
    SelectClause,
    /// Inside WHERE/ON/HAVING clauses. Suggests columns.
    WhereClause,
    /// Inside ORDER BY / GROUP BY. Suggests columns.
    OrderGroupClause,
    /// After INSERT INTO <table> ( <— suggests columns.
    InsertColumns,
    /// After AS — usually expects an alias, not a generic keyword soup.
    AliasName,
    /// Fallback — suggest everything.
    General,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TableReference {
    table_name: String,
    alias: Option<String>,
}

pub(crate) struct SqlContextParser;

impl SqlContextParser {
    /// Keywords that indicate a table name should follow.
    const TABLE_KEYWORDS: &'static [&'static str] = &[
        "FROM", "JOIN", "INNER", "LEFT", "RIGHT", "OUTER", "CROSS",
        "FULL", "NATURAL", "INTO", "UPDATE", "TABLE",
    ];

    /// Keywords that indicate column names should follow.
    const COLUMN_KEYWORDS: &'static [&'static str] = &[
        "SELECT", "WHERE", "ON", "AND", "OR", "SET",
        "HAVING", "ORDER", "GROUP", "BY",
    ];

    /// Determine the SQL context at the given cursor position.
    /// `cursor_char_index` is the byte index (NOT char index) of the cursor in `sql`.
    pub fn parse(sql: &str, cursor_char_index: usize) -> SqlContext {
        // Clamp cursor to valid range, then align to UTF-8 boundary
        let cursor = floor_char_boundary(sql, cursor_char_index.min(sql.len()));
        if Self::in_string_or_comment(sql, cursor) {
            return SqlContext::Suppressed;
        }
        let prefix = &sql[..cursor];
        let current_prefix = Self::current_token_prefix(sql, cursor_char_index);

        // --- 1) Check for `alias.` or `table.` pattern immediately before cursor ---
        if let Some(ctx) = Self::after_dot_context(sql, cursor) {
            return ctx;
        }

        // --- 2) INSERT INTO table (...) 整个列列表期间都保持列补全上下文 ---
        if Self::is_insert_columns_context(prefix) {
            return SqlContext::InsertColumns;
        }

        // --- 2) Walk backward from cursor to find the preceding keyword ---
        let tokens = Self::tokenize_backwards(prefix);
        let scan_tokens: &[String] = if !current_prefix.is_empty()
            && tokens
                .first()
                .is_some_and(|token| token.eq_ignore_ascii_case(&current_prefix))
        {
            &tokens[1..]
        } else {
            &tokens
        };

        for token in scan_tokens {
            let upper = token.to_ascii_uppercase();

            // Skip comma — keep looking
            if upper == "," {
                continue;
            }

            if upper == "(" {
                continue;
            }

            if upper == "AS" {
                return SqlContext::AliasName;
            }

            if Self::TABLE_KEYWORDS.contains(&upper.as_str()) {
                return SqlContext::AfterKeyword { keyword: upper };
            }

            if Self::COLUMN_KEYWORDS.contains(&upper.as_str()) {
                if matches!(upper.as_str(), "ORDER" | "GROUP" | "BY") {
                    return SqlContext::OrderGroupClause;
                }
                if matches!(upper.as_str(), "WHERE" | "ON" | "HAVING" | "AND" | "OR") {
                    return SqlContext::WhereClause;
                }
                return SqlContext::SelectClause;
            }

            // Any other known keyword means we're in a general context after it
            if Self::is_sql_keyword(&upper) {
                return SqlContext::General;
            }

            // A non-keyword token — if preceded by a comma, we're in a column list
            if Self::preceded_by_comma_in_scan(scan_tokens, &upper) {
                // Check what bigger clause we're in
                if let Some(clause_ctx) = Self::enclosing_clause(scan_tokens) {
                    return clause_ctx;
                }
                // Default: treat comma-separated list as columns
                return SqlContext::SelectClause;
            }

            break;
        }

        SqlContext::General
    }

    /// Extract the partial token the user is currently typing, just before the cursor.
    /// Returns the token text from after the last whitespace/comma/dot to the cursor.
    pub fn current_token_prefix(sql: &str, cursor_char_index: usize) -> String {
        let chars: Vec<char> = sql.chars().collect();
        let cursor = cursor_char_index.min(chars.len());
        let mut start = cursor;
        while start > 0 && is_identifier_char(chars[start - 1]) {
            start -= 1;
        }
        chars[start..cursor].iter().collect()
    }

    /// Return the identifier token bounds around the cursor as char offsets.
    pub fn current_token_bounds(sql: &str, cursor_char_index: usize) -> (usize, usize) {
        let chars: Vec<char> = sql.chars().collect();
        let cursor = cursor_char_index.min(chars.len());
        let mut start = cursor;
        while start > 0 && is_identifier_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = cursor;
        while end < chars.len() && is_identifier_char(chars[end]) {
            end += 1;
        }
        (start, end)
    }

    /// 判断光标是否位于字符串字面量或注释内。
    /// 位于其中时不提供补全，避免在 `'...'`、`-- ...`、`/* ... */` 里弹无关候选。
    /// 反引号/双引号包裹的是标识符（MySQL 反引号、PG 双引号），需要继续提示表名/列名。
    pub(crate) fn in_string_or_comment(sql: &str, cursor_byte: usize) -> bool {
        #[derive(PartialEq)]
        enum Mode {
            Normal,
            Single,
            Double,
            Backtick,
            LineComment,
            BlockComment,
        }

        let bytes = sql.as_bytes();
        let end = cursor_byte.min(bytes.len());
        let mut mode = Mode::Normal;
        let mut i = 0usize;
        while i < end {
            let b = bytes[i];
            match mode {
                Mode::Normal => {
                    if b == b'\'' {
                        mode = Mode::Single;
                        i += 1;
                    } else if b == b'"' {
                        mode = Mode::Double;
                        i += 1;
                    } else if b == b'`' {
                        mode = Mode::Backtick;
                        i += 1;
                    } else if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                        mode = Mode::LineComment;
                        i += 2;
                    } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                        mode = Mode::BlockComment;
                        i += 2;
                    } else if b == b'#' {
                        mode = Mode::LineComment;
                        i += 1;
                    } else {
                        i += 1;
                    }
                }
                Mode::Single => {
                    if b == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else if b == b'\'' {
                        // 兼容 '' 转义
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                        } else {
                            mode = Mode::Normal;
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                }
                Mode::Double => {
                    if b == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else if b == b'"' {
                        mode = Mode::Normal;
                        i += 1;
                    } else {
                        i += 1;
                    }
                }
                Mode::Backtick => {
                    if b == b'`' {
                        mode = Mode::Normal;
                    }
                    i += 1;
                }
                Mode::LineComment => {
                    if b == b'\n' {
                        mode = Mode::Normal;
                    }
                    i += 1;
                }
                Mode::BlockComment => {
                    if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                        mode = Mode::Normal;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
        }
        !matches!(mode, Mode::Normal | Mode::Double | Mode::Backtick)
    }

    /// Check if the cursor is right after `<identifier>.` — if so return AfterColumnDot.
    fn after_dot_context(sql: &str, cursor: usize) -> Option<SqlContext> {
        let prefix = &sql[..cursor];
        // Look for a dot immediately before the cursor or with only valid identifier chars between
        let bytes = prefix.as_bytes();
        let mut dot_pos: Option<usize> = None;
        for (i, &b) in bytes.iter().enumerate().rev() {
            if b == b'.' {
                dot_pos = Some(i);
                break;
            }
            if !(b.is_ascii_alphanumeric() || b == b'_') {
                break;
            }
        }
        let dot = dot_pos?;
        // Extract identifier before the dot
        let before_dot = &prefix[..dot];
        let parent: String = before_dot
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if parent.is_empty() {
            return None;
        }
        Some(SqlContext::AfterColumnDot { parent })
    }

    /// 解析光标所在整条语句引用的表与别名。
    /// 用整条语句而不是「光标前」的片段：在 SELECT 列表里输入时，
    /// FROM / JOIN 通常位于光标之后，仍需据此限定列候选。
    fn table_references_in_statement(sql: &str, cursor_char_index: usize) -> Vec<TableReference> {
        let cursor = floor_char_boundary(sql, cursor_char_index.min(sql.len()));
        let start = sql[..cursor].rfind(';').map(|idx| idx + 1).unwrap_or(0);
        let end = sql[cursor..]
            .find(';')
            .map(|idx| cursor + idx)
            .unwrap_or(sql.len());
        let tokens = Self::tokenize_forwards(&sql[start..end]);
        let mut refs = Vec::new();
        let mut i = 0usize;
        while i < tokens.len() {
            let upper = tokens[i].to_ascii_uppercase();
            if matches!(upper.as_str(), "FROM" | "JOIN" | "UPDATE" | "INTO") {
                if let Some((table_name, next_idx)) = Self::next_identifier_token(&tokens, i + 1) {
                    let mut alias = None;
                    let mut j = next_idx;
                    if j < tokens.len() {
                        let next_upper = tokens[j].to_ascii_uppercase();
                        if next_upper == "AS" {
                            if let Some((alias_name, alias_idx)) =
                                Self::next_identifier_token(&tokens, j + 1)
                            {
                                alias = Some(alias_name);
                                j = alias_idx;
                            }
                        } else if Self::is_identifier_token(&tokens[j])
                            && !Self::is_clause_boundary(&next_upper)
                        {
                            alias = Some(tokens[j].clone());
                            j += 1;
                        }
                    }
                    refs.push(TableReference { table_name, alias });
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        refs
    }

    fn insert_target_table(sql: &str, cursor_char_index: usize) -> Option<String> {
        let cursor = floor_char_boundary(sql, cursor_char_index.min(sql.len()));
        let tokens = Self::tokenize_forwards(&sql[..cursor]);
        for idx in 0..tokens.len() {
            if tokens[idx].eq_ignore_ascii_case("INTO") {
                if let Some((table_name, _)) = Self::next_identifier_token(&tokens, idx + 1) {
                    return Some(table_name);
                }
            }
        }
        None
    }

    /// Backwards tokenizer: returns tokens from right-to-left, uppercased.
    fn tokenize_backwards(sql: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut i = sql.len();
        let bytes = sql.as_bytes();
        while i > 0 {
            // skip whitespace
            while i > 0 && bytes[i - 1].is_ascii_whitespace() {
                i -= 1;
            }
            if i == 0 {
                break;
            }
            let end = i;
            i -= 1;
            let b = bytes[i];
            if b == b',' || b == b'(' || b == b')' || b == b';' {
                tokens.push(String::from_utf8_lossy(&bytes[i..end]).to_string());
                continue;
            }
            if b == b'`' || b == b'\'' || b == b'"' {
                // skip quoted strings
                let quote = b;
                while i > 0 {
                    i -= 1;
                    if bytes[i] == quote {
                        // Check for escaped quote
                        if i > 0 && bytes[i - 1] == b'\\' {
                            i -= 1;
                            continue;
                        }
                        break;
                    }
                }
                continue;
            }
            // identifier / keyword
            while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_' || bytes[i - 1] == b'.') {
                i -= 1;
            }
            tokens.push(String::from_utf8_lossy(&bytes[i..end]).to_string());
        }
        tokens
    }

    fn tokenize_forwards(sql: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let bytes = sql.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let start = i;
            let b = bytes[i];
            if matches!(b, b',' | b'(' | b')' | b';') {
                tokens.push(String::from_utf8_lossy(&bytes[i..=i]).to_string());
                i += 1;
                continue;
            }
            if b == b'\'' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' && (i == 0 || bytes[i - 1] != b'\\') {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            if b == b'`' || b == b'"' {
                let quote = b;
                i += 1;
                let ident_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                if i > ident_start {
                    tokens.push(String::from_utf8_lossy(&bytes[ident_start..i]).to_string());
                }
                if i < bytes.len() {
                    i += 1;
                }
                continue;
            }
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'.'))
            {
                i += 1;
            }
            if i > start {
                tokens.push(String::from_utf8_lossy(&bytes[start..i]).to_string());
            } else {
                i += 1;
            }
        }
        tokens
    }

    fn is_sql_keyword(token: &str) -> bool {
        SQL_KEYWORDS.contains(&token)
    }

    fn is_insert_columns_context(prefix: &str) -> bool {
        let tokens = Self::tokenize_forwards(prefix);
        let mut saw_insert = false;
        let mut saw_into = false;
        let mut saw_target = false;
        for token in tokens {
            let upper = token.to_ascii_uppercase();
            if !saw_insert {
                if upper == "INSERT" {
                    saw_insert = true;
                }
                continue;
            }
            if !saw_into {
                if upper == "INTO" {
                    saw_into = true;
                }
                continue;
            }
            if !saw_target {
                if Self::is_identifier_token(&token) {
                    saw_target = true;
                }
                continue;
            }
            if upper == "VALUES" || upper == "SELECT" {
                return false;
            }
            if token == "(" {
                return true;
            }
        }
        false
    }

    fn alias_source_token(sql: &str, cursor_char_index: usize) -> Option<String> {
        let cursor = floor_char_boundary(sql, cursor_char_index.min(sql.len()));
        let tokens = Self::tokenize_forwards(&sql[..cursor]);
        let as_index = tokens
            .iter()
            .rposition(|token| token.eq_ignore_ascii_case("AS"))?;
        tokens[..as_index]
            .iter()
            .rev()
            .find(|token| Self::is_identifier_token(token))
            .cloned()
    }

    fn preceded_by_comma_in_scan(tokens: &[String], _current: &str) -> bool {
        tokens.first().map(|t| t == ",").unwrap_or(false)
    }

    fn enclosing_clause(tokens: &[String]) -> Option<SqlContext> {
        for t in tokens.iter().skip(1) {
            let u = t.to_ascii_uppercase();
            if u == "WHERE" || u == "ON" || u == "HAVING" {
                return Some(SqlContext::WhereClause);
            }
            if u == "SELECT" {
                return Some(SqlContext::SelectClause);
            }
            if u == "ORDER" || u == "GROUP" {
                return Some(SqlContext::OrderGroupClause);
            }
        }
        None
    }

    fn next_identifier_token(tokens: &[String], start: usize) -> Option<(String, usize)> {
        let mut idx = start;
        while idx < tokens.len() {
            if Self::is_identifier_token(&tokens[idx]) {
                return Some((tokens[idx].clone(), idx + 1));
            }
            if matches!(tokens[idx].as_str(), "(" | ")" | "," | ";") {
                return None;
            }
            idx += 1;
        }
        None
    }

    fn is_identifier_token(token: &str) -> bool {
        !token.is_empty()
            && !matches!(token, "(" | ")" | "," | ";")
            && !Self::is_sql_keyword(&token.to_ascii_uppercase())
    }

    fn is_clause_boundary(token_upper: &str) -> bool {
        matches!(
            token_upper,
            "WHERE"
                | "ON"
                | "GROUP"
                | "ORDER"
                | "HAVING"
                | "LIMIT"
                | "OFFSET"
                | "JOIN"
                | "LEFT"
                | "RIGHT"
                | "INNER"
                | "OUTER"
                | "FULL"
                | "CROSS"
                | "NATURAL"
                | "SET"
                | "VALUES"
                | "SELECT"
                | "UNION"
        )
    }
}

/// All uppercase SQL keywords used for context detection + suggestion.
pub(crate) const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "ORDER", "BY", "GROUP", "HAVING", "LIMIT",
    "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "JOIN", "LEFT",
    "RIGHT", "INNER", "OUTER", "FULL", "CROSS", "NATURAL", "ON",
    "AS", "AND", "OR", "NOT", "NULL", "IS", "IN", "EXISTS",
    "CREATE", "ALTER", "DROP", "TABLE", "VIEW", "DATABASE", "SCHEMA",
    "INDEX", "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "DISTINCT",
    "UNION", "ALL", "CASE", "WHEN", "THEN", "ELSE", "END",
    "LIKE", "DESC", "ASC", "OFFSET", "LIMIT", "BETWEEN", "COUNT",
    "SUM", "AVG", "MIN", "MAX", "TRUE", "FALSE", "IF", "UNIQUE",
    "ADD", "COLUMN", "RENAME", "TO", "DEFAULT", "CHECK", "CONSTRAINT",
    "CASCADE", "RESTRICT", "TRUNCATE", "REPLACE", "USE", "SHOW",
    "DESCRIBE", "EXPLAIN", "ANALYZE", "BEGIN", "COMMIT", "ROLLBACK",
    "GRANT", "REVOKE", "WITH", "RECURSIVE", "OVER", "PARTITION",
    "ROW", "ROWS", "RANGE", "UNBOUNDED", "PRECEDING", "FOLLOWING",
    "CURRENT", "INTERVAL", "CAST", "COALESCE", "NULLIF",
];

/// 关键字常用度排序：越靠前越常用，未列出的排在后面。
const KEYWORD_RANK: &[&str] = &[
    "SELECT", "FROM", "WHERE", "JOIN", "LEFT", "INNER", "ON", "GROUP", "ORDER", "BY",
    "HAVING", "LIMIT", "OFFSET", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE",
    "CREATE", "ALTER", "DROP", "TABLE", "VIEW", "AS", "AND", "OR", "NOT", "IS", "IN",
    "LIKE", "BETWEEN", "NULL", "DISTINCT", "UNION", "ALL", "CASE", "WHEN", "THEN", "ELSE",
    "END", "COUNT", "SUM", "AVG", "MIN", "MAX", "DESC", "ASC", "EXISTS", "TRUE", "FALSE",
];

fn keyword_rank(label: &str) -> usize {
    KEYWORD_RANK
        .iter()
        .position(|kw| kw.eq_ignore_ascii_case(label))
        .unwrap_or(KEYWORD_RANK.len())
}

/// 语句骨架片段：触发词 → (插入文本, 首个空位的光标偏移)。
const SQL_STATEMENT_SNIPPETS: &[(&str, &str, usize)] = &[
    ("sel", "SELECT * FROM ", 14),
    ("ins", "INSERT INTO  () VALUES ()", 12),
    ("upd", "UPDATE  SET  WHERE ", 7),
    ("del", "DELETE FROM  WHERE ", 12),
    ("joi", "JOIN  ON ", 5),
    ("whr", "WHERE ", 6),
];

const MONGO_METHODS: &[&str] = &[
    "find", "findOne", "aggregate", "insertOne", "insertMany",
    "updateOne", "updateMany", "deleteOne", "deleteMany",
    "countDocuments", "distinct", "createIndex", "dropIndex", "drop",
    "bulkWrite", "replaceOne", "findOneAndUpdate", "findOneAndDelete",
    "findOneAndReplace",
];

const MONGO_CURSOR_METHODS: &[&str] = &[
    "sort", "limit", "skip", "project", "collation", "hint", "comment", "explain",
    "allowDiskUse", "batchSize", "maxTimeMS",
];

const MONGO_OPERATORS: &[&str] = &[
    "$match", "$group", "$sort", "$limit", "$skip", "$project",
    "$unwind", "$lookup", "$addFields", "$count", "$out", "$merge",
    "$set", "$unset", "$push", "$pull", "$addToSet", "$rename",
    "$min", "$max", "$currentDate", "$mul", "$inc",
    "$gt", "$gte", "$lt", "$lte", "$eq", "$ne", "$in", "$nin",
    "$and", "$or", "$not", "$nor", "$exists", "$type", "$regex",
    "$sum", "$avg", "$first", "$last", "$size", "$elemMatch", "$all", "$slice",
    "$arrayFilters", "$position",
];

const MONGO_UPDATE_OPERATORS: &[&str] = &[
    "$set", "$unset", "$inc", "$mul", "$min", "$max", "$rename",
    "$push", "$pull", "$addToSet", "$pop", "$currentDate",
];

/// 返回方法的参数模板和光标偏移（相对于模板起始位置）
fn mongo_method_template(method: &str) -> (String, usize) {
    match method {
        "find" | "findOne" | "deleteOne" | "deleteMany" | "countDocuments" |
        "findOneAndDelete" | "drop" => {
            let t = if method == "drop" {
                format!("{method}()")
            } else {
                format!("{method}({{ field: value }})")
            };
            let c = method.len() + 2;
            (t, c)
        }
        "insertOne" | "replaceOne" => {
            (
                format!("{method}({{ field: value }})"),
                method.len() + 2,
            )
        }
        "updateOne" | "updateMany" | "findOneAndUpdate" | "findOneAndReplace" => {
            (
                format!("{method}({{ _id: value }}, {{ $set: {{ field: value }} }})"),
                method.len() + 2,
            )
        }
        "insertMany" | "bulkWrite" | "aggregate" => {
            let template = match method {
                "aggregate" => "([{ $match: { field: value } }])".to_string(),
                _ => "([{ field: value }])".to_string(),
            };
            (format!("{method}{template}"), method.len() + 2)
        }
        "distinct" => {
            ("distinct(\"field\")".to_string(), 10)
        }
        "createIndex" | "dropIndex" => {
            let template = if method == "createIndex" {
                "({ field: 1 }, { name: \"idx_field\" })"
            } else {
                "(\"idx_field\")"
            };
            (format!("{method}{template}"), method.len() + 1)
        }
        // Cursor methods
        "sort" | "project" | "hint" | "collation" => {
            let template = match method {
                "sort" | "hint" => "({ field: 1 })",
                "project" => "({ field: 1, _id: 0 })",
                _ => "({ locale: \"en\" })",
            };
            (format!("{method}{template}"), method.len() + 2)
        }
        "limit" | "skip" | "batchSize" | "maxTimeMS" => {
            let value = match method {
                "limit" | "batchSize" => "10",
                "skip" => "0",
                _ => "1000",
            };
            (format!("{method}({value})"), method.len() + 1)
        }
        "comment" => ("comment(\"query intent\")".to_string(), 9),
        "explain" | "allowDiskUse" => {
            let template = if method == "allowDiskUse" { "(true)" } else { "()" };
            (format!("{method}{template}"), method.len() + 1)
        }
        _ => (format!("{}()", method), method.len() + 1),
    }
}

/// MongoDB 查询上下文
enum MongoContext {
    /// db. → 建议集合名
    AfterDbDot,
    /// db.coll. → 建议方法名
    AfterCollection { collection: String },
    /// db.coll.method( → 在方法参数内，建议操作符
    AfterMethod {
        collection: String,
        method: String,
        arg_index: usize,
        field_context: bool,
        field_path_prefix: Option<String>,
        active_object_key: Option<String>,
    },
    /// 非 MongoDB 查询
    NotMongo,
}

/// 将字节索引安全地钳制到最近的 UTF-8 字符边界
fn clamp_to_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    loop {
        if s.is_char_boundary(i) {
            return i;
        }
        i -= 1;
    }
}

fn detect_mongo_context(sql: &str, cursor: usize) -> MongoContext {
    let cursor = clamp_to_char_boundary(sql, cursor);
    let before = &sql[..cursor];
    if !before.starts_with("db.") && !before.contains("\ndb.") {
        return MongoContext::NotMongo;
    }

    let prefix = if let Some(pos) = before.rfind("\ndb.") {
        &before[pos + 1..]
    } else if before.starts_with("db.") {
        before
    } else {
        return MongoContext::NotMongo;
    };

    if !prefix.starts_with("db.") {
        return MongoContext::NotMongo;
    }

    let after_db = &prefix[3..]; // everything after "db."

    // Check for collection + dot (with optional method): db.coll. or db.coll.method(
    if let Some(caps) = regex_captures2(r"^([a-zA-Z0-9_]+)\.(.*)$", after_db) {
        let collection = caps.1.to_string();
        let rest = caps.2;

        if !rest.is_empty() {
            // Extract the current method being typed/used
            // For "find(" → method = "find", for "find().sort(" → method = "sort"
            let method_part = if let Some(paren_pos) = rest.rfind('(') {
                &rest[..paren_pos]
            } else {
                rest
            };
            let method = method_part.rsplit('.').next().unwrap_or("").trim();
            if !method.is_empty() && method.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                // Has opening paren → inside method args
                if let Some(paren_pos) = rest.rfind('(') {
                    let args = &rest[paren_pos + 1..];
                    return MongoContext::AfterMethod {
                        collection,
                        method: method.to_string(),
                        arg_index: mongo_arg_index(args),
                        field_context: mongo_is_field_context(args),
                        field_path_prefix: mongo_field_path_prefix(args),
                        active_object_key: mongo_active_object_key(args),
                    };
                }
                // No paren yet → still typing method name, show collection methods
                return MongoContext::AfterCollection { collection };
            }
        }

        // rest is empty or only dot → AfterCollection
        return MongoContext::AfterCollection { collection };
    }

    // db. with nothing or partial identifier after
    if after_db.is_empty()
        || after_db.ends_with('.')
        || after_db.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return MongoContext::AfterDbDot;
    }

    MongoContext::NotMongo
}

fn mongo_arg_index(args: &str) -> usize {
    let mut depth_brace = 0usize;
    let mut depth_bracket = 0usize;
    let mut depth_paren = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escape = false;
    let mut commas = 0usize;
    for ch in args.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == quote {
                in_string = false;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_string = true;
            quote = ch;
            continue;
        }
        match ch {
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            ',' if depth_brace == 0 && depth_bracket == 0 && depth_paren == 0 => commas += 1,
            _ => {}
        }
    }
    commas
}

fn mongo_is_field_context(args: &str) -> bool {
    let trimmed = args.trim_end();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.ends_with('{') || trimmed.ends_with(',') {
        return true;
    }
    if trimmed.ends_with(':') {
        return false;
    }
    let mut depth_brace = 0usize;
    let mut depth_bracket = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escape = false;
    let mut last_top_level_colon = false;
    for ch in trimmed.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == quote {
                in_string = false;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_string = true;
            quote = ch;
            continue;
        }
        match ch {
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            ':' if depth_brace == 1 && depth_bracket == 0 => last_top_level_colon = true,
            ',' if depth_brace == 1 && depth_bracket == 0 => last_top_level_colon = false,
            ',' if depth_brace == 0 && depth_bracket == 0 => last_top_level_colon = false,
            _ => {}
        }
    }
    !last_top_level_colon
}

fn mongo_field_path_prefix(args: &str) -> Option<String> {
    let trimmed = args.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let mut start = trimmed.len();
    while start > 0 {
        let ch = trimmed[..start].chars().next_back()?;
        if ch.is_alphanumeric() || matches!(ch, '_' | '.' | '"' | '\'') {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }
    let prefix = trimmed[start..]
        .trim_matches(|c| c == '"' || c == '\'')
        .trim();
    (!prefix.is_empty()).then(|| prefix.to_string())
}

fn mongo_active_object_key(args: &str) -> Option<String> {
    let trimmed = args.trim_end();
    let brace_pos = trimmed.rfind('{')?;
    let before = trimmed[..brace_pos].trim_end();
    let before = before.strip_suffix(':')?.trim_end();
    let mut start = before.len();
    while start > 0 {
        let ch = before[..start].chars().next_back()?;
        if ch.is_alphanumeric() || matches!(ch, '_' | '.' | '$' | '"' | '\'') {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }
    let key = before[start..]
        .trim_matches(|c| c == '"' || c == '\'')
        .trim();
    (!key.is_empty()).then(|| key.to_string())
}

fn mongo_field_insertion_text(label: &str, path_prefix: Option<&str>) -> Option<String> {
    let prefix = path_prefix
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty())
        .map(|prefix| prefix.trim_matches(|c| c == '"' || c == '\''))
        .unwrap_or("");
    if prefix.is_empty() {
        return Some(label.to_string());
    }

    // 补全范围是「当前正在输入的末段」：保留已输入的父路径，只补剩余部分。
    if let Some((parent_path, typed_segment)) = prefix.rsplit_once('.') {
        let parent_path = parent_path.trim_matches(|c| c == '"' || c == '\'');
        if parent_path.is_empty() {
            return Some(label.to_string());
        }
        let parent_prefix = format!("{parent_path}.");
        if let Some(remainder) = label.strip_prefix(&parent_prefix) {
            if typed_segment.is_empty() {
                return Some(remainder.to_string());
            }
            // user.pr → ofile.nickname
            if let Some(tail) = remainder.strip_prefix(typed_segment) {
                return Some(tail.to_string());
            }
            return Some(remainder.to_string());
        }
        if label == parent_path {
            return Some(label.to_string());
        }
        return None;
    }

    if label.starts_with(prefix) {
        return Some(label.to_string());
    }
    None
}

/// Extract captures from a regex-like pattern (simplified helper).
fn regex_captures<'a>(pattern: &str, text: &'a str) -> Option<(&'a str, &'a str)> {
    // Manually implement: ^([a-zA-Z0-9_]+)\.[a-zA-Z0-9_]*\.?$
    if !pattern.contains("[a-zA-Z0-9_]+") {
        return None;
    }
    let bytes = text.as_bytes();
    // Match ^[a-zA-Z0-9_]+\.
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i == 0 || i >= bytes.len() || bytes[i] != b'.' {
        return None;
    }
    let first = &text[..i];
    i += 1; // skip .
    // Match [a-zA-Z0-9_]*\.?$
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    // Optional trailing dot
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
    }
    if i != bytes.len() {
        return None;
    }
    Some((text, first))
}

/// Extract captures: ^([a-zA-Z0-9_]+)\.(.*)$
fn regex_captures2<'a>(pattern: &str, text: &'a str) -> Option<(&'a str, &'a str, &'a str)> {
    if !pattern.contains("[a-zA-Z0-9_]+") || !pattern.contains("(.*)") {
        return None;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i == 0 || i >= bytes.len() || bytes[i] != b'.' {
        return None;
    }
    let first = &text[..i];
    i += 1; // skip .
    let second = &text[i..];
    Some((text, first, second))
}

pub(crate) struct AutocompleteEngine;

impl AutocompleteEngine {
    /// Generate ranked suggestions based on current SQL and cursor position.
    pub fn suggest(
        sql: &str,
        cursor_char_index: usize,
        cache: &SchemaCache,
        connection_id: Option<&str>,
        database: Option<&str>,
        db_kind: Option<DatabaseKind>,
        usage_memory: Option<&AutocompleteUsageMemory>,
    ) -> Vec<AutocompleteSuggestion> {
        // MongoDB 模式下优先检测 MongoDB 查询
        if db_kind == Some(DatabaseKind::MongoDb) {
            let mongo_ctx = detect_mongo_context(sql, cursor_char_index);
            if !matches!(mongo_ctx, MongoContext::NotMongo) {
                let prefix = SqlContextParser::current_token_prefix(sql, cursor_char_index);
                return Self::suggest_mongo(
                    mongo_ctx,
                    &prefix,
                    cache,
                    connection_id,
                    database,
                    usage_memory,
                );
            }
        }

        let prefix = SqlContextParser::current_token_prefix(sql, cursor_char_index);
        let context = SqlContextParser::parse(sql, cursor_char_index);
        if matches!(context, SqlContext::Suppressed) {
            return Vec::new();
        }
        let table_refs = SqlContextParser::table_references_in_statement(sql, cursor_char_index);

        let mut suggestions = Vec::new();

        match &context {
            SqlContext::Suppressed => return Vec::new(),
            SqlContext::AfterColumnDot { parent } => {
                // 1. 库名. / schema 名 → 该库或 schema 下的表
                if let Some(conn) = connection_id {
                    let mut tables = cache.tables_for_database(conn, parent);
                    let mut parent_database: Option<&str> = Some(parent.as_str());
                    if tables.is_empty() {
                        tables = cache.tables_for_schema(conn, parent);
                        parent_database = database;
                    }
                    if !tables.is_empty() {
                        for table_name in tables {
                            let is_view = cache.is_view_of(Some(conn), parent_database, &table_name);
                            suggestions.push(Self::table_suggestion(table_name, is_view));
                        }
                        return Self::finish(suggestions, &prefix, &context, &table_refs, usage_memory);
                    }
                }

                // 2. 别名 / 表名 → 该表的列
                let resolved_parent = table_refs
                    .iter()
                    .find(|r| r.alias.as_deref() == Some(parent.as_str()))
                    .map(|r| r.table_name.as_str())
                    .unwrap_or(parent.as_str());
                if let Some(cols) =
                    cache.columns_for_table(connection_id, database, resolved_parent)
                {
                    for col in cols {
                        suggestions.push(Self::column_suggestion(resolved_parent, col));
                    }
                } else if !prefix.is_empty() {
                    // 3. 既不是库/模式，也不是已知表/别名：退化为表名提示。
                    // 缓存里的表可能没带 schema 信息（如 PG 的 public.xxx），
                    // 这里不要让用户完全看不到候选。
                    Self::push_scoped_tables(&mut suggestions, cache, connection_id, database);
                }
                return Self::finish(suggestions, &prefix, &context, &table_refs, usage_memory);
            }
            SqlContext::AfterKeyword { .. } => {
                // 表位置只提示当前连接（及当前库）的表 / 视图
                Self::push_scoped_tables(&mut suggestions, cache, connection_id, database);
                if !prefix.is_empty() {
                    Self::push_databases_and_schemas(
                        &mut suggestions,
                        cache,
                        connection_id,
                        database,
                    );
                }
            }
            SqlContext::InsertColumns => {
                let mut pushed_target = false;
                if let Some(target_table) =
                    SqlContextParser::insert_target_table(sql, cursor_char_index)
                {
                    if let Some(cols) =
                        cache.columns_for_table(connection_id, database, &target_table)
                    {
                        for col in cols {
                            suggestions.push(Self::column_suggestion(&target_table, col));
                        }
                        pushed_target = true;
                    }
                }
                if !pushed_target {
                    Self::push_scoped_columns(&mut suggestions, cache, connection_id, database);
                }
            }
            SqlContext::AliasName => {
                suggestions.extend(Self::alias_suggestions(sql, cursor_char_index, &table_refs));
            }
            SqlContext::SelectClause
            | SqlContext::WhereClause
            | SqlContext::OrderGroupClause => {
                // 优先当前语句已引用的表；否则回退到当前连接/库的全部列
                let mut pushed_referenced = false;
                for table_ref in &table_refs {
                    if let Some(cols) =
                        cache.columns_for_table(connection_id, database, &table_ref.table_name)
                    {
                        for col in cols {
                            suggestions.push(Self::column_suggestion(&table_ref.table_name, col));
                        }
                        pushed_referenced = true;
                    }
                }
                if !pushed_referenced {
                    Self::push_scoped_columns(&mut suggestions, cache, connection_id, database);
                }
                if !prefix.is_empty() {
                    Self::push_keywords(&mut suggestions);
                    Self::push_function_snippets(&mut suggestions, &context, db_kind);
                } else if matches!(context, SqlContext::WhereClause) {
                    Self::push_where_operator_snippets(&mut suggestions);
                }
            }
            SqlContext::General => {
                // 语句开头：没有前缀不弹，避免空白处刷屏
                if prefix.is_empty() {
                    return Vec::new();
                }
                Self::push_keywords(&mut suggestions);
                if Self::is_statement_start(sql, cursor_char_index) {
                    Self::push_statement_snippets(&mut suggestions);
                }
                Self::push_function_snippets(&mut suggestions, &context, db_kind);
                Self::push_scoped_tables(&mut suggestions, cache, connection_id, database);
                Self::push_databases_and_schemas(&mut suggestions, cache, connection_id, database);
            }
        }

        Self::finish(suggestions, &prefix, &context, &table_refs, usage_memory)
    }

    fn finish(
        suggestions: Vec<AutocompleteSuggestion>,
        prefix: &str,
        context: &SqlContext,
        table_refs: &[TableReference],
        usage_memory: Option<&AutocompleteUsageMemory>,
    ) -> Vec<AutocompleteSuggestion> {
        let filtered = Self::filter_by_prefix(suggestions, prefix);
        Self::rank(filtered, prefix, context, table_refs, usage_memory)
    }

    fn table_suggestion(name: String, is_view: bool) -> AutocompleteSuggestion {
        AutocompleteSuggestion::new(
            name,
            if is_view {
                SuggestionKind::View
            } else {
                SuggestionKind::Table
            },
        )
    }

    fn column_suggestion(table: &str, col: &ColumnDefinition) -> AutocompleteSuggestion {
        AutocompleteSuggestion::new(
            col.name.clone(),
            SuggestionKind::Column {
                parent_table: table.to_string(),
            },
        )
    }

    fn push_scoped_tables(
        suggestions: &mut Vec<AutocompleteSuggestion>,
        cache: &SchemaCache,
        connection_id: Option<&str>,
        database: Option<&str>,
    ) {
        for (name, is_view) in cache.table_names_for(connection_id, database) {
            suggestions.push(Self::table_suggestion(name.to_string(), is_view));
        }
    }

    fn push_scoped_columns(
        suggestions: &mut Vec<AutocompleteSuggestion>,
        cache: &SchemaCache,
        connection_id: Option<&str>,
        database: Option<&str>,
    ) {
        for (table_name, col) in cache.column_entries_for_scope(connection_id, database) {
            suggestions.push(Self::column_suggestion(table_name, col));
        }
    }

    fn push_databases_and_schemas(
        suggestions: &mut Vec<AutocompleteSuggestion>,
        cache: &SchemaCache,
        connection_id: Option<&str>,
        _database: Option<&str>,
    ) {
        let Some(connection_id) = connection_id else {
            return;
        };
        for name in cache.database_names_for(connection_id) {
            suggestions.push(AutocompleteSuggestion::new(
                name.to_string(),
                SuggestionKind::Database,
            ));
        }
        for name in cache.schema_names_for(connection_id) {
            suggestions.push(AutocompleteSuggestion::new(
                name.to_string(),
                SuggestionKind::Schema,
            ));
        }
    }

    fn push_keywords(suggestions: &mut Vec<AutocompleteSuggestion>) {
        for kw in SQL_KEYWORDS {
            suggestions.push(AutocompleteSuggestion::new(
                (*kw).to_string(),
                SuggestionKind::Keyword,
            ));
        }
    }

    /// 光标是否位于语句开头（当前正在输入的标识符之前只有空白与注释）。
    fn is_statement_start(sql: &str, cursor_char_index: usize) -> bool {
        let cursor = floor_char_boundary(sql, cursor_char_index.min(sql.len()));
        let prefix = SqlContextParser::current_token_prefix(sql, cursor);
        let token_start = cursor.saturating_sub(prefix.len());
        let before_token = &sql[..token_start];
        let segment = before_token.rsplit(';').next().unwrap_or("");
        segment.trim().is_empty()
    }


    fn suggest_mongo(
        ctx: MongoContext,
        prefix: &str,
        cache: &SchemaCache,
        connection_id: Option<&str>,
        database: Option<&str>,
        usage_memory: Option<&AutocompleteUsageMemory>,
    ) -> Vec<AutocompleteSuggestion> {
        let mut suggestions = Vec::new();
        match ctx {
            MongoContext::AfterDbDot => {
                for (name, _is_view) in cache.table_names_for(connection_id, database) {
                    suggestions.push(AutocompleteSuggestion::new(
                        name.to_string(),
                        SuggestionKind::Table,
                    ));
                }
            }
            MongoContext::AfterCollection { .. } => {
                for m in MONGO_METHODS {
                    let (insertion, cursor_off) = mongo_method_template(m);
                    suggestions.push(AutocompleteSuggestion {
                        label: m.to_string(),
                        kind: SuggestionKind::MongoKeyword,
                        matched_indices: Vec::new(),
                        insertion_text: Some(insertion),
                        cursor_offset: Some(cursor_off),
                    });
                }
                for m in MONGO_CURSOR_METHODS {
                    let (insertion, cursor_off) = mongo_method_template(m);
                    suggestions.push(AutocompleteSuggestion {
                        label: m.to_string(),
                        kind: SuggestionKind::MongoKeyword,
                        matched_indices: Vec::new(),
                        insertion_text: Some(insertion),
                        cursor_offset: Some(cursor_off),
                    });
                }
            }
            MongoContext::AfterMethod {
                collection,
                method,
                arg_index,
                field_context,
                field_path_prefix,
                active_object_key,
            } => {
                match method.as_str() {
                    "find" | "findOne" | "deleteOne" | "deleteMany" | "countDocuments" => {
                        if field_context {
                            Self::push_mongo_collection_fields(
                                &mut suggestions,
                                cache,
                                connection_id,
                                database,
                                &collection,
                                field_path_prefix.as_deref(),
                            );
                        } else {
                            Self::push_mongo_operators(&mut suggestions, MONGO_OPERATORS);
                        }
                    }
                    "sort" | "project" | "hint" => {
                        Self::push_mongo_collection_fields(
                            &mut suggestions,
                            cache,
                            connection_id,
                            database,
                            &collection,
                            field_path_prefix.as_deref(),
                        );
                    }
                    "updateOne" | "updateMany" | "findOneAndUpdate" => {
                        if arg_index == 0 {
                            if field_context {
                                Self::push_mongo_collection_fields(
                                    &mut suggestions,
                                    cache,
                                    connection_id,
                                    database,
                                    &collection,
                                    field_path_prefix.as_deref(),
                                );
                            } else {
                                Self::push_mongo_operators(&mut suggestions, MONGO_OPERATORS);
                            }
                        } else if matches!(
                            active_object_key.as_deref(),
                            Some("$set" | "$project" | "$sort")
                        ) {
                            Self::push_mongo_collection_fields(
                                &mut suggestions,
                                cache,
                                connection_id,
                                database,
                                &collection,
                                field_path_prefix.as_deref(),
                            );
                        } else if field_context {
                            Self::push_mongo_operators(&mut suggestions, MONGO_UPDATE_OPERATORS);
                        } else {
                            Self::push_mongo_collection_fields(
                                &mut suggestions,
                                cache,
                                connection_id,
                                database,
                                &collection,
                                field_path_prefix.as_deref(),
                            );
                        }
                    }
                    _ => {
                        Self::push_mongo_operators(&mut suggestions, MONGO_OPERATORS);
                    }
                }
            }
            MongoContext::NotMongo => {}
        }
        let filtered = Self::filter_by_prefix(suggestions, prefix);
        Self::rank(filtered, prefix, &SqlContext::General, &[], usage_memory)
    }

    fn push_mongo_collection_fields(
        suggestions: &mut Vec<AutocompleteSuggestion>,
        cache: &SchemaCache,
        connection_id: Option<&str>,
        database: Option<&str>,
        collection: &str,
        path_prefix: Option<&str>,
    ) {
        if let Some(cols) = cache.columns_for_table(connection_id, database, collection) {
            let mut seen = HashMap::<String, String>::new();
            for col in cols {
                let Some(insertion_text) =
                    mongo_field_insertion_text(&col.name, path_prefix)
                else {
                    continue;
                };
                seen.entry(col.name.clone()).or_insert(insertion_text);
            }
            for (label, insertion_text) in seen {
                suggestions.push(AutocompleteSuggestion {
                    label,
                    kind: SuggestionKind::Column {
                        parent_table: collection.to_string(),
                    },
                    matched_indices: Vec::new(),
                    insertion_text: Some(insertion_text),
                    cursor_offset: None,
                });
            }
        }
    }

    fn push_mongo_operators(
        suggestions: &mut Vec<AutocompleteSuggestion>,
        operators: &[&str],
    ) {
        for op in operators {
            suggestions.push(AutocompleteSuggestion::new(
                op.to_string(),
                SuggestionKind::MongoKeyword,
            ));
        }
    }

    fn filter_by_prefix(
        suggestions: Vec<AutocompleteSuggestion>,
        prefix: &str,
    ) -> Vec<AutocompleteSuggestion> {
        if prefix.is_empty() {
            return suggestions;
        }
        let prefix_lower = prefix.to_lowercase();
        let prefix_len = prefix_lower.chars().count();
        let prefix_chars: Vec<char> = prefix_lower.chars().collect();
        let mut matched = Vec::new();
        for mut s in suggestions {
            let keyword_like = matches!(
                s.kind,
                SuggestionKind::Keyword | SuggestionKind::Snippet | SuggestionKind::Function
            );
            if s.label.to_lowercase().starts_with(&prefix_lower) {
                s.matched_indices = (0..prefix_len).collect();
                matched.push(s);
            } else if !keyword_like {
                // 标识符支持子串匹配（ord → orders），排序时前缀命中优先
                if let Some(indices) = subsequence_match(&s.label, &prefix_chars) {
                    s.matched_indices = indices;
                    matched.push(s);
                }
            }
        }
        matched
    }

    fn rank(
        mut suggestions: Vec<AutocompleteSuggestion>,
        prefix: &str,
        context: &SqlContext,
        table_refs: &[TableReference],
        usage_memory: Option<&AutocompleteUsageMemory>,
    ) -> Vec<AutocompleteSuggestion> {
        let prefix_lower = prefix.to_lowercase();

        let context_priority = |s: &AutocompleteSuggestion| -> i32 {
            match context {
                SqlContext::AfterKeyword { .. } => match s.kind {
                    SuggestionKind::Table | SuggestionKind::View => 5,
                    SuggestionKind::Database | SuggestionKind::Schema => 3,
                    SuggestionKind::Keyword => 1,
                    _ => 0,
                },
                SqlContext::AfterColumnDot { .. } | SqlContext::InsertColumns => match s.kind {
                    SuggestionKind::Column { .. } => 5,
                    _ => 0,
                },
                SqlContext::WhereClause
                | SqlContext::OrderGroupClause
                | SqlContext::SelectClause => match &s.kind {
                    SuggestionKind::Column { parent_table } => {
                        let referenced = table_refs.iter().any(|r| {
                            r.table_name.eq_ignore_ascii_case(parent_table)
                                || r.alias.as_deref() == Some(parent_table.as_str())
                        });
                        if referenced {
                            5
                        } else {
                            3
                        }
                    }
                    SuggestionKind::Function => 2,
                    SuggestionKind::Keyword => 1,
                    _ => 0,
                },
                SqlContext::AliasName => match s.kind {
                    SuggestionKind::Keyword => 5,
                    _ => 0,
                },
                _ => match s.kind {
                    SuggestionKind::Keyword => 6,
                    SuggestionKind::Snippet => 5,
                    SuggestionKind::Table | SuggestionKind::View => 4,
                    SuggestionKind::Database => 3,
                    SuggestionKind::Schema => 2,
                    SuggestionKind::Function => 1,
                    _ => 0,
                },
            }
        };

        suggestions.sort_by(|a, b| {
            let a_exact = a.label.to_lowercase() == prefix_lower;
            let b_exact = b.label.to_lowercase() == prefix_lower;
            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }
            // "starts with" = first matched char is at index 0
            let a_starts = a.matched_indices.first() == Some(&0);
            let b_starts = b.matched_indices.first() == Some(&0);
            if a_starts != b_starts {
                return b_starts.cmp(&a_starts);
            }
            let a_priority = context_priority(a);
            let b_priority = context_priority(b);
            if a_priority != b_priority {
                return b_priority.cmp(&a_priority);
            }
            // 关键字按常用度排序：SELECT 排在 SET 前
            if matches!(a.kind, SuggestionKind::Keyword) && matches!(b.kind, SuggestionKind::Keyword) {
                let a_rank = keyword_rank(&a.label);
                let b_rank = keyword_rank(&b.label);
                if a_rank != b_rank {
                    return a_rank.cmp(&b_rank);
                }
            }
            let a_recent = usage_memory.map(|m| m.score(&a.label)).unwrap_or(0);
            let b_recent = usage_memory.map(|m| m.score(&b.label)).unwrap_or(0);
            if a_recent != b_recent {
                return b_recent.cmp(&a_recent);
            }
            // Shorter labels rank higher（仅在按前缀过滤时有意义）；无前缀时按字母序
            if !prefix_lower.is_empty() && a.label.len() != b.label.len() {
                return a.label.len().cmp(&b.label.len());
            }
            a.label.to_lowercase().cmp(&b.label.to_lowercase())
        });

        // 去重：同一列按（表, 列）保留一条；函数与关键字同名的只保留一条。
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        suggestions.retain(|s| {
            let key = match &s.kind {
                SuggestionKind::Column { parent_table } => format!(
                    "col\u{1}{}\u{1}{}",
                    parent_table.to_lowercase(),
                    s.label.to_lowercase()
                ),
                SuggestionKind::Keyword | SuggestionKind::Function => {
                    format!("kwfn\u{1}{}", s.label.to_lowercase())
                }
                SuggestionKind::Table | SuggestionKind::View => {
                    format!("table\u{1}{}", s.label.to_lowercase())
                }
                other => format!("{other:?}\u{1}{}", s.label.to_lowercase()),
            };
            seen.insert(key)
        });

        // Limit to 50 suggestions
        suggestions.truncate(50);
        suggestions
    }

    fn push_statement_snippets(suggestions: &mut Vec<AutocompleteSuggestion>) {
        for (trigger, insertion, cursor_offset) in SQL_STATEMENT_SNIPPETS {
            suggestions.push(AutocompleteSuggestion {
                label: (*trigger).to_string(),
                kind: SuggestionKind::Snippet,
                matched_indices: Vec::new(),
                insertion_text: Some((*insertion).to_string()),
                cursor_offset: Some(*cursor_offset),
            });
        }
    }

    fn alias_suggestions(
        sql: &str,
        cursor_char_index: usize,
        table_refs: &[TableReference],
    ) -> Vec<AutocompleteSuggestion> {
        let mut suggestions = Vec::new();
        let Some(base_token) = SqlContextParser::alias_source_token(sql, cursor_char_index) else {
            return suggestions;
        };
        let alias_source = table_refs
            .iter()
            .rev()
            .find(|r| r.table_name.eq_ignore_ascii_case(&base_token))
            .map(|r| r.table_name.as_str())
            .unwrap_or(base_token.as_str());
        for alias in alias_candidates(alias_source) {
            suggestions.push(AutocompleteSuggestion::new(alias, SuggestionKind::Keyword));
        }
        suggestions
    }

    fn push_function_snippets(
        suggestions: &mut Vec<AutocompleteSuggestion>,
        context: &SqlContext,
        db_kind: Option<DatabaseKind>,
    ) {
        if matches!(context, SqlContext::InsertColumns | SqlContext::AfterKeyword { .. } | SqlContext::AfterColumnDot { .. } | SqlContext::AliasName) {
            return;
        }
        for (label, insertion, cursor_offset) in COMMON_SQL_FUNCTION_SNIPPETS {
            suggestions.push(AutocompleteSuggestion {
                label: (*label).to_string(),
                kind: SuggestionKind::Function,
                matched_indices: vec![],
                insertion_text: Some((*insertion).to_string()),
                cursor_offset: Some(*cursor_offset),
            });
        }
        for (label, insertion, cursor_offset) in dialect_function_snippets_for(db_kind) {
            suggestions.push(AutocompleteSuggestion {
                label: (*label).to_string(),
                kind: SuggestionKind::Function,
                matched_indices: vec![],
                insertion_text: Some((*insertion).to_string()),
                cursor_offset: Some(*cursor_offset),
            });
        }
    }

    fn push_where_operator_snippets(suggestions: &mut Vec<AutocompleteSuggestion>) {
        for (label, insertion, cursor_offset) in WHERE_OPERATOR_SNIPPETS {
            suggestions.push(AutocompleteSuggestion {
                label: (*label).to_string(),
                kind: SuggestionKind::Keyword,
                matched_indices: vec![],
                insertion_text: Some((*insertion).to_string()),
                cursor_offset: Some(*cursor_offset),
            });
        }
    }
}

const COMMON_SQL_FUNCTION_SNIPPETS: &[(&str, &str, usize)] = &[
    ("COUNT", "COUNT(*)", 6),
    ("SUM", "SUM()", 4),
    ("AVG", "AVG()", 4),
    ("MIN", "MIN()", 4),
    ("MAX", "MAX()", 4),
    ("COALESCE", "COALESCE()", 9),
    ("NULLIF", "NULLIF()", 7),
    ("CAST", "CAST(expr AS type)", 5),
];

const MYSQL_FUNCTION_SNIPPETS: &[(&str, &str, usize)] = &[
    ("IFNULL", "IFNULL(expr, fallback)", 7),
    ("DATE_FORMAT", "DATE_FORMAT(date, '%Y-%m-%d')", 12),
    ("JSON_EXTRACT", "JSON_EXTRACT(json_doc, '$.path')", 13),
];

const POSTGRES_FUNCTION_SNIPPETS: &[(&str, &str, usize)] = &[
    ("DATE_TRUNC", "DATE_TRUNC('day', ts)", 12),
    ("TO_CHAR", "TO_CHAR(value, 'YYYY-MM-DD')", 8),
    ("JSONB_BUILD_OBJECT", "JSONB_BUILD_OBJECT('key', value)", 20),
];

fn dialect_function_snippets_for(db_kind: Option<DatabaseKind>) -> &'static [(&'static str, &'static str, usize)] {
    match db_kind {
        Some(DatabaseKind::MySql) => MYSQL_FUNCTION_SNIPPETS,
        Some(DatabaseKind::Postgres) => POSTGRES_FUNCTION_SNIPPETS,
        _ => &[],
    }
}

fn alias_candidates(source: &str) -> Vec<String> {
    let base = source
        .rsplit('.')
        .next()
        .unwrap_or(source)
        .trim_matches('`')
        .trim_matches('"')
        .trim()
        .to_string();
    if base.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let lowered = base.to_ascii_lowercase();
    let parts: Vec<&str> = lowered
        .split('_')
        .filter(|part| !part.is_empty())
        .collect();
    if let Some(first) = parts.first() {
        candidates.push((*first).to_string());
        candidates.push(first.chars().take(1).collect());
    }
    if parts.len() > 1 {
        let initials: String = parts.iter().filter_map(|p| p.chars().next()).collect();
        if !initials.is_empty() {
            candidates.push(initials);
        }
        if let Some(last) = parts.last() {
            candidates.push((*last).to_string());
        }
    }
    candidates.push(lowered);
    let mut deduped = Vec::new();
    for candidate in candidates {
        if !candidate.is_empty() && !deduped.contains(&candidate) {
            deduped.push(candidate);
        }
    }
    deduped
}

fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(pos, _)| pos)
        .unwrap_or(s.len())
}

/// Apply a suggestion by replacing the full identifier token around the cursor.
/// This removes redundant suffixes when completing in the middle of an existing token.
pub(crate) fn apply_autocomplete_suggestion(
    sql: &str,
    cursor_char_index: usize,
    prefix_start_index: usize,
    replacement: &str,
    cursor_offset: Option<usize>,
) -> (String, usize) {
    let (token_start, token_end) = SqlContextParser::current_token_bounds(sql, cursor_char_index);
    let replace_start = prefix_start_index.min(token_start);

    // 只有当补全文本覆盖整个 token 时才向前扩展到 token_end，
    // 否则保留光标后的标识符字符，避免误删用户想保留的内容。
    let token = &sql[char_to_byte_index(sql, token_start)..char_to_byte_index(sql, token_end)];
    let replace_end = if replacement.to_lowercase().starts_with(&token.to_lowercase()) {
        token_end
    } else {
        cursor_char_index
    };

    let before = &sql[..char_to_byte_index(sql, replace_start)];
    let after = &sql[char_to_byte_index(sql, replace_end)..];
    let new_cursor = replace_start + cursor_offset.unwrap_or(replacement.chars().count());

    // 反引号/双引号包裹的标识符：用户还没输入收尾引号时自动补上，
    // 避免补全后 SQL 变成 `` `users `` 这种语法错误。
    let open_quote = before
        .chars()
        .last()
        .filter(|c| matches!(c, '`' | '"'));
    let mut inserted = replacement.to_string();
    if let Some(quote) = open_quote {
        if !inserted.ends_with(quote) && after.chars().next() != Some(quote) {
            inserted.push(quote);
        }
    }

    (format!("{}{}{}", before, inserted, after), new_cursor)
}

/// Tracks the autocomplete popup's state across frames.
#[derive(Clone, Default)]
pub(crate) struct AutocompleteState {
    /// Whether the popup is currently visible.
    pub visible: bool,
    /// Currently highlighted suggestion index.
    pub selected_index: usize,
    /// Row index that was clicked (needs a second click to commit).
    pub clicked_index: Option<usize>,
    /// Screen position to anchor the popup (cursor bottom-left in editor).
    pub anchor_pos: Option<egui::Pos2>,
    /// The partial token prefix at the time the popup was opened.
    pub prefix: String,
    /// The cursor byte-offset that marks the start of the prefix (for replacement).
    pub prefix_start_index: usize,
    /// Screen rect of the popup (for click-outside-to-dismiss).
    pub popup_rect: Option<egui::Rect>,
    /// Set to true when selected_index changes via keyboard, triggers scroll_to_me once.
    pub need_scroll_to_selected: bool,
}

impl AutocompleteState {
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.selected_index = 0;
        self.clicked_index = None;
        self.anchor_pos = None;
        self.popup_rect = None;
        self.need_scroll_to_selected = false;
    }
}

/// Icon character for each suggestion kind.
pub(crate) fn suggestion_kind_icon(kind: SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Loading => "...",
        SuggestionKind::Database => "\u{1F4BE}",
        SuggestionKind::Schema => "\u{1F4C1}",
        SuggestionKind::Table => "\u{1F4E6}",
        SuggestionKind::View => "\u{1F441}",
        SuggestionKind::Column { .. } => "\u{1F4CB}",
        SuggestionKind::Function => "\u{0192}",
        SuggestionKind::Keyword => "\u{1F511}",
        SuggestionKind::Snippet => "\u{1F4DD}",
        SuggestionKind::MongoKeyword => "\u{26AB}",
    }
}

/// Secondary label text for display (e.g., "(column)" or table name for columns).
pub(crate) fn suggestion_kind_label(kind: SuggestionKind) -> String {
    match kind {
        SuggestionKind::Loading => tr!("加载中").to_string(),
        SuggestionKind::Database => tr!("数据库").to_string(),
        SuggestionKind::Schema => "Schema".to_string(),
        SuggestionKind::Table => tr!("表").to_string(),
        SuggestionKind::View => tr!("视图").to_string(),
        SuggestionKind::Column { parent_table } => {
            tr!("列 · {}", compact_object_name(&parent_table))
        }
        SuggestionKind::Function => tr!("函数").to_string(),
        SuggestionKind::Keyword => tr!("关键字").to_string(),
        SuggestionKind::Snippet => tr!("片段").to_string(),
        SuggestionKind::MongoKeyword => "Mongo".to_string(),
    }
}

/// Render the autocomplete popup. Returns (display_label, insertion_text, cursor_offset) if committed.
pub(crate) fn render_autocomplete_popup(
    ctx: &egui::Context,
    state: &mut AutocompleteState,
    suggestions: &[AutocompleteSuggestion],
    palette: &AutocompletePalette,
) -> Option<(String, String, Option<usize>)> {
    if !state.visible || suggestions.is_empty() {
        state.dismiss();
        return None;
    }

    let anchor = state.anchor_pos.unwrap_or(egui::Pos2::ZERO);
    // Compute dynamic popup width based on suggestion content lengths
    let min_popup_width: f32 = 420.0;
    let max_popup_width: f32 = 700.0;
    let popup_width: f32 = {
        let painter = ctx.debug_painter();
        let label_font = FontId::new(13.0, FontFamily::Monospace);
        let kind_font = FontId::new(11.0, FontFamily::Proportional);
        let max_content_width = suggestions
            .iter()
            .map(|s| {
                let label_w = painter
                    .fonts_mut(|f| {
                        f.layout_no_wrap(s.label.clone(), label_font.clone(), Color32::WHITE)
                    })
                    .rect
                    .width();
                let kind_w = painter
                    .fonts_mut(|f| {
                        f.layout_no_wrap(
                            suggestion_kind_label(s.kind.clone()),
                            kind_font.clone(),
                            Color32::WHITE,
                        )
                    })
                    .rect
                    .width();
                // 8 (left margin) + 24 (icon) + label + 12 (gap) + kind + 8 (right margin)
                8.0 + 24.0 + label_w + 12.0 + kind_w + 8.0
            })
            .fold(min_popup_width, f32::max);
        max_content_width.clamp(min_popup_width, max_popup_width)
    };
    let row_height = 30.0;
    let min_visible_rows = 12;
    let max_visible_rows = 18;
    // 弹窗至少保持 12 行高度，避免候选少时窗口过小难以浏览
    let visible_rows = suggestions.len().clamp(min_visible_rows, max_visible_rows);
    let popup_height = visible_rows as f32 * row_height + 4.0;

    // Clamp selected_index
    if state.selected_index >= suggestions.len() {
        state.selected_index = suggestions.len().saturating_sub(1);
    }

    let popup_id = Id::from("autocomplete-popup");

    let mut committed: Option<(String, String, Option<usize>)> = None;

    let area_response = Area::new(popup_id)
        .order(Order::Foreground)
        .fixed_pos(anchor)
        .constrain(true)
        .interactable(true)
        .show(ctx, |ui| {
            // Keyboard input (ArrowUp/Down, Enter/Tab, Escape) is handled
            // globally in app.rs BEFORE the TextEdit renders, so this popup
            // only needs mouse/click interaction.

            let frame = egui::Frame::popup(&ctx.style())
                .fill(palette.popup_bg)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::same(2));
            frame.show(ui, |ui| {
                ui.set_max_width(popup_width);
                ui.set_min_width(popup_width);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                ScrollArea::vertical()
                    .id_salt("autocomplete-scroll")
                    .max_height(popup_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (i, suggestion) in suggestions.iter().enumerate() {
                            let is_selected = i == state.selected_index;
                            let bg = if is_selected {
                                palette.selected_bg
                            } else {
                                palette.popup_bg
                            };
                            let text_color = if is_selected {
                                palette.selected_text
                            } else {
                                palette.text
                            };
                            let weak_color = if is_selected {
                                palette.selected_text
                            } else {
                                palette.weak_text
                            };

                            // Each row gets the same size; egui's cursor stacks them
                            // vertically since item_spacing is 0.
                            let row_size = egui::vec2(popup_width, row_height);
                            let (row_id, row_rect) =
                                ui.allocate_space(row_size);

                            let is_loading_row = matches!(suggestion.kind, SuggestionKind::Loading);
                            let row_response = ui.interact(
                                row_rect,
                                row_id,
                                if is_loading_row { Sense::hover() } else { Sense::click() },
                            );

                            // Keep selected row visible when navigating with keyboard
                            if is_selected && state.need_scroll_to_selected {
                                row_response.scroll_to_me(None);
                            }

                            // Paint background — use screen-space row_rect from allocate_space
                            if ui.is_rect_visible(row_rect) {
                                ui.painter()
                                    .rect_filled(row_rect, 4.0, bg);

                                let icon_x = row_rect.left() + 8.0;
                                let icon_y = row_rect.center().y;
                                let label_x = icon_x + 24.0;
                                let kind_label_x = row_rect.right() - 8.0;

                                // Icon
                                ui.painter().text(
                                    egui::pos2(icon_x, icon_y),
                                    Align2::LEFT_CENTER,
                                    suggestion_kind_icon(suggestion.kind.clone()),
                                    FontId::new(14.0, FontFamily::Proportional),
                                    text_color,
                                );

                                // Main label with matched character highlighting
                                let label_font = FontId::new(13.0, FontFamily::Monospace);
                                let highlight_color = if is_selected {
                                    palette.match_yellow
                                } else {
                                    palette.match_blue
                                };
                                let matched_set: std::collections::HashSet<usize> =
                                    suggestion.matched_indices.iter().copied().collect();
                                let mut job = LayoutJob::default();
                                for (ci, ch) in suggestion.label.chars().enumerate() {
                                    let color = if matched_set.contains(&ci) {
                                        highlight_color
                                    } else {
                                        text_color
                                    };
                                    job.append(
                                        &ch.to_string(),
                                        0.0,
                                        TextFormat {
                                            font_id: label_font.clone(),
                                            color,
                                            ..Default::default()
                                        },
                                    );
                                }
                                let label_galley = ui.painter().layout_job(job);
                                ui.painter().galley(
                                    egui::pos2(label_x, icon_y - label_galley.size().y * 0.5),
                                    label_galley,
                                    Color32::TRANSPARENT, // color is per-glyph in the galley
                                );

                                // Kind label (right-aligned)
                                let kind_str = suggestion_kind_label(suggestion.kind.clone());
                                let kind_galley = ui.painter().layout_no_wrap(
                                    kind_str,
                                    FontId::new(11.0, FontFamily::Proportional),
                                    weak_color,
                                );
                                ui.painter().galley(
                                    egui::pos2(
                                        kind_label_x - kind_galley.size().x,
                                        icon_y - kind_galley.size().y * 0.5,
                                    ),
                                    kind_galley,
                                    weak_color,
                                );
                            }

                            // 单击即补全（与 DataGrip 一致）；此前需要点两次才生效，体验很差
                            if !is_loading_row && row_response.clicked() {
                                let text = suggestion
                                    .insertion_text
                                    .clone()
                                    .unwrap_or_else(|| suggestion.label.clone());
                                committed = Some((suggestion.label.clone(), text, suggestion.cursor_offset));
                                state.clicked_index = Some(i);
                            }
                            // Only update selected_index on hover when the mouse actually moves,
                            // so arrow key selection isn't overridden by a stationary mouse.
                            if row_response.hovered() && ctx.input(|i| i.pointer.velocity().length() > 0.5) {
                                state.selected_index = i;
                            }
                        }
                    });
                // Consume the flag after one frame so it only triggers once
                state.need_scroll_to_selected = false;
            });
        });

    // Store popup rect for click-outside-to-dismiss
    state.popup_rect = Some(area_response.response.rect);

    if committed.is_some() {
        state.dismiss();
    }

    committed
}

/// Colors for the autocomplete popup.
#[derive(Clone, Copy)]
pub(crate) struct AutocompletePalette {
    pub popup_bg: Color32,
    pub border: Color32,
    pub text: Color32,
    pub weak_text: Color32,
    pub selected_bg: Color32,
    pub selected_text: Color32,
    pub match_blue: Color32,
    pub match_yellow: Color32,
}

impl From<&ui_theme::ThemeColors> for AutocompletePalette {
    fn from(c: &ui_theme::ThemeColors) -> Self {
        Self {
            popup_bg: c.autocomplete_popup_bg,
            border: c.autocomplete_border,
            text: c.autocomplete_text,
            weak_text: c.autocomplete_weak_text,
            selected_bg: c.autocomplete_selected_bg,
            selected_text: c.autocomplete_selected_text,
            match_blue: c.autocomplete_match_blue,
            match_yellow: c.autocomplete_match_yellow,
        }
    }
}

/// Derive autocomplete palette from the application theme.
pub(crate) fn autocomplete_palette(dark_mode: bool, dark_variant: ui_theme::DarkVariant, light_variant: ui_theme::LightVariant) -> AutocompletePalette {
    AutocompletePalette::from(&ui_theme::Theme::new(dark_mode, dark_variant, light_variant).colors)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CONN: &str = "test-conn";

    fn suggest_sql(sql: &str, cursor: usize, cache: &SchemaCache) -> Vec<AutocompleteSuggestion> {
        AutocompleteEngine::suggest(sql, cursor, cache, Some(TEST_CONN), None, None, None)
    }

    fn make_cache(tables: &[(&str, bool, &[&str])]) -> SchemaCache {
        let mut cache = SchemaCache::new();
        for (name, is_view, cols) in tables {
            cache.add_table(TEST_CONN, None, None, name.to_string(), *is_view);
            if !cols.is_empty() {
                cache.add_columns(
                    TEST_CONN,
                    None,
                    None,
                    name.to_string(),
                    cols.iter()
                        .map(|c| ColumnDefinition {
                            name: c.to_string(),
                            data_type: "text".into(),
                            nullable: true,
                            primary_key: false,
                            unique: false,
                            auto_increment: false,
                            on_update_current_timestamp: false,
                            default_value: None,
                            comment: None,
                        })
                        .collect(),
                );
            }
        }
        cache
    }

    /// 同一物理表以「裸表名 + 库限定」两条路径注册时，
    /// SelectClause 上下文下同一列名只应出现一次。
    #[test]
    fn select_clause_dedups_columns_across_multiple_cache_keys() {
        let mut cache = SchemaCache::new();
        let column = ColumnDefinition {
            name: "name".into(),
            data_type: "text".into(),
            nullable: true,
            primary_key: false,
            unique: false,
            auto_increment: false,
            on_update_current_timestamp: false,
            default_value: None,
            comment: None,
        };
        cache.add_table(TEST_CONN, None, None, "users".to_string(), false);
        cache.add_columns(TEST_CONN, None, None, "users".to_string(), vec![column.clone()]);
        cache.add_table(TEST_CONN, Some("mydb"), None, "users".to_string(), false);
        cache.add_columns(TEST_CONN, Some("mydb"), None, "users".to_string(), vec![column]);
        let suggestions = suggest_sql("SELECT na", 9, &cache);
        let name_cols: Vec<&AutocompleteSuggestion> = suggestions
            .iter()
            .filter(|s| matches!(s.kind, SuggestionKind::Column { .. }) && s.label == "name")
            .collect();
        assert_eq!(
            name_cols.len(),
            1,
            "同一列名在多缓存键下不应重复，实际 {} 次：{:?}",
            name_cols.len(),
            suggestions
                .iter()
                .filter(|s| s.label == "name")
                .map(|s| &s.kind)
                .collect::<Vec<_>>()
        );
    }

    /// 智能提示只应加载连接树确认存在的表的列；用户 SQL 里随意输入的
    /// 未知表名（未打完的表名、schema 名当裸表名）应被识别为未知并跳过。
    #[test]
    fn is_known_table_guards_unknown_names() {
        let mut cache = SchemaCache::new();
        // 模拟连接树加载：schema ods 下的表 flow，db mydb 下的表 users
        cache.add_table(TEST_CONN, None, None, "flow".to_string(), false);
        cache.add_table(TEST_CONN, None, Some("ods"), "flow".to_string(), false);
        cache.add_schema(TEST_CONN, "ods".to_string());
        cache.add_table(TEST_CONN, Some("mydb"), None, "users".to_string(), false);
        cache.add_database(TEST_CONN, "mydb".to_string());

        // 已知表：schema 匹配放行
        assert!(cache.is_known_table(Some(TEST_CONN), Some("ods"), None, "flow"));
        // 已知表：database 匹配放行
        assert!(cache.is_known_table(Some(TEST_CONN), None, Some("mydb"), "users"));
        // 已知表：裸表名在缓存中
        assert!(cache.is_known_table(Some(TEST_CONN), None, None, "flow"));
        // 未知：schema 里没这个表
        assert!(!cache.is_known_table(Some(TEST_CONN), Some("ods"), None, "flowx"));
        // 未知：schema 不存在
        assert!(!cache.is_known_table(Some(TEST_CONN), Some("unknown_schema"), None, "flow"));
        // 未知：database 里没这个表
        assert!(!cache.is_known_table(Some(TEST_CONN), None, Some("mydb"), "nope"));
        // 未知：裸表名（用户输入 schema 名或乱输表名）不在缓存
        assert!(!cache.is_known_table(Some(TEST_CONN), None, None, "ods_ng_plutus_all"));
        assert!(!cache.is_known_table(Some(TEST_CONN), None, None, "flowx"));
        // 未知：没有连接时不命中任何连接的表
        assert!(!cache.is_known_table(None, None, None, "flow"));
    }

    #[test]
    fn context_parser_dot_triggers_after_column_dot() {
        let c = SqlContextParser::parse("SELECT users.nam", 15);
        assert_eq!(c, SqlContext::AfterColumnDot { parent: "users".into() });
    }

    #[test]
    fn context_parser_from_suggests_tables() {
        let c = SqlContextParser::parse("SELECT * FROM ", 14);
        assert_eq!(c, SqlContext::AfterKeyword { keyword: "FROM".into() });
    }

    #[test]
    fn context_parser_select_suggests_columns() {
        // Cursor after comma+space in select list: "SELECT id, " → comma-detected
        let c = SqlContextParser::parse("SELECT id, ", 12);
        assert_eq!(c, SqlContext::SelectClause);
    }

    #[test]
    fn context_parser_after_comma_in_select_list() {
        // "SELECT id, name, " — cursor right after trailing comma, suggests columns
        let c = SqlContextParser::parse("SELECT id, name, ", 18);
        assert_eq!(c, SqlContext::SelectClause);
    }

    #[test]
    fn context_parser_order_by_suggests_columns() {
        // Cursor after comma in ORDER BY
        let c = SqlContextParser::parse("SELECT * FROM t ORDER BY a,", 29);
        assert_eq!(c, SqlContext::OrderGroupClause);
    }

    #[test]
    fn context_parser_where_suggests_columns() {
        let c = SqlContextParser::parse("SELECT * FROM x WHERE ", 22);
        assert_eq!(c, SqlContext::WhereClause);
    }

    #[test]
    fn context_parser_where_with_partial_identifier_stays_in_where_clause() {
        let c = SqlContextParser::parse("SELECT * FROM x WHERE pro", 25);
        assert_eq!(c, SqlContext::WhereClause);
    }

    #[test]
    fn context_parser_insert_column_list_stays_in_insert_context_after_comma() {
        let c = SqlContextParser::parse("INSERT INTO users (id, ", 23);
        assert_eq!(c, SqlContext::InsertColumns);
    }

    #[test]
    fn context_parser_insert_values_is_not_column_context() {
        let c = SqlContextParser::parse("INSERT INTO users VALUES (", 26);
        assert_eq!(c, SqlContext::General);
    }

    #[test]
    fn context_parser_join_suggests_tables() {
        let c = SqlContextParser::parse("SELECT * FROM t JOIN ", 22);
        assert_eq!(c, SqlContext::AfterKeyword { keyword: "JOIN".into() });
    }

    #[test]
    fn context_parser_as_uses_alias_context() {
        let c = SqlContextParser::parse("SELECT total_price AS ", 22);
        assert_eq!(c, SqlContext::AliasName);
    }

    #[test]
    fn current_token_prefix_extracts_partial_identifier() {
        let p = SqlContextParser::current_token_prefix("SELECT us", 9);
        assert_eq!(p, "us");
    }

    #[test]
    fn current_token_prefix_empty_at_start() {
        let p = SqlContextParser::current_token_prefix("SELECT", 0);
        assert_eq!(p, "");
    }

    #[test]
    fn current_token_bounds_cover_suffix_after_cursor() {
        let bounds = SqlContextParser::current_token_bounds("SELECT name FROM users", 10);
        assert_eq!(bounds, (7, 11));
    }

    #[test]
    fn engine_suggests_columns_in_select_context() {
        let cache = make_cache(&[("users", false, &["id", "name", "email"])]);
        // Cursor after comma+space in select list: "SELECT id, "
        let suggestions = AutocompleteEngine::suggest("SELECT id, ", 12, &cache, Some(TEST_CONN), None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "name"));
    }

    #[test]
    fn engine_suggests_columns_in_where_with_partial_prefix() {
        let cache = make_cache(&[(
            "aep.aep_abtest_config_deploy_log_2",
            false,
            &["project_id", "process_id", "status"],
        )]);
        let suggestions = AutocompleteEngine::suggest(
            "SELECT * FROM aep.aep_abtest_config_deploy_log_2 WHERE pro",
            58,
            &cache,
            Some(TEST_CONN),
            None,
            None,
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "project_id"));
        assert!(suggestions.iter().any(|s| s.label == "process_id"));
        assert!(!suggestions.iter().any(|s| s.label == "projects"));
        assert!(!suggestions
            .iter()
            .any(|s| s.label == "aep.aep_abtest_config_deploy_log_2.project_id"));
    }

    #[test]
    fn engine_suggests_tables_after_from() {
        let cache = make_cache(&[
            ("users", false, &["id"]),
            ("orders", false, &["id"]),
        ]);
        let suggestions = AutocompleteEngine::suggest("SELECT * FROM ", 14, &cache, Some(TEST_CONN), None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "users"));
        assert!(suggestions.iter().any(|s| s.label == "orders"));
        assert!(!suggestions.iter().any(|s| matches!(s.kind, SuggestionKind::Database)));
        assert!(!suggestions.iter().any(|s| matches!(s.kind, SuggestionKind::Schema)));
    }

    #[test]
    fn engine_suggests_columns_after_dot() {
        let cache = make_cache(&[("users", false, &["id", "name", "email"])]);
        let suggestions = AutocompleteEngine::suggest("SELECT users.nam", 15, &cache, Some(TEST_CONN), None, None, None);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].label, "name");
    }

    #[test]
    fn engine_resolves_alias_after_dot() {
        let cache = make_cache(&[
            ("users", false, &["id", "name"]),
            ("orders", false, &["id", "total"]),
        ]);
        let suggestions =
            AutocompleteEngine::suggest("SELECT u. FROM users u JOIN orders o ON u.id = o.id", 9, &cache, Some(TEST_CONN), None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "name"));
        assert!(!suggestions.iter().any(|s| s.label == "total"));
    }

    #[test]
    fn engine_limits_column_suggestions_to_referenced_tables() {
        let cache = make_cache(&[
            ("users", false, &["id", "name"]),
            ("orders", false, &["id", "total"]),
            ("products", false, &["sku"]),
        ]);
        let sql = "SELECT  FROM users u JOIN orders o ON u.id = o.id";
        let suggestions = AutocompleteEngine::suggest(sql, 7, &cache, Some(TEST_CONN), None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "total"));
        assert!(!suggestions.iter().any(|s| s.label == "products.sku"));
        assert!(!suggestions.iter().any(|s| s.label == "sku"));
    }

    #[test]
    fn engine_manual_trigger_without_prefix_in_where_prefers_columns_only() {
        let cache = make_cache(&[("users", false, &["id", "name"])]);
        let suggestions =
            AutocompleteEngine::suggest("SELECT * FROM users WHERE ", 26, &cache, Some(TEST_CONN), None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "name"));
        assert!(suggestions.iter().any(|s| s.label == "IS NULL"));
        assert!(suggestions.iter().any(|s| s.label == "IN"));
        assert!(suggestions.iter().any(|s| s.label == "LIKE"));
        assert!(!suggestions.iter().any(|s| s.label == "WHERE"));
        assert!(!suggestions.iter().any(|s| matches!(s.kind, SuggestionKind::Function)));
    }

    #[test]
    fn engine_insert_columns_uses_target_table_columns() {
        let cache = make_cache(&[
            ("users", false, &["id", "name"]),
            ("orders", false, &["order_id"]),
        ]);
        let suggestions = AutocompleteEngine::suggest("INSERT INTO users (", 19, &cache, Some(TEST_CONN), None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "name"));
        assert!(!suggestions.iter().any(|s| s.label == "order_id"));
    }

    #[test]
    fn engine_suggests_alias_candidates_after_as() {
        let cache = make_cache(&[("order_items", false, &["id"])]);
        let suggestions =
            AutocompleteEngine::suggest("SELECT * FROM order_items AS ", 29, &cache, Some(TEST_CONN), None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "oi"));
        assert!(suggestions.iter().any(|s| s.label == "order_items"));
        assert!(!suggestions.iter().any(|s| matches!(s.kind, SuggestionKind::Function)));
    }

    #[test]
    fn engine_suggests_function_snippets_in_select_context() {
        let cache = make_cache(&[("users", false, &["id"])]);
        let suggestions = suggest_sql("SELECT C", 8, &cache);
        let count = suggestions.iter().find(|s| s.label == "COUNT").unwrap();
        assert_eq!(count.insertion_text.as_deref(), Some("COUNT(*)"));
        assert_eq!(count.cursor_offset, Some(6));
        let coalesce = suggestions.iter().find(|s| s.label == "COALESCE").unwrap();
        assert!(matches!(coalesce.kind, SuggestionKind::Function));
        let cast = suggestions.iter().find(|s| s.label == "CAST").unwrap();
        assert_eq!(cast.insertion_text.as_deref(), Some("CAST(expr AS type)"));
        assert_eq!(cast.cursor_offset, Some(5));
    }

    #[test]
    fn engine_adds_mysql_specific_function_snippets() {
        let cache = make_cache(&[("users", false, &["id"])]);
        let suggestions = AutocompleteEngine::suggest(
            "SELECT DATE_F",
            13,
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MySql),
            None,
        );
        let date_format = suggestions
            .iter()
            .find(|s| s.label == "DATE_FORMAT")
            .unwrap();
        assert_eq!(
            date_format.insertion_text.as_deref(),
            Some("DATE_FORMAT(date, '%Y-%m-%d')")
        );
        assert_eq!(date_format.cursor_offset, Some(12));
        assert!(!suggestions.iter().any(|s| s.label == "DATE_TRUNC"));

        let ifnull = AutocompleteEngine::suggest(
            "SELECT IF",
            9,
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MySql),
            None,
        );
        let ifnull = ifnull.iter().find(|s| s.label == "IFNULL").unwrap();
        assert_eq!(ifnull.insertion_text.as_deref(), Some("IFNULL(expr, fallback)"));
    }

    #[test]
    fn engine_adds_postgres_specific_function_snippets() {
        let cache = make_cache(&[("users", false, &["id"])]);
        let suggestions = AutocompleteEngine::suggest(
            "SELECT DATE_T",
            13,
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::Postgres),
            None,
        );
        let date_trunc = suggestions
            .iter()
            .find(|s| s.label == "DATE_TRUNC")
            .unwrap();
        assert_eq!(date_trunc.insertion_text.as_deref(), Some("DATE_TRUNC('day', ts)"));
        assert_eq!(date_trunc.cursor_offset, Some(12));
        assert!(!suggestions.iter().any(|s| s.label == "DATE_FORMAT"));

        let to_char = AutocompleteEngine::suggest(
            "SELECT TO_C",
            11,
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::Postgres),
            None,
        );
        let to_char = to_char.iter().find(|s| s.label == "TO_CHAR").unwrap();
        assert_eq!(to_char.insertion_text.as_deref(), Some("TO_CHAR(value, 'YYYY-MM-DD')"));
    }

    #[test]
    fn usage_memory_promotes_recent_suggestions() {
        let cache = make_cache(&[("users", false, &["name", "nickname"])]);
        let mut usage = AutocompleteUsageMemory::default();
        usage.record("nickname");
        let suggestions = AutocompleteEngine::suggest(
            "SELECT ni",
            9,
            &cache,
            Some(TEST_CONN),
            None,
            None,
            Some(&usage),
        );
        assert_eq!(suggestions.first().map(|s| s.label.as_str()), Some("nickname"));
    }

    #[test]
    fn loading_suggestion_has_dedicated_label_and_icon() {
        assert_eq!(suggestion_kind_label(SuggestionKind::Loading), "加载中");
        assert_eq!(suggestion_kind_icon(SuggestionKind::Loading), "...");
    }

    #[test]
    fn engine_prefix_filter_ranks_exact_first() {
        let cache = make_cache(&[("t", false, &["id", "idea", "aid"])]);
        let suggestions = AutocompleteEngine::suggest("SELECT id", 9, &cache, Some(TEST_CONN), None, None, None);
        assert_eq!(suggestions.first().unwrap().label, "id");
        assert_eq!(suggestions[1].label, "idea"); // starts-with id
        assert_eq!(suggestions[2].label, "aid");  // contains id
    }

    #[test]
    fn apply_autocomplete_replaces_middle_of_identifier() {
        let (sql, cursor) = apply_autocomplete_suggestion("SELECT na|me FROM users".replace('|', "").as_str(), 9, 7, "name", None);
        assert_eq!(sql, "SELECT name FROM users");
        assert_eq!(cursor, 11);
    }

    #[test]
    fn apply_autocomplete_keeps_content_after_cursor_when_token_not_covered() {
        // 光标在标识符中间，补全文本不覆盖整个 token：光标后的内容必须保留。
        let (sql, cursor) = apply_autocomplete_suggestion("SELECT * FROM public|_users".replace('|', "").as_str(), 20, 14, "public", None);
        assert_eq!(sql, "SELECT * FROM public_users");
        assert_eq!(cursor, 20);
    }

    #[test]
    fn apply_autocomplete_keeps_content_after_cursor_for_table() {
        // 表名同样适用：选 "users" 时保留光标后的 "_archive"。
        let (sql, cursor) = apply_autocomplete_suggestion("SELECT * FROM users|_archive".replace('|', "").as_str(), 19, 14, "users", None);
        assert_eq!(sql, "SELECT * FROM users_archive");
        assert_eq!(cursor, 19);
    }

    #[test]
    fn apply_autocomplete_replaces_full_token_when_covered() {
        // 补全文本覆盖整个 token（"public" 以 "pub" 开头）：正常补全到 token_end。
        let (sql, cursor) = apply_autocomplete_suggestion("SELECT * FROM pub", 17, 14, "public", None);
        assert_eq!(sql, "SELECT * FROM public");
        assert_eq!(cursor, 20);
    }

    #[test]
    fn apply_autocomplete_keeps_templates_cursor_offset() {
        let (sql, cursor) = apply_autocomplete_suggestion("db.users.fi|nd".replace('|', "").as_str(), 11, 9, "find({})", Some(5));
        assert_eq!(sql, "db.users.find({})");
        assert_eq!(cursor, 14);
    }

    #[test]
    fn apply_autocomplete_closes_open_backtick() {
        let (sql, cursor) = apply_autocomplete_suggestion("SELECT * FROM `ord", 18, 15, "orders", None);
        assert_eq!(sql, "SELECT * FROM `orders`");
        assert_eq!(cursor, 21, "光标应停在收尾反引号之前");
    }

    #[test]
    fn apply_autocomplete_does_not_duplicate_closing_quote() {
        let (sql, _) = apply_autocomplete_suggestion("SELECT * FROM \"ord\"", 18, 15, "orders", None);
        assert_eq!(sql, "SELECT * FROM \"orders\"");
    }

    #[test]
    fn mongo_find_prefers_collection_fields_inside_filter_object() {
        let cache = make_cache(&[("users", false, &["name", "age", "email"])]);
        let suggestions = AutocompleteEngine::suggest(
            "db.users.find({ na",
            18,
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MongoDb),
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "name"));
        assert!(!suggestions.iter().any(|s| s.label == "$set"));
        let name = suggestions.iter().find(|s| s.label == "name").unwrap();
        assert_eq!(name.insertion_text.as_deref(), Some("name"));
    }

    #[test]
    fn mongo_find_value_position_prefers_query_operators() {
        let cache = make_cache(&[("users", false, &["name", "age"])]);
        let sql = "db.users.find({ age: $g";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MongoDb),
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "$gt"));
        assert!(suggestions.iter().any(|s| s.label == "$gte"));
        assert!(!suggestions.iter().any(|s| s.label == "age"));
    }

    #[test]
    fn mongo_sort_prefers_collection_fields() {
        let cache = make_cache(&[("users", false, &["createdAt", "name"])]);
        let suggestions = AutocompleteEngine::suggest(
            "db.users.find({}).sort({ cr",
            27,
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MongoDb),
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "createdAt"));
        assert!(!suggestions.iter().any(|s| s.label == "$gt"));
    }

    #[test]
    fn mongo_update_second_argument_prefers_update_operators() {
        let cache = make_cache(&[("users", false, &["name", "age"])]);
        let sql = "db.users.updateOne({ _id: 1 }, { $s";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MongoDb),
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "$set"));
        assert!(suggestions.iter().any(|s| s.label == "$unset"));
        assert!(!suggestions.iter().any(|s| s.label == "name"));
    }

    #[test]
    fn mongo_project_prefers_nested_field_paths() {
        let cache = make_cache(&[(
            "users",
            false,
            &["user.name", "user.profile.nickname", "createdAt"],
        )]);
        let sql = "db.users.find({}).project({ user.pr";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MongoDb),
            None,
        );
        let nickname = suggestions
            .iter()
            .find(|s| s.label == "user.profile.nickname")
            .unwrap();
        assert_eq!(nickname.insertion_text.as_deref(), Some("ofile.nickname"));
    }

    #[test]
    fn mongo_set_object_prefers_nested_field_paths_over_update_operators() {
        let cache = make_cache(&[(
            "users",
            false,
            &["profile.name", "profile.nickname", "age"],
        )]);
        let sql = "db.users.updateOne({ _id: 1 }, { $set: { prof";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MongoDb),
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "profile.name"));
        assert!(!suggestions.iter().any(|s| s.label == "$unset"));
    }

    #[test]
    fn mongo_templates_use_compass_style_defaults() {
        let (find_template, _) = mongo_method_template("find");
        let (update_template, _) = mongo_method_template("updateOne");
        let (project_template, _) = mongo_method_template("project");
        assert_eq!(find_template, "find({ field: value })");
        assert_eq!(
            update_template,
            "updateOne({ _id: value }, { $set: { field: value } })"
        );
        assert_eq!(project_template, "project({ field: 1, _id: 0 })");
    }

    #[test]
    fn mongo_field_context_suggests_fields_with_single_char_prefix() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, None, None, "users".to_string(), false);
        cache.add_columns(TEST_CONN, None, None, "users".to_string(), vec![
            ColumnDefinition {
                name: "name".into(),
                data_type: "text".into(),
                nullable: true,
                primary_key: false,
                unique: false,
                auto_increment: false,
                on_update_current_timestamp: false,
                default_value: None,
                comment: None,
            },
        ]);
        let sql = "db.users.find({ n";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            None,
            Some(DatabaseKind::MongoDb),
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "name"));
    }

    #[test]
    fn cache_stores_and_retrieves_tables() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, None, None, "users".to_string(), false);
        cache.add_columns(TEST_CONN, None, None, "users".to_string(), vec![
            ColumnDefinition {
                name: "id".into(),
                data_type: "int".into(),
                nullable: false,
                primary_key: true,
                unique: true,
                auto_increment: false,
                on_update_current_timestamp: false,
                default_value: None,
                comment: None,
            },
        ]);
        let names = cache.table_names_for(Some(TEST_CONN), None);
        assert!(names.iter().any(|(name, _)| *name == "users"));
        assert_eq!(
            cache
                .columns_for_table(Some(TEST_CONN), None, "users")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn schema_cache_is_view_flag() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, None, None, "v".to_string(), true);
        assert!(cache.is_view_of(Some(TEST_CONN), None, "v"));
        cache.add_table(TEST_CONN, None, None, "t".to_string(), false);
        assert!(!cache.is_view_of(Some(TEST_CONN), None, "t"));
    }

    #[test]
    fn schema_cache_has_tables_per_connection() {
        let mut cache = SchemaCache::new();
        assert!(!cache.has_tables(TEST_CONN));
        cache.add_table(TEST_CONN, Some("shop"), None, "orders".to_string(), false);
        assert!(cache.has_tables(TEST_CONN));
        assert!(!cache.has_tables("other-conn"));
    }

    #[test]
    fn cache_lists_table_locations_for_database() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("db1"), None, "users".to_string(), false);
        cache.add_table(
            TEST_CONN,
            Some("db1"),
            Some("public"),
            "orders".to_string(),
            true,
        );
        cache.add_table(TEST_CONN, Some("db2"), None, "secret".to_string(), false);

        let locations = cache.table_locations(TEST_CONN, Some("db1"));
        assert_eq!(locations.len(), 2);
        assert!(locations.iter().any(|(db, schema, table, is_view)| {
            db.as_deref() == Some("db1") && schema.is_none() && table == "users" && !*is_view
        }));
        assert!(locations.iter().any(|(db, schema, table, is_view)| {
            db.as_deref() == Some("db1")
                && schema.as_deref() == Some("public")
                && table == "orders"
                && *is_view
        }));
        assert!(!locations.iter().any(|(_, _, table, _)| table == "secret"));
    }

    #[test]
    fn autocomplete_suggestion_clone_and_eq() {
        let a = AutocompleteSuggestion {
            label: "x".into(),
            kind: SuggestionKind::Table,
            matched_indices: vec![],
            insertion_text: None,
            cursor_offset: None,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn context_parser_handles_empty_sql() {
        let c = SqlContextParser::parse("", 0);
        assert_eq!(c, SqlContext::General);
    }

    #[test]
    fn context_parser_general_at_unrecognized_position() {
        let c = SqlContextParser::parse("SEL", 3);
        assert_eq!(c, SqlContext::General);
    }

    #[test]
    fn engine_keyword_prefix_excludes_subsequence_noise() {
        let cache = make_cache(&[]);
        let suggestions = suggest_sql("se", 2, &cache);
        assert_eq!(
            suggestions.first().map(|s| s.label.as_str()),
            Some("SELECT")
        );
        assert!(suggestions.iter().any(|s| s.label == "SET"));
        // 关键字只做前缀匹配：USE / CASE / ELSE 这类子序列命中不应出现
        assert!(!suggestions
            .iter()
            .any(|s| matches!(s.kind, SuggestionKind::Keyword)
                && matches!(s.label.as_str(), "USE" | "CASE" | "ELSE" | "FALSE")));
    }

    #[test]
    fn engine_identifier_supports_substring_match() {
        let cache = make_cache(&[("public_orders", false, &["id"])]);
        let suggestions = suggest_sql("SELECT * FROM ord", 16, &cache);
        assert!(suggestions.iter().any(|s| s.label == "public_orders"));
    }

    #[test]
    fn engine_from_without_prefix_suggests_tables() {
        let cache = make_cache(&[("users", false, &["id"]), ("orders", true, &[])]);
        let suggestions = suggest_sql("SELECT * FROM ", 14, &cache);
        assert!(suggestions.iter().any(|s| s.label == "users"));
        assert!(suggestions.iter().any(|s| s.label == "orders"));
        assert!(suggestions
            .iter()
            .any(|s| matches!(s.kind, SuggestionKind::View) && s.label == "orders"));
    }

    #[test]
    fn engine_suppresses_inside_strings_and_comments() {
        let cache = make_cache(&[("users", false, &["id"])]);
        let in_string = "SELECT * FROM users WHERE name = 'us";
        assert!(suggest_sql(in_string, in_string.chars().count(), &cache).is_empty());
        assert!(suggest_sql("-- us", 5, &cache).is_empty());
        assert!(suggest_sql("/* us", 5, &cache).is_empty());
        // 字符串闭合后恢复补全
        let after_string = "SELECT * FROM users WHERE name = 'x' AND i";
        assert!(!suggest_sql(after_string, after_string.chars().count(), &cache).is_empty());
    }

    #[test]
    fn engine_scopes_tables_to_current_connection() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, None, None, "users".into(), false);
        cache.add_table("other-conn", None, None, "secrets".into(), false);
        let suggestions = suggest_sql("SELECT * FROM ", 14, &cache);
        assert!(suggestions.iter().any(|s| s.label == "users"));
        assert!(!suggestions.iter().any(|s| s.label == "secrets"));
    }

    #[test]
    fn engine_offers_statement_snippets_at_statement_start() {
        let cache = make_cache(&[]);
        let suggestions = suggest_sql("sel", 3, &cache);
        let snippet = suggestions.iter().find(|s| s.label == "sel").unwrap();
        assert!(matches!(snippet.kind, SuggestionKind::Snippet));
        assert_eq!(snippet.insertion_text.as_deref(), Some("SELECT * FROM "));
        assert_eq!(snippet.cursor_offset, Some(14));

        let sql = "SELECT * FROM users WHERE sel";
        let mid = suggest_sql(sql, sql.chars().count(), &cache);
        assert!(!mid.iter().any(|s| matches!(s.kind, SuggestionKind::Snippet)));
    }

    #[test]
    fn needs_space_padding_identifier_chars() {
        assert!(needs_space_padding('a'));
        assert!(needs_space_padding('Z'));
        assert!(needs_space_padding('0'));
        assert!(needs_space_padding('_'));
    }

    #[test]
    fn needs_space_padding_whitespace() {
        assert!(!needs_space_padding(' '));
        assert!(!needs_space_padding('\n'));
        assert!(!needs_space_padding('\t'));
        assert!(!needs_space_padding('\r'));
    }

    #[test]
    fn needs_space_padding_delimiters() {
        assert!(!needs_space_padding('('));
        assert!(!needs_space_padding(')'));
        assert!(!needs_space_padding(','));
        assert!(!needs_space_padding('.'));
        assert!(!needs_space_padding(';'));
    }

    #[test]
    fn needs_space_padding_quotes() {
        assert!(!needs_space_padding('\''));
        assert!(!needs_space_padding('"'));
        assert!(!needs_space_padding('`'));
    }

    #[test]
    fn needs_space_padding_operators() {
        assert!(needs_space_padding('='));
        assert!(needs_space_padding('>'));
        assert!(needs_space_padding('+'));
        assert!(needs_space_padding('-'));
        assert!(needs_space_padding('*'));
    }

    /// 只有表名、没有列缓存的场景（连接树刚加载完）也必须能提示表名。
    #[test]
    fn engine_suggests_tables_after_from_without_columns() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("shop"), None, "orders".to_string(), false);
        cache.add_table(TEST_CONN, Some("shop"), None, "users".to_string(), false);

        // 空前缀：FR OM 之后应直接列出库内全部表
        let empty_prefix = AutocompleteEngine::suggest(
            "SELECT * FROM ",
            14,
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(empty_prefix.iter().any(|s| s.label == "orders"));
        assert!(empty_prefix.iter().any(|s| s.label == "users"));

        // 有前缀：应命中前缀匹配的表
        let with_prefix = AutocompleteEngine::suggest(
            "SELECT * FROM or",
            16,
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        let labels: Vec<&str> = with_prefix.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"orders"), "候选: {labels:?}");
    }

    /// 查询页尚未选定库（database = None）时，也要回退到连接内全部表。
    #[test]
    fn engine_suggests_tables_when_tab_database_unknown() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("shop"), None, "orders".to_string(), false);
        let suggestions = AutocompleteEngine::suggest(
            "SELECT * FROM ord",
            17,
            &cache,
            Some(TEST_CONN),
            None,
            None,
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "orders"));
    }

    /// 语句前有注释、多语句场景下也应提示表名。
    #[test]
    fn engine_suggests_tables_with_leading_comment_and_previous_statement() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("shop"), None, "orders".to_string(), false);

        let sql = "-- 报表查询\nSELECT 1;\n/* 明细 */\nSELECT * FROM ord";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(
            suggestions.iter().any(|s| s.label == "orders"),
            "候选: {:?}",
            suggestions.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()
        );
    }

    /// 跨行书写（FROM 换行后输入表名）也应能提示。
    #[test]
    fn engine_suggests_tables_after_multiline_from() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("shop"), None, "customers".to_string(), false);
        let sql = "SELECT *\nFROM\n    cus";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "customers"));
    }

    /// 库名/模式名限定（含反引号形式）后应提示表名。
    #[test]
    fn engine_suggests_tables_after_qualified_database_prefix() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("shop"), None, "orders".to_string(), false);

        for sql in ["SELECT * FROM shop.ord", "SELECT * FROM `shop`.`ord"] {
            let suggestions = AutocompleteEngine::suggest(
                sql,
                sql.chars().count(),
                &cache,
                Some(TEST_CONN),
                Some("shop"),
                None,
                None,
            );
            assert!(
                suggestions.iter().any(|s| s.label == "orders"),
                "{sql} 候选: {:?}",
                suggestions.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()
            );
        }
    }

    /// schema 限定名在缓存缺 schema 信息时，也要能退化提示到表名。
    #[test]
    fn engine_suggests_tables_after_schema_qualifier_without_schema_metadata() {
        let mut cache = SchemaCache::new();
        // 表只带库、没有 schema（部分驱动返回的节点如此）
        cache.add_table(TEST_CONN, Some("shop"), None, "orders".to_string(), false);
        let sql = "SELECT * FROM public.ord";
        let suggestions = AutocompleteEngine::suggest(
            sql,
            sql.chars().count(),
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(
            suggestions.iter().any(|s| s.label == "orders"),
            "候选: {:?}",
            suggestions.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()
        );
    }

    /// JOIN 后同样应提示表名。
    #[test]
    fn engine_suggests_tables_after_join() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("shop"), None, "order_items".to_string(), false);
        let suggestions = AutocompleteEngine::suggest(
            "SELECT * FROM orders JOIN order_",
            32,
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(suggestions.iter().any(|s| s.label == "order_items"));
    }

    /// 反引号/双引号包裹的标识符内也应继续提示表名（MySQL 常用反引号）。
    #[test]
    fn engine_suggests_tables_inside_quoted_identifier() {
        let mut cache = SchemaCache::new();
        cache.add_table(TEST_CONN, Some("shop"), None, "orders".to_string(), false);

        let backtick = AutocompleteEngine::suggest(
            "SELECT * FROM `ord",
            17,
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(
            backtick.iter().any(|s| s.label == "orders"),
            "反引号内候选: {:?}",
            backtick.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()
        );

        let dquote = AutocompleteEngine::suggest(
            "SELECT * FROM \"ord",
            17,
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(
            dquote.iter().any(|s| s.label == "orders"),
            "双引号内候选: {:?}",
            dquote.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()
        );
    }

    /// 集成链路：持久化元数据缓存条目回填 SchemaCache 后，
    /// 表名（即使没有列）与列名都应立即可补全。
    #[test]
    fn cached_metadata_entries_warm_up_table_and_column_completion() {
        use metadata_cache::CachedEntry;
        let entries = vec![
            CachedEntry {
                database: Some("shop".into()),
                schema: None,
                table: "orders".into(),
                is_view: false,
                columns: vec![ColumnDefinition {
                    name: "id".into(),
                    data_type: "int".into(),
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    auto_increment: false,
                    on_update_current_timestamp: false,
                    default_value: None,
                    comment: None,
                }],
            },
            CachedEntry {
                database: Some("shop".into()),
                schema: None,
                table: "customers".into(),
                is_view: false,
                columns: vec![],
            },
        ];
        let mut cache = SchemaCache::new();
        crate::app::apply_cached_metadata_entries(&mut cache, TEST_CONN, &entries);

        // 只有表名、没有列的表也必须能提示
        let tables = AutocompleteEngine::suggest(
            "SELECT * FROM cus",
            17,
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(
            tables.iter().any(|s| s.label == "customers"),
            "候选: {:?}",
            tables.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()
        );

        // 有列缓存的表在 WHERE 里能继续提示列名
        let columns = AutocompleteEngine::suggest(
            "SELECT * FROM orders WHERE i",
            28,
            &cache,
            Some(TEST_CONN),
            Some("shop"),
            None,
            None,
        );
        assert!(
            columns.iter().any(|s| s.label == "id"),
            "候选: {:?}",
            columns.iter().map(|s| s.label.as_str()).collect::<Vec<_>>()
        );
    }
}
