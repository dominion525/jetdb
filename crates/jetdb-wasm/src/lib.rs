//! JavaScript bindings of jetdb, built as WebAssembly for the `jetdb-wasm`
//! npm package.
//!
//! [`Database`] holds a database opened from bytes in memory, and each of its
//! methods reads one thing from it the way the matching `jetdb` CLI command
//! does.

mod js;

use std::io::Cursor;

use jetdb::format::{column_flags, index_flags, index_type, ColumnType, ObjectType};
use jetdb::{
    calculated_column_types, read_catalog, read_table_def, read_table_rows, timestamp, ColumnDef,
    FileError, IndexColumnOrder, PageReader, TableDef, Value,
};

/// A database opened from bytes in memory.
pub struct Database {
    reader: PageReader,
}

/// A column of a table, as `Database::columns` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    /// The type name, such as `Long` or `Text` (see [`type_name`]). For a
    /// calculated column, the type of its result, which its values have.
    pub type_name: String,
    /// The size in bytes as stored, such as 100 for a Text column of 50
    /// characters in Jet4 and later, which store two bytes a character.
    pub size: u16,
    /// Precision of a Numeric column; 0 for the other types and for calculated
    /// columns, whose values each carry their own scale.
    pub precision: u8,
    /// Scale of a Numeric column; 0 for the other types and for calculated
    /// columns.
    pub scale: u8,
    /// An AutoNumber column: a Long or a GUID that Access fills in.
    pub auto_number: bool,
    /// A calculated column (Access 2010 and later).
    pub calculated: bool,
}

/// An index of a table, as `Database::indexes` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    pub name: String,
    pub primary_key: bool,
    pub columns: Vec<IndexColumn>,
    /// No two rows have the same values in the index columns.
    pub unique: bool,
    /// Rows whose index columns are all NULL are left out of the index.
    pub ignore_nulls: bool,
    /// The index columns cannot be NULL.
    pub required: bool,
}

/// A column of an index.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexColumn {
    pub name: String,
    pub descending: bool,
}

/// The rows of a table, as `Database::rows` returns them.
#[derive(Debug, Clone, PartialEq)]
pub struct Rows {
    /// The names of the columns, in the order of the values in each row.
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    /// The number of rows that could not be read and were left out.
    pub skipped: usize,
}

/// A value of a row, in the JavaScript type it becomes.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    Number(f64),
    BigInt(i64),
    String(String),
    Bytes(Vec<u8>),
}

impl From<Value> for Cell {
    /// Money, Numeric and DateTimeExtended stay strings so that no digits are
    /// lost, and Timestamp becomes the string `jetdb export` writes by
    /// default.
    fn from(value: Value) -> Self {
        match value {
            Value::Null => Cell::Null,
            Value::Bool(v) => Cell::Bool(v),
            number @ (Value::Byte(_)
            | Value::Int(_)
            | Value::Long(_)
            | Value::Float(_)
            | Value::Double(_)) => Cell::Number(number_value(&number)),
            Value::BigInt(v) => Cell::BigInt(v),
            Value::Timestamp(ts) => Cell::String(timestamp_string(ts)),
            Value::Text(s)
            | Value::Money(s)
            | Value::Numeric(s)
            | Value::Guid(s)
            | Value::DateTimeExtended(s) => Cell::String(s),
            Value::Binary(bytes) => Cell::Bytes(bytes),
        }
    }
}

/// The number of a Byte, Int, Long, Float or Double value, which is all that
/// `Cell::from` passes here.
fn number_value(value: &Value) -> f64 {
    match *value {
        Value::Byte(v) => v.into(),
        Value::Int(v) => v.into(),
        Value::Long(v) => v.into(),
        // Through the shortest decimal of the f32, so that 1.1 stays 1.1
        // rather than becoming 1.100000023841858.
        Value::Float(v) => v.to_string().parse().unwrap_or(v.into()),
        Value::Double(v) => v,
        _ => unreachable!("not a number value: {value:?}"),
    }
}

