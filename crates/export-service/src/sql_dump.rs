use core_domain::{DatabaseKind, QueryCellValue, QueryResult, TableDefinition};

/// Format a single table's SQL dump.
///
/// - `table_def`: column metadata and optional CREATE TABLE SQL (MySQL provides this natively).
/// - `data`: full query result with all rows, or `None` for structure-only.
/// - `db_kind`: MySQL vs Postgres (determines quoting style).
pub fn dump_table_sql(
    table_name: &str,
    table_def: &TableDefinition,
    data: Option<&QueryResult>,
    db_kind: DatabaseKind,
    include_data: bool,
) -> String {
    let mut out = String::new();

    // ── header ──
    out.push_str(&format!("-- Table: {table_name}\n"));
    out.push_str("DROP TABLE IF EXISTS ");
    out.push_str(&quote_ident(table_name, db_kind));
    out.push_str(";\n");

    // ── CREATE TABLE ──
    match db_kind {
        DatabaseKind::MySql => {
            if let Some(ref create_sql) = table_def.create_sql {
                out.push_str(create_sql);
                if !create_sql.ends_with(';') {
                    out.push(';');
                }
                out.push('\n');
            } else {
                out.push_str(&build_create_table_from_columns(table_name, table_def, db_kind));
            }
        }
        DatabaseKind::Postgres => {
            out.push_str(&build_create_table_from_columns(table_name, table_def, db_kind));
        }
        DatabaseKind::MongoDb => {}
    }

    // ── INSERT data ──
    if include_data {
        if let Some(result) = data {
            out.push('\n');
            out.push_str(&format_inserts(table_name, result, db_kind));
        }
    }

    out.push('\n');
    out
}

/// Format an entire database dump (multiple tables).
pub fn dump_database_sql(
    tables: Vec<(String, TableDefinition, Option<QueryResult>)>,
    db_kind: DatabaseKind,
    include_data: bool,
) -> String {
    let mut out = String::new();
    out.push_str("-- =============================================\n");
    out.push_str("-- FreeDB SQL Dump\n");
    out.push_str(&format!("-- Tables: {}\n", tables.len()));
    out.push_str("-- =============================================\n\n");

    match db_kind {
        DatabaseKind::MySql => out.push_str("SET FOREIGN_KEY_CHECKS = 0;\n\n"),
        DatabaseKind::Postgres => out.push_str("SET session_replication_role = 'replica';\n\n"),
        DatabaseKind::MongoDb => {}
    }

    for (name, def, data) in &tables {
        out.push_str(&dump_table_sql(name, def, data.as_ref(), db_kind, include_data));
        out.push('\n');
    }

    match db_kind {
        DatabaseKind::MySql => out.push_str("SET FOREIGN_KEY_CHECKS = 1;\n"),
        DatabaseKind::Postgres => out.push_str("SET session_replication_role = 'origin';\n"),
        DatabaseKind::MongoDb => {}
    }
    out
}

// ── Internal helpers ──

fn quote_ident(name: &str, db_kind: DatabaseKind) -> String {
    match db_kind {
        DatabaseKind::MySql => format!("`{}`", name.replace('`', "``")),
        DatabaseKind::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        DatabaseKind::MongoDb => name.to_string(),
    }
}

fn escape_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "''")
}

fn format_cell(value: &QueryCellValue, db_kind: DatabaseKind) -> String {
    match value {
        QueryCellValue::Null => "NULL".to_string(),
        QueryCellValue::Text(text) => {
            // Try to detect numeric / boolean values and avoid quoting them
            if looks_like_number(text) {
                text.clone()
            } else {
                match db_kind {
                    DatabaseKind::MySql => format!("'{}'", escape_string(text)),
                    DatabaseKind::Postgres => format!("'{}'", escape_string(text)),
                    DatabaseKind::MongoDb => format!("'{}'", escape_string(text)),
                }
            }
        }
    }
}

fn looks_like_number(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Integer
    if s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '+') {
        return true;
    }
    // Float
    if let Ok(_) = s.parse::<f64>() {
        return true;
    }
    // Boolean
    matches!(s, "true" | "false" | "TRUE" | "FALSE")
}

