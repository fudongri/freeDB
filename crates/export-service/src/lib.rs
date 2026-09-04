use anyhow::Result;
use core_domain::{MongoValue, QueryCellValue, QueryResult};
use csv::QuoteStyle;
use rust_xlsxwriter::Workbook;
use std::path::Path;

pub mod sql_dump;

#[derive(Clone, Default)]
pub struct ExportService;

impl ExportService {
    pub fn export_query_result_csv(&self, result: &QueryResult, path: impl AsRef<Path>) -> Result<()> {
        let mut writer = csv::WriterBuilder::new()
            .quote_style(QuoteStyle::Always)
            .from_path(path)?;
        writer.write_record(&result.columns)?;
        for row in &result.rows {
            let record = result
                .columns
                .iter()
                .map(|column| {
                    row.get(column)
                        .map(|value| {
                            if value.is_null() {
                                String::new()
                            } else {
                                value.display_text().to_string()
                            }
                        })
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            writer.write_record(record)?;
        }
        writer.flush()?;
        Ok(())
    }

    pub fn export_query_result_xlsx(&self, result: &QueryResult, path: impl AsRef<Path>) -> Result<()> {
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        // 写表头
        for (col, name) in result.columns.iter().enumerate() {
            worksheet.write_string(0, col as u16, name)?;
        }
        // 写数据
        for (row_idx, row) in result.rows.iter().enumerate() {
            let r = (row_idx + 1) as u32;
            for (col, col_name) in result.columns.iter().enumerate() {
                match row.get(col_name) {
                    Some(val) => {
                        let text = val.display_text();
                        // 尝试解析数字；长整型无法无损写入 f64 时回退为文本，保留精度
                        if val.is_null() {
                            worksheet.write_string(r, col as u16, "")?;
                        } else if let Some(n) = xlsx_lossless_number(text) {
                            worksheet.write_number(r, col as u16, n)?;
                        } else {
                            worksheet.write_string(r, col as u16, text)?;
                        }
                    }
                    None => { worksheet.write_string(r, col as u16, "")?; }
                }
            }
        }
        workbook.save(path)?;
        Ok(())
    }

    pub fn export_query_result_sql(&self, result: &QueryResult, table_name: &str, path: impl AsRef<Path>) -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "-- Exported table: {table_name}")?;
        writeln!(file, "-- Rows: {}", result.rows.len())?;
        writeln!(file)?;
        let col_list = result.columns.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ");
        for row in &result.rows {
            let values = result.columns.iter().map(|c| {
                match row.get(c) {
                    Some(val) if !val.is_null() => {
                        let text = val.display_text();
                        // 数字直接输出，字符串转义
                        if text.parse::<f64>().is_ok() {
                            text.to_string()
                        } else {
                            format!("'{}'", text.replace('\'', "''"))
                        }
                    }
                    _ => "NULL".to_string(),
                }
            }).collect::<Vec<_>>().join(", ");
            writeln!(file, "INSERT INTO `{table_name}` ({col_list}) VALUES ({values});")?;
        }
        Ok(())
    }

    pub fn export_query_result_mongo(&self, result: &QueryResult, collection: &str, path: impl AsRef<Path>) -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "// Collection: {collection}")?;
        writeln!(file, "// Rows: {}", result.rows.len())?;
        writeln!(file)?;
        writeln!(file, "db.{collection}.insertMany([")?;
        let mongo_types = &result.mongo_types;
        for (i, row) in result.rows.iter().enumerate() {
            writeln!(file, "  {{")?;
            for (j, col) in result.columns.iter().enumerate() {
                let mt = mongo_types.get(&(i, col.clone())).copied();
                let value = match row.get(col) {
                    Some(val) if !val.is_null() => mongo_export_literal(val, mt),
                    _ => "null".to_string(),
                };
                let comma = if j < result.columns.len() - 1 { "," } else { "" };
                writeln!(file, "    \"{}\": {}{}", col, value, comma)?;
            }
            let comma = if i < result.rows.len() - 1 { "," } else { "" };
            writeln!(file, "  }}{}", comma)?;
        }
        writeln!(file, "])")?;
        Ok(())
    }
}

fn mongo_export_literal(value: &QueryCellValue, mongo_type: Option<MongoValue>) -> String {
    match value {
        QueryCellValue::Null => "null".into(),
        QueryCellValue::Text(text) => {
            if text.is_empty() {
                "\"\"".into()
            } else {
                match mongo_type {
                    Some(MongoValue::ObjectId) => format!("ObjectId(\"{}\")", text),
                    Some(MongoValue::DateTime) => format!("ISODate(\"{}\")", text),
                    Some(MongoValue::Timestamp) => text.clone(),
                    Some(MongoValue::Decimal128) => format!("NumberDecimal(\"{}\")", text),
                    Some(MongoValue::Binary) => format!("HexData(\"{}\")", text),
                    Some(MongoValue::RegularExpression) => text.clone(),
                    None => {
                        if text.parse::<f64>().is_ok() || text == "true" || text == "false" {
                            text.to_string()
                        } else {
                            format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
                        }
                    }
                }
            }
        }
    }
}

/// 解析为可无损写入 Excel 数字单元格的 f64；无法无损表示时返回 None，
/// 由调用方回退为文本原样写入以保留精度。
///
/// Excel 数字单元格本质是 f64（53 位尾数），超出 2^53 的长整型（如 BIGINT /
/// MongoDB Int64 的 19 位值）直接解析为 f64 会丢失低位精度，
/// 因此仅在整数经 f64 往返后可还原时才按数字写入。
fn xlsx_lossless_number(text: &str) -> Option<f64> {
    // 含小数点或指数的按数字写入（非长整型场景，如小数、科学计数法）
    if text.contains(['.', 'e', 'E']) {
        return text.parse().ok();
    }
    let number: f64 = text.parse().ok()?;
    text.parse::<i128>()
        .map(|integer| integer == number as i128)
        .unwrap_or(false)
        .then_some(number)
}

#[cfg(test)]
mod tests {
    use super::xlsx_lossless_number;

    #[test]
    fn xlsx_long_integer_preserves_precision() {
        // 2^53 + 1：超出 f64 可无损表示范围，必须回退为文本
        assert_eq!(xlsx_lossless_number("9007199254740993"), None);
        // i64::MAX 同理
        assert_eq!(xlsx_lossless_number("9223372036854775807"), None);
        // MongoDB 典型 ObjectId 时间戳/自增 long 也超出范围
        assert_eq!(xlsx_lossless_number("92233720368547758070"), None);
    }

    #[test]
    fn xlsx_small_integer_and_decimal_stay_numeric() {
        assert_eq!(xlsx_lossless_number("42"), Some(42.0));
        assert_eq!(xlsx_lossless_number("-123"), Some(-123.0));
        assert_eq!(xlsx_lossless_number("0"), Some(0.0));
        // 2^53 本身可精确表示
        assert_eq!(xlsx_lossless_number("9007199254740992"), Some(9007199254740992.0));
        // 小数 / 科学计数法维持数字写入
        assert_eq!(xlsx_lossless_number("3.14"), Some(3.14));
        assert_eq!(xlsx_lossless_number("1e10"), Some(1e10));
        // 非数字
        assert_eq!(xlsx_lossless_number("abc"), None);
        assert_eq!(xlsx_lossless_number(""), None);
    }
}
