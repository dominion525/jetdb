//! The JavaScript interface: thin `wasm-bindgen` wrappers over [`crate::Database`].

use js_sys::{Array, ArrayBuffer, BigInt, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{error_code, Cell, Column, Database, Index, Relationship};

// Types for the TypeScript declarations. Version, Column, Index, Relationship
// and Rows are used through unchecked_return_type, and DdlDialect through
// unchecked_param_type. TablesOptions, RelationshipsOptions and DdlOptions
// are extern types rather than unchecked_param_type, which would make the
// parameter required.
#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** The database engine version. */
export type Version = "JET3" | "JET4" | "ACE12" | "ACE14" | "ACE15" | "ACE16" | "ACE17";

/** The options of `Database.tables`. */
export interface TablesOptions {
    /** Include the system and hidden tables too. */
    system?: boolean;
}

/** The type of a column, `Unknown` for a type jetdb does not know. */
export type ColumnType =
    | "Boolean" | "Byte" | "Int" | "Long" | "Money" | "Float" | "Double"
    | "Timestamp" | "Binary" | "Text" | "Ole" | "Memo" | "Guid" | "Numeric"
    | "ComplexType" | "BigInt" | "DateTimeExtended" | "Unknown";

/** A column of a table. */
export interface Column {
    name: string;
    /** For a calculated column, the type of its result, which its values have. */
    type: ColumnType;
    /**
     * The size in bytes as stored, such as 100 for a Text column of 50
     * characters in Jet4 and later, which store two bytes a character.
     */
    size: number;
    /**
     * Precision of a Numeric column; 0 for the other types and for calculated
     * columns, whose values each carry their own scale.
     */
    precision: number;
    /** Scale of a Numeric column; 0 for the other types and for calculated columns. */
    scale: number;
    /** An AutoNumber column: a Long or a GUID that Access fills in. */
    autoNumber: boolean;
    /** A calculated column (Access 2010 and later). */
    calculated: boolean;
}

/** An index of a table. */
export interface Index {
    name: string;
    primaryKey: boolean;
    columns: IndexColumn[];
    /** No two rows have the same values in the index columns. */
    unique: boolean;
    /** Rows whose index columns are all NULL are left out of the index. */
    ignoreNulls: boolean;
    /** The index columns cannot be NULL. */
    required: boolean;
}

/** A column of an index. */
export interface IndexColumn {
    name: string;
    descending: boolean;
}

/** The options of `Database.relationships`. */
export interface RelationshipsOptions {
    /** Include the relationships of system and hidden tables too. */
    system?: boolean;
}

/** A relationship between two tables. */
export interface Relationship {
    name: string;
    /** The table whose columns refer to the other table. */
    fromTable: string;
    /** The table referred to. */
    toTable: string;
    columns: RelationshipColumn[];
    /** Access keeps the rows of the two tables consistent. */
    referentialIntegrity: boolean;
    cascadeUpdate: boolean;
    cascadeDelete: boolean;
}

/** A pair of columns of a relationship. */
export interface RelationshipColumn {
    /** The column of `fromTable`. */
    from: string;
    /** The column of `toTable` it refers to. */
    to: string;
}

/** The SQL dialect of `Database.ddl`. */
export type DdlDialect = "sqlite" | "postgres" | "mysql" | "access";

/** The options of `Database.ddl`. */
export interface DdlOptions {
    /** Only this table, which may be a system table. */
    table?: string;
    /** Include the indexes (CREATE INDEX). Default true. */
    indexes?: boolean;
    /**
     * Include the foreign keys of the relationships. Default true. With false,
     * the relationships are not read, so a file whose relationships cannot be
     * read still gives the DDL of its tables.
     */
    relations?: boolean;
}

/**
 * A value in a row, by the type of its column:
 *
 * - Byte, Int, Long, Float and Double: a number. A Float is the shortest
 *   decimal that gives back the stored single-precision value, as Access shows
 *   it, so 1.1 stays 1.1; `Math.fround(value)` gives the stored value itself.
 * - BigInt: a bigint.
 * - Boolean: a boolean.
 * - Text, Memo, GUID, Money and Numeric: a string, so that no digits are lost.
 * - Timestamp: a string such as `2021-06-14`, or `2021-06-14 22:45:12` when
 *   the time is not midnight, rounded to the second as Access shows it.
 * - DateTimeExtended: a string with all its digits, such as
 *   `2021-06-14 22:45:12.3456789`, or the stored bytes when they cannot be
 *   read as a date and time.
 * - Binary and OLE: a Uint8Array, as are the values of a column type that
 *   jetdb does not know.
 * - ComplexType (attachments, multiple values, version history): a number,
 *   the ID of the column's values in a hidden table.
 * - NULL: null.
 */
export type Value = null | boolean | number | bigint | string | Uint8Array;

/** A row, keyed by the column names. */
export type Row = Record<string, Value>;

/** The rows of a table, as `Database.rows` returns them. */
export interface Rows {
    rows: Row[];
    /** The number of rows that could not be read and were left out. */
    skipped: number;
}

/**
 * The kind of a `JetdbError`, one for each thing a caller may do about it:
 *
 * - `PASSWORD_REQUIRED`: the file is password-protected and no password was
 *   given.
 * - `INVALID_PASSWORD`: the password is wrong.
 * - `UNSUPPORTED_ENCRYPTION`: the file is encrypted in a way jetdb cannot
 *   read.
 * - `TABLE_NOT_FOUND`, `QUERY_NOT_FOUND`, `MODULE_NOT_FOUND`,
 *   `FORM_NOT_FOUND`, `MACRO_NOT_FOUND`: no object of that name.
 * - `INVALID_FILE`: not an Access database, or a broken one; the message
 *   says what is wrong.
 * - `INVALID_ARGUMENT`: a method was called with an argument of the wrong
 *   kind.
 * - `IO`: reading the bytes failed.
 */
export type ErrorCode =
    | "PASSWORD_REQUIRED" | "INVALID_PASSWORD" | "UNSUPPORTED_ENCRYPTION"
    | "TABLE_NOT_FOUND" | "QUERY_NOT_FOUND" | "MODULE_NOT_FOUND"
    | "FORM_NOT_FOUND" | "MACRO_NOT_FOUND"
    | "INVALID_FILE" | "INVALID_ARGUMENT" | "IO";

/** The error that the methods of `Database` throw. */
export interface JetdbError extends Error {
    name: "JetdbError";
    code: ErrorCode;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "TablesOptions")]
    pub type TablesOptions;

    #[wasm_bindgen(typescript_type = "RelationshipsOptions")]
    pub type RelationshipsOptions;

    #[wasm_bindgen(typescript_type = "DdlOptions")]
    pub type DdlOptions;

    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

