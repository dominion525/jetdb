use crate::format::ColumnType;
use crate::table::ColumnDef;

use super::DdlDialect;

pub struct Access;

impl DdlDialect for Access {
    fn quote_id(&self, name: &str) -> String {
        format!("[{}]", name.replace(']', "]]"))
    }

    fn map_column_type(&self, col: &ColumnDef, is_auto: bool) -> String {
        if is_auto {
            return "COUNTER NOT NULL".to_string();
        }
        match col.col_type {
            ColumnType::Boolean => "YESNO".to_string(),
            ColumnType::Byte => "BYTE".to_string(),
            ColumnType::Int => "SHORT".to_string(),
            ColumnType::Long => "LONG".to_string(),
            ColumnType::Money => "CURRENCY".to_string(),
            ColumnType::Float => "SINGLE".to_string(),
            ColumnType::Double => "DOUBLE".to_string(),
            ColumnType::Timestamp => "DATETIME".to_string(),
            ColumnType::Binary => format!("BINARY({})", col.col_size),
            ColumnType::Text => format!("TEXT({})", col.col_size),
            ColumnType::Memo => "MEMO".to_string(),
            ColumnType::Ole => "OLEOBJECT".to_string(),
            ColumnType::Guid => "UNIQUEIDENTIFIER".to_string(),
            // No fixed precision (a calculated column): the precision of Access's Decimal is at most 28.
            ColumnType::Numeric if col.precision == 0 => "DECIMAL(28,10)".to_string(),
            ColumnType::Numeric => format!("DECIMAL({},{})", col.precision, col.scale),
            ColumnType::ComplexType => "LONG".to_string(),
            ColumnType::BigInt => "BIGINT".to_string(),
            ColumnType::DateTimeExtended => "DATETIME".to_string(),
            ColumnType::Unknown(_) => "BINARY".to_string(),
        }
    }

    fn auto_increment_absorbs_pk(&self) -> bool {
        false
    }

    fn inline_foreign_keys(&self) -> bool {
        false
    }
}
