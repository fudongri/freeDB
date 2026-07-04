use core_domain::{ColumnDefinition, DatabaseKind};
use eframe::egui::{self, Align2, Area, Color32, FontFamily, FontId, Id, Order, RichText, ScrollArea, Sense, Stroke};
use eframe::egui::text::{LayoutJob, TextFormat};
use i18n::tr;
use std::collections::HashMap;

/// Snap a byte index to the nearest preceding UTF-8 character boundary.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    let mut bound = index.min(s.len());
    while bound > 0 && !s.is_char_boundary(bound) {
        bound -= 1;
    }
    bound
}

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
}

impl AutocompleteSuggestion {
    pub fn new(label: String, kind: SuggestionKind) -> Self {
        Self { label, kind, matched_indices: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SuggestionKind {
    Database,
    Schema,
    Table,
    View,
    Column { parent_table: String },
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
    /// Fallback — suggest everything.
    General,
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

        // --- 1) Check for `alias.` or `table.` pattern immediately before cursor ---
        if let Some(ctx) = Self::after_dot_context(sql, cursor) {
            return ctx;
        }

        // --- 2) Walk backward from cursor to find the preceding keyword ---
        let prefix = &sql[..cursor];
        let tokens = Self::tokenize_backwards(prefix);

        for token in &tokens {
            let upper = token.to_ascii_uppercase();

            // Skip comma — keep looking
            if upper == "," {
                continue;
            }

            // After ( — in INSERT INTO ... VALUES( — suggest columns if in insert context
            if upper == "(" {
                if Self::check_insert_columns(&tokens) {
                    return SqlContext::InsertColumns;
                }
                continue;
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
            if Self::preceded_by_comma_in_scan(&tokens, &upper) {
                // Check what bigger clause we're in
                if let Some(clause_ctx) = Self::enclosing_clause(&tokens) {
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
        let cursor = cursor_char_index.min(sql.len());
        // Clamp to a valid UTF-8 char boundary (egui may return byte indices)
        let cursor = floor_char_boundary(sql, cursor);
        let prefix = &sql[..cursor];
        let mut chars: Vec<char> = prefix.chars().collect();
        let mut result = String::new();
        // Walk backwards collecting identifier characters
        while let Some(&ch) = chars.last() {
            if ch.is_alphanumeric() || ch == '_' {
                result.push(ch);
                chars.pop();
            } else {
                break;
            }
        }
        result.chars().rev().collect()
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

    fn is_sql_keyword(token: &str) -> bool {
        SQL_KEYWORDS.contains(&token)
    }

    fn check_insert_columns(tokens: &[String]) -> bool {
        // If we see a pattern suggesting INSERT INTO ... VALUES ( context
        for t in tokens {
            let u = t.to_ascii_uppercase();
            if u == "INTO" || u == "VALUES" {
                return true;
            }
        }
        false
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

/// MongoDB 查询上下文
enum MongoContext {
    /// db. → 建议集合名
    AfterDbDot,
    /// db.coll. → 建议方法名
    AfterCollection { collection: String },
    /// db.coll.method( → 在方法参数内，建议操作符
    AfterMethod { collection: String, method: String },
    /// 非 MongoDB 查询
    NotMongo,
}

fn detect_mongo_context(sql: &str, cursor: usize) -> MongoContext {
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
                if rest.contains('(') {
                    return MongoContext::AfterMethod { collection, method: method.to_string() };
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
    ) -> Vec<AutocompleteSuggestion> {
        // MongoDB 模式下优先检测 MongoDB 查询
        if db_kind == Some(DatabaseKind::MongoDb) {
            let mongo_ctx = detect_mongo_context(sql, cursor_char_index);
            if !matches!(mongo_ctx, MongoContext::NotMongo) {
                let prefix = SqlContextParser::current_token_prefix(sql, cursor_char_index);
                return Self::suggest_mongo(mongo_ctx, &prefix, cache);
            }
        }

        let prefix = SqlContextParser::current_token_prefix(sql, cursor_char_index);
        let context = SqlContextParser::parse(sql, cursor_char_index);

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
                        });
                    }
                    let filtered = Self::filter_by_prefix(suggestions, &prefix);
                    return Self::rank(filtered, &prefix);
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
                        });
                    }
                    let filtered = Self::filter_by_prefix(suggestions, &prefix);
                    return Self::rank(filtered, &prefix);
                }

                // 3. Fallback: treat `parent` as a table name → suggest columns
                if let Some(cols) = cache.columns_for_table(parent) {
                    for col in cols {
                        suggestions.push(AutocompleteSuggestion {
                            label: col.name.clone(),
                            kind: SuggestionKind::Column {
                                parent_table: parent.clone(),
                            },
                            matched_indices: vec![],
                        });
                    }
                }
                let filtered = Self::filter_by_prefix(suggestions, &prefix);
                return Self::rank(filtered, &prefix);
            }
            SqlContext::AfterKeyword { .. } | SqlContext::InsertColumns => {
                // Suggest table names + views
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
                    });
                }
                // Also suggest database and schema names (filtered by connection)
                let db_names = match connection_id {
                    Some(cid) => cache.database_names_for(cid),
                    None => cache.database_names(),
                };
                for name in db_names {
                    suggestions.push(AutocompleteSuggestion {
                        label: name.to_string(),
                        kind: SuggestionKind::Database,
                        matched_indices: vec![],
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
                    });
                }
            }
            SqlContext::SelectClause
            | SqlContext::WhereClause
            | SqlContext::OrderGroupClause => {
                // Suggest columns (from all cached tables) + keywords
                for (table_name, (_is_view, cols)) in &cache.tables {
                    for col in cols {
                        suggestions.push(AutocompleteSuggestion {
                            label: col.name.clone(),
                            kind: SuggestionKind::Column {
                                parent_table: table_name.clone(),
                            },
                            matched_indices: vec![],
                        });
                    }
                }
                // Also add table-qualified form: table.column
                for (table_name, (_is_view, cols)) in &cache.tables {
                    for col in cols {
                        let qualified = format!("{}.{}", table_name, col.name);
                        if !suggestions.iter().any(|s| s.label == qualified) {
                            suggestions.push(AutocompleteSuggestion {
                                label: qualified,
                                kind: SuggestionKind::Column {
                                    parent_table: table_name.clone(),
                                },
                                matched_indices: vec![],
                            });
                        }
                    }
                }
                // Add keyword suggestions for column contexts
                for kw in SQL_KEYWORDS {
                    suggestions.push(AutocompleteSuggestion {
                        label: kw.to_string(),
                        kind: SuggestionKind::Keyword,
                        matched_indices: vec![],
                    });
                }
            }
            SqlContext::General => {
                // Suggest everything: keywords + tables + databases + schemas + columns
                for kw in SQL_KEYWORDS {
                    suggestions.push(AutocompleteSuggestion {
                        label: kw.to_string(),
                        kind: SuggestionKind::Keyword,
                        matched_indices: vec![],
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
                        });
                    }
                }
            }
        }

        let filtered = Self::filter_by_prefix(suggestions, &prefix);
        Self::rank(filtered, &prefix)
    }

    fn suggest_mongo(
        ctx: MongoContext,
        prefix: &str,
        cache: &SchemaCache,
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
                    suggestions.push(AutocompleteSuggestion::new(
                        m.to_string(),
                        SuggestionKind::MongoKeyword,
                    ));
                }
                for m in MONGO_CURSOR_METHODS {
                    suggestions.push(AutocompleteSuggestion::new(
                        m.to_string(),
                        SuggestionKind::MongoKeyword,
                    ));
                }
            }
            MongoContext::AfterMethod { .. } => {
                for op in MONGO_OPERATORS {
                    suggestions.push(AutocompleteSuggestion::new(
                        op.to_string(),
                        SuggestionKind::MongoKeyword,
                    ));
                }
            }
            MongoContext::NotMongo => {}
        }
        let filtered = Self::filter_by_prefix(suggestions, prefix);
        Self::rank(filtered, prefix)
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
            // Shorter labels rank higher (closer match)
            if a.label.len() != b.label.len() {
                return a.label.len().cmp(&b.label.len());
            }
            // Then by kind: databases first, then schemas, tables, columns, keywords
            let kind_order = |k: &SuggestionKind| match k {
                SuggestionKind::Database => 0,
                SuggestionKind::Schema => 1,
                SuggestionKind::Table => 2,
                SuggestionKind::View => 3,
                SuggestionKind::Column { .. } => 4,
                SuggestionKind::Keyword => 5,
                SuggestionKind::MongoKeyword => 5,
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
}