/// Prints a panic to the console before the Wasm traps, as JavaScript sees no
/// more of it than `RuntimeError: unreachable`.
#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| console_error(&panic_message(info))));
}

/// Where and why the panic happened.
fn panic_message(info: &std::panic::PanicHookInfo) -> String {
    format!("jetdb-wasm {info}")
}

/// A Microsoft Access database opened from its bytes.
#[wasm_bindgen(js_name = Database)]
pub struct JsDatabase {
    inner: Database,
}

#[wasm_bindgen(js_class = Database)]
impl JsDatabase {
    /// Opens a database from its bytes, as a Uint8Array (a Node.js Buffer is
    /// one) or an ArrayBuffer, with the password of a password-protected
    /// `.accdb`.
    pub fn open(
        #[wasm_bindgen(unchecked_param_type = "Uint8Array | ArrayBuffer")] bytes: JsValue,
        password: Option<String>,
    ) -> Result<JsDatabase, JsValue> {
        let inner = Database::open(bytes_of(&bytes)?, password.as_deref()).map_err(to_js_error)?;
        Ok(JsDatabase { inner })
    }

    /// The database engine version: `JET3`, `JET4`, `ACE12`, `ACE14`,
    /// `ACE15`, `ACE16`, or `ACE17`.
    #[wasm_bindgen(unchecked_return_type = "Version")]
    pub fn version(&self) -> String {
        self.inner.version().to_string()
    }

    /// The names of the user tables, sorted. With `{ system: true }`, the
    /// system and hidden tables are included too.
    pub fn tables(&mut self, options: Option<TablesOptions>) -> Result<Vec<String>, JsValue> {
        let include_system = option_bool(options.as_deref(), "system", false)?;
        self.inner.tables(include_system).map_err(to_js_error)
    }

    /// The columns of a table in the order Access shows them, without the
    /// columns Access maintains and hides in user tables.
    #[wasm_bindgen(unchecked_return_type = "Column[]")]
    pub fn columns(&mut self, table: &str) -> Result<Array, JsValue> {
        let columns = self.inner.columns(table).map_err(to_js_error)?;
        Ok(columns.iter().map(column_object).collect())
    }

