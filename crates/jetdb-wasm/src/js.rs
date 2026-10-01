//! The JavaScript interface: thin `wasm-bindgen` wrappers over [`crate::Database`].

use js_sys::{Array, BigInt, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::{Cell, Column, Database, Index};

// Types for the TypeScript declarations. Version and Column are used through
// unchecked_return_type. TablesOptions is an extern type rather than
// unchecked_param_type, which would make the parameter required.
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

/**
 * A value in a row. Byte, Int, Long, Float and Double are numbers, BigInt is
 * a bigint, Text, Memo, GUID, Money, Numeric and DateTimeExtended are strings,
 * a Timestamp is a string such as `2021-06-14` or `2021-06-14 22:45:12`, and
 * Binary and OLE are bytes.
 */
export type Value = null | boolean | number | bigint | string | Uint8Array;

/** A row, keyed by the column names. */
export type Row = Record<string, Value>;
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "TablesOptions")]
    pub type TablesOptions;

    #[wasm_bindgen(js_namespace = console)]
    fn warn(message: &str);
}

/// A Microsoft Access database opened from its bytes.
#[wasm_bindgen(js_name = Database)]
pub struct JsDatabase {
    inner: Database,
}

#[wasm_bindgen(js_class = Database)]
impl JsDatabase {
    /// Opens a database from its bytes, with the password of a
    /// password-protected `.accdb`.
    pub fn open(bytes: Vec<u8>, password: Option<String>) -> Result<JsDatabase, JsError> {
        let inner = Database::open(bytes, password.as_deref()).map_err(to_js_error)?;
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
    pub fn tables(&mut self, options: Option<TablesOptions>) -> Result<Vec<String>, JsError> {
        let include_system = option_bool(options.as_deref(), "system")?;
        self.inner.tables(include_system).map_err(to_js_error)
    }

    /// The columns of a table in the order Access shows them, without the
    /// columns Access maintains and hides in user tables.
    #[wasm_bindgen(unchecked_return_type = "Column[]")]
    pub fn columns(&mut self, table: &str) -> Result<Array, JsError> {
        let columns = self.inner.columns(table).map_err(to_js_error)?;
        Ok(columns.iter().map(column_object).collect())
    }

    /// The indexes of a table, without the foreign key references, which
    /// Access keeps as indexes of their own.
    #[wasm_bindgen(unchecked_return_type = "Index[]")]
    pub fn indexes(&mut self, table: &str) -> Result<Array, JsError> {
        let indexes = self.inner.indexes(table).map_err(to_js_error)?;
        Ok(indexes.iter().map(index_object).collect())
    }

    /// The rows of a table, each an object keyed by the column names, with
    /// the columns that `columns` returns. Rows that cannot be read are left
    /// out with a warning on the console, as `jetdb export` warns of them.
    #[wasm_bindgen(unchecked_return_type = "Row[]")]
    pub fn rows(&mut self, table: &str) -> Result<Array, JsError> {
        let rows = self.inner.rows(table).map_err(to_js_error)?;
        if rows.skipped > 0 {
            warn(&format!(
                "{table}: {} row(s) skipped due to parse errors",
                rows.skipped
            ));
        }
        let names: Vec<JsValue> = rows.columns.iter().map(|c| JsValue::from_str(c)).collect();
        Ok(rows
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
            .collect())
    }
}

fn to_js_error(e: jetdb::FileError) -> JsError {
    JsError::new(&e.to_string())
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

/// Reads the boolean option `name`, which is `false` when absent.
fn option_bool(options: Option<&JsValue>, name: &str) -> Result<bool, JsError> {
    let Some(options) = options else {
        return Ok(false);
    };
    let value = Reflect::get(options, &JsValue::from_str(name))
        .map_err(|_| JsError::new(&format!("cannot read option {name}")))?;
    if value.is_undefined() || value.is_null() {
        return Ok(false);
    }
    value
        .as_bool()
        .ok_or_else(|| JsError::new(&format!("option {name} must be a boolean")))
}