fn build_create_table_from_columns(
    table_name: &str,
    table_def: &TableDefinition,
    db_kind: DatabaseKind,
) -> String {
    let mut out = String::from("CREATE TABLE ");
    out.push_str(&quote_ident(table_name, db_kind));
    out.push_str(" (\n");

    let pk_columns: Vec<&str> = table_def
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.as_str())
        .collect();

    for (i, col) in table_def.columns.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&quote_ident(&col.name, db_kind));
        out.push(' ');

        // Map generic data_type to dialect-specific type
        let dtype = map_data_type(&col.data_type, db_kind, col.auto_increment);
        out.push_str(&dtype);

        if col.auto_increment {
            match db_kind {
                DatabaseKind::MySql => out.push_str(" AUTO_INCREMENT"),
                DatabaseKind::Postgres => {} // SERIAL already implies this
                DatabaseKind::MongoDb => {}
            }
        }

        if !col.nullable {
            out.push_str(" NOT NULL");
        }

        if let Some(ref default) = col.default_value {
            if !col.auto_increment {
                out.push_str(&format!(" DEFAULT {}", format_default_value(default, db_kind)));
            }
        }

        if let Some(ref comment) = col.comment {
            match db_kind {
                DatabaseKind::MySql => out.push_str(&format!(" COMMENT '{}'", escape_string(comment))),
                DatabaseKind::Postgres => {}
                DatabaseKind::MongoDb => {}
            }
        }

        if i < table_def.columns.len() - 1 || !pk_columns.is_empty() {
            out.push(',');
        }
        out.push('\n');
    }

    // Primary key constraint
    if !pk_columns.is_empty() {
        out.push_str("    PRIMARY KEY (");
        out.push_str(
            &pk_columns
                .iter()
                .map(|c| quote_ident(c, db_kind))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(")\n");
    }

    out.push_str(");\n");

    // Postgres: add COMMENT ON statements
    if matches!(db_kind, DatabaseKind::Postgres) {
        for col in &table_def.columns {
            if let Some(ref comment) = col.comment {
                out.push_str(&format!(
                    "COMMENT ON COLUMN {}.{} IS '{}';\n",
                    quote_ident(table_name, db_kind),
                    quote_ident(&col.name, db_kind),
                    escape_string(comment),
                ));
            }
        }
    }

    out
}

fn map_data_type(data_type: &str, db_kind: DatabaseKind, is_auto: bool) -> String {
    let lower = data_type.to_ascii_lowercase();
    match db_kind {
        DatabaseKind::MySql => data_type.to_string(),
        DatabaseKind::Postgres => {
            if is_auto {
                match lower.as_str() {
                    "integer" | "int4" | "int" => "SERIAL".to_string(),
                    "bigint" | "int8" => "BIGSERIAL".to_string(),
                    "smallint" | "int2" => "SMALLSERIAL".to_string(),
                    _ => format!("{} GENERATED BY DEFAULT AS IDENTITY", data_type),
                }
            } else {
                data_type.to_string()
            }
        }
        DatabaseKind::MongoDb => data_type.to_string(),
    }
}

fn format_default_value(default: &str, _db_kind: DatabaseKind) -> String {
    let lower = default.to_ascii_lowercase();
    // Postgres nextval → skip (handled by SERIAL)
    if lower.starts_with("nextval(") {
        return "DEFAULT".to_string(); // Should not happen if auto_increment is filtered
    }
    // Postgres now() / CURRENT_TIMESTAMP etc. → pass through
    if lower.contains("now()")
        || lower.contains("current_timestamp")
        || lower.contains("current_date")
        || lower.starts_with("'")
    {
        return default.to_string();
    }
    // MySQL CURRENT_TIMESTAMP etc.
    if lower == "current_timestamp" || lower == "current_date" || lower == "current_time" {
        return default.to_ascii_uppercase();
    }
    // If it looks numeric, pass through
    if looks_like_number(default) {
        return default.to_string();
    }
    // Otherwise quote it
    format!("'{}'", escape_string(default))
}

fn format_inserts(table_name: &str, result: &QueryResult, db_kind: DatabaseKind) -> String {
    if result.rows.is_empty() {
        return String::new();
    }

    let columns = &result.columns;
    let quoted_cols: Vec<String> = columns.iter().map(|c| quote_ident(c, db_kind)).collect();
    let col_list = quoted_cols.join(", ");

    let mut out = String::new();
    let max_batch = 100; // rows per INSERT statement
    let mut row_count = 0;

    for chunk in result.rows.chunks(max_batch) {
        out.push_str(&format!("INSERT INTO {} ({}) VALUES\n", quote_ident(table_name, db_kind), col_list));
        for (i, row) in chunk.iter().enumerate() {
            out.push('(');
            for (j, col) in columns.iter().enumerate() {
                let value = row.get(col).unwrap_or(&QueryCellValue::Null);
                out.push_str(&format_cell(value, db_kind));
                if j < columns.len() - 1 {
                    out.push_str(", ");
                }
            }
            out.push(')');
            if i < chunk.len() - 1 {
                out.push(',');
            }
            out.push('\n');
            row_count += 1;
        }
        out.push_str(";\n\n");
    }

    out.push_str(&format!("-- Rows inserted: {row_count}\n"));
    out
}
