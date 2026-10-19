//! JavaScript bindings of jetdb, built as WebAssembly for the `jetdb-wasm`
//! npm package.
//!
//! [`Database`] holds a database opened from bytes in memory, and each of its
//! methods reads one thing from it the way the matching `jetdb` CLI command
//! does.

mod js;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::Cursor;

use jetdb::ddl::{self, with_calculated_column_types, DdlDialect};
use jetdb::format::{index_flags, index_type, ColumnType, ObjectType};
use jetdb::{
    calculated_column_types_in, control_type_name, data_macros_to_text, embedded_macro_to_text,
    entry_properties, find_table, list_forms_in, list_macros_in, query_to_sql, read_access_storage,
    read_catalog, read_complex_column, read_data_macros_in, read_embedded_macros_in,
    read_form_properties_in, read_form_stream_in, read_macro_in, read_object_properties,
    read_queries_in, read_relationships_in, read_table_def, read_vba_project_in,
    relationship_flags, timestamp, write_csv, AccessStorage, BlobProperty, BlobValue, CatalogEntry,
    ComplexColumn, ComplexValues, CsvOptions, FileError, FormObjectType, IndexColumnOrder,
    MacroDef, MacroGrid, PageReader, PropMapType, QueryDef, QueryType, RowPosition, StreamKind,
    TableDef, TableRows, Value, VbaModule, VbaModuleType,
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
    /// The VBA modules, read when they are first needed, as the catalog is.
    modules: Option<Vec<VbaModule>>,
    /// The storage of forms, reports, macros and VBA, read when it is first
    /// needed, as the catalog is.
    storage: Option<AccessStorage>,
    /// Where the values of each complex column are, by the table name and
    /// the index of the column, read when they are first needed, so that
    /// reading the rows of a table a part at a time reads them once.
    complex: HashMap<(String, usize), Option<ComplexColumn>>,
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

/// A VBA module, as `Database::modules` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub name: String,
    /// `Standard`, `Class`, or `Document` for the module of a form or report
    /// (see [`module_type_name`]).
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

/// A form or report, as `Database::forms` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub name: String,
    /// `Form` or `Report`.
    pub type_name: &'static str,
}

/// The design of a form or report, as `Database::form_properties` returns
/// it: its own properties and its controls and sections.
#[derive(Debug, Clone, PartialEq)]
pub struct FormDesign {
    pub name: String,
    /// `Form` or `Report`.
    pub type_name: &'static str,
    pub properties: Vec<FormProperty>,
    pub controls: Vec<Control>,
}

/// A property of a form or report, or of one of its controls: its number,
/// which stays the same as jetdb learns more names, and its name, or the
/// number as `0x....` for one jetdb does not know.
#[derive(Debug, Clone, PartialEq)]
pub struct FormProperty {
    pub id: u16,
    pub name: String,
    pub value: Cell,
}

/// A control or section of a form or report.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub name: String,
    /// The control type, such as `TextBox` or `Detail`, or its number as
    /// `0x....` for a type jetdb does not know, as `jetdb form props` shows it.
    pub type_name: String,
    /// The number of the control type, which stays the same as jetdb learns
    /// more names.
    pub type_code: u16,
    pub properties: Vec<FormProperty>,
}

/// A macro, with the SaveAsText text `jetdb macro` prints for it: `None` for a
/// data macro, whose text is that of all the data macros of its table
/// (`Database::data_macros_text`), and for a named macro whose grid could
/// not be read.
#[derive(Debug, Clone, PartialEq)]
pub struct Macro {
    pub def: MacroDef,
    pub text: Option<String>,
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
    /// The position of the row after these, to start the next part at; `None`
    /// when there are no more rows.
    pub next: Option<RowPosition>,
}

/// The number of `position` that JavaScript gets: `page * 65536 + row`.
pub fn position_number(position: RowPosition) -> f64 {
    f64::from(position.page) * 65536.0 + f64::from(position.row)
}

/// The position of a number [`position_number`] gives; `None` for a number
/// it cannot give: negative, not an integer, or past 2^48.
pub fn number_position(number: f64) -> Option<RowPosition> {
    if !(number >= 0.0 && number.fract() == 0.0 && number < 281_474_976_710_656.0) {
        return None;
    }
    let number = number as u64;
    Some(RowPosition {
        page: u32::try_from(number >> 16).ok()?,
        row: (number & 0xFFFF) as u16,
    })
}

/// A control of a form or report, as `Database::form_controls` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlEntry {
    pub name: String,
    /// The type name (see [`control_type`]).
    pub type_name: String,
    pub type_code: u16,
    /// The place of the control in the list, from 0.
    pub index: u32,
}

/// The sections and controls of `design`, as `jetdb form controls` lists
/// them.
fn controls_of(design: jetdb::FormProperties) -> Vec<ControlEntry> {
    design
        .controls
        .into_iter()
        .zip(0..)
        .map(|(c, index)| ControlEntry {
            type_name: control_type(c.type_code),
            name: c.name,
            type_code: c.type_code,
            index,
        })
        .collect()
}