/// A Timestamp as `jetdb export` writes it by default: the date alone when
/// the time is midnight, and the date and time otherwise.
fn timestamp_string(ts: f64) -> String {
    let format = if timestamp::is_date_only(ts) {
        "%Y-%m-%d"
    } else {
        "%Y-%m-%d %H:%M:%S"
    };
    timestamp::format_timestamp(ts, format)
}

impl Database {
    /// Open a database from its bytes, with the password for a
    /// password-protected `.accdb`.
    pub fn open(bytes: Vec<u8>, password: Option<&str>) -> Result<Self, FileError> {
        let reader = PageReader::open_reader_with_password(Cursor::new(bytes), password)?;
        Ok(Self { reader })
    }

    /// The database engine version, as the `jetdb ver` command prints it
    /// (`JET3`, `JET4`, `ACE12`, and so on).
    pub fn version(&self) -> &'static str {
        self.reader.header().version.short_name()
    }

    /// The table names, sorted, as the `jetdb tables` command lists them:
    /// user tables, and with `include_system` also system and hidden tables.
    pub fn tables(&mut self, include_system: bool) -> Result<Vec<String>, FileError> {
        let mut names: Vec<String> = read_catalog(&mut self.reader)?
            .into_iter()
            .filter(|e| {
                e.object_type == ObjectType::Table && (include_system || !e.is_system_or_hidden())
            })
            .map(|e| e.name)
            .collect();
        names.sort_unstable();
        Ok(names)
    }

    /// The columns of `table` in the order Access shows them, without the
    /// columns Access maintains and hides, as `jetdb export` leaves them out.
    pub fn columns(&mut self, table: &str) -> Result<Vec<Column>, FileError> {
        let (tdef, system_table) = self.table_def(table)?;
        let calculated = calculated_column_types(&mut self.reader, &tdef);
        Ok(tdef
            .columns
            .iter()
            .filter(|c| is_shown_column(c, system_table))
            .map(|c| {
                // The declared type of a calculated column is a placeholder;
                // its values have the type of its result, as rows reads them.
                let col_type = calculated
                    .get(&c.name.to_ascii_lowercase())
                    .unwrap_or(&c.col_type);
                let fixed_numeric = *col_type == ColumnType::Numeric && !c.is_calculated;
                Column {
                    name: c.name.clone(),
                    type_name: type_name(col_type),
                    size: c.col_size,
                    precision: if fixed_numeric { c.precision } else { 0 },
                    scale: if fixed_numeric { c.scale } else { 0 },
                    auto_number: c.flags & (column_flags::AUTO_LONG | column_flags::AUTO_UUID) != 0,
                    calculated: c.is_calculated,
                }
            })
            .collect())
    }

    /// The indexes of `table`, as `jetdb schema` lists them: without the
    /// foreign key references, which Access keeps as indexes of their own.
    pub fn indexes(&mut self, table: &str) -> Result<Vec<Index>, FileError> {
        let (tdef, _) = self.table_def(table)?;
        let column_name = |col_num: u16| {
            tdef.columns
                .iter()
                .find(|c| c.col_num == col_num)
                .map_or("?", |c| c.name.as_str())
                .to_string()
        };
        Ok(tdef
            .indexes
            .iter()
            .filter(|i| i.index_type != index_type::FOREIGN_KEY)
            .map(|i| Index {
                name: i.name.clone(),
                primary_key: i.index_type == index_type::PRIMARY,
                columns: i
                    .columns
                    .iter()
                    .map(|c| IndexColumn {
                        name: column_name(c.col_num),
                        descending: matches!(c.order, IndexColumnOrder::Descending),
                    })
                    .collect(),
                unique: i.flags & index_flags::UNIQUE != 0,
                ignore_nulls: i.flags & index_flags::IGNORE_NULLS != 0,
                required: i.flags & index_flags::REQUIRED != 0,
            })
            .collect())
    }

    /// The rows of `table`, with the columns that [`Database::columns`]
    /// returns, as `jetdb export` reads them.
    pub fn rows(&mut self, table: &str) -> Result<Rows, FileError> {
        let (tdef, system_table) = self.table_def(table)?;
        let shown: Vec<usize> = (0..tdef.columns.len())
            .filter(|&i| is_shown_column(&tdef.columns[i], system_table))
            .collect();
        let result = read_table_rows(&mut self.reader, &tdef)?;
        Ok(Rows {
            columns: shown
                .iter()
                .map(|&i| tdef.columns[i].name.clone())
                .collect(),
            rows: result
                .rows
                .into_iter()
                .map(|mut row| {
                    shown
                        .iter()
                        .map(|&i| std::mem::replace(&mut row[i], Value::Null).into())
                        .collect()
                })
                .collect(),
            skipped: result.skipped_rows,
        })
    }

    /// The definition of `table`, and whether it is a system table.
    fn table_def(&mut self, table: &str) -> Result<(TableDef, bool), FileError> {
        let entry = read_catalog(&mut self.reader)?
            .into_iter()
            .find(|e| e.object_type == ObjectType::Table && e.name == table)
            .ok_or_else(|| FileError::TableNotFound {
                name: table.to_string(),
            })?;
        let tdef = read_table_def(&mut self.reader, &entry.name, entry.table_page)?;
        Ok((tdef, entry.is_system()))
    }
}