impl AutocompleteState {
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.selected_index = 0;
        self.clicked_index = None;
        self.anchor_pos = None;
        self.popup_rect = None;
        self.trigger_requested = false;
    }
}

/// Icon character for each suggestion kind.
pub(crate) fn suggestion_kind_icon(kind: SuggestionKind) -> &'static str {
    match kind {
        SuggestionKind::Database => "\u{1F4BE}",
        SuggestionKind::Schema => "\u{1F4C1}",
        SuggestionKind::Table => "\u{1F4E6}",
        SuggestionKind::View => "\u{1F441}",
        SuggestionKind::Column { .. } => "\u{1F4CB}",
        SuggestionKind::Keyword => "\u{1F511}",
        SuggestionKind::MongoKeyword => "\u{26AB}",
    }
}

/// Secondary label text for display (e.g., "(column)" or table name for columns).
pub(crate) fn suggestion_kind_label(kind: SuggestionKind) -> String {
    match kind {
        SuggestionKind::Database => tr!("数据库").to_string(),
        SuggestionKind::Schema => "Schema".to_string(),
        SuggestionKind::Table => tr!("表").to_string(),
        SuggestionKind::View => tr!("视图").to_string(),
        SuggestionKind::Column { parent_table } => tr!("列 · {}", parent_table),
        SuggestionKind::Keyword => tr!("关键字").to_string(),
        SuggestionKind::MongoKeyword => "Mongo".to_string(),
    }
}

