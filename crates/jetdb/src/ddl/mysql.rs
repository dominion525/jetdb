use crate::format::ColumnType;
use crate::table::ColumnDef;

use super::DdlDialect;

pub struct Mysql;

impl DdlDialect for Mysql {
    fn quote_id(&self, name: &str) -> String {
        format!("`{}`", name.replace('`', "``"))
    }

    fn map_column_type(&self, col: &ColumnDef, is_auto: bool) -> String {
        if is_auto && col.col_type == ColumnType::Guid {
            // A new GUID in the braced form jetdb exports. An expression
            // default needs MySQL 8.0.13 or later.
            return "CHAR(38) NOT NULL DEFAULT (CONCAT('{', UPPER(UUID()), '}'))".to_string();
        }
        if is_auto {
            return "INT NOT NULL AUTO_INCREMENT".to_string();
        }
        match col.col_type {
            ColumnType::Boolean => "BOOLEAN".to_string(),
            ColumnType::Byte => "TINYINT UNSIGNED".to_string(),
            ColumnType::Int => "SMALLINT".to_string(),
            ColumnType::Long => "INT".to_string(),
            ColumnType::Money => "DECIMAL(19,4)".to_string(),
            ColumnType::Float => "FLOAT".to_string(),
            ColumnType::Double => "DOUBLE".to_string(),
            ColumnType::Timestamp => "DATETIME".to_string(),
            ColumnType::Binary => format!("VARBINARY({})", col.col_size),
            ColumnType::Text => format!("VARCHAR({})", col.col_size),
            ColumnType::Memo => "LONGTEXT".to_string(),
            ColumnType::Ole => "LONGBLOB".to_string(),
            // Braces included, as jetdb reads and exports a GUID.
            ColumnType::Guid => "CHAR(38)".to_string(),
            // No fixed precision (a calculated column): the largest DECIMAL MySQL allows.
            ColumnType::Numeric if col.precision == 0 => "DECIMAL(65,30)".to_string(),
            ColumnType::Numeric => format!("DECIMAL({},{})", col.precision, col.scale),
            ColumnType::ComplexType => "INT".to_string(),
            ColumnType::BigInt => "BIGINT".to_string(),
            ColumnType::DateTimeExtended => "DATETIME(6)".to_string(),
            ColumnType::Unknown(_) => "LONGBLOB".to_string(),
        }
    }

    fn auto_increment_absorbs_pk(&self) -> bool {
        false
    }

    fn inline_foreign_keys(&self) -> bool {
        false
    }

    fn index_names_per_table(&self) -> bool {
        true
    }
}