    /// The indexes of a table, without the foreign key references, which
    /// Access keeps as indexes of their own.
    #[wasm_bindgen(unchecked_return_type = "Index[]")]
    pub fn indexes(&mut self, table: &str) -> Result<Array, JsValue> {
        let indexes = self.inner.indexes(table).map_err(to_js_error)?;
        Ok(indexes.iter().map(index_object).collect())
    }

    /// The relationships between tables, sorted by name, without those of
    /// system and hidden tables. With `{ system: true }`, those are included
    /// too.
    #[wasm_bindgen(unchecked_return_type = "Relationship[]")]
    pub fn relationships(
        &mut self,
        options: Option<RelationshipsOptions>,
    ) -> Result<Array, JsValue> {
        let include_system = option_bool(options.as_deref(), "system", false)?;
        let relationships = self
            .inner
            .relationships(include_system)
            .map_err(to_js_error)?;
        Ok(relationships.iter().map(relationship_object).collect())
    }

    /// The DDL of the user tables in a SQL dialect, as `jetdb schema --ddl`
    /// writes it: CREATE TABLE, CREATE INDEX and the foreign keys.
    pub fn ddl(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "DdlDialect")] dialect: JsValue,
        options: Option<DdlOptions>,
    ) -> Result<String, JsValue> {
        let dialect = dialect
            .as_string()
            .and_then(|name| jetdb::ddl::dialect(&name))
            .ok_or_else(|| {
                jetdb_error(
                    "INVALID_ARGUMENT",
                    &format!(
                        "dialect must be one of {}",
                        jetdb::ddl::DIALECT_NAMES.join(", ")
                    ),
                )
            })?;
        let options = options.as_deref();
        let table = option_string(options, "table")?;
        let indexes = option_bool(options, "indexes", true)?;
        let relations = option_bool(options, "relations", true)?;
        self.inner
            .ddl(dialect, table.as_deref(), indexes, relations)
            .map_err(to_js_error)
    }

    /// The rows of a table, each an object keyed by the column names, with
    /// the columns that `columns` returns, and the number of rows that could
    /// not be read and were left out.
    #[wasm_bindgen(unchecked_return_type = "Rows")]
    pub fn rows(&mut self, table: &str) -> Result<Object, JsValue> {
        let rows = self.inner.rows(table).map_err(to_js_error)?;
        let names: Vec<JsValue> = rows.columns.iter().map(|c| JsValue::from_str(c)).collect();
        let objects: Array = rows
            .rows
            .into_iter()
            .map(|row| {
                let entries: Array = names
                    .iter()
                    .zip(row)
                    .map(|(name, cell)| Array::of2(name, &cell_value(cell)))
                    .collect();
                // fromEntries defines each key as an own property, even a
                // column named __proto__, which assigning would not.
                Object::from_entries(&entries).expect("an object from entries")
            })
            .collect();
        let object = Object::new();
        set(&object, "rows", objects.into());
        set(&object, "skipped", (rows.skipped as f64).into());
        Ok(object)
    }
}

/// The bytes of a Uint8Array or an ArrayBuffer. A `Vec<u8>` parameter would
/// take any other value, an ArrayBuffer included, as no bytes at all.
fn bytes_of(value: &JsValue) -> Result<Vec<u8>, JsValue> {
    if let Some(array) = value.dyn_ref::<Uint8Array>() {
        return Ok(array.to_vec());
    }
    if let Some(buffer) = value.dyn_ref::<ArrayBuffer>() {
        return Ok(Uint8Array::new(buffer).to_vec());
    }
    Err(jetdb_error(
        "INVALID_ARGUMENT",
        "bytes must be a Uint8Array or an ArrayBuffer",
    ))
}

fn to_js_error(e: jetdb::FileError) -> JsValue {
    jetdb_error(error_code(&e), &e.to_string())
}

/// A JavaScript Error named JetdbError, with the stable `code` of the error
/// for a caller to tell the kinds apart without reading the message.
fn jetdb_error(code: &str, message: &str) -> JsValue {
    let error = js_sys::Error::new(message);
    error.set_name("JetdbError");
    set(&error, "code", code.into());
    error.into()
}