/// The name of the control type `code`, or the code as `0x....` for a type
/// jetdb does not know, as `jetdb form controls` shows it.
fn control_type(code: u16) -> String {
    control_type_name(code).map_or_else(|| format!("0x{code:04X}"), str::to_string)
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
    /// The values of a complex column in a row.
    List(Vec<Cell>),
    /// A file of an attachment column or a version of a version history,
    /// with the names of its fields.
    Record(Vec<(&'static str, Cell)>),
}

impl From<BlobValue> for Cell {
    /// A GUID becomes its string, and a number a JavaScript number: a 32-bit
    /// value of type 0x04, often a color, is the signed number Access writes
    /// in SaveAsText (see `BlobValue::Long`).
    fn from(value: BlobValue) -> Self {
        match value {
            BlobValue::Bool(v) => Cell::Bool(v),
            BlobValue::NotDefault => Cell::String("NotDefault".to_string()),
            BlobValue::Short(v) => Cell::Number(v.into()),
            BlobValue::Long(v) => Cell::Number(v.into()),
            BlobValue::Double(v) => Cell::Number(v),
            BlobValue::Guid(s) | BlobValue::Text(s) => Cell::String(s),
            BlobValue::Binary(bytes) => Cell::Bytes(bytes),
        }
    }
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

/// The cells of the columns at the indexes in `shown` of a row whose values
/// are `values`, with `columns` their complex columns.
fn row_cells(
    reader: &mut PageReader,
    mut values: Vec<Value>,
    shown: &[usize],
    columns: &[Option<&ComplexColumn>],
) -> Result<Vec<Cell>, FileError> {
    shown
        .iter()
        .zip(columns)
        .map(|(&i, column)| {
            let value = std::mem::replace(&mut values[i], Value::Null);
            cell(reader, value, *column)
        })
        .collect()
}

/// The cell of `value`, with `column` its column if it is a complex column:
/// the values of the row for a kind jetdb knows, and the number otherwise.
fn cell(
    reader: &mut PageReader,
    value: Value,
    column: Option<&ComplexColumn>,
) -> Result<Cell, FileError> {
    match (column, value) {
        (Some(column), Value::Long(n)) => {
            Ok(complex_cell(column.values(reader, n)?).unwrap_or(Cell::Number(n.into())))
        }
        (_, value) => Ok(value.into()),
    }
}

/// The cell of the values of a complex column in a row; `None` for a kind
/// jetdb does not know.
fn complex_cell(values: ComplexValues) -> Option<Cell> {
    let cells = match values {
        ComplexValues::MultiValue(values) => values.into_iter().map(Cell::from).collect(),
        ComplexValues::Attachment(files) => files
            .into_iter()
            .map(|f| {
                Cell::Record(vec![
                    ("name", f.name.into()),
                    ("type", f.file_type.into()),
                    ("data", f.data.into()),
                    ("url", f.url.into()),
                    ("timestamp", f.timestamp.into()),
                    ("flags", f.flags.into()),
                ])
            })
            .collect(),
        ComplexValues::VersionHistory(versions) => versions
            .into_iter()
            .map(|v| {
                Cell::Record(vec![
                    ("value", v.value.into()),
                    ("modified", v.modified.into()),
                ])
            })
            .collect(),
        ComplexValues::Unknown => return None,
    };
    Some(Cell::List(cells))
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
            modules: None,
            storage: None,
            complex: HashMap::new(),
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
        self.rows_from(table, RowPosition::default(), None)
    }

    /// The rows of `table` from `start`, at most `limit` of them, as
    /// [`Database::rows`] reads them; [`Rows::next`] is where the next part
    /// starts.
    pub fn rows_from(
        &mut self,
        table: &str,
        start: RowPosition,
        limit: Option<usize>,
    ) -> Result<Rows, FileError> {
        let (tdef, system_table) = self.table_def(table)?;
        let shown: Vec<usize> = (0..tdef.columns.len())
            .filter(|&i| tdef.columns[i].is_shown(system_table))
            .collect();
        self.read_complex_columns(&tdef, &shown)?;
        let Self {
            reader,
            catalog,
            complex,
            ..
        } = self;
        let catalog = catalog.as_deref().expect("the catalog was read");
        let columns: Vec<Option<&ComplexColumn>> = shown
            .iter()
            .map(|&i| complex[&(tdef.name.clone(), i)].as_ref())
            .collect();
        let calculated = calculated_column_types_in(catalog, &tdef);
        let mut table_rows = TableRows::new(&tdef, &calculated)
            .with_columns(&shown)
            .starting_at(start);
        let mut rows = Vec::new();
        while limit.is_none_or(|limit| rows.len() < limit) {
            let Some(row) = table_rows.next_row(reader)? else {
                break;
            };
            rows.push(row_cells(reader, row.values, &shown, &columns)?);
        }
        let next = table_rows.peek(reader)?;
        Ok(Rows {
            columns: shown
                .iter()
                .map(|&i| tdef.columns[i].name.clone())
                .collect(),
            rows,
            skipped: table_rows.skipped_rows(),
            next,
        })
    }

    /// Read where the values of the complex columns among the columns of
    /// `table` at the indexes in `columns` are, for those not read yet.
    fn read_complex_columns(
        &mut self,
        table: &TableDef,
        columns: &[usize],
    ) -> Result<(), FileError> {
        self.catalog()?;
        let Self {
            reader,
            catalog,
            complex,
            ..
        } = self;
        let catalog = catalog.as_deref().expect("the catalog was just read");
        for &i in columns {
            if let Entry::Vacant(entry) = complex.entry((table.name.clone(), i)) {
                entry.insert(read_complex_column(
                    reader,
                    catalog,
                    table,
                    &table.columns[i],
                )?);
            }
        }
        Ok(())
    }

    /// The CSV of `table`, as `jetdb export` writes it with `options`: an
    /// attachment field gives the file names. Rows that cannot be read are
    /// left out, as [`Database::rows`] leaves them out.
    pub fn csv(&mut self, table: &str, options: &CsvOptions) -> Result<String, FileError> {
        let (tdef, _) = self.table_def(table)?;
        let (reader, catalog) = self.reader_and_catalog()?;
        let mut out = Vec::new();
        write_csv(reader, catalog, &tdef, options, &mut out, None)?;
        Ok(String::from_utf8_lossy(&out).into_owned())
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

    /// The VBA modules, sorted by name, as `jetdb vba list` lists them, and
    /// none when the database has no VBA.
    pub fn modules(&mut self) -> Result<Vec<Module>, FileError> {
        let mut out: Vec<Module> = self
            .vba_modules()?
            .iter()
            .map(|m| Module {
                name: m.name.clone(),
                type_name: module_type_name(m.module_type),
            })
            .collect();
        out.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The source of the VBA module `name`, as `jetdb vba show` prints it.
    pub fn module_source(&mut self, name: &str) -> Result<String, FileError> {
        self.vba_modules()?
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.source.clone())
            .ok_or_else(|| FileError::ModuleNotFound {
                name: name.to_string(),
            })
    }

    /// The forms and reports, sorted by name, as `jetdb form list` lists them,
    /// and a form before a report of the same name.
    pub fn forms(&mut self) -> Result<Vec<Form>, FileError> {
        let mut out: Vec<Form> = list_forms_in(self.storage()?)?
            .into_iter()
            .map(|f| Form {
                name: f.name,
                type_name: form_type_name(f.object_type),
            })
            .collect();
        // Form sorts before Report.
        out.sort_unstable_by(|a, b| (&a.name, a.type_name).cmp(&(&b.name, b.type_name)));
        Ok(out)
    }

    /// The sections and controls of the form or report `name`, as `jetdb form
    /// controls` lists them: in the order of its Blob, with their places.
    pub fn form_controls(
        &mut self,
        name: &str,
        form_type: Option<FormObjectType>,
    ) -> Result<Vec<ControlEntry>, PropertiesError> {
        let form_type = self.form_type(name, form_type)?;
        let design = read_form_properties_in(self.storage()?, name, form_type)?;
        Ok(controls_of(design))
    }

    /// The bytes of the stream `kind` of the form or report `name`, as
    /// `jetdb form dump` writes them.
    pub fn form_stream(
        &mut self,
        name: &str,
        kind: StreamKind,
        form_type: Option<FormObjectType>,
    ) -> Result<Vec<u8>, PropertiesError> {
        let form_type = self.form_type(name, form_type)?;
        Ok(read_form_stream_in(self.storage()?, name, form_type, kind)?.data)
    }

    /// The design of the form or report `name`, as `jetdb form props` shows
    /// it: each property by its name, or by its number as `0x....` for one
    /// jetdb does not know.
    pub fn form_properties(
        &mut self,
        name: &str,
        form_type: Option<FormObjectType>,
    ) -> Result<FormDesign, PropertiesError> {
        let form_type = self.form_type(name, form_type)?;
        let read = read_form_properties_in(self.storage()?, name, form_type)?;
        let properties = |properties: Vec<BlobProperty>| {
            properties
                .into_iter()
                .map(|p| FormProperty {
                    id: p.prop_id,
                    name: p.display_name(),
                    value: p.value.into(),
                })
                .collect()
        };
        Ok(FormDesign {
            name: read.form_name,
            type_name: form_type_name(read.object_type),
            properties: properties(read.properties),
            controls: read
                .controls
                .into_iter()
                .map(|c| Control {
                    name: c.name,
                    type_name: control_type(c.type_code),
                    type_code: c.type_code,
                    properties: properties(c.properties),
                })
                .collect(),
        })
    }

    /// The names of the named macros, sorted, without those Access makes for
    /// itself and names with `~`, such as the `~TMPCLPMacro` the macro
    /// designer makes on copy and paste, and with `include_system` all of them,
    /// as `jetdb macro list` lists them. Access does not mark these in the
    /// catalog, so they are told by their names.
    pub fn macros(&mut self, include_system: bool) -> Result<Vec<String>, FileError> {
        let (reader, storage) = self.reader_and_storage()?;
        let mut names: Vec<String> = list_macros_in(reader, storage)?
            .into_iter()
            .map(|m| m.name)
            .filter(|name| include_system || !name.starts_with('~'))
            .collect();
        names.sort_unstable();
        Ok(names)
    }

    /// The named macro `name`, with the text `jetdb macro show` prints.
    pub fn named_macro(&mut self, name: &str) -> Result<Macro, FileError> {
        let (reader, storage) = self.reader_and_storage()?;
        let def = read_macro_in(reader, storage, name)?;
        let text = def.grid.as_ref().map(MacroGrid::to_text);
        Ok(Macro { def, text })
    }

    /// The macros embedded in the events of the form or report `form`, with
    /// the text `jetdb macro embedded` prints for each.
    pub fn embedded_macros(
        &mut self,
        form: &str,
        form_type: Option<FormObjectType>,
    ) -> Result<Vec<Macro>, PropertiesError> {
        let form_type = self.form_type(form, form_type)?;
        let macros = read_embedded_macros_in(self.storage()?, form, form_type)?;
        Ok(macros
            .into_iter()
            .map(|def| Macro {
                text: embedded_macro_to_text(&def),
                def,
            })
            .collect())
    }

    /// The data macros of the table `table`.
    pub fn data_macros(&mut self, table: &str) -> Result<Vec<Macro>, FileError> {
        Ok(read_data_macros_in(self.catalog()?, table)?
            .into_iter()
            .map(|def| Macro { def, text: None })
            .collect())
    }

    /// The text of all the data macros of the table `table`, as
    /// `jetdb macro data` prints it.
    pub fn data_macros_text(&mut self, table: &str) -> Result<String, FileError> {
        Ok(data_macros_to_text(&read_data_macros_in(
            self.catalog()?,
            table,
        )?))
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
        let text = self.reader.text_encoding();
        let entries: Vec<&CatalogEntry> = self
            .catalog()?
            .iter()
            .filter(|e| e.name == name && object_type.is_none_or(|t| e.object_type == t))
            .collect();
        // Rows of the same type are one type: the first of them is read, as
        // read_object_properties_of_type reads the first.
        let mut types: Vec<ObjectType> = Vec::new();
        for e in &entries {
            if !types.contains(&e.object_type) {
                types.push(e.object_type);
            }
        }
        let entry = match (types.len(), entries.first()) {
            (_, None) => None,
            (1, Some(first)) => Some(*first),
            _ => return Err(PropertiesError::SeveralTypes(types)),
        };
        let read = match entry {
            Some(entry) => entry_properties(entry, text)?,
            // The catalog leaves out objects of types jetdb does not know:
            // without a type, read MSysObjects again by the name alone, as
            // `jetdb prop` does.
            None if object_type.is_none() => {
                let read = read_object_properties(&mut self.reader, name)?;
                if read.maps.is_empty() {
                    return Err(PropertiesError::NotFound);
                }
                read
            }
            None => return Err(PropertiesError::NotFound),
        };
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

    /// The type of the form or report `name`: `given`, or the only one of the
    /// name, or `None` when there is none. A form and a report of the same
    /// name are an error without `given`, as there is no telling which one
    /// is meant.
    fn form_type(
        &mut self,
        name: &str,
        given: Option<FormObjectType>,
    ) -> Result<Option<FormObjectType>, PropertiesError> {
        if given.is_some() {
            return Ok(given);
        }
        let mut types: Vec<FormObjectType> = Vec::new();
        for entry in list_forms_in(self.storage()?)? {
            if entry.name == name && !types.contains(&entry.object_type) {
                types.push(entry.object_type);
            }
        }
        match types.as_slice() {
            [_, _, ..] => Err(PropertiesError::SeveralTypes(vec![
                ObjectType::Form,
                ObjectType::Report,
            ])),
            _ => Ok(types.first().copied()),
        }
    }

    /// The storage of forms, reports, macros and VBA, read on the first call.
    /// Like the catalog, it is not kept when it cannot be read, and the error
    /// comes from each call that needs it.
    fn storage(&mut self) -> Result<&AccessStorage, FileError> {
        if self.storage.is_none() {
            self.storage = Some(read_access_storage(&mut self.reader)?);
        }
        Ok(self.storage.as_ref().expect("the storage was just read"))
    }

    /// The reader together with the storage, read on the first call, for the
    /// library functions that take both.
    fn reader_and_storage(&mut self) -> Result<(&mut PageReader, &AccessStorage), FileError> {
        self.storage()?;
        let storage = self.storage.as_ref().expect("the storage was just read");
        Ok((&mut self.reader, storage))
    }

    /// The VBA modules, read on the first call. Like the catalog, they are
    /// not kept when they cannot be read, and the error comes from each call
    /// that needs them.
    fn vba_modules(&mut self) -> Result<&[VbaModule], FileError> {
        if self.modules.is_none() {
            let (reader, storage) = self.reader_and_storage()?;
            self.modules = Some(read_vba_project_in(reader, storage)?.modules);
        }
        Ok(self.modules.as_deref().expect("the modules were just read"))
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

/// The name of a form type, as JavaScript gets it.
pub(crate) fn form_type_name(form_type: FormObjectType) -> &'static str {
    match form_type {
        FormObjectType::Form => "Form",
        FormObjectType::Report => "Report",
    }
}

/// The name of a VBA module type, as JavaScript gets it.
fn module_type_name(module_type: VbaModuleType) -> &'static str {
    match module_type {
        VbaModuleType::Standard => "Standard",
        VbaModuleType::Class => "Class",
        VbaModuleType::Document => "Document",
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
    // The library functions that read the storage or the catalog themselves,
    // which the tests compare Database with.
    use jetdb::{
        list_forms, list_macros, read_data_macros, read_embedded_macros, read_form_properties,
        read_form_properties_of_type, read_form_stream_of_type, read_macro, read_vba_project,
    };

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
            lv_extra: None,
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

    #[test]
    fn macros_without_those_access_makes_for_itself() {
        // ~TMPCLPMacro is the macro the macro designer makes on copy and paste.
        let bytes = skip_if_missing!("V2010/macroTestV2010.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let visible = ["AutoExec", "mcrConditions", "mcrLongText", "mcrSimple"];
        assert_eq!(db.macros(false).unwrap(), visible);
        let all = db.macros(true).unwrap();
        assert_eq!(all[..4], visible);
        assert_eq!(all[4..], ["~TMPCLPMacro"]);
    }

    #[test]
    fn macros_as_jetdb_macro_shows_them() {
        // Every named, embedded and data macro is the macro the library reads,
        // with the text jetdb macro show, embedded and data print.
        let mut compared = (0, 0, 0);
        for file in [
            "V2003/macroTestV2003.mdb",
            "V2010/macroTestV2010.accdb",
            "V2010/macroGeneratedTestV2010.accdb",
        ] {
            let bytes = skip_if_missing!(file);
            let mut db = Database::open(bytes.clone(), None).unwrap();
            let mut reader =
                PageReader::open_reader_with_password(Cursor::new(bytes), None).unwrap();

            let mut names: Vec<String> = list_macros(&mut reader)
                .unwrap()
                .into_iter()
                .map(|m| m.name)
                .collect();
            names.sort_unstable();
            assert_eq!(db.macros(true).unwrap(), names, "{file}");
            for name in &names {
                let def = read_macro(&mut reader, name).unwrap();
                let read = db.named_macro(name).unwrap();
                assert_eq!(read.text, def.grid.as_ref().map(MacroGrid::to_text));
                assert_eq!(read.def, def, "{file}: {name}");
                compared.0 += 1;
            }

            for form in list_forms(&mut reader).unwrap() {
                let defs = read_embedded_macros(&mut reader, &form.name).unwrap();
                let read = db.embedded_macros(&form.name, None).unwrap();
                let texts: Vec<_> = defs.iter().map(embedded_macro_to_text).collect();
                assert_eq!(
                    read.iter().map(|m| m.text.clone()).collect::<Vec<_>>(),
                    texts
                );
                assert_eq!(read.into_iter().map(|m| m.def).collect::<Vec<_>>(), defs);
                compared.1 += defs.len();
            }

            for table in db.tables(false).unwrap() {
                let defs = read_data_macros(&mut reader, &table).unwrap();
                let read = db.data_macros(&table).unwrap();
                assert!(read.iter().all(|m| m.text.is_none()));
                assert_eq!(read.into_iter().map(|m| m.def).collect::<Vec<_>>(), defs);
                assert_eq!(
                    db.data_macros_text(&table).unwrap(),
                    data_macros_to_text(&defs)
                );
                compared.2 += defs.len();
            }

            assert!(matches!(
                db.named_macro("NoSuchMacro"),
                Err(FileError::MacroNotFound { .. })
            ));
            assert!(matches!(
                db.embedded_macros("NoSuchForm", None),
                Err(PropertiesError::File(FileError::FormNotFound { .. }))
            ));
            assert!(matches!(
                db.data_macros("NoSuchTable"),
                Err(FileError::TableNotFound { .. })
            ));
        }
        // Named, embedded and data macros were all compared.
        assert!(
            compared.0 > 0 && compared.1 > 0 && compared.2 > 0,
            "{compared:?}"
        );
    }

    #[test]
    fn storage_read_once_and_kept() {
        // The storage of forms, macros and VBA is read by the first call that
        // needs it and kept for the others.
        let bytes = skip_if_missing!("V2010/macroGeneratedTestV2010.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert!(db.storage.is_none());
        let forms = db.forms().unwrap();
        assert!(db.storage.is_some());
        assert_eq!(forms.len(), 2);
        assert!(!db.macros(false).unwrap().is_empty());
        assert!(!db.embedded_macros("frmEmbedded", None).unwrap().is_empty());
    }

    #[test]
    fn form_and_report_of_the_same_name() {
        // sameNameTestV2007.accdb has a form and a report both named Shared.
        let bytes = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(
            db.forms().unwrap(),
            [form("Shared", "Form"), form("Shared", "Report")]
        );
        // Without a type there is no telling which one is meant.
        assert!(matches!(
            db.form_properties("Shared", None),
            Err(PropertiesError::SeveralTypes(_))
        ));
        assert!(matches!(
            db.embedded_macros("Shared", None),
            Err(PropertiesError::SeveralTypes(_))
        ));
        for (form_type, type_name, caption, event) in [
            (FormObjectType::Form, "Form", "Shared form", "OnLoad"),
            (FormObjectType::Report, "Report", "Shared report", "OnOpen"),
        ] {
            let design = db.form_properties("Shared", Some(form_type)).unwrap();
            assert_eq!(design.type_name, type_name);
            assert_eq!(
                *form_property(&design.properties, "Caption"),
                Cell::String(caption.into())
            );
            let macros = db.embedded_macros("Shared", Some(form_type)).unwrap();
            let events: Vec<&str> = macros
                .iter()
                .map(|m| match &m.def.source {
                    jetdb::MacroSource::Embedded { event, .. } => event.as_str(),
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(events, [event], "{type_name}");
        }
    }

    fn form(name: &str, type_name: &'static str) -> Form {
        Form {
            name: name.to_string(),
            type_name,
        }
    }

    /// The stream `kind` of the form or report `entry`, read by Database and
    /// by the library function jetdb form dump uses, with the error as text.
    fn both_streams(
        db: &mut Database,
        reader: &mut PageReader,
        entry: &jetdb::FormEntry,
        kind: StreamKind,
    ) -> (Result<Vec<u8>, String>, Result<Vec<u8>, String>) {
        let expected = read_form_stream_of_type(reader, &entry.name, entry.object_type, kind)
            .map(|s| s.data)
            .map_err(|e| e.to_string());
        let got = db
            .form_stream(&entry.name, kind, Some(entry.object_type))
            .map_err(|e| match e {
                PropertiesError::File(e) => e.to_string(),
                other => panic!("{other:?}"),
            });
        (got, expected)
    }

    #[test]
    fn form_streams_as_jetdb_form_dump_writes_them() {
        // Each stream of each form and report is what the library gives
        // jetdb form dump, or the same error.
        let kinds = [
            StreamKind::Blob,
            StreamKind::TypeInfo,
            StreamKind::PropData,
            StreamKind::BlobDelta,
        ];
        for file in ["formPropTest.accdb", "saveastext/SportsAdmin/Sports.accdb"] {
            let bytes = skip_if_missing!(file);
            let mut db = Database::open(bytes.clone(), None).unwrap();
            let mut reader = PageReader::open_reader(Cursor::new(bytes)).unwrap();
            let entries = list_forms(&mut reader).unwrap();
            assert!(!entries.is_empty(), "{file}");
            for (entry, kind) in entries.iter().flat_map(|e| kinds.map(|k| (e, k))) {
                let (got, expected) = both_streams(&mut db, &mut reader, entry, kind);
                assert_eq!(got, expected, "{file} {} {kind:?}", entry.name);
            }
        }
    }

    /// The controls of `entry` as Database gives them, and as the library
    /// function jetdb form controls uses gives them, with the error as text.
    fn both_control_lists(
        db: &mut Database,
        reader: &mut PageReader,
        entry: &jetdb::FormEntry,
    ) -> (
        Result<Vec<ControlEntry>, String>,
        Result<Vec<ControlEntry>, String>,
    ) {
        let expected = read_form_properties_of_type(reader, &entry.name, entry.object_type)
            .map(|design| {
                design
                    .controls
                    .into_iter()
                    .zip(0..)
                    .map(|(c, index)| ControlEntry {
                        type_name: control_name_of_cli(c.type_code),
                        name: c.name,
                        type_code: c.type_code,
                        index,
                    })
                    .collect()
            })
            .map_err(|e| e.to_string());
        let got = db
            .form_controls(&entry.name, Some(entry.object_type))
            .map_err(|e| match e {
                PropertiesError::File(e) => e.to_string(),
                other => panic!("{other:?}"),
            });
        (got, expected)
    }

    /// The type name jetdb form controls prints for `code`.
    fn control_name_of_cli(code: u16) -> String {
        control_type_name(code)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("0x{code:04X}"))
    }

    #[test]
    fn form_controls_as_jetdb_form_controls_lists_them() {
        // The controls of each form and report are those jetdb form controls
        // lists, in its order; their names and types are those of the
        // controls of formProperties.
        for file in ["formPropTest.accdb", "saveastext/SportsAdmin/Sports.accdb"] {
            let bytes = skip_if_missing!(file);
            let mut db = Database::open(bytes.clone(), None).unwrap();
            let mut reader = PageReader::open_reader(Cursor::new(bytes)).unwrap();
            let entries = list_forms(&mut reader).unwrap();
            assert!(!entries.is_empty(), "{file}");
            for entry in &entries {
                let (got, expected) = both_control_lists(&mut db, &mut reader, entry);
                assert_eq!(got, expected, "{file} {}", entry.name);
                let Ok(controls) = got else {
                    continue;
                };
                let mut listed: Vec<(String, String)> = controls
                    .into_iter()
                    .map(|c| (c.name.to_lowercase(), c.type_name))
                    .collect();
                let design = db
                    .form_properties(&entry.name, Some(entry.object_type))
                    .unwrap();
                let mut designed: Vec<(String, String)> = design
                    .controls
                    .into_iter()
                    .map(|c| (c.name.to_lowercase(), c.type_name))
                    .collect();
                listed.sort();
                designed.sort();
                assert_eq!(listed, designed, "{file} {}", entry.name);
            }
        }
    }

    #[test]
    fn form_controls_of_a_form_and_report_of_the_same_name() {
        let bytes = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut db = Database::open(bytes.clone(), None).unwrap();
        assert!(matches!(
            db.form_controls("Shared", None),
            Err(PropertiesError::SeveralTypes(_))
        ));
        let mut reader = PageReader::open_reader(Cursor::new(bytes)).unwrap();
        for object_type in [FormObjectType::Form, FormObjectType::Report] {
            let entry = jetdb::FormEntry {
                name: "Shared".to_string(),
                object_type,
            };
            let (got, expected) = both_control_lists(&mut db, &mut reader, &entry);
            assert_eq!(got, expected, "{object_type:?}");
        }
        assert!(matches!(
            db.form_controls("NoSuchForm", None),
            Err(PropertiesError::File(FileError::FormNotFound { .. }))
        ));
    }

    #[test]
    fn form_stream_of_a_form_and_report_of_the_same_name() {
        let bytes = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut db = Database::open(bytes.clone(), None).unwrap();
        assert!(matches!(
            db.form_stream("Shared", StreamKind::Blob, None),
            Err(PropertiesError::SeveralTypes(_))
        ));
        let mut reader = PageReader::open_reader(Cursor::new(bytes)).unwrap();
        for object_type in [FormObjectType::Form, FormObjectType::Report] {
            let expected =
                read_form_stream_of_type(&mut reader, "Shared", object_type, StreamKind::Blob)
                    .unwrap()
                    .data;
            assert_eq!(
                db.form_stream("Shared", StreamKind::Blob, Some(object_type))
                    .unwrap(),
                expected
            );
        }
        assert!(matches!(
            db.form_stream("NoSuchForm", StreamKind::Blob, None),
            Err(PropertiesError::File(FileError::FormNotFound { .. }))
        ));
    }

    #[test]
    fn forms_and_reports() {
        let bytes = skip_if_missing!("formPropTest.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(
            db.forms().unwrap(),
            [
                form("F_Buttons", "Form"),
                form("F_Table0", "Form"),
                form("F_Table1", "Form"),
                form("jp_フォーム_2", "Form"),
            ]
        );

        let bytes = skip_if_missing!("saveastext/SportsAdmin/Sports.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let forms = db.forms().unwrap();
        let count = |type_name| forms.iter().filter(|f| f.type_name == type_name).count();
        assert_eq!((count("Form"), count("Report")), (74, 42));
        assert!(forms.windows(2).all(|w| w[0].name < w[1].name));
    }

    #[test]
    fn form_properties_as_jetdb_form_props_shows_them() {
        for (file, name) in [
            ("formPropTest.accdb", "jp_フォーム_2"),
            ("formPropTest.accdb", "F_Buttons"),
            ("saveastext/SportsAdmin/Sports.accdb", "Statistical Reports"),
            ("saveastext/SportsAdmin/Sports.accdb", "Field (High Jump)"),
        ] {
            let bytes = skip_if_missing!(file);
            let mut db = Database::open(bytes.clone(), None).unwrap();
            let design = db.form_properties(name, None).unwrap();
            let mut reader =
                PageReader::open_reader_with_password(Cursor::new(bytes), None).unwrap();
            let read = read_form_properties(&mut reader, name).unwrap();
            // Each property by its number and its name.
            let names = |properties: &[FormProperty]| -> Vec<(u16, String)> {
                properties.iter().map(|p| (p.id, p.name.clone())).collect()
            };
            let read_names = |properties: &[BlobProperty]| -> Vec<(u16, String)> {
                properties
                    .iter()
                    .map(|p| (p.prop_id, p.display_name()))
                    .collect()
            };
            assert_eq!(design.name, name);
            assert_eq!(names(&design.properties), read_names(&read.properties));
            assert_eq!(design.controls.len(), read.controls.len(), "{name}");
            for (control, read) in design.controls.iter().zip(&read.controls) {
                assert_eq!(control.name, read.name);
                let type_name = control_type_name(read.type_code)
                    .map_or_else(|| format!("0x{:04X}", read.type_code), str::to_string);
                assert_eq!(control.type_name, type_name);
                assert_eq!(control.type_code, read.type_code);
                assert_eq!(names(&control.properties), read_names(&read.properties));
            }
        }

        // Values in the types JavaScript gets.
        let bytes = skip_if_missing!("formPropTest.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let design = db.form_properties("jp_フォーム_2", None).unwrap();
        assert_eq!(design.type_name, "Form");
        let value = |name| form_property(&design.properties, name);
        assert_eq!(*value("RecordSource"), Cell::String("jp_クエリ_02".into()));
        // A Yes/No property stored without a value, as SaveAsText writes NotDefault.
        assert_eq!(*value("0x013A"), Cell::String("NotDefault".into()));
        assert_eq!(*value("DefaultView"), Cell::Number(5.0));
        // A color as the number Access writes.
        assert_eq!(
            *value("DatasheetGridlinesColor"),
            Cell::Number(f64::from(0x00E8_E8E8))
        );
        assert!(matches!(value("0x0178"), Cell::String(s) if s.starts_with('{')));
        assert!(matches!(value("0x0179"), Cell::Bytes(b) if b.len() == 384));
        assert!(matches!(
            db.form_properties("NoSuchForm", None),
            Err(PropertiesError::File(FileError::FormNotFound { .. }))
        ));

        let bytes = skip_if_missing!("saveastext/SportsAdmin/Sports.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let design = db.form_properties("Field (High Jump)", None).unwrap();
        assert_eq!(design.type_name, "Report");
        assert_eq!(
            *form_property(&design.properties, "RecordSource"),
            Cell::String("Field Events".into())
        );
        // A property jetdb names has its number too.
        let record_source = design
            .properties
            .iter()
            .find(|p| p.name == "RecordSource")
            .unwrap();
        assert_eq!(record_source.id, 0x009C);
        // The same 32-bit type holds numbers that are not colors, such as
        // HelpContextId, which SaveAsText writes as 270.
        let design = db.form_properties("Statistical Reports", None).unwrap();
        assert_eq!(
            *form_property(&design.properties, "HelpContextId"),
            Cell::Number(270.0)
        );
    }

    /// The value of the form property `name`.
    fn form_property<'a>(properties: &'a [FormProperty], name: &str) -> &'a Cell {
        &properties.iter().find(|p| p.name == name).unwrap().value
    }

    fn module(name: &str, type_name: &'static str) -> Module {
        Module {
            name: name.to_string(),
            type_name,
        }
    }

    #[test]
    fn modules_and_their_source() {
        for (file, expected) in [
            (
                "vbaV2000.mdb",
                vec![
                    module("Class1", "Class"),
                    module("Form_Form1", "Document"),
                    module("Module1", "Standard"),
                ],
            ),
            (
                "vbaV2007.accdb",
                vec![
                    module("Class1", "Class"),
                    module("Form_Form1", "Document"),
                    module("Module1", "Standard"),
                ],
            ),
            (
                "vbaJapaneseV2007.accdb",
                vec![
                    module("VbaJapaneseGenerator", "Standard"),
                    module("日本語モジュール", "Standard"),
                    module("顧客クラス", "Class"),
                ],
            ),
        ] {
            let bytes = skip_if_missing!(file);
            let mut db = Database::open(bytes.clone(), None).unwrap();
            assert_eq!(db.modules().unwrap(), expected, "{file}");

            // The source of each module is the source the library reads, as
            // jetdb vba show prints it.
            let mut reader =
                PageReader::open_reader_with_password(Cursor::new(bytes), None).unwrap();
            for read in read_vba_project(&mut reader).unwrap().modules {
                assert_eq!(
                    db.module_source(&read.name).unwrap(),
                    read.source,
                    "{file}: {}",
                    read.name
                );
            }
            assert!(matches!(
                db.module_source("NoSuchModule"),
                Err(FileError::ModuleNotFound { .. })
            ));
        }
    }

    #[test]
    fn modules_of_a_database_without_vba() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        assert_eq!(db.modules().unwrap(), []);
        assert!(matches!(
            db.module_source("Module1"),
            Err(FileError::ModuleNotFound { .. })
        ));
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
    fn properties_of_an_object_listed_twice() {
        // Two catalog rows of the same name and type are one object type.
        let bytes = skip_if_missing!("V1997/nwind.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let catalog = db.catalog().unwrap();
        let table = catalog
            .iter()
            .find(|e| e.name == "Customers" && e.object_type == ObjectType::Table)
            .unwrap()
            .clone();
        db.catalog.as_mut().unwrap().push(table);

        let table = db.properties("Customers", Some(ObjectType::Table)).unwrap();
        assert!(table.columns.iter().any(|c| c.name == "CustomerID"));
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
    fn properties_of_an_object_the_catalog_leaves_out() {
        // The catalog leaves out objects of types jetdb does not know; take
        // Table1 out of it to stand for one.
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let expected = db.properties("Table1", None).unwrap();
        db.catalog.as_mut().unwrap().retain(|e| e.name != "Table1");

        // Without a type, it is read by the name alone.
        assert_eq!(db.properties("Table1", None).unwrap(), expected);
        // With a type, only the catalog is searched.
        assert!(matches!(
            db.properties("Table1", Some(ObjectType::Table)),
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
    fn rows_of_complex_columns() {
        let bytes = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db.rows("Table1").unwrap();
        let list = |values: &[&str]| Cell::List(values.iter().map(|s| string(s)).collect());
        assert_eq!(cell(&rows, string("row1"), "multi-value-data"), list(&[]));
        assert_eq!(
            cell(&rows, string("row2"), "multi-value-data"),
            list(&["value1", "value4"])
        );
        let file = |name: &str, data: &str| {
            Cell::Record(vec![
                ("name", string(name)),
                ("type", string("txt")),
                ("data", Cell::Bytes(data.as_bytes().to_vec())),
                ("url", Cell::Null),
                ("timestamp", Cell::Null),
                ("flags", Cell::Null),
            ])
        };
        assert_eq!(
            cell(&rows, string("row4"), "attach-data"),
            Cell::List(vec![file(
                "test_data2.txt",
                "this is some more test data for attachment."
            )])
        );
        let version = |value: &str, modified: &str| {
            Cell::Record(vec![
                ("value", string(value)),
                ("modified", string(modified)),
            ])
        };
        assert_eq!(
            cell(
                &rows,
                string("row3"),
                "VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880"
            ),
            Cell::List(vec![
                version("row3-memo", "2011-09-12 21:21:19"),
                version("row3-memo-revised", "2011-09-12 21:22:33"),
                version("row3-memo-again", "2011-09-12 21:22:45"),
            ])
        );
    }

    #[test]
    fn rows_of_a_complex_column_of_an_unknown_kind() {
        // The type table of UnknownComplex has a column of a type jetdb does
        // not know, so the column keeps its numbers.
        let bytes = skip_if_missing!("V2007/unsupportedFieldsTestV2007.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db.rows("Test").unwrap();
        let i = rows
            .columns
            .iter()
            .position(|c| c == "UnknownComplex")
            .unwrap();
        assert!(rows.rows.iter().all(|r| matches!(r[i], Cell::Number(_))));
    }

    /// The rows of `table` read `limit` at a time, joined, with the number
    /// of parts.
    fn rows_in_parts(db: &mut Database, table: &str, limit: usize) -> (Rows, usize) {
        let mut all = db
            .rows_from(table, RowPosition::default(), Some(limit))
            .unwrap();
        let mut parts = 1;
        while let Some(start) = all.next {
            assert_eq!(
                all.rows.len() % limit,
                0,
                "{table}: a part short of {limit}"
            );
            let part = db.rows_from(table, start, Some(limit)).unwrap();
            assert!(part.rows.len() <= limit);
            assert_eq!(part.columns, all.columns);
            all.rows.extend(part.rows);
            all.skipped += part.skipped;
            all.next = part.next;
            parts += 1;
        }
        (all, parts)
    }

    #[test]
    fn rows_read_a_part_at_a_time() {
        // Joining the parts gives all the rows, complex columns included,
        // whatever the limit.
        for (file, table, limits) in [
            ("V2003/testV2003.mdb", "Table1", &[1, 2, 3][..]),
            (
                "V2007/complexDataTestV2007.accdb",
                "Table1",
                &[1, 2, 3, 4, 5][..],
            ),
            ("V2007/testOleV2007.accdb", "Table1", &[1, 4, 100][..]),
            (
                "V2007/testIndexCodesV2007.accdb",
                "Table14",
                &[1000, 7777][..],
            ),
        ] {
            let bytes = skip_if_missing!(file);
            let mut db = Database::open(bytes, None).unwrap();
            let whole = db.rows(table).unwrap();
            assert_eq!(whole.next, None);
            for &limit in limits {
                let (parts, count) = rows_in_parts(&mut db, table, limit);
                assert_eq!(parts, whole, "{file} {table} by {limit}");
                assert_eq!(count, whole.rows.len().div_ceil(limit).max(1));
            }
        }
    }

    #[test]
    fn rows_from_past_the_last_row() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db
            .rows_from("Table1", RowPosition { page: 9, row: 0 }, Some(10))
            .unwrap();
        assert!(rows.rows.is_empty());
        assert_eq!(rows.next, None);
        assert_eq!(rows.skipped, 0);
    }

    #[test]
    fn rows_from_counts_the_rows_it_passes_over() {
        // Table1's second row cannot be read (see rows_left_out_when_unreadable);
        // the part with the first row passes over it, so next is null.
        let mut bytes = skip_if_missing!("V2003/testV2003.mdb");
        let pos = 27 * 4096 + 16;
        bytes[pos..pos + 2].copy_from_slice(&0x0FB8u16.to_le_bytes());
        let mut db = Database::open(bytes, None).unwrap();
        let rows = db
            .rows_from("Table1", RowPosition::default(), Some(1))
            .unwrap();
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.skipped, 1);
        assert_eq!(rows.next, None);
    }

    #[test]
    fn complex_columns_are_read_once() {
        let bytes = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        let mut db = Database::open(bytes, None).unwrap();
        db.rows_from("Table1", RowPosition::default(), Some(1))
            .unwrap();
        let columns = db.complex.len();
        assert_eq!(db.complex.values().filter(|c| c.is_some()).count(), 3);
        db.rows_from("Table1", RowPosition { page: 0, row: 1 }, Some(1))
            .unwrap();
        db.rows("Table1").unwrap();
        assert_eq!(db.complex.len(), columns);
    }

    #[test]
    fn position_numbers() {
        for position in [
            RowPosition { page: 0, row: 0 },
            RowPosition {
                page: 0,
                row: 65535,
            },
            RowPosition { page: 3, row: 7 },
            RowPosition {
                page: u32::MAX,
                row: u16::MAX,
            },
        ] {
            assert_eq!(number_position(position_number(position)), Some(position));
        }
        assert_eq!(position_number(RowPosition { page: 1, row: 2 }), 65538.0);
        for number in [-1.0, 0.5, f64::NAN, f64::INFINITY, 281_474_976_710_656.0] {
            assert_eq!(number_position(number), None, "{number}");
        }
    }

    #[test]
    fn csv_as_jetdb_export_writes_it() {
        // Every table, system tables included, is the CSV the library writes
        // for jetdb export, with the default options and with others.
        let other = CsvOptions {
            header: false,
            delimiter: ';',
            date_format: "%d.%m.%Y".to_string(),
            datetime_format: "%Y%m%d %H%M".to_string(),
            binary: jetdb::BinaryFormat::Octal,
            null: "NULL".to_string(),
            boolean_words: true,
            system_columns: true,
        };
        for file in [
            "V2003/testV2003.mdb",
            "V2007/complexDataTestV2007.accdb",
            "V2010/binIdxTestV2010.accdb",
        ] {
            let bytes = skip_if_missing!(file);
            let mut db = Database::open(bytes.clone(), None).unwrap();
            let mut reader = PageReader::open_reader(Cursor::new(bytes)).unwrap();
            let catalog = read_catalog(&mut reader).unwrap();
            let tables: Vec<&CatalogEntry> = catalog
                .iter()
                .filter(|e| e.object_type == ObjectType::Table)
                .collect();
            assert!(tables.len() > 5, "{file}");
            for entry in tables {
                let tdef = read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
                for options in [CsvOptions::default(), other.clone()] {
                    let mut expected = Vec::new();
                    write_csv(&mut reader, &catalog, &tdef, &options, &mut expected, None).unwrap();
                    assert_eq!(
                        db.csv(&entry.name, &options).unwrap(),
                        String::from_utf8(expected).unwrap(),
                        "{file} {}",
                        entry.name
                    );
                }
            }
        }
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