/// Whether a column is shown, as `jetdb export` decides without
/// `--system-columns`: columns flagged as maintained and hidden by Access are
/// left out, except in system tables, where every column has that flag.
fn is_shown_column(column: &ColumnDef, system_table: bool) -> bool {
    system_table || !jetdb::is_replication_column(column)
}

/// The name of a column type, as `jetdb schema` prints it but without the
/// size, and `Unknown` for a type jetdb does not know, which `jetdb schema`
/// prints with its code.
fn type_name(column_type: &ColumnType) -> String {
    match column_type {
        ColumnType::Unknown(_) => "Unknown".to_string(),
        known => known.to_string(),
    }
}

/// The stable code of an error, which JavaScript gets as the `code` of the
/// error it catches: one code for each thing a caller may do about it, so the
/// ways a file can be broken all share `INVALID_FILE`, their message telling
/// them apart.
pub fn error_code(error: &FileError) -> &'static str {
    match error {
        FileError::PasswordRequired => "PASSWORD_REQUIRED",
        FileError::InvalidPassword => "INVALID_PASSWORD",
        FileError::UnsupportedEncryption { .. } => "UNSUPPORTED_ENCRYPTION",
        not_found @ (FileError::TableNotFound { .. }
        | FileError::QueryNotFound { .. }
        | FileError::ModuleNotFound { .. }
        | FileError::FormNotFound { .. }
        | FileError::MacroNotFound { .. }) => not_found_code(not_found),
        FileError::Format(_)
        | FileError::FileTooSmall { .. }
        | FileError::PageOutOfRange { .. }
        | FileError::InvalidRow { .. }
        | FileError::InvalidUsageMap { .. }
        | FileError::InvalidTableDef { .. }
        | FileError::InvalidProperty { .. }
        | FileError::InvalidVbaProject { .. }
        | FileError::InvalidFormData { .. }
        | FileError::InvalidMacroData { .. } => "INVALID_FILE",
        FileError::Io(_) => "IO",
    }
}