fn column_object(column: &Column) -> Object {
    let object = Object::new();
    set(&object, "name", column.name.as_str().into());
    set(&object, "type", column.type_name.as_str().into());
    set(&object, "size", column.size.into());
    set(&object, "precision", column.precision.into());
    set(&object, "scale", column.scale.into());
    set(&object, "autoNumber", column.auto_number.into());
    set(&object, "calculated", column.calculated.into());
    object
}

fn index_object(index: &Index) -> Object {
    let columns: Array = index
        .columns
        .iter()
        .map(|c| {
            let object = Object::new();
            set(&object, "name", c.name.as_str().into());
            set(&object, "descending", c.descending.into());
            object
        })
        .collect();
    let object = Object::new();
    set(&object, "name", index.name.as_str().into());
    set(&object, "primaryKey", index.primary_key.into());
    set(&object, "columns", columns.into());
    set(&object, "unique", index.unique.into());
    set(&object, "ignoreNulls", index.ignore_nulls.into());
    set(&object, "required", index.required.into());
    object
}

fn relationship_object(relationship: &Relationship) -> Object {
    let columns: Array = relationship
        .columns
        .iter()
        .map(|c| {
            let object = Object::new();
            set(&object, "from", c.from.as_str().into());
            set(&object, "to", c.to.as_str().into());
            object
        })
        .collect();
    let object = Object::new();
    set(&object, "name", relationship.name.as_str().into());
    set(
        &object,
        "fromTable",
        relationship.from_table.as_str().into(),
    );
    set(&object, "toTable", relationship.to_table.as_str().into());
    set(&object, "columns", columns.into());
    set(
        &object,
        "referentialIntegrity",
        relationship.referential_integrity.into(),
    );
    set(&object, "cascadeUpdate", relationship.cascade_update.into());
    set(&object, "cascadeDelete", relationship.cascade_delete.into());
    object
}

fn cell_value(cell: Cell) -> JsValue {
    match cell {
        Cell::Null => JsValue::NULL,
        Cell::Bool(v) => v.into(),
        Cell::Number(v) => v.into(),
        Cell::BigInt(v) => BigInt::from(v).into(),
        Cell::String(s) => s.into(),
        Cell::Bytes(bytes) => Uint8Array::from(bytes.as_slice()).into(),
    }
}

/// Sets a property on an object made here, which cannot fail.
fn set(object: &Object, key: &str, value: JsValue) {
    Reflect::set(object, &JsValue::from_str(key), &value).expect("set a property");
}

/// Reads the option `name`, which is `None` when absent, undefined or null.
fn option_value(options: Option<&JsValue>, name: &str) -> Result<Option<JsValue>, JsValue> {
    let Some(options) = options else {
        return Ok(None);
    };
    let value = Reflect::get(options, &JsValue::from_str(name))
        .map_err(|_| jetdb_error("INVALID_ARGUMENT", &format!("cannot read option {name}")))?;
    Ok((!value.is_undefined() && !value.is_null()).then_some(value))
}

/// Reads the boolean option `name`, which is `default` when absent.
fn option_bool(options: Option<&JsValue>, name: &str, default: bool) -> Result<bool, JsValue> {
    let Some(value) = option_value(options, name)? else {
        return Ok(default);
    };
    value.as_bool().ok_or_else(|| {
        jetdb_error(
            "INVALID_ARGUMENT",
            &format!("option {name} must be a boolean"),
        )
    })
}

/// Reads the string option `name`, which is `None` when absent.
fn option_string(options: Option<&JsValue>, name: &str) -> Result<Option<String>, JsValue> {
    let Some(value) = option_value(options, name)? else {
        return Ok(None);
    };
    value.as_string().map(Some).ok_or_else(|| {
        jetdb_error(
            "INVALID_ARGUMENT",
            &format!("option {name} must be a string"),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::panic;
    use std::sync::{Arc, Mutex};

    use super::panic_message;

    #[test]
    fn panic_message_has_the_location_and_the_message() {
        let printed = Arc::new(Mutex::new(String::new()));
        let sink = printed.clone();
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            *sink.lock().unwrap() = panic_message(info);
        }));
        let line = line!() + 1;
        let result = panic::catch_unwind(|| panic!("broken page {}", 42));
        panic::set_hook(previous);

        assert!(result.is_err());
        let printed = printed.lock().unwrap();
        assert!(printed.starts_with("jetdb-wasm panicked at "), "{printed}");
        assert!(printed.contains(&format!("js.rs:{line}:")), "{printed}");
        assert!(printed.ends_with("broken page 42"), "{printed}");
    }
}