/// Render the autocomplete popup. Returns the selected suggestion label if committed.
pub(crate) fn render_autocomplete_popup(
    ctx: &egui::Context,
    state: &mut AutocompleteState,
    suggestions: &[AutocompleteSuggestion],
    palette: &AutocompletePalette,
) -> Option<String> {
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

    let mut committed: Option<String> = None;

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

                            let row_response = ui.interact(row_rect, row_id, Sense::click());

                            // Keep selected row visible when navigating with keyboard
                            if is_selected {
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
                                    Color32::from_rgb(255, 255, 120) // bright yellow on dark selected bg
                                } else {
                                    Color32::from_rgb(86, 156, 214) // VS Code-like blue for matches
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
                            if row_response.double_clicked() {
                                committed = Some(suggestion.label.clone());
                            } else if row_response.clicked() {
                                if state.clicked_index == Some(i) {
                                    committed = Some(suggestion.label.clone());
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
}

/// Derive autocomplete palette from the application theme.
pub(crate) fn autocomplete_palette(dark_mode: bool) -> AutocompletePalette {
    if dark_mode {
        AutocompletePalette {
            popup_bg: Color32::from_rgb(40, 43, 48),
            border: Color32::from_rgb(80, 85, 94),
            text: Color32::from_rgb(214, 222, 235),
            weak_text: Color32::from_rgb(108, 121, 145),
            selected_bg: Color32::from_rgb(9, 71, 143),
            selected_text: Color32::from_rgb(243, 247, 252),
        }
    } else {
        AutocompletePalette {
            popup_bg: Color32::from_rgb(248, 249, 251),
            border: Color32::from_rgb(200, 205, 215),
            text: Color32::from_rgb(34, 42, 56),
            weak_text: Color32::from_rgb(120, 132, 148),
            selected_bg: Color32::from_rgb(9, 97, 215),
            selected_text: Color32::WHITE,
        }
    }
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
    fn context_parser_join_suggests_tables() {
        let c = SqlContextParser::parse("SELECT * FROM t JOIN ", 22);
        assert_eq!(c, SqlContext::AfterKeyword { keyword: "JOIN".into() });
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
    fn engine_suggests_columns_in_select_context() {
        let cache = make_cache(&[("users", false, &["id", "name", "email"])]);
        // Cursor after comma+space in select list: "SELECT id, "
        let suggestions = AutocompleteEngine::suggest("SELECT id, ", 12, &cache);
        assert!(suggestions.iter().any(|s| s.label == "id"));
        assert!(suggestions.iter().any(|s| s.label == "name"));
    }

    #[test]
    fn engine_suggests_tables_after_from() {
        let cache = make_cache(&[
            ("users", false, &["id"]),
            ("orders", false, &["id"]),
        ]);
        let suggestions = AutocompleteEngine::suggest("SELECT * FROM ", 14, &cache);
        assert!(suggestions.iter().any(|s| s.label == "users"));
        assert!(suggestions.iter().any(|s| s.label == "orders"));
    }

    #[test]
    fn engine_suggests_columns_after_dot() {
        let cache = make_cache(&[("users", false, &["id", "name", "email"])]);
        let suggestions = AutocompleteEngine::suggest("SELECT users.nam", 15, &cache);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].label, "name");
    }

    #[test]
    fn engine_prefix_filter_ranks_exact_first() {
        let cache = make_cache(&[("t", false, &["id", "idea", "aid"])]);
        let suggestions = AutocompleteEngine::suggest("SELECT id", 9, &cache);
        assert_eq!(suggestions.first().unwrap().label, "id");
        assert_eq!(suggestions[1].label, "idea"); // starts-with id
        assert_eq!(suggestions[2].label, "aid");  // contains id
    }

    #[test]
    fn cache_stores_and_retrieves_tables() {
        let mut cache = SchemaCache::new();
        cache.add_table("users".into(), false);
        cache.add_columns("users".into(), vec![
            ColumnDefinition { name: "id".into(), data_type: "int".into(), nullable: false, primary_key: true, default_value: None, comment: None },
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
        let a = AutocompleteSuggestion { label: "x".into(), kind: SuggestionKind::Table, matched_indices: vec![] };
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
}
