use crate::format::ColumnType;
use crate::table::ColumnDef;

use super::DdlDialect;

pub struct Postgres;

impl DdlDialect for Postgres {
    fn quote_id(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }

    fn map_column_type(&self, col: &ColumnDef, is_auto: bool) -> String {
        if is_auto && col.col_type == ColumnType::Guid {
            // gen_random_uuid() is a core function from PostgreSQL 13.
            return "UUID NOT NULL DEFAULT gen_random_uuid()".to_string();
        }
        if is_auto {
            return "INTEGER NOT NULL GENERATED ALWAYS AS IDENTITY".to_string();
        }
        match col.col_type {
            ColumnType::Boolean => "BOOLEAN".to_string(),
            ColumnType::Byte => "SMALLINT".to_string(),
            ColumnType::Int => "SMALLINT".to_string(),
            ColumnType::Long => "INTEGER".to_string(),
            ColumnType::Money => "NUMERIC(19,4)".to_string(),
            ColumnType::Float => "REAL".to_string(),
            ColumnType::Double => "DOUBLE PRECISION".to_string(),
            ColumnType::Timestamp => "TIMESTAMP WITHOUT TIME ZONE".to_string(),
            ColumnType::Binary => "BYTEA".to_string(),
            ColumnType::Text => format!("VARCHAR({})", col.col_size),
            ColumnType::Memo => "TEXT".to_string(),
            ColumnType::Ole => "BYTEA".to_string(),
            ColumnType::Guid => "UUID".to_string(),
            // No fixed precision (a calculated column): NUMERIC without a precision holds any value exactly.
            ColumnType::Numeric if col.precision == 0 => "NUMERIC".to_string(),
            ColumnType::Numeric => format!("NUMERIC({},{})", col.precision, col.scale),
            ColumnType::ComplexType => "INTEGER".to_string(),
            ColumnType::BigInt => "BIGINT".to_string(),
            ColumnType::DateTimeExtended => "TIMESTAMP WITHOUT TIME ZONE".to_string(),
            ColumnType::Unknown(_) => "BYTEA".to_string(),
        }
    }

    fn auto_increment_absorbs_pk(&self) -> bool {
        false
    }

    fn inline_foreign_keys(&self) -> bool {
        false
    }

    fn index_names_per_table(&self) -> bool {
        false
    }
}
