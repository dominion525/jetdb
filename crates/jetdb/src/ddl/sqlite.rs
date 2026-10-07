use crate::format::ColumnType;
use crate::table::ColumnDef;

use super::DdlDialect;

pub struct Sqlite;

impl DdlDialect for Sqlite {
    fn quote_id(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }

    fn map_column_type(&self, col: &ColumnDef, is_auto: bool) -> String {
        if is_auto && col.col_type == ColumnType::Guid {
            // A new GUID of random hex digits, in the braced form jetdb
            // exports. SQLite has no function to make one.
            return "TEXT NOT NULL DEFAULT ('{' || upper(hex(randomblob(4))) || '-' \
                    || upper(hex(randomblob(2))) || '-' || upper(hex(randomblob(2))) || '-' \
                    || upper(hex(randomblob(2))) || '-' || upper(hex(randomblob(6))) || '}')"
                .to_string();
        }
        if is_auto {
            return "INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT".to_string();
        }
        match col.col_type {
            ColumnType::Boolean => "INTEGER".to_string(),
            ColumnType::Byte => "INTEGER".to_string(),
            ColumnType::Int => "INTEGER".to_string(),
            ColumnType::Long => "INTEGER".to_string(),
            ColumnType::Money => "NUMERIC".to_string(),
            ColumnType::Float => "REAL".to_string(),
            ColumnType::Double => "REAL".to_string(),
            ColumnType::Timestamp => "TEXT".to_string(),
            ColumnType::Binary => "BLOB".to_string(),
            ColumnType::Text => "TEXT".to_string(),
            ColumnType::Memo => "TEXT".to_string(),
            ColumnType::Ole => "BLOB".to_string(),
            ColumnType::Guid => "TEXT".to_string(),
            ColumnType::Numeric => "NUMERIC".to_string(),
            ColumnType::ComplexType => "INTEGER".to_string(),
            ColumnType::BigInt => "INTEGER".to_string(),
            ColumnType::DateTimeExtended => "TEXT".to_string(),
            ColumnType::Unknown(_) => "BLOB".to_string(),
        }
    }

    fn auto_increment_absorbs_pk(&self) -> bool {
        true
    }

    fn inline_foreign_keys(&self) -> bool {
        true
    }

    fn auto_increment_needs_key(&self) -> bool {
        false
    }

    fn index_names_per_table(&self) -> bool {
        false
    }
}
