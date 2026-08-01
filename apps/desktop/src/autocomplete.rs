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
    MongoKeyword,
}

/// Schema metadata cache for autocomplete.
///
/// L1: table/view names (populated from explorer tree)
/// L2: column definitions (populated from loaded TableDefinitions)
/// L3: background pre-fetch is driven externally via `add_columns`.
#[derive(Clone, Default)]
pub(crate) struct SchemaCache {
    /// table_name → (is_view, columns)
    tables: HashMap<String, (bool, Vec<ColumnDefinition>)>,
    /// database_name → list of table names in that database
    database_tables: HashMap<String, Vec<String>>,
    /// schema_name → list of table names in that schema (Postgres)
    schema_tables: HashMap<String, Vec<String>>,
    /// (connection_id, database_name) pairs
    databases: Vec<(String, String)>,
    /// (connection_id, schema_name) pairs
    schemas: Vec<(String, String)>,
}

impl SchemaCache {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            database_tables: HashMap::new(),
            schema_tables: HashMap::new(),
            databases: Vec::new(),
            schemas: Vec::new(),
        }
    }

    /// Register a table (or view) name with its type flag. Idempotent.
    pub fn add_table(&mut self, name: String, is_view: bool) {
        let entry = self.tables.entry(name).or_insert((is_view, Vec::new()));
        // If we learn it's a view later, update the flag
        if is_view {
            entry.0 = true;
        }
    }

    /// Register a database name for a connection. Idempotent.
    pub fn add_database(&mut self, connection_id: &str, name: String) {
        if !self.databases.iter().any(|(c, n)| c == connection_id && n == &name) {
            self.databases.push((connection_id.to_string(), name));
        }
    }

    /// Register a schema name for a connection. Idempotent.
    pub fn add_schema(&mut self, connection_id: &str, name: String) {
        if !self.schemas.iter().any(|(c, n)| c == connection_id && n == &name) {
            self.schemas.push((connection_id.to_string(), name));
        }
    }

    /// Register a table under a specific database. Also adds the table to the flat map.
    pub fn add_table_to_database(&mut self, database: &str, table: String, is_view: bool) {
        self.add_table(table.clone(), is_view);
        let entry = self.database_tables
            .entry(database.to_string())
            .or_insert_with(Vec::new);
        if !entry.contains(&table) {
            entry.push(table);
        }
    }

    /// Register a table under a specific schema. Also adds the table to the flat map.
    pub fn add_table_to_schema(&mut self, schema: &str, table: String, is_view: bool) {
        self.add_table(table.clone(), is_view);
        let entry = self.schema_tables
            .entry(schema.to_string())
            .or_insert_with(Vec::new);
        if !entry.contains(&table) {
            entry.push(table);
        }
    }

    /// Return database names for a specific connection.
    pub fn database_names_for(&self, connection_id: &str) -> Vec<&str> {
        self.databases
            .iter()
            .filter(|(c, _)| c == connection_id)
            .map(|(_, n)| n.as_str())
            .collect()
    }

    /// Return schema names for a specific connection.
    pub fn schema_names_for(&self, connection_id: &str) -> Vec<&str> {
        self.schemas
            .iter()
            .filter(|(c, _)| c == connection_id)
            .map(|(_, n)| n.as_str())
            .collect()
    }

    /// Return all known database names (across all connections).
    pub fn database_names(&self) -> Vec<&str> {
        self.databases.iter().map(|(_, n)| n.as_str()).collect()
    }

    /// Return all known schema names (across all connections).
    pub fn schema_names(&self) -> Vec<&str> {
        self.schemas.iter().map(|(_, n)| n.as_str()).collect()
    }

    /// Return table names for a given database, if known.
    pub fn tables_for_database(&self, database: &str) -> Option<&[String]> {
        self.database_tables
            .get(database)
            .map(|v| v.as_slice())
            .filter(|v| !v.is_empty())
    }

    /// Return table names for a given schema, if known.
    pub fn tables_for_schema(&self, schema: &str) -> Option<&[String]> {
        self.schema_tables
            .get(schema)
            .map(|v| v.as_slice())
            .filter(|v| !v.is_empty())
    }

    /// Store column definitions for a table.
    pub fn add_columns(&mut self, table: String, columns: Vec<ColumnDefinition>) {
        let entry = self.tables.entry(table).or_insert((false, Vec::new()));
        entry.1 = columns;
    }

    /// Return all known table + view names.
    pub fn table_names(&self) -> Vec<&str> {
        self.tables.keys().map(|k| k.as_str()).collect()
    }

    /// Return column definitions for a given table, if cached.
    pub fn columns_for_table(&self, table: &str) -> Option<&[ColumnDefinition]> {
        self.tables
            .get(table)
            .map(|(_, cols)| cols.as_slice())
            .filter(|cols| !cols.is_empty())
    }

    /// Whether a table is a view.
    pub fn is_view(&self, table: &str) -> bool {
        self.tables
            .get(table)
            .map(|(v, _)| *v)
            .unwrap_or(false)
    }

    /// Number of tables with cached column definitions.
    pub fn tables_with_columns_count(&self) -> usize {
        self.tables
            .values()
            .filter(|(_, cols)| !cols.is_empty())
            .count()
    }

    /// Clear all cached tables.
    pub fn clear(&mut self) {
        self.tables.clear();
        self.database_tables.clear();
        self.schema_tables.clear();
        self.databases.clear();
        self.schemas.clear();
    }
}

