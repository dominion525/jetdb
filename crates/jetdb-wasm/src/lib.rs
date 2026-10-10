//! JavaScript bindings of jetdb, built as WebAssembly for the `jetdb-wasm`
//! npm package.
//!
//! [`Database`] holds a database opened from bytes in memory, and each of its
//! methods reads one thing from it the way the matching `jetdb` CLI command
//! does.

mod js;

use std::io::Cursor;

use jetdb::ddl::{self, with_calculated_column_types, DdlDialect};
use jetdb::format::{index_flags, index_type, ColumnType, ObjectType};
use jetdb::{
    calculated_column_types_in, entry_properties, find_table, query_to_sql, read_catalog,
    read_queries_in, read_relationships_in, read_table_def, read_table_rows_with,
    relationship_flags, timestamp, CatalogEntry, FileError, IndexColumnOrder, PageReader,
    PropMapType, QueryDef, QueryType, TableDef, Value,
};

/// A database opened from bytes in memory.
pub struct Database {
    reader: PageReader,
    /// The catalog, read when it is first needed. The bytes never change, so
    /// it stays as read.
    catalog: Option<Vec<CatalogEntry>>,
    /// The definitions of the saved queries, read when they are first needed,
    /// as the catalog is.
    queries: Option<Vec<QueryDef>>,
}

/// A column of a table, as `Database::columns` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    /// The type name, such as `Long` or `Text` (see [`type_name`]). For a
    /// calculated column, the type of its result, which its values have.
    pub type_name: String,
    /// The size as Access shows it: in characters for a Text column, such as
    /// 255, and in bytes for the other types.
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

/// A relationship between two tables, as `Database::relationships` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Relationship {
    pub name: String,
    /// The table whose columns refer to the other table.
    pub from_table: String,
    /// The table referred to.
    pub to_table: String,
    pub columns: Vec<RelationshipColumn>,
    /// Access keeps the rows of the two tables consistent.
    pub referential_integrity: bool,
    pub cascade_update: bool,
    pub cascade_delete: bool,
}

/// A pair of columns of a relationship.
#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipColumn {
    /// The column of `from_table`.
    pub from: String,
    /// The column of `to_table` it refers to.
    pub to: String,
}

/// A saved query, as `Database::queries` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub name: String,
    /// The type name, such as `Select` or `MakeTable` (see [`query_type_name`]).
    pub type_name: &'static str,
}

/// The properties of an object, as `Database::properties` returns them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectProperties {
    /// The properties of the object itself.
    pub object: Vec<Property>,
    /// The properties of each column of a table.
    pub columns: Vec<PropertyGroup>,
    /// The additional groups of properties.
    pub additional: Vec<PropertyGroup>,
}

/// A named group of properties, such as those of a column.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyGroup {
    pub name: String,
    pub properties: Vec<Property>,
}

/// A property, with its value as the rows of a table have it.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub value: Cell,
}

/// Why `Database::properties` gave no properties.
#[derive(Debug)]
pub enum PropertiesError {
    File(FileError),
    /// No object of that name, or of that name and type.
    NotFound,
    /// Objects of these types share the name, and no type was given.
    SeveralTypes(Vec<ObjectType>),
}

impl From<FileError> for PropertiesError {
    fn from(error: FileError) -> Self {
        Self::File(error)
    }
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
            Value::Timestamp(ts) => Cell::String(timestamp::format_default(ts)),
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

impl Database {
    /// Open a database from its bytes, with the password for a
    /// password-protected `.accdb`.
    pub fn open(bytes: Vec<u8>, password: Option<&str>) -> Result<Self, FileError> {
        let reader = PageReader::open_reader_with_password(Cursor::new(bytes), password)?;
        Ok(Self {
            reader,
            catalog: None,
            queries: None,
        })
    }

    /// The database engine version, as the `jetdb ver` command prints it
    /// (`JET3`, `JET4`, `ACE12`, and so on).
    pub fn version(&self) -> &'static str {
        self.reader.header().version.short_name()
    }

    /// The table names, sorted, as the `jetdb tables` command lists them:
    /// user tables, and with `include_system` also system and hidden tables.
    pub fn tables(&mut self, include_system: bool) -> Result<Vec<String>, FileError> {
        let mut names: Vec<String> = self
            .catalog()?
            .iter()
            .filter(|e| {
                e.object_type == ObjectType::Table && (include_system || !e.is_system_or_hidden())
            })
            .map(|e| e.name.clone())
            .collect();
        names.sort_unstable();
        Ok(names)
    }

