//! JavaScript bindings of jetdb, built as WebAssembly for the `jetdb-wasm`
//! npm package.
//!
//! [`Database`] holds a database opened from bytes in memory, and each of its
//! methods reads one thing from it the way the matching `jetdb` CLI command
//! does.

mod js;

use std::io::Cursor;

use jetdb::format::{
    catalog_flags, column_flags, index_flags, index_type, ColumnType, JetVersion, ObjectType,
};
use jetdb::{
    read_catalog, read_table_def, ColumnDef, FileError, IndexColumnOrder, PageReader, TableDef,
};

/// A database opened from bytes in memory.
pub struct Database {
    reader: PageReader,
}

/// A column of a table, as `Database::columns` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    /// The type name, such as `Long` or `Text` (see [`type_name`]).
    pub type_name: &'static str,
    /// The size in bytes as stored, such as 100 for a Text column of 50
    /// characters in Jet4 and later, which store two bytes a character.
    pub size: u16,
    /// Precision of a Numeric column; 0 for the other types.
    pub precision: u8,
    /// Scale of a Numeric column; 0 for the other types.
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
        match self.reader.header().version {
            JetVersion::Jet3 => "JET3",
            JetVersion::Jet4 => "JET4",
            JetVersion::Ace12 => "ACE12",
            JetVersion::Ace14 => "ACE14",
            JetVersion::Ace15 => "ACE15",
            JetVersion::Ace16 => "ACE16",
            JetVersion::Ace17 => "ACE17",
        }
    }

    /// The table names, sorted, as the `jetdb tables` command lists them:
    /// user tables, and with `include_system` also system and hidden tables.
    pub fn tables(&mut self, include_system: bool) -> Result<Vec<String>, FileError> {
        let mut names: Vec<String> = read_catalog(&mut self.reader)?
            .into_iter()
            .filter(|e| {
                e.object_type == ObjectType::Table
                    && (include_system
                        || e.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN) == 0)
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
        Ok(tdef
            .columns
            .iter()
            .filter(|c| is_shown_column(c, system_table))
            .map(|c| Column {
                name: c.name.clone(),
                type_name: type_name(&c.col_type),
                size: c.col_size,
                precision: if c.col_type == ColumnType::Numeric {
                    c.precision
                } else {
                    0
                },
                scale: if c.col_type == ColumnType::Numeric {
                    c.scale
                } else {
                    0
                },
                auto_number: c.flags & (column_flags::AUTO_LONG | column_flags::AUTO_UUID) != 0,
                calculated: c.is_calculated,
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

    /// The definition of `table`, and whether it is a system table.
    fn table_def(&mut self, table: &str) -> Result<(TableDef, bool), FileError> {
        let entry = read_catalog(&mut self.reader)?
            .into_iter()
            .find(|e| e.object_type == ObjectType::Table && e.name == table)
            .ok_or_else(|| FileError::TableNotFound {
                name: table.to_string(),
            })?;
        let tdef = read_table_def(&mut self.reader, &entry.name, entry.table_page)?;
        Ok((tdef, entry.flags & catalog_flags::SYSTEM != 0))
    }
}

/// Whether a column is shown, as `jetdb export` decides without
/// `--system-columns`: columns flagged as maintained and hidden by Access are
/// left out, except in system tables, where every column has that flag.
fn is_shown_column(column: &ColumnDef, system_table: bool) -> bool {
    system_table || !jetdb::is_replication_column(column)
}

/// The name of a column type, as `jetdb schema` prints it but without the
/// size, and `Unknown` for a type jetdb does not know.
fn type_name(column_type: &ColumnType) -> &'static str {
    match column_type {
        ColumnType::Boolean => "Boolean",
        ColumnType::Byte => "Byte",
        ColumnType::Int => "Int",
        ColumnType::Long => "Long",
        ColumnType::Money => "Money",
        ColumnType::Float => "Float",
        ColumnType::Double => "Double",
        ColumnType::Timestamp => "Timestamp",
        ColumnType::Binary => "Binary",
        ColumnType::Text => "Text",
        ColumnType::Ole => "Ole",
        ColumnType::Memo => "Memo",
        ColumnType::Guid => "Guid",
        ColumnType::Numeric => "Numeric",
        ColumnType::ComplexType => "ComplexType",
        ColumnType::BigInt => "BigInt",
        ColumnType::DateTimeExtended => "DateTimeExtended",
        ColumnType::Unknown(_) => "Unknown",
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
            .map(|c| (c.name.as_str(), c.type_name, c.size))
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
            (popularity.type_name, popularity.precision, popularity.scale),
            ("Numeric", 18, 6)
        );
        assert!(!column("FirstName").calculated);
        assert!(column("LastFirst").calculated);
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