/// The code of an error for an object that is not in the database, which is
/// all that `error_code` passes here.
fn not_found_code(error: &FileError) -> &'static str {
    match error {
        FileError::TableNotFound { .. } => "TABLE_NOT_FOUND",
        FileError::QueryNotFound { .. } => "QUERY_NOT_FOUND",
        FileError::ModuleNotFound { .. } => "MODULE_NOT_FOUND",
        FileError::FormNotFound { .. } => "FORM_NOT_FOUND",
        FileError::MacroNotFound { .. } => "MACRO_NOT_FOUND",
        _ => unreachable!("not a not-found error: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data(relative: &str) -> Option<Vec<u8>> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(relative);
        std::fs::read(path).ok()
    }

    macro_rules! skip_if_missing {
        ($path:expr) => {
            match test_data($path) {
                Some(bytes) => bytes,
                None => {
                    eprintln!("SKIP: test data not found: {}", $path);
                    return;
                }
            }
        };
    }

    #[test]
    fn version_of_each_format() {
        for (file, expected) in [
            ("V1997/testV1997.mdb", "JET3"),
            ("V2003/testV2003.mdb", "JET4"),
            ("V2007/testV2007.accdb", "ACE12"),
            ("V2010/testV2010.accdb", "ACE14"),
        ] {
            let bytes = skip_if_missing!(file);
            assert_eq!(
                Database::open(bytes, None).unwrap().version(),
                expected,
                "{file}"
            );
        }
    }

    #[test]
    fn tables_user_and_system() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let user = db.tables(false).unwrap();
        assert!(user.contains(&"Table1".to_string()), "{user:?}");
        assert!(user.iter().all(|n| !n.starts_with("MSys")), "{user:?}");
        assert!(user.windows(2).all(|w| w[0] <= w[1]), "sorted: {user:?}");
        let all = db.tables(true).unwrap();
        assert!(all.contains(&"MSysObjects".to_string()), "{all:?}");
        assert!(all.len() > user.len());
    }

    #[test]
    fn password_protected() {
        let bytes = skip_if_missing!("db2007-enc.accdb");
        assert!(matches!(
            Database::open(bytes.clone(), None),
            Err(FileError::PasswordRequired)
        ));
        assert!(matches!(
            Database::open(bytes.clone(), Some("wrong")),
            Err(FileError::InvalidPassword)
        ));
        let mut db = Database::open(bytes, Some("Test123")).unwrap();
        assert_eq!(db.tables(false).unwrap(), ["Table1"]);
    }

    #[test]
    fn columns_names_types_and_sizes() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let columns = db.columns("Table1").unwrap();
        let summary: Vec<(&str, &str, u16)> = columns
            .iter()
            .map(|c| (c.name.as_str(), c.type_name.as_str(), c.size))
            .collect();
        assert_eq!(
            summary,
            [
                ("A", "Text", 100),
                ("B", "Text", 200),
                ("C", "Byte", 1),
                ("D", "Int", 2),
                ("E", "Long", 4),
                ("F", "Double", 8),
                ("G", "Timestamp", 8),
                ("H", "Money", 8),
                ("I", "Boolean", 1),
            ]
        );
        assert!(columns
            .iter()
            .all(|c| !c.auto_number && !c.calculated && c.precision == 0 && c.scale == 0));
    }

    #[test]
    fn type_names_match_the_typescript_column_type() {
        // These names are the ColumnType union declared in js.rs.
        let types = [
            ColumnType::Boolean,
            ColumnType::Byte,
            ColumnType::Int,
            ColumnType::Long,
            ColumnType::Money,
            ColumnType::Float,
            ColumnType::Double,
            ColumnType::Timestamp,
            ColumnType::Binary,
            ColumnType::Text,
            ColumnType::Ole,
            ColumnType::Memo,
            ColumnType::Guid,
            ColumnType::Numeric,
            ColumnType::ComplexType,
            ColumnType::BigInt,
            ColumnType::DateTimeExtended,
            ColumnType::Unknown(0x99),
        ];
        let names: Vec<String> = types.iter().map(type_name).collect();
        assert_eq!(
            names,
            [
                "Boolean",
                "Byte",
                "Int",
                "Long",
                "Money",
                "Float",
                "Double",
                "Timestamp",
                "Binary",
                "Text",
                "Ole",
                "Memo",
                "Guid",
                "Numeric",
                "ComplexType",
                "BigInt",
                "DateTimeExtended",
                "Unknown",
            ]
        );
    }

    #[test]
    fn columns_auto_number_numeric_and_calculated() {
        let bytes = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let columns = db.columns("Table1").unwrap();
        let column = |name: &str| columns.iter().find(|c| c.name == name).unwrap();
        assert_eq!(column("ID").type_name, "Long");
        assert!(column("ID").auto_number);
        assert!(!column("FirstName").auto_number);
        let popularity = column("Popularity");
        assert_eq!(
            (
                popularity.type_name.as_str(),
                popularity.precision,
                popularity.scale
            ),
            ("Numeric", 18, 6)
        );
        assert!(!column("FirstName").calculated);
        assert!(column("LastFirst").calculated);
    }

    #[test]
    fn columns_calculated_have_the_type_of_their_values() {
        // Access declares most numeric results as Double; rows reads the
        // values as the result type, and columns says the same.
        let bytes = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let columns = db.columns("Table1").unwrap();
        let summary = |name: &str| {
            let c = columns.iter().find(|c| c.name == name).unwrap();
            (c.type_name.as_str(), c.precision, c.scale)
        };
        assert_eq!(summary("MonthlySalary"), ("Money", 0, 0));
        assert_eq!(summary("IsRich"), ("Boolean", 0, 0));
        assert_eq!(summary("FloatTest"), ("Float", 0, 0));
        assert_eq!(summary("DecimalTest"), ("Numeric", 0, 0));
        assert_eq!(summary("LastFirstLen"), ("Long", 0, 0));
        let rows = db.rows("Table1").unwrap();
        let first =
            |name: &str| rows.rows[0][rows.columns.iter().position(|c| c == name).unwrap()].clone();
        assert_eq!(first("MonthlySalary"), string("83333.3333"));
        assert_eq!(first("IsRich"), Cell::Bool(true));
    }

    #[test]
    fn columns_of_a_system_table() {
        // Every column of a system table is flagged as maintained and hidden
        // by Access, and all of them are kept.
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let names: Vec<String> = db
            .columns("MSysObjects")
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert!(names.contains(&"Name".to_string()), "{names:?}");
        assert!(names.contains(&"Type".to_string()), "{names:?}");
    }

    #[test]
    fn columns_of_a_missing_table() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert!(matches!(
            db.columns("NoSuchTable"),
            Err(FileError::TableNotFound { name }) if name == "NoSuchTable"
        ));
    }

    fn index_column(name: &str, descending: bool) -> IndexColumn {
        IndexColumn {
            name: name.to_string(),
            descending,
        }
    }

    #[test]
    fn indexes_with_their_columns_and_flags() {
        let bytes = skip_if_missing!("V2003/testIndexPropertiesV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(
            db.indexes("TableIgnoreNulls2").unwrap(),
            [
                Index {
                    name: "DataIndex".to_string(),
                    primary_key: false,
                    columns: vec![index_column("data1", false), index_column("data2", false)],
                    unique: false,
                    ignore_nulls: true,
                    required: false,
                },
                Index {
                    name: "PrimaryKey".to_string(),
                    primary_key: true,
                    columns: vec![index_column("row", false)],
                    unique: true,
                    ignore_nulls: false,
                    required: true,
                },
            ]
        );
        let unique = &db.indexes("TableUnique2_temp").unwrap()[0];
        assert_eq!(unique.name, "DataIndex");
        assert!(unique.unique && !unique.primary_key && !unique.ignore_nulls);
    }

    #[test]
    fn indexes_descending_column() {
        let bytes = skip_if_missing!("V2003/compIndexTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let indexes = db.indexes("Table1").unwrap();
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].columns, [index_column("CD_AGENTE", true)]);
    }

    #[test]
    fn indexes_leave_out_foreign_key_references() {
        let bytes = skip_if_missing!("V2003/indexTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let (tdef, _) = db.table_def("Table1").unwrap();
        assert!(
            tdef.indexes
                .iter()
                .any(|i| i.index_type == index_type::FOREIGN_KEY),
            "Table1 has foreign key references to leave out"
        );
        let names: Vec<String> = db
            .indexes("Table1")
            .unwrap()
            .into_iter()
            .map(|i| i.name)
            .collect();
        assert_eq!(names, ["id", "PrimaryKey"]);
    }

    #[test]
    fn indexes_of_a_missing_table() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert!(matches!(
            db.indexes("NoSuchTable"),
            Err(FileError::TableNotFound { name }) if name == "NoSuchTable"
        ));
    }

    fn string(s: &str) -> Cell {
        Cell::String(s.to_string())
    }

    /// The value in `column` of the row whose first value is `key`.
    fn cell(rows: &Rows, key: Cell, column: &str) -> Cell {
        let i = rows.columns.iter().position(|c| c == column).unwrap();
        let row = rows.rows.iter().find(|r| r[0] == key).unwrap();
        row[i].clone()
    }

    #[test]
    fn rows_text_numbers_money_timestamp_and_boolean() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db.rows("Table1").unwrap();
        assert_eq!(rows.columns, ["A", "B", "C", "D", "E", "F", "G", "H", "I"]);
        assert_eq!(
            rows.rows,
            [
                vec![
                    string("abcdefg"),
                    string("hijklmnop"),
                    Cell::Number(2.0),
                    Cell::Number(222.0),
                    Cell::Number(333333333.0),
                    Cell::Number(444.555),
                    string("1974-09-21"),
                    string("3.5000"),
                    Cell::Bool(true),
                ],
                vec![
                    string("a"),
                    string("b"),
                    Cell::Number(0.0),
                    Cell::Number(0.0),
                    Cell::Number(0.0),
                    Cell::Number(0.0),
                    string("1981-12-12"),
                    string("0.0000"),
                    Cell::Bool(false),
                ],
            ]
        );
        assert_eq!(rows.skipped, 0);
    }

    #[test]
    fn rows_single_timestamp_numeric_and_guid() {
        let bytes = skip_if_missing!("V2003/testIndexCodesV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let single = db.rows("Table5").unwrap();
        assert_eq!(cell(&single, string("row1"), "data"), Cell::Number(3245.0));
        assert_eq!(
            cell(&single, string("row10"), "data"),
            Cell::Number(-0.00035134)
        );
        assert_eq!(
            cell(&single, string("row11"), "data"),
            Cell::Number(804983.4)
        );
        assert_eq!(cell(&single, string("row5"), "data"), Cell::Null);
        let timestamp = db.rows("Table6").unwrap();
        assert_eq!(
            cell(&timestamp, string("row0"), "data"),
            string("1899-12-30")
        );
        // -0.00035134 days, which Access shows as 30 seconds past midnight
        // of 1899-12-30.
        assert_eq!(
            cell(&timestamp, string("row10"), "data"),
            string("1899-12-30 00:00:30")
        );
        let numeric = db.rows("Table7").unwrap();
        assert_eq!(
            cell(&numeric, string("row11"), "data"),
            string("804983.3458740000")
        );
        let guid = db.rows("Table13").unwrap();
        assert_eq!(
            cell(&guid, string("row0"), "data"),
            string("{BC96303A-53B8-474D-ACC1-D2EB8D0B09D5}")
        );
    }

    #[test]
    fn rows_binary() {
        let bytes = skip_if_missing!("V2010/binIdxTestV2010.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db.rows("Test").unwrap();
        assert_eq!(
            cell(&rows, Cell::Number(1.0), "BinAsc"),
            Cell::Bytes(b"ab".to_vec())
        );
        assert_eq!(cell(&rows, Cell::Number(200.0), "BinAsc"), Cell::Null);
    }

    #[test]
    fn rows_date_time_extended() {
        let bytes = skip_if_missing!("V2019/extDateTestV2019.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db.rows("Table1").unwrap();
        assert_eq!(
            cell(&rows, Cell::Number(6.0), "DateExt"),
            string("2021-06-14 22:45:12.3456789")
        );
        assert_eq!(
            cell(&rows, Cell::Number(6.0), "DateNormal"),
            string("2021-06-14 22:45:12")
        );
    }

    #[test]
    fn rows_bigint() {
        let bytes = skip_if_missing!("V2016/bigIntTestV2016.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db.rows("BigIntTable").unwrap();
        assert_eq!(
            rows.rows,
            [
                vec![Cell::Number(1.0), Cell::BigInt(9007199254740993)],
                vec![Cell::Number(2.0), Cell::BigInt(-9007199254740993)],
                vec![Cell::Number(3.0), Cell::BigInt(0)],
                vec![Cell::Number(4.0), Cell::Null],
            ]
        );
    }

    #[test]
    fn rows_left_out_when_unreadable() {
        // Table1's two rows are on page 27 of the 4096-byte pages. Giving the
        // second row the offset of the first leaves it no bytes, so it cannot
        // be read.
        let mut bytes = skip_if_missing!("V2003/testV2003.mdb");
        let pos = 27 * 4096 + 16;
        assert_eq!(u16::from_le_bytes([bytes[pos], bytes[pos + 1]]), 0x0F89);
        bytes[pos..pos + 2].copy_from_slice(&0x0FB8u16.to_le_bytes());
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db.rows("Table1").unwrap();
        assert_eq!(rows.skipped, 1);
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0][0], string("abcdefg"));
    }

    #[test]
    fn rows_of_a_missing_table() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert!(matches!(
            db.rows("NoSuchTable"),
            Err(FileError::TableNotFound { name }) if name == "NoSuchTable"
        ));
    }

    #[test]
    fn cells_of_values_without_test_data() {
        // Values no test database holds: the largest BigInt, and a Float
        // that is not finite.
        assert_eq!(Cell::from(Value::BigInt(i64::MAX)), Cell::BigInt(i64::MAX));
        assert_eq!(Cell::from(Value::Float(1.1)), Cell::Number(1.1));
        assert!(matches!(Cell::from(Value::Float(f32::NAN)), Cell::Number(v) if v.is_nan()));
        assert_eq!(
            Cell::from(Value::Float(f32::INFINITY)),
            Cell::Number(f64::INFINITY)
        );
    }

    #[test]
    fn error_codes_of_each_kind() {
        let name = || "x".to_string();
        let codes = [
            (FileError::PasswordRequired, "PASSWORD_REQUIRED"),
            (FileError::InvalidPassword, "INVALID_PASSWORD"),
            (
                FileError::UnsupportedEncryption { reason: name() },
                "UNSUPPORTED_ENCRYPTION",
            ),
            (FileError::TableNotFound { name: name() }, "TABLE_NOT_FOUND"),
            (FileError::QueryNotFound { name: name() }, "QUERY_NOT_FOUND"),
            (
                FileError::ModuleNotFound { name: name() },
                "MODULE_NOT_FOUND",
            ),
            (FileError::FormNotFound { name: name() }, "FORM_NOT_FOUND"),
            (FileError::MacroNotFound { name: name() }, "MACRO_NOT_FOUND"),
            (
                FileError::FileTooSmall {
                    expected: 21,
                    actual: 0,
                },
                "INVALID_FILE",
            ),
            (FileError::InvalidTableDef { reason: "x" }, "INVALID_FILE"),
            (FileError::Io(std::io::Error::other("x")), "IO"),
        ];
        for (error, code) in codes {
            assert_eq!(error_code(&error), code, "{error}");
        }
    }

    #[test]
    fn error_codes_from_real_files() {
        let bytes = skip_if_missing!("db2007-enc.accdb");
        let code = |r: Result<Database, FileError>| error_code(&r.err().unwrap());
        assert_eq!(
            code(Database::open(bytes.clone(), None)),
            "PASSWORD_REQUIRED"
        );
        assert_eq!(
            code(Database::open(bytes, Some("wrong"))),
            "INVALID_PASSWORD"
        );
        assert_eq!(code(Database::open(vec![0; 10], None)), "INVALID_FILE");
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(
            error_code(&db.columns("NoSuchTable").unwrap_err()),
            "TABLE_NOT_FOUND"
        );
    }

    #[test]
    fn hidden_columns_are_shown_only_in_system_tables() {
        let column = |flags| ColumnDef {
            name: "c".to_string(),
            col_type: ColumnType::Long,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 4,
            flags,
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
        };
        let hidden = column(column_flags::REPLICATION);
        assert!(!is_shown_column(&hidden, false));
        assert!(is_shown_column(&hidden, true));
        assert!(is_shown_column(&column(0), false));
    }
}