    /// The columns of `table` in the order Access shows them, without the
    /// columns Access maintains and hides, as `jetdb export` leaves them out.
    pub fn columns(&mut self, table: &str) -> Result<Vec<Column>, FileError> {
        let (tdef, system_table) = self.table_def(table)?;
        // The declared type of a calculated column is a placeholder; its
        // values have the type of its result, as rows reads them, and no
        // fixed precision or scale, as in the DDL.
        let calculated = calculated_column_types_in(self.catalog()?, &tdef);
        let tdef = with_calculated_column_types(&tdef, &calculated);
        Ok(tdef
            .columns
            .iter()
            .filter(|c| c.is_shown(system_table))
            .map(|c| {
                let numeric = c.col_type == ColumnType::Numeric;
                Column {
                    name: c.name.clone(),
                    type_name: type_name(&c.col_type),
                    size: tdef.shown_size(c),
                    precision: if numeric { c.precision } else { 0 },
                    scale: if numeric { c.scale } else { 0 },
                    auto_number: c.is_auto_number(),
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
            .filter(|&i| tdef.columns[i].is_shown(system_table))
            .collect();
        let (reader, catalog) = self.reader_and_catalog()?;
        let calculated = calculated_column_types_in(catalog, &tdef);
        let result = read_table_rows_with(reader, &tdef, &calculated)?;
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

    /// The relationships between tables, sorted by name: without those of
    /// system and hidden tables, as `tables` leaves them out, and with
    /// `include_system` all of them.
    pub fn relationships(&mut self, include_system: bool) -> Result<Vec<Relationship>, FileError> {
        let (reader, catalog) = self.reader_and_catalog()?;
        let relationships = read_relationships_in(reader, catalog)?;
        let system_or_hidden = |name: &str| is_system_or_hidden_table(catalog, name);
        let mut out: Vec<Relationship> = relationships
            .into_iter()
            .filter(|r| {
                include_system
                    || !(system_or_hidden(&r.from_table) || system_or_hidden(&r.to_table))
            })
            .map(|r| Relationship {
                referential_integrity: r.has_referential_integrity(),
                cascade_update: r.flags & relationship_flags::CASCADE_UPDATE != 0,
                cascade_delete: r.flags & relationship_flags::CASCADE_DELETE != 0,
                columns: r
                    .columns
                    .into_iter()
                    .map(|c| RelationshipColumn {
                        from: c.from_column,
                        to: c.to_column,
                    })
                    .collect(),
                name: r.name,
                from_table: r.from_table,
                to_table: r.to_table,
            })
            .collect();
        out.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The saved queries, sorted by name: without the system and hidden
    /// ones, such as those Access makes for forms and reports, as `tables`
    /// leaves them out, and with `include_system` all of them.
    pub fn queries(&mut self, include_system: bool) -> Result<Vec<Query>, FileError> {
        self.query_defs()?;
        let catalog = self
            .catalog
            .as_deref()
            .expect("the catalog was read for the queries");
        let queries = self.queries.as_deref().expect("the queries were just read");
        let system_or_hidden = |name: &str| {
            catalog.iter().any(|e| {
                e.object_type == ObjectType::Query && e.name == name && e.is_system_or_hidden()
            })
        };
        let mut out: Vec<Query> = queries
            .iter()
            .filter(|q| include_system || !system_or_hidden(&q.name))
            .map(|q| Query {
                type_name: query_type_name(q.query_type),
                name: q.name.clone(),
            })
            .collect();
        out.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The SQL of the saved query `name`, system and hidden ones included, as
    /// `jetdb queries show` prints it.
    pub fn query_sql(&mut self, name: &str) -> Result<String, FileError> {
        let query = self
            .query_defs()?
            .iter()
            .find(|q| q.name == name)
            .ok_or_else(|| FileError::QueryNotFound {
                name: name.to_string(),
            })?;
        if query.incomplete {
            return Err(FileError::IncompleteQuery {
                name: query.name.clone(),
            });
        }
        Ok(query_to_sql(query))
    }

    /// The properties of the object `name`, of `object_type` when it is
    /// given, as `jetdb prop` reads them. Without a type, objects of several
    /// types sharing the name are an error, as there is no telling which one
    /// is meant.
    pub fn properties(
        &mut self,
        name: &str,
        object_type: Option<ObjectType>,
    ) -> Result<ObjectProperties, PropertiesError> {
        let is_jet3 = self.reader.header().version.is_jet3();
        let entries: Vec<&CatalogEntry> = self
            .catalog()?
            .iter()
            .filter(|e| e.name == name && object_type.is_none_or(|t| e.object_type == t))
            .collect();
        let entry = match entries.as_slice() {
            [] => return Err(PropertiesError::NotFound),
            [only] => *only,
            several => {
                let types = several.iter().map(|e| e.object_type).collect();
                return Err(PropertiesError::SeveralTypes(types));
            }
        };
        let read = entry_properties(entry, is_jet3)?;
        let mut out = ObjectProperties::default();
        for map in read.maps {
            let properties: Vec<Property> = map
                .properties
                .into_iter()
                .map(|p| Property {
                    name: p.name,
                    value: p.value.into(),
                })
                .collect();
            match map.map_type {
                PropMapType::Default => out.object.extend(properties),
                PropMapType::Column => out.columns.push(PropertyGroup {
                    name: map.name,
                    properties,
                }),
                PropMapType::Additional => out.additional.push(PropertyGroup {
                    name: map.name,
                    properties,
                }),
            }
        }
        Ok(out)
    }

    /// The DDL of the user tables, or of `table` alone, in `dialect`, as
    /// `jetdb schema --ddl` writes it: with the indexes unless `indexes` is
    /// false, and the foreign keys unless `relations` is false.
    pub fn ddl(
        &mut self,
        dialect: &dyn DdlDialect,
        table: Option<&str>,
        indexes: bool,
        relations: bool,
    ) -> Result<String, FileError> {
        let (reader, catalog) = self.reader_and_catalog()?;
        let tables = ddl::schema_tables(reader, catalog, table)?;
        let relationships = if relations {
            read_relationships_in(reader, catalog)?
        } else {
            Vec::new()
        };
        Ok(ddl::generate_ddl(
            dialect,
            &tables,
            &relationships,
            indexes,
            relations,
        ))
    }

    /// The definition of `table`, and whether it is a system table.
    fn table_def(&mut self, table: &str) -> Result<(TableDef, bool), FileError> {
        let entry = find_table(self.catalog()?, table)?;
        let (name, page, system_table) = (entry.name.clone(), entry.table_page, entry.is_system());
        let tdef = read_table_def(&mut self.reader, &name, page)?;
        Ok((tdef, system_table))
    }

    /// The catalog, read on the first call. A file whose catalog cannot be
    /// read still opens, and the error comes from each call that needs it.
    fn catalog(&mut self) -> Result<&[CatalogEntry], FileError> {
        if self.catalog.is_none() {
            self.catalog = Some(read_catalog(&mut self.reader)?);
        }
        Ok(self.catalog.as_deref().expect("the catalog was just read"))
    }

    /// The reader together with the catalog, read on the first call, for the
    /// library functions that take both.
    fn reader_and_catalog(&mut self) -> Result<(&mut PageReader, &[CatalogEntry]), FileError> {
        self.catalog()?;
        let catalog = self.catalog.as_deref().expect("the catalog was just read");
        Ok((&mut self.reader, catalog))
    }

    /// The definitions of the saved queries, read on the first call. Like the
    /// catalog, they are not kept when they cannot be read, and the error
    /// comes from each call that needs them.
    fn query_defs(&mut self) -> Result<&[QueryDef], FileError> {
        if self.queries.is_none() {
            let (reader, catalog) = self.reader_and_catalog()?;
            self.queries = Some(read_queries_in(reader, catalog)?);
        }
        Ok(self.queries.as_deref().expect("the queries were just read"))
    }
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

/// `true` when `name` is a table, linked or not, that Access keeps from users,
/// whose relationships `Database::relationships` leaves out.
fn is_system_or_hidden_table(catalog: &[CatalogEntry], name: &str) -> bool {
    catalog.iter().any(|e| {
        matches!(
            e.object_type,
            ObjectType::Table | ObjectType::LinkedTable | ObjectType::LinkedOdbcTable
        ) && e.name == name
            && e.is_system_or_hidden()
    })
}

/// The name of a query type, as JavaScript gets it.
fn query_type_name(query_type: QueryType) -> &'static str {
    match query_type {
        QueryType::Select => "Select",
        QueryType::MakeTable => "MakeTable",
        QueryType::Append => "Append",
        QueryType::Update => "Update",
        QueryType::Delete => "Delete",
        QueryType::Crosstab => "Crosstab",
        QueryType::Ddl => "Ddl",
        QueryType::Passthrough => "Passthrough",
        QueryType::Union => "Union",
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
        | FileError::InvalidMacroData { .. }
        | FileError::IncompleteQuery { .. } => "INVALID_FILE",
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
    fn catalog_read_once_and_reused() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert!(db.catalog.is_none());
        let user = db.tables(false).unwrap();
        let other = user.iter().find(|n| *n != "Table1").unwrap().clone();

        // Put a catalog of only Table1 in its place: the calls that follow
        // see it, rather than reading the catalog again.
        let table1 = db
            .catalog
            .as_ref()
            .unwrap()
            .iter()
            .find(|e| e.name == "Table1")
            .unwrap()
            .clone();
        db.catalog = Some(vec![table1]);
        assert_eq!(db.tables(false).unwrap(), ["Table1"]);
        assert!(!db.columns("Table1").unwrap().is_empty());
        assert!(matches!(
            db.columns(&other),
            Err(FileError::TableNotFound { .. })
        ));
    }

    #[test]
    fn catalog_unreadable_after_open() {
        let mut bytes = skip_if_missing!("V2003/testV2003.mdb");
        // Blank the page of the catalog's table definition (page 2; Jet4
        // pages are 4096 bytes).
        bytes[2 * 4096..3 * 4096].fill(0);

        // The file still opens and gives its version, and each call that
        // needs the catalog fails, as before the catalog was kept.
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(db.version(), "JET4");
        for _ in 0..2 {
            let error = db.tables(false).unwrap_err();
            assert_eq!(error_code(&error), "INVALID_FILE", "{error}");
        }
        let error = db.columns("Table1").unwrap_err();
        assert_eq!(error_code(&error), "INVALID_FILE", "{error}");
        assert!(db.catalog.is_none());
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
                ("A", "Text", 50),
                ("B", "Text", 100),
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

    fn relationship(
        name: &str,
        (from_table, from): (&str, &str),
        (to_table, to): (&str, &str),
        (cascade_update, cascade_delete): (bool, bool),
    ) -> Relationship {
        Relationship {
            name: name.to_string(),
            from_table: from_table.to_string(),
            to_table: to_table.to_string(),
            columns: vec![RelationshipColumn {
                from: from.to_string(),
                to: to.to_string(),
            }],
            referential_integrity: true,
            cascade_update,
            cascade_delete,
        }
    }

    #[test]
    fn relationships_with_their_columns_and_cascades() {
        let bytes = skip_if_missing!("V1997/nwind.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let relationships = db.relationships(false).unwrap();
        let names: Vec<&str> = relationships.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "CategoriesProducts",
                "CustomersOrders",
                "EmployeesOrders",
                "OrdersOrder Details",
                "ProductsOrder Details",
                "ShippersOrders",
                "SuppliersProducts",
            ]
        );
        let by_name = |name: &str| relationships.iter().find(|r| r.name == name).unwrap();
        assert_eq!(
            *by_name("CustomersOrders"),
            relationship(
                "CustomersOrders",
                ("Orders", "CustomerID"),
                ("Customers", "CustomerID"),
                (true, false)
            )
        );
        assert_eq!(
            *by_name("OrdersOrder Details"),
            relationship(
                "OrdersOrder Details",
                ("Order Details", "OrderID"),
                ("Orders", "OrderID"),
                (false, true)
            )
        );
        // The columns of the two tables have different names here.
        assert_eq!(
            *by_name("ShippersOrders"),
            relationship(
                "ShippersOrders",
                ("Orders", "ShipVia"),
                ("Shippers", "ShipperID"),
                (false, false)
            )
        );
    }

    #[test]
    fn hidden_tables_linked_or_not() {
        let entry = |name: &str, object_type, flags| CatalogEntry {
            name: name.to_string(),
            object_type,
            table_page: 0,
            flags,
            lv_prop: None,
        };
        let hidden = jetdb::format::catalog_flags::HIDDEN;
        let catalog = [
            entry("Shown", ObjectType::Table, 0),
            entry("Hidden", ObjectType::Table, hidden),
            entry("HiddenLink", ObjectType::LinkedTable, hidden),
            entry("HiddenOdbc", ObjectType::LinkedOdbcTable, hidden),
            entry("ShownLink", ObjectType::LinkedTable, 0),
            entry("HiddenQuery", ObjectType::Query, hidden),
        ];
        let hidden_ones: Vec<&str> = catalog
            .iter()
            .map(|e| e.name.as_str())
            .filter(|&name| is_system_or_hidden_table(&catalog, name))
            .collect();
        assert_eq!(hidden_ones, ["Hidden", "HiddenLink", "HiddenOdbc"]);
    }

    #[test]
    fn relationships_of_system_tables_only_with_system() {
        let bytes = skip_if_missing!("V2003/indexTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let names =
            |rels: Vec<Relationship>| -> Vec<String> { rels.into_iter().map(|r| r.name).collect() };
        assert_eq!(
            names(db.relationships(false).unwrap()),
            ["Table2Table1", "Table3Table1"]
        );

        let bytes = skip_if_missing!("V2000/indexTestV2000.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(
            names(db.relationships(false).unwrap()),
            ["Table2Table1", "Table3Table1"]
        );
        assert_eq!(
            names(db.relationships(true).unwrap()),
            [
                "MSysNavPaneGroupCategoriesMSysNavPaneGroups",
                "MSysNavPaneGroupsMSysNavPaneGroupToObjects",
                "Table2Table1",
                "Table3Table1",
            ]
        );
    }

    #[test]
    fn relationships_without_referential_integrity() {
        // Table2 is a linked table, on which Access cannot enforce
        // referential integrity.
        let bytes = skip_if_missing!("V2007/linkerTestV2007.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let mut expected = relationship(
            "Table1Table2",
            ("Table2", "Field1"),
            ("Table1", "Field1"),
            (false, false),
        );
        expected.referential_integrity = false;
        assert_eq!(db.relationships(false).unwrap(), [expected]);
    }

    #[test]
    fn relationships_none() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(db.relationships(true).unwrap(), []);
    }

    fn dialect(name: &str) -> &'static dyn DdlDialect {
        ddl::dialect(name).unwrap()
    }

    #[test]
    fn ddl_tables_indexes_and_foreign_keys() {
        let bytes = skip_if_missing!("V2003/indexTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let sql = db.ddl(dialect("postgres"), None, true, true).unwrap();
        for expected in [
            "CREATE TABLE \"Table1\" (\n    \"id\" INTEGER,",
            "CREATE TABLE \"Table2\" (",
            "CREATE TABLE \"Table3\" (",
            "CREATE INDEX \"Table1_id_idx\" ON \"Table1\" (\"id\");",
            "ALTER TABLE \"Table1\" ADD CONSTRAINT \"Table2Table1\"\n    \
             FOREIGN KEY (\"otherfk1\") REFERENCES \"Table2\" (\"id\")\n    \
             ON DELETE CASCADE;",
            "ALTER TABLE \"Table1\" ADD CONSTRAINT \"Table3Table1\"\n    \
             FOREIGN KEY (\"otherfk2\") REFERENCES \"Table3\" (\"id\")\n    \
             ON UPDATE CASCADE;",
        ] {
            assert!(sql.contains(expected), "{expected} in:\n{sql}");
        }
        assert!(!sql.contains("MSys"), "{sql}");

        let sql = db.ddl(dialect("postgres"), None, false, false).unwrap();
        assert!(sql.contains("CREATE TABLE \"Table1\""), "{sql}");
        assert!(!sql.contains("CREATE INDEX"), "{sql}");
        assert!(!sql.contains("FOREIGN KEY"), "{sql}");
    }

    #[test]
    fn ddl_of_one_table_in_each_dialect() {
        let bytes = skip_if_missing!("V2003/indexTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        // SQLite writes the foreign keys inside CREATE TABLE.
        let sql = db
            .ddl(dialect("sqlite"), Some("Table1"), true, true)
            .unwrap();
        assert!(sql.starts_with("CREATE TABLE \"Table1\" ("), "{sql}");
        assert!(!sql.contains("CREATE TABLE \"Table2\""), "{sql}");
        assert!(
            sql.contains("    \"data\" TEXT,\n    \"otherfk3\" INTEGER,\n    PRIMARY KEY (\"id\"),\n    FOREIGN KEY (\"otherfk1\")"),
            "{sql}"
        );
        for (name, expected) in [
            ("mysql", "CREATE TABLE `Table1` ("),
            ("access", "CREATE TABLE [Table1] ("),
        ] {
            let sql = db.ddl(dialect(name), Some("Table1"), true, true).unwrap();
            assert!(sql.starts_with(expected), "{name}:\n{sql}");
        }
        assert!(matches!(
            db.ddl(dialect("sqlite"), Some("NoSuchTable"), true, true),
            Err(FileError::TableNotFound { .. })
        ));
    }

    #[test]
    fn ddl_without_relations_when_they_cannot_be_read() {
        let mut bytes = skip_if_missing!("V2003/indexTestV2003.mdb");
        let page = {
            let mut db = Database::open(bytes.clone(), None).unwrap();
            find_table(db.catalog().unwrap(), "MSysRelationships")
                .unwrap()
                .table_page as usize
        };
        // Blank the page of the definition of MSysRelationships (Jet4 pages
        // are 4096 bytes).
        bytes[page * 4096..(page + 1) * 4096].fill(0);

        let mut db = Database::open(bytes, None).unwrap();
        let error = db.relationships(false).unwrap_err();
        assert_eq!(error_code(&error), "INVALID_FILE", "{error}");
        let error = db.ddl(dialect("postgres"), None, true, true).unwrap_err();
        assert_eq!(error_code(&error), "INVALID_FILE", "{error}");
        let sql = db.ddl(dialect("postgres"), None, true, false).unwrap();
        assert!(sql.contains("CREATE TABLE \"Table1\""), "{sql}");
    }

    fn query(name: &str, type_name: &'static str) -> Query {
        Query {
            name: name.to_string(),
            type_name,
        }
    }

    #[test]
    fn query_definitions_read_once_and_reused() {
        let bytes = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert!(db.queries.is_none());
        assert_eq!(db.queries(false).unwrap().len(), 9);

        // Keep only DeleteQuery in their place: the calls that follow see
        // that, rather than reading MSysQueries again.
        db.queries
            .as_mut()
            .unwrap()
            .retain(|q| q.name == "DeleteQuery");
        assert_eq!(db.queries(false).unwrap(), [query("DeleteQuery", "Delete")]);
        assert!(db.query_sql("DeleteQuery").is_ok());
        assert!(matches!(
            db.query_sql("SelectQuery"),
            Err(FileError::QueryNotFound { .. })
        ));
    }

    #[test]
    fn queries_of_each_type() {
        // The type names are the QueryType union declared in js.rs.
        let bytes = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(
            db.queries(false).unwrap(),
            [
                query("AppendQuery", "Append"),
                query("CrosstabQuery", "Crosstab"),
                query("DataDefinitionQuery", "Ddl"),
                query("DeleteQuery", "Delete"),
                query("MakeTableQuery", "MakeTable"),
                query("PassthroughQuery", "Passthrough"),
                query("SelectQuery", "Select"),
                query("UnionQuery", "Union"),
                query("UpdateQuery", "Update"),
            ]
        );
    }

    #[test]
    fn queries_system_and_hidden_only_with_system() {
        // Access made ~sq_rStatistics-byPlace for a report.
        let bytes = skip_if_missing!("saveastext/SportsAdmin/Sports.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let names =
            |queries: Vec<Query>| -> Vec<String> { queries.into_iter().map(|q| q.name).collect() };
        let user = names(db.queries(false).unwrap());
        let all = names(db.queries(true).unwrap());
        assert_eq!(user.len(), 100);
        assert_eq!(all.len(), 101);
        assert!(!user.iter().any(|n| n.starts_with('~')), "{user:?}");
        assert!(all.contains(&"~sq_rStatistics-byPlace".to_string()));
        assert!(all.windows(2).all(|w| w[0] <= w[1]), "sorted: {all:?}");

        let sql = db.query_sql("~sq_rStatistics-byPlace").unwrap();
        assert!(sql.starts_with("TRANSFORM Count("), "{sql}");
    }

    #[test]
    fn query_sql_as_jetdb_queries_show() {
        let bytes = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(
            db.query_sql("DeleteQuery").unwrap(),
            "DELETE Table1.col1, Table1.col2, Table1.col3\nFROM Table1\nWHERE (((Table1.col1)>\"blah\"));"
        );
        assert!(matches!(
            db.query_sql("NoSuchQuery"),
            Err(FileError::QueryNotFound { .. })
        ));
        // Query names are matched as they are written, as by the CLI.
        assert!(matches!(
            db.query_sql("deletequery"),
            Err(FileError::QueryNotFound { .. })
        ));
    }

    #[test]
    fn query_sql_of_definitions_that_may_be_incomplete() {
        // The row offset at 123108 gets the offset of the row before it,
        // which leaves one row of MSysQueries no bytes.
        let mut bytes = skip_if_missing!("V2003/queryTestV2003.mdb");
        bytes.copy_within(123106..123108, 123108);
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(db.queries(false).unwrap().len(), 9);
        let error = db.query_sql("DeleteQuery").unwrap_err();
        assert!(
            matches!(&error, FileError::IncompleteQuery { name } if name == "DeleteQuery"),
            "{error}"
        );
        assert_eq!(error_code(&error), "INVALID_FILE");
    }

    #[test]
    fn queries_none() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(db.queries(true).unwrap(), []);
        assert!(matches!(
            db.query_sql("Query1"),
            Err(FileError::QueryNotFound { .. })
        ));
    }

    fn property<'a>(properties: &'a [Property], name: &str) -> &'a Cell {
        &properties.iter().find(|p| p.name == name).unwrap().value
    }

    #[test]
    fn properties_of_a_table_and_its_columns() {
        let bytes = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let props = db.properties("Table1", None).unwrap();
        assert!(matches!(
            property(&props.object, "GUID"),
            Cell::String(s) if s.starts_with('{')
        ));
        assert_eq!(
            *property(&props.object, "FCMinReadVer"),
            string("14.0.0000.0000")
        );
        assert_eq!(*property(&props.object, "TotalsRow"), Cell::Bool(false));
        assert!(matches!(property(&props.object, "NameMap"), Cell::Bytes(_)));

        let column_names: Vec<&str> = props.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(column_names.contains(&"FirstName"), "{column_names:?}");
        let first_name = &props
            .columns
            .iter()
            .find(|c| c.name == "FirstName")
            .unwrap();
        assert_eq!(
            *property(&first_name.properties, "AllowZeroLength"),
            Cell::Bool(true)
        );
        assert_eq!(
            *property(&first_name.properties, "ColumnWidth"),
            Cell::Number(1380.0)
        );
        assert!(props.additional.is_empty());
    }

    #[test]
    fn properties_of_a_query() {
        let bytes = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let props = db
            .properties("SelectQuery", Some(ObjectType::Query))
            .unwrap();
        assert_eq!(*property(&props.object, "ODBCTimeout"), Cell::Number(60.0));
        assert!(props.columns.is_empty());
    }

    #[test]
    fn properties_of_objects_sharing_a_name() {
        // nwind.mdb has a form, a macro and a table all named Customers.
        let bytes = skip_if_missing!("V1997/nwind.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let PropertiesError::SeveralTypes(mut types) =
            db.properties("Customers", None).unwrap_err()
        else {
            panic!("expected several types");
        };
        types.sort_by_key(|t| t.to_string());
        assert_eq!(
            types,
            [ObjectType::Form, ObjectType::Macro, ObjectType::Table]
        );

        let table = db.properties("Customers", Some(ObjectType::Table)).unwrap();
        assert!(table.columns.iter().any(|c| c.name == "CustomerID"));
        let form = db.properties("Customers", Some(ObjectType::Form)).unwrap();
        assert!(form.columns.is_empty());
        assert!(matches!(
            property(&form.object, "Description"),
            Cell::String(s) if s.contains("Single-column form")
        ));
        assert!(matches!(
            db.properties("Customers", Some(ObjectType::Query)),
            Err(PropertiesError::NotFound)
        ));
    }

    #[test]
    fn properties_of_a_missing_object() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert!(matches!(
            db.properties("NoSuchObject", None),
            Err(PropertiesError::NotFound)
        ));
    }

    #[test]
    fn object_types_and_their_names() {
        // These names are the ObjectType union declared in js.rs.
        let names = ObjectType::ALL.map(|t| t.to_string());
        assert_eq!(
            names,
            [
                "Table",
                "Query",
                "Form",
                "Report",
                "Macro",
                "Module",
                "LinkedTable",
                "LinkedOdbcTable",
                "Relationship",
                "Container",
                "Database",
                "DatabaseProperty",
                "UserInfo",
            ]
        );
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
            (FileError::IncompleteQuery { name: name() }, "INVALID_FILE"),
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
}