/// The SQL context around the cursor, used to filter suggestion types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SqlContext {
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

    fn table_references_before_cursor(sql: &str, cursor_char_index: usize) -> Vec<TableReference> {
        let cursor = floor_char_boundary(sql, cursor_char_index.min(sql.len()));
        let tokens = Self::tokenize_forwards(&sql[..cursor]);
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

    if let Some((parent_path, _)) = prefix.rsplit_once('.') {
        let parent_path = parent_path.trim_matches(|c| c == '"' || c == '\'');
        if parent_path.is_empty() {
            return Some(label.to_string());
        }
        let parent_prefix = format!("{parent_path}.");
        if let Some(remainder) = label.strip_prefix(&parent_prefix) {
            return Some(remainder.to_string());
        }
        if label == parent_path {
            return Some(label.to_string());
        }
        return None;
    }

    if label.starts_with(prefix) || prefix.starts_with(label) {
        return Some(label.to_string());
    }
    None
}

pub(crate) fn autocomplete_min_prefix_len(
    sql: &str,
    cursor_char_index: usize,
    db_kind: Option<DatabaseKind>,
) -> usize {
    if db_kind == Some(DatabaseKind::MongoDb) {
        match detect_mongo_context(sql, cursor_char_index) {
            MongoContext::AfterMethod {
                method,
                field_context,
                active_object_key,
                ..
            } if field_context
                || matches!(method.as_str(), "sort" | "project" | "hint")
                || matches!(
                    active_object_key.as_deref(),
                    Some("$set" | "$project" | "$sort")
                ) =>
            {
                return 1;
            }
            MongoContext::NotMongo => {}
            _ => return 2,
        }
    }

    let context = SqlContextParser::parse(sql, cursor_char_index);
    if matches!(
        context,
        SqlContext::SelectClause
            | SqlContext::WhereClause
            | SqlContext::OrderGroupClause
            | SqlContext::InsertColumns
            | SqlContext::AfterColumnDot { .. }
    ) {
        1
    } else {
        2
    }
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
        db_kind: Option<DatabaseKind>,
        usage_memory: Option<&AutocompleteUsageMemory>,
    ) -> Vec<AutocompleteSuggestion> {
        // MongoDB 模式下优先检测 MongoDB 查询
        if db_kind == Some(DatabaseKind::MongoDb) {
            let mongo_ctx = detect_mongo_context(sql, cursor_char_index);
            if !matches!(mongo_ctx, MongoContext::NotMongo) {
                let prefix = SqlContextParser::current_token_prefix(sql, cursor_char_index);
                return Self::suggest_mongo(mongo_ctx, &prefix, cache, usage_memory);
            }
        }

        let prefix = SqlContextParser::current_token_prefix(sql, cursor_char_index);
        let context = SqlContextParser::parse(sql, cursor_char_index);
        let table_refs = SqlContextParser::table_references_before_cursor(sql, cursor_char_index);

        let mut suggestions = Vec::new();

        match &context {
            SqlContext::AfterColumnDot { parent } => {
                // 1. Check if `parent` is a database name → suggest tables in that database
                if let Some(db_tables) = cache.tables_for_database(parent) {
                    for table_name in db_tables {
                        let is_view = cache.is_view(table_name);
                        suggestions.push(AutocompleteSuggestion {
                            label: table_name.to_string(),
                            kind: if is_view {
                                SuggestionKind::View
                            } else {
                                SuggestionKind::Table
                            },
                            matched_indices: vec![],
                            insertion_text: None,
                            cursor_offset: None,
                        });
                    }
                    let filtered = Self::filter_by_prefix(suggestions, &prefix);
                    return Self::rank(filtered, &prefix, &context, usage_memory);
                }

                // 2. Check if `parent` is a schema name → suggest tables in that schema
                if let Some(schema_tables) = cache.tables_for_schema(parent) {
                    for table_name in schema_tables {
                        let is_view = cache.is_view(table_name);
                        suggestions.push(AutocompleteSuggestion {
                            label: table_name.to_string(),
                            kind: if is_view {
                                SuggestionKind::View
                            } else {
                                SuggestionKind::Table
                            },
                            matched_indices: vec![],
                            insertion_text: None,
                            cursor_offset: None,
                        });
                    }
                    let filtered = Self::filter_by_prefix(suggestions, &prefix);
                    return Self::rank(filtered, &prefix, &context, usage_memory);
                }

                // 3. Resolve alias/table name from the current query scope → suggest columns
                let resolved_parent = table_refs
                    .iter()
                    .find(|r| r.alias.as_deref() == Some(parent.as_str()))
                    .map(|r| r.table_name.as_str())
                    .unwrap_or(parent.as_str());
                if let Some(cols) = lookup_columns_for_table(cache, resolved_parent) {
                    for col in cols {
                        suggestions.push(AutocompleteSuggestion {
                            label: col.name.clone(),
                            kind: SuggestionKind::Column {
                                parent_table: resolved_parent.to_string(),
                            },
                            matched_indices: vec![],
                            insertion_text: None,
                            cursor_offset: None,
                        });
                    }
                }
                let filtered = Self::filter_by_prefix(suggestions, &prefix);
                return Self::rank(filtered, &prefix, &context, usage_memory);
            }
            SqlContext::AfterKeyword { .. } => {
                // Keep object-entry contexts focused on tables/views.
                for name in cache.table_names() {
                    let is_view = cache.is_view(name);
                    suggestions.push(AutocompleteSuggestion {
                        label: name.to_string(),
                        kind: if is_view {
                            SuggestionKind::View
                        } else {
                            SuggestionKind::Table
                        },
                        matched_indices: vec![],
                        insertion_text: None,
                        cursor_offset: None,
                    });
                }
                if !prefix.is_empty() {
                    let db_names = match connection_id {
                        Some(cid) => cache.database_names_for(cid),
                        None => cache.database_names(),
                    };
                    for name in db_names {
                        suggestions.push(AutocompleteSuggestion {
                            label: name.to_string(),
                            kind: SuggestionKind::Database,
                            matched_indices: vec![],
                            insertion_text: None,
                            cursor_offset: None,
                        });
                    }
                    let schema_names = match connection_id {
                        Some(cid) => cache.schema_names_for(cid),
                        None => cache.schema_names(),
                    };
                    for name in schema_names {
                        suggestions.push(AutocompleteSuggestion {
                            label: name.to_string(),
                            kind: SuggestionKind::Schema,
                            matched_indices: vec![],
                            insertion_text: None,
                            cursor_offset: None,
                        });
                    }
                }
            }
            SqlContext::InsertColumns => {
                if let Some(target_table) = SqlContextParser::insert_target_table(sql, cursor_char_index) {
                    if let Some(cols) = lookup_columns_for_table(cache, &target_table) {
                        for col in cols {
                            suggestions.push(AutocompleteSuggestion {
                                label: col.name.clone(),
                                kind: SuggestionKind::Column {
                                    parent_table: target_table.clone(),
                                },
                                matched_indices: vec![],
                                insertion_text: None,
                                cursor_offset: None,
                            });
                        }
                    }
                }
                if suggestions.is_empty() {
                    for (table_name, (_is_view, cols)) in &cache.tables {
                        for col in cols {
                            suggestions.push(AutocompleteSuggestion {
                                label: col.name.clone(),
                                kind: SuggestionKind::Column {
                                    parent_table: table_name.clone(),
                                },
                                matched_indices: vec![],
                                insertion_text: None,
                                cursor_offset: None,
                            });
                        }
                    }
                }
            }
            SqlContext::AliasName => {
                suggestions.extend(Self::alias_suggestions(sql, cursor_char_index, &table_refs));
            }
            SqlContext::SelectClause
            | SqlContext::WhereClause
            | SqlContext::OrderGroupClause => {
                // Prefer columns from tables already referenced in the current query.
                if !table_refs.is_empty() {
                    for table_ref in &table_refs {
                        if let Some(cols) = lookup_columns_for_table(cache, &table_ref.table_name) {
                            for col in cols {
                                suggestions.push(AutocompleteSuggestion {
                                    label: col.name.clone(),
                                    kind: SuggestionKind::Column {
                                        parent_table: table_ref.table_name.clone(),
                                    },
                                    matched_indices: vec![],
                                    insertion_text: None,
                                    cursor_offset: None,
                                });
                            }
                        }
                    }
                } else {
                    for (table_name, (_is_view, cols)) in &cache.tables {
                        for col in cols {
                            suggestions.push(AutocompleteSuggestion {
                                label: col.name.clone(),
                                kind: SuggestionKind::Column {
                                    parent_table: table_name.clone(),
                                },
                                matched_indices: vec![],
                                insertion_text: None,
                                cursor_offset: None,
                            });
                        }
                    }
                }
                if !prefix.is_empty() {
                    // Add keyword suggestions once the user starts narrowing.
                    for kw in SQL_KEYWORDS {
                        suggestions.push(AutocompleteSuggestion {
                            label: kw.to_string(),
                            kind: SuggestionKind::Keyword,
                            matched_indices: vec![],
                            insertion_text: None,
                            cursor_offset: None,
                        });
                    }
                    Self::push_function_snippets(&mut suggestions, &context, db_kind);
                } else if matches!(context, SqlContext::WhereClause) {
                    Self::push_where_operator_snippets(&mut suggestions);
                }
            }
            SqlContext::General => {
                // Suggest everything: keywords + tables + databases + schemas + columns
                for kw in SQL_KEYWORDS {
                    suggestions.push(AutocompleteSuggestion {
                        label: kw.to_string(),
                        kind: SuggestionKind::Keyword,
                        matched_indices: vec![],
                        insertion_text: None,
                        cursor_offset: None,
                    });
                }
                for name in cache.table_names() {
                    let is_view = cache.is_view(name);
                    suggestions.push(AutocompleteSuggestion {
                        label: name.to_string(),
                        kind: if is_view {
                            SuggestionKind::View
                        } else {
                            SuggestionKind::Table
                        },
                        matched_indices: vec![],
                        insertion_text: None,
                        cursor_offset: None,
                    });
                }
                let db_names = match connection_id {
                    Some(cid) => cache.database_names_for(cid),
                    None => cache.database_names(),
                };
                for name in db_names {
                    suggestions.push(AutocompleteSuggestion {
                        label: name.to_string(),
                        kind: SuggestionKind::Database,
                        matched_indices: vec![],
                        insertion_text: None,
                        cursor_offset: None,
                    });
                }
                let schema_names = match connection_id {
                    Some(cid) => cache.schema_names_for(cid),
                    None => cache.schema_names(),
                };
                for name in schema_names {
                    suggestions.push(AutocompleteSuggestion {
                        label: name.to_string(),
                        kind: SuggestionKind::Schema,
                        matched_indices: vec![],
                        insertion_text: None,
                        cursor_offset: None,
                    });
                }
                for (table_name, (_is_view, cols)) in &cache.tables {
                    for col in cols {
                        suggestions.push(AutocompleteSuggestion {
                            label: col.name.clone(),
                            kind: SuggestionKind::Column {
                                parent_table: table_name.clone(),
                            },
                            matched_indices: vec![],
                            insertion_text: None,
                            cursor_offset: None,
                        });
                    }
                }
                Self::push_function_snippets(&mut suggestions, &context, db_kind);
            }
        }

        let filtered = Self::filter_by_prefix(suggestions, &prefix);
        Self::rank(filtered, &prefix, &context, usage_memory)
    }

    fn suggest_mongo(
        ctx: MongoContext,
        prefix: &str,
        cache: &SchemaCache,
        usage_memory: Option<&AutocompleteUsageMemory>,
    ) -> Vec<AutocompleteSuggestion> {
        let mut suggestions = Vec::new();
        match ctx {
            MongoContext::AfterDbDot => {
                for name in cache.table_names() {
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
                                &collection,
                                field_path_prefix.as_deref(),
                            );
                        } else if field_context {
                            Self::push_mongo_operators(&mut suggestions, MONGO_UPDATE_OPERATORS);
                        } else {
                            Self::push_mongo_collection_fields(
                                &mut suggestions,
                                cache,
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
        Self::rank(filtered, prefix, &SqlContext::General, usage_memory)
    }

    fn push_mongo_collection_fields(
        suggestions: &mut Vec<AutocompleteSuggestion>,
        cache: &SchemaCache,
        collection: &str,
        path_prefix: Option<&str>,
    ) {
        if let Some(cols) = lookup_columns_for_table(cache, collection) {
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
        let prefix_chars: Vec<char> = prefix.to_lowercase().chars().collect();
        suggestions
            .into_iter()
            .filter_map(|mut s| {
                if let Some(indices) = subsequence_match(&s.label, &prefix_chars) {
                    s.matched_indices = indices;
                    Some(s)
                } else {
                    None
                }
            })
            .collect()
    }

    fn rank(
        mut suggestions: Vec<AutocompleteSuggestion>,
        prefix: &str,
        context: &SqlContext,
        usage_memory: Option<&AutocompleteUsageMemory>,
    ) -> Vec<AutocompleteSuggestion> {
        let prefix_lower = prefix.to_lowercase();
        // Sort: exact match → starts-with prefix → shorter label → kind
        suggestions.sort_by(|a, b| {
            let a_exact = a.label.to_lowercase() == prefix_lower;
            let b_exact = b.label.to_lowercase() == prefix_lower;
            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }
            // "starts with" = first matched char is at index 0
            let a_starts = a.matched_indices.first().map_or(false, |&i| i == 0);
            let b_starts = b.matched_indices.first().map_or(false, |&i| i == 0);
            if a_starts != b_starts {
                return b_starts.cmp(&a_starts);
            }
            let context_priority = |s: &AutocompleteSuggestion| -> i32 {
                match context {
                    SqlContext::AliasName => match s.kind {
                        SuggestionKind::Loading => 4,
                        SuggestionKind::Column { .. } => 0,
                        SuggestionKind::Function => 1,
                        SuggestionKind::Keyword => 2,
                        _ => 3,
                    },
                    SqlContext::WhereClause | SqlContext::OrderGroupClause | SqlContext::SelectClause => {
                        match s.kind {
                            SuggestionKind::Loading => 5,
                            SuggestionKind::Column { .. } => {
                                if s.label.contains('.') { 3 } else { 4 }
                            }
                            SuggestionKind::Function => 1,
                            SuggestionKind::Keyword => 0,
                            _ => 0,
                        }
                    }
                    _ => 0,
                }
            };
            let a_priority = context_priority(a);
            let b_priority = context_priority(b);
            if a_priority != b_priority {
                return b_priority.cmp(&a_priority);
            }
            let a_recent = usage_memory.map(|m| m.score(&a.label)).unwrap_or(0);
            let b_recent = usage_memory.map(|m| m.score(&b.label)).unwrap_or(0);
            if a_recent != b_recent {
                return b_recent.cmp(&a_recent);
            }
            // Shorter labels rank higher (closer match)
            if a.label.len() != b.label.len() {
                return a.label.len().cmp(&b.label.len());
            }
            // Then by kind: databases first, then schemas, tables, columns, keywords
            let kind_order = |k: &SuggestionKind| match k {
                SuggestionKind::Loading => 0,
                SuggestionKind::Database => 1,
                SuggestionKind::Schema => 2,
                SuggestionKind::Table => 3,
                SuggestionKind::View => 4,
                SuggestionKind::Column { .. } => 5,
                SuggestionKind::Function => 6,
                SuggestionKind::Keyword => 7,
                SuggestionKind::MongoKeyword => 7,
            };
            let a_kind = kind_order(&a.kind);
            let b_kind = kind_order(&b.kind);
            if a_kind != b_kind {
                return a_kind.cmp(&b_kind);
            }
            a.label.to_lowercase().cmp(&b.label.to_lowercase())
        });
        // Limit to 50 suggestions
        suggestions.truncate(50);
        suggestions
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

fn lookup_columns_for_table<'a>(
    cache: &'a SchemaCache,
    table: &str,
) -> Option<&'a [ColumnDefinition]> {
    cache
        .columns_for_table(table)
        .or_else(|| table.rsplit('.').next().and_then(|short| cache.columns_for_table(short)))
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
    let replace_end = token_end.max(cursor_char_index);

    let before = &sql[..char_to_byte_index(sql, replace_start)];
    let after = &sql[char_to_byte_index(sql, replace_end)..];
    let new_cursor = replace_start + cursor_offset.unwrap_or(replacement.chars().count());

    (format!("{}{}{}", before, replacement, after), new_cursor)
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
    /// Timestamp of last keystroke (used for 300ms debounce auto-trigger).
    pub last_keystroke: Option<std::time::Instant>,
    /// Whether to trigger on next frame (for Ctrl+Space and `.` triggers).
    pub trigger_requested: bool,
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
        self.trigger_requested = false;
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
    let min_popup_width: f32 = 220.0;
    let max_popup_width: f32 = 380.0;
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
    let row_height = 28.0;
    let min_visible_rows = 8;
    let max_visible_rows = 15;
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

                            // Single click: first click selects, second click commits
                            // Double click: commits directly
                            if !is_loading_row && row_response.double_clicked() {
                                let text = suggestion.insertion_text.clone().unwrap_or_else(|| suggestion.label.clone());
                                committed = Some((suggestion.label.clone(), text, suggestion.cursor_offset));
                            } else if !is_loading_row && row_response.clicked() {
                                if state.clicked_index == Some(i) {
                                    let text = suggestion.insertion_text.clone().unwrap_or_else(|| suggestion.label.clone());
                                    committed = Some((suggestion.label.clone(), text, suggestion.cursor_offset));
                                } else {
                                    state.clicked_index = Some(i);
                                    state.selected_index = i;
                                }
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

    fn make_cache(tables: &[(&str, bool, &[&str])]) -> SchemaCache {
        let mut cache = SchemaCache::new();
        for (name, is_view, cols) in tables {
            cache.add_table(name.to_string(), *is_view);
            if !cols.is_empty() {
                cache.add_columns(
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
        let suggestions = AutocompleteEngine::suggest("SELECT id, ", 12, &cache, None, None, None);
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
        let suggestions = AutocompleteEngine::suggest("SELECT * FROM ", 14, &cache, None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "users"));
        assert!(suggestions.iter().any(|s| s.label == "orders"));
        assert!(!suggestions.iter().any(|s| matches!(s.kind, SuggestionKind::Database)));
        assert!(!suggestions.iter().any(|s| matches!(s.kind, SuggestionKind::Schema)));
    }

    #[test]
    fn engine_suggests_columns_after_dot() {
        let cache = make_cache(&[("users", false, &["id", "name", "email"])]);
        let suggestions = AutocompleteEngine::suggest("SELECT users.nam", 15, &cache, None, None, None);
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
            AutocompleteEngine::suggest("SELECT u. FROM users u JOIN orders o ON u.id = o.id", 9, &cache, None, None, None);
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
        let suggestions = AutocompleteEngine::suggest(sql, 7, &cache, None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "total"));
        assert!(!suggestions.iter().any(|s| s.label == "products.sku"));
        assert!(!suggestions.iter().any(|s| s.label == "sku"));
    }

    #[test]
    fn engine_manual_trigger_without_prefix_in_where_prefers_columns_only() {
        let cache = make_cache(&[("users", false, &["id", "name"])]);
        let suggestions =
            AutocompleteEngine::suggest("SELECT * FROM users WHERE ", 26, &cache, None, None, None);
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
        let suggestions = AutocompleteEngine::suggest("INSERT INTO users (", 19, &cache, None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "name"));
        assert!(!suggestions.iter().any(|s| s.label == "order_id"));
    }

    #[test]
    fn engine_suggests_alias_candidates_after_as() {
        let cache = make_cache(&[("order_items", false, &["id"])]);
        let suggestions =
            AutocompleteEngine::suggest("SELECT * FROM order_items AS ", 29, &cache, None, None, None);
        assert!(suggestions.iter().any(|s| s.label == "oi"));
        assert!(suggestions.iter().any(|s| s.label == "order_items"));
        assert!(!suggestions.iter().any(|s| matches!(s.kind, SuggestionKind::Function)));
    }

    #[test]
    fn engine_suggests_function_snippets_in_select_context() {
        let cache = make_cache(&[("users", false, &["id"])]);
        let suggestions = AutocompleteEngine::suggest("SELECT CO", 9, &cache, None, None, None);
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
            "SELECT DA",
            9,
            &cache,
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
        let ifnull = suggestions.iter().find(|s| s.label == "IFNULL").unwrap();
        assert_eq!(ifnull.insertion_text.as_deref(), Some("IFNULL(expr, fallback)"));
        assert!(!suggestions.iter().any(|s| s.label == "DATE_TRUNC"));
    }

    #[test]
    fn engine_adds_postgres_specific_function_snippets() {
        let cache = make_cache(&[("users", false, &["id"])]);
        let suggestions = AutocompleteEngine::suggest(
            "SELECT DA",
            9,
            &cache,
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
        let to_char = suggestions.iter().find(|s| s.label == "TO_CHAR").unwrap();
        assert_eq!(to_char.insertion_text.as_deref(), Some("TO_CHAR(value, 'YYYY-MM-DD')"));
        assert!(!suggestions.iter().any(|s| s.label == "DATE_FORMAT"));
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
        let suggestions = AutocompleteEngine::suggest("SELECT id", 9, &cache, None, None, None);
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
    fn apply_autocomplete_keeps_templates_cursor_offset() {
        let (sql, cursor) = apply_autocomplete_suggestion("db.users.fi|nd".replace('|', "").as_str(), 11, 9, "find({})", Some(5));
        assert_eq!(sql, "db.users.find({})");
        assert_eq!(cursor, 14);
    }

    #[test]
    fn mongo_find_prefers_collection_fields_inside_filter_object() {
        let cache = make_cache(&[("users", false, &["name", "age", "email"])]);
        let suggestions = AutocompleteEngine::suggest(
            "db.users.find({ na",
            18,
            &cache,
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
    fn mongo_field_context_uses_single_char_trigger_threshold() {
        assert_eq!(
            autocomplete_min_prefix_len(
                "db.users.find({ n",
                "db.users.find({ n".chars().count(),
                Some(DatabaseKind::MongoDb),
            ),
            1
        );
        assert_eq!(
            autocomplete_min_prefix_len(
                "db.users.up",
                "db.users.up".chars().count(),
                Some(DatabaseKind::MongoDb),
            ),
            2
        );
    }

    #[test]
    fn cache_stores_and_retrieves_tables() {
        let mut cache = SchemaCache::new();
        cache.add_table("users".into(), false);
        cache.add_columns("users".into(), vec![
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
        assert!(cache.table_names().contains(&"users"));
        assert_eq!(cache.columns_for_table("users").unwrap().len(), 1);
    }

    #[test]
    fn schema_cache_is_view_flag() {
        let mut cache = SchemaCache::new();
        cache.add_table("v".into(), true);
        assert!(cache.is_view("v"));
        cache.add_table("t".into(), false);
        assert!(!cache.is_view("t"));
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
}
