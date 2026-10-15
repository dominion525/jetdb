//! The JavaScript interface: thin `wasm-bindgen` wrappers over [`crate::Database`].

use jetdb::format::ObjectType;
use jetdb::{FormObjectType, MacroSource, MacroStatement, MacroXmlElement};
use js_sys::{Array, ArrayBuffer, BigInt, Object, Reflect, Symbol, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{
    error_code, form_type_name, Cell, Column, Database, FormProperty, Index, Macro,
    PropertiesError, Property, PropertyGroup, Relationship,
};

// Types for the TypeScript declarations. Version, Column, Index, Relationship,
// Query, Module, Form, FormProperties, Macro, ObjectProperties and Rows are used
// through unchecked_return_type, and DdlDialect through unchecked_param_type.
// TablesOptions, RelationshipsOptions, QueriesOptions, MacrosOptions,
// FormOptions, PropertiesOptions and DdlOptions are extern types rather than
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
     * The size as Access shows it: in characters for a Text column, such as
     * 255, and in bytes for the other types.
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

/** The options of `Database.queries`. */
export interface QueriesOptions {
    /**
     * Include the system and hidden queries too, such as those Access makes
     * for forms and reports.
     */
    system?: boolean;
}

/** The options of `Database.macros`. */
export interface MacrosOptions {
    /**
     * Include the macros Access makes for itself and names with `~`, such as
     * the `~TMPCLPMacro` the macro designer makes on copy and paste.
     */
    system?: boolean;
}

/** The type of a saved query. `Ddl` is a data-definition query. */
export type QueryType =
    | "Select" | "MakeTable" | "Append" | "Update" | "Delete"
    | "Crosstab" | "Ddl" | "Passthrough" | "Union";

/** A saved query. */
export interface Query {
    name: string;
    type: QueryType;
}

/**
 * The type of a VBA module: a standard module, a class module, or the module
 * of a form or report, which VBA calls a document module.
 */
export type ModuleType = "Standard" | "Class" | "Document";

/** A VBA module. */
export interface Module {
    name: string;
    type: ModuleType;
}

/** Whether an object with a design is a form or a report. */
export type FormType = "Form" | "Report";

/** A form or report. */
export interface Form {
    name: string;
    type: FormType;
}

/** A control of a form or report, or one of its sections. */
export interface Control {
    name: string;
    /**
     * The control type, such as `TextBox` or `Detail`, or its number as
     * `0x....` for a type jetdb does not know.
     */
    type: string;
    /** The number of the control type, which stays the same as jetdb learns more names. */
    typeCode: number;
    properties: FormProperty[];
}

/**
 * A property of a form or report, or of one of its controls. `name` is the
 * property name, or its number as `0x....` for one jetdb does not know; `id`
 * is the number, which stays the same as jetdb learns more names.
 */
export interface FormProperty {
    id: number;
    name: string;
    value: Scalar;
}

/**
 * The design of a form or report: its own properties, and its controls and
 * sections. A property jetdb does not know is named by its number, as
 * `0x....`. A color is a number, `0x00BBGGRR` or negative for a system color,
 * as Access writes it in SaveAsText.
 */
export interface FormProperties {
    name: string;
    type: FormType;
    properties: FormProperty[];
    controls: Control[];
}

/** A name and its value, such as an argument of a macro action. */
export interface NameValue {
    name: string;
    value: string;
}

/** A parameter of a named data macro, with the attributes of its element. */
export interface MacroParameter {
    name: string;
    attributes: NameValue[];
}

/** Where a macro is. */
export type MacroSource =
    | { kind: "named"; name: string }
    | {
          kind: "embedded";
          objectType: FormType;
          objectName: string;
          /**
           * The control the event belongs to, or null for the form or report
           * itself and for a macro stored without XML.
           */
          control: string | null;
          /** The event, such as `OnClick`; empty for a macro stored without XML. */
          event: string;
      }
    | {
          kind: "data";
          table: string;
          event: string | null;
          name: string | null;
          parameters: MacroParameter[];
      };

/** A macro XML element jetdb does not interpret, as it is. */
export interface MacroXmlElement {
    name: string;
    attributes: NameValue[];
    text: string;
    children: MacroXmlElement[];
}

/** One statement of a macro. */
export type MacroStatement =
    | { kind: "action"; name: string; arguments: NameValue[] }
    | { kind: "comment"; text: string }
    | { kind: "if"; branches: { condition: string | null; statements: MacroStatement[] }[] }
    | { kind: "group"; description: string; statements: MacroStatement[] }
    | { kind: "submacro"; name: string; statements: MacroStatement[] }
    | {
          kind: "dataBlock";
          /** The record block, such as `ForEachRecord` or `EditRecord`. */
          type: string;
          data: NameValue[];
          statements: MacroStatement[];
      }
    | { kind: "unknown"; element: MacroXmlElement };

/**
 * A macro: where it is, its statements, and the XML and SaveAsText text
 * Access keeps for it.
 *
 * The XML is empty for a macro saved before Access 2010. For a data macro it
 * is the XML document the macro was stored in, which may hold other data
 * macros of the table too.
 *
 * The text is that of `jetdb macro show` or `jetdb macro embedded`. It is
 * null for a data macro, whose text `dataMacrosText` gives for its whole
 * table; for a named macro whose grid of actions could not be read; and for
 * an embedded macro stored without XML.
 */
export interface Macro {
    source: MacroSource;
    statements: MacroStatement[];
    xml: string;
    text: string | null;
}

/** The type of an object of a database. */
export type ObjectType =
    | "Table" | "Query" | "Form" | "Report" | "Macro" | "Module"
    | "LinkedTable" | "LinkedOdbcTable" | "Relationship" | "Container"
    | "Database" | "DatabaseProperty" | "UserInfo";

/** The options of `Database.properties`. */
export interface PropertiesOptions {
    /**
     * The type of the object, needed when objects of different types share
     * the name, such as a table and a form both named `Customers`.
     */
    type?: ObjectType;
}

/** The options of `Database.formProperties` and `Database.embeddedMacros`. */
export interface FormOptions {
    /**
     * Form or report, needed when a form and a report share the name. Without
     * it, the only form or report of the name is read.
     */
    type?: FormType;
}

/** The properties of an object, as Access keeps them. */
export interface ObjectProperties {
    /** The properties of the object itself. */
    object: Property[];
    /** The properties of each column of a table. */
    columns: PropertyGroup[];
    /** The additional groups of properties. */
    additional: PropertyGroup[];
}

/** A named group of properties, such as those of a column. */
export interface PropertyGroup {
    name: string;
    properties: Property[];
}

/** A property, with its value of the types a row has. */
export interface Property {
    name: string;
    value: Scalar;
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
 * - ComplexType: an array of the column's values in the row, empty when it
 *   has none: for multiple values, the values, each by the type of the
 *   values; for attachments, an `Attachment` for each file; for the version
 *   history of an append-only Long Text column, a `ColumnVersion` for each
 *   version, oldest first. A complex column of a kind jetdb does not know
 *   gives a number, the ID of its values in a hidden table.
 * - NULL: null.
 */
export type Value = Scalar | Scalar[] | Attachment[] | ColumnVersion[];

/** A value that is not of a complex column. */
export type Scalar = null | boolean | number | bigint | string | Uint8Array;

/** A file attached to an attachment column. */
export interface Attachment {
    /** The file name. */
    name: string | null;
    /** The file type Access gives the file, its extension without the dot. */
    type: string | null;
    /** The content of the file, or null if it could not be decoded. */
    data: Uint8Array | null;
    /** The FileURL Access keeps for the file, usually null. */
    url: string | null;
    /** The FileTimeStamp Access keeps for the file, usually null. */
    timestamp: string | null;
    /** The FileFlags Access keeps for the file, usually null. */
    flags: number | null;
}

/** A version of an append-only Long Text column. */
export interface ColumnVersion {
    /** The text of the column in this version. */
    value: string | null;
    /** When the version was written, as a Timestamp value is shown. */
    modified: string | null;
}

/** A row, keyed by the column names. */
export type Row = Record<string, Value>;

/** The rows of a table, as `Database.rows` returns them. */
export interface Rows {
    /**
     * The names of the columns in the order of the table, as `Database.columns`
     * lists them, also when there are no rows. The keys of a `Row` do not keep
     * this order: JavaScript puts a key that looks like an integer, such as
     * `"2019"`, before the others.
     */
    columns: string[];
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
 *   `FORM_NOT_FOUND`, `MACRO_NOT_FOUND`, `OBJECT_NOT_FOUND`: no object of
 *   that name.
 * - `INVALID_FILE`: not an Access database, or a broken one; the message
 *   says what is wrong.
 * - `INVALID_ARGUMENT`: a method was called with an argument of the wrong
 *   kind.
 * - `IO`: reading the bytes failed.
 */
export type ErrorCode =
    | "PASSWORD_REQUIRED" | "INVALID_PASSWORD" | "UNSUPPORTED_ENCRYPTION"
    | "TABLE_NOT_FOUND" | "QUERY_NOT_FOUND" | "MODULE_NOT_FOUND"
    | "FORM_NOT_FOUND" | "MACRO_NOT_FOUND" | "OBJECT_NOT_FOUND"
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

    // The password of Database.open, checked to be a string. An extern type
    // keeps the parameter optional, which unchecked_param_type would not.
    #[wasm_bindgen(typescript_type = "string")]
    pub type Password;

    #[wasm_bindgen(typescript_type = "RelationshipsOptions")]
    pub type RelationshipsOptions;

    #[wasm_bindgen(typescript_type = "QueriesOptions")]
    pub type QueriesOptions;

    #[wasm_bindgen(typescript_type = "MacrosOptions")]
    pub type MacrosOptions;

    #[wasm_bindgen(typescript_type = "FormOptions")]
    pub type FormOptions;

    #[wasm_bindgen(typescript_type = "PropertiesOptions")]
    pub type PropertiesOptions;

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
        password: Option<Password>,
    ) -> Result<JsDatabase, JsValue> {
        let password = match password.as_deref() {
            Some(value) => Some(string_arg(value, "password")?),
            None => None,
        };
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
    pub fn columns(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] table: JsValue,
    ) -> Result<Array, JsValue> {
        let columns = self
            .inner
            .columns(&string_arg(&table, "table")?)
            .map_err(to_js_error)?;
        Ok(columns.iter().map(column_object).collect())
    }

    /// The indexes of a table, without the foreign key references, which
    /// Access keeps as indexes of their own.
    #[wasm_bindgen(unchecked_return_type = "Index[]")]
    pub fn indexes(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] table: JsValue,
    ) -> Result<Array, JsValue> {
        let indexes = self
            .inner
            .indexes(&string_arg(&table, "table")?)
            .map_err(to_js_error)?;
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

    /// The saved queries, sorted by name, without the system and hidden
    /// ones. With `{ system: true }`, those are included too.
    #[wasm_bindgen(unchecked_return_type = "Query[]")]
    pub fn queries(&mut self, options: Option<QueriesOptions>) -> Result<Array, JsValue> {
        let include_system = option_bool(options.as_deref(), "system", false)?;
        let queries = self.inner.queries(include_system).map_err(to_js_error)?;
        Ok(queries
            .iter()
            .map(|q| {
                let object = Object::new();
                set(&object, "name", q.name.as_str().into());
                set(&object, "type", q.type_name.into());
                object
            })
            .collect())
    }

    /// The SQL of a saved query, system and hidden ones included, as Access
    /// shows it in SQL view.
    #[wasm_bindgen(js_name = querySql)]
    pub fn query_sql(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] name: JsValue,
    ) -> Result<String, JsValue> {
        self.inner
            .query_sql(&string_arg(&name, "name")?)
            .map_err(to_js_error)
    }

    /// The VBA modules, sorted by name: none when the database has no VBA,
    /// and none for an Access 97 file, whose VBA jetdb does not read.
    #[wasm_bindgen(unchecked_return_type = "Module[]")]
    pub fn modules(&mut self) -> Result<Array, JsValue> {
        let modules = self.inner.modules().map_err(to_js_error)?;
        Ok(modules
            .iter()
            .map(|m| {
                let object = Object::new();
                set(&object, "name", m.name.as_str().into());
                set(&object, "type", m.type_name.into());
                object
            })
            .collect())
    }

    /// The source of a VBA module, with the `Attribute` lines that Access
    /// keeps before it, as `jetdb vba show` prints it.
    #[wasm_bindgen(js_name = moduleSource)]
    pub fn module_source(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] name: JsValue,
    ) -> Result<String, JsValue> {
        self.inner
            .module_source(&string_arg(&name, "name")?)
            .map_err(to_js_error)
    }

    /// The forms and reports, sorted by name. None for an Access 97 file,
    /// whose form and report designs jetdb does not read.
    #[wasm_bindgen(unchecked_return_type = "Form[]")]
    pub fn forms(&mut self) -> Result<Array, JsValue> {
        let forms = self.inner.forms().map_err(to_js_error)?;
        Ok(forms
            .iter()
            .map(|f| {
                let object = Object::new();
                set(&object, "name", f.name.as_str().into());
                set(&object, "type", f.type_name.into());
                object
            })
            .collect())
    }

    /// The design of a form or report, as `jetdb form props` shows it. In an
    /// Access 97 file, whose designs jetdb does not read, every name is
    /// FORM_NOT_FOUND.
    #[wasm_bindgen(js_name = formProperties, unchecked_return_type = "FormProperties")]
    pub fn form_properties(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] name: JsValue,
        options: Option<FormOptions>,
    ) -> Result<Object, JsValue> {
        let name = string_arg(&name, "name")?;
        let form_type = form_type_option(options.as_deref())?;
        let design = self
            .inner
            .form_properties(&name, form_type)
            .map_err(|e| properties_error(e, &name))?;
        let object = Object::new();
        set(&object, "name", design.name.into());
        set(&object, "type", design.type_name.into());
        set(
            &object,
            "properties",
            form_property_array(design.properties).into(),
        );
        let controls: Array = design
            .controls
            .into_iter()
            .map(|c| {
                let control = Object::new();
                set(&control, "name", c.name.into());
                set(&control, "type", c.type_name.into());
                set(&control, "typeCode", c.type_code.into());
                set(
                    &control,
                    "properties",
                    form_property_array(c.properties).into(),
                );
                control
            })
            .collect();
        set(&object, "controls", controls.into());
        Ok(object)
    }

    /// The names of the named macros, sorted, without those Access makes for
    /// itself and names with `~`. With `{ system: true }`, those are included
    /// too.
    pub fn macros(&mut self, options: Option<MacrosOptions>) -> Result<Vec<String>, JsValue> {
        let include_system = option_bool(options.as_deref(), "system", false)?;
        self.inner.macros(include_system).map_err(to_js_error)
    }

    /// A named macro.
    #[wasm_bindgen(js_name = macro, unchecked_return_type = "Macro")]
    pub fn named_macro(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] name: JsValue,
    ) -> Result<Object, JsValue> {
        let m = self
            .inner
            .named_macro(&string_arg(&name, "name")?)
            .map_err(to_js_error)?;
        Ok(macro_object(m))
    }

    /// The macros embedded in the events of a form or report. In an Access 97
    /// file, whose designs jetdb does not read, every name is FORM_NOT_FOUND.
    #[wasm_bindgen(js_name = embeddedMacros, unchecked_return_type = "Macro[]")]
    pub fn embedded_macros(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] form: JsValue,
        options: Option<FormOptions>,
    ) -> Result<Array, JsValue> {
        let form = string_arg(&form, "form")?;
        let form_type = form_type_option(options.as_deref())?;
        let macros = self
            .inner
            .embedded_macros(&form, form_type)
            .map_err(|e| properties_error(e, &form))?;
        Ok(macros.into_iter().map(macro_object).collect())
    }

    /// The data macros of a table.
    #[wasm_bindgen(js_name = dataMacros, unchecked_return_type = "Macro[]")]
    pub fn data_macros(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] table: JsValue,
    ) -> Result<Array, JsValue> {
        let macros = self
            .inner
            .data_macros(&string_arg(&table, "table")?)
            .map_err(to_js_error)?;
        Ok(macros.into_iter().map(macro_object).collect())
    }

    /// The SaveAsText text of all the data macros of a table, as
    /// `jetdb macro data` prints it.
    #[wasm_bindgen(js_name = dataMacrosText)]
    pub fn data_macros_text(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] table: JsValue,
    ) -> Result<String, JsValue> {
        self.inner
            .data_macros_text(&string_arg(&table, "table")?)
            .map_err(to_js_error)
    }
    /// The properties of an object, such as a table or a query, as Access
    /// keeps them. When objects of different types share the name, the type
    /// option chooses one.
    #[wasm_bindgen(unchecked_return_type = "ObjectProperties")]
    pub fn properties(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] name: JsValue,
        options: Option<PropertiesOptions>,
    ) -> Result<Object, JsValue> {
        let name = string_arg(&name, "name")?;
        let object_type = match option_string(options.as_deref(), "type")? {
            Some(type_name) => Some(
                ObjectType::ALL
                    .into_iter()
                    .find(|t| t.to_string() == type_name)
                    .ok_or_else(|| {
                        let names = ObjectType::ALL.map(|t| t.to_string());
                        jetdb_error(
                            "INVALID_ARGUMENT",
                            &format!("option type must be one of {}", names.join(", ")),
                        )
                    })?,
            ),
            None => None,
        };
        let properties = self
            .inner
            .properties(&name, object_type)
            .map_err(|e| properties_error(e, &name))?;
        let object = Object::new();
        set(&object, "object", property_array(properties.object).into());
        set(&object, "columns", group_array(properties.columns).into());
        set(
            &object,
            "additional",
            group_array(properties.additional).into(),
        );
        Ok(object)
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

    /// The names of the columns, in the order of the table, and the rows of
    /// a table, each an object keyed by the column names, with the columns
    /// that `columns` returns, and the number of rows that could not be read
    /// and were left out.
    #[wasm_bindgen(unchecked_return_type = "Rows")]
    pub fn rows(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "string")] table: JsValue,
    ) -> Result<Object, JsValue> {
        let rows = self
            .inner
            .rows(&string_arg(&table, "table")?)
            .map_err(to_js_error)?;
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
        set(&object, "columns", names.iter().collect::<Array>().into());
        set(&object, "rows", objects.into());
        set(&object, "skipped", (rows.skipped as f64).into());
        Ok(object)
    }
}

/// A string argument. A `&str` parameter takes a value of any other type
/// without an error of its own: no argument fails in the generated glue, and
/// a number or an object makes the Wasm access memory out of bounds.
fn string_arg(value: &JsValue, name: &str) -> Result<String, JsValue> {
    value
        .as_string()
        .ok_or_else(|| jetdb_error("INVALID_ARGUMENT", &format!("{name} must be a string")))
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
    // One made in another realm, such as an iframe, is not an instance of
    // this realm's classes, which dyn_ref tests; its toStringTag is the same
    // in every realm. Uint8Array::new copies a typed array, and reads an
    // ArrayBuffer, of any realm.
    if matches!(
        to_string_tag(value).as_deref(),
        Some("Uint8Array" | "ArrayBuffer")
    ) {
        return Ok(Uint8Array::new(value).to_vec());
    }
    Err(jetdb_error(
        "INVALID_ARGUMENT",
        "bytes must be a Uint8Array or an ArrayBuffer",
    ))
}

/// The `Symbol.toStringTag` of an object, such as `"Uint8Array"`.
fn to_string_tag(value: &JsValue) -> Option<String> {
    if !value.is_object() {
        return None;
    }
    Reflect::get(value, &Symbol::to_string_tag())
        .ok()?
        .as_string()
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

/// The JavaScript error of `Database.properties` for the object `name`.
/// The `type` option of `formProperties` and `embeddedMacros`.
fn form_type_option(options: Option<&JsValue>) -> Result<Option<FormObjectType>, JsValue> {
    match option_string(options, "type")?.as_deref() {
        None => Ok(None),
        Some("Form") => Ok(Some(FormObjectType::Form)),
        Some("Report") => Ok(Some(FormObjectType::Report)),
        Some(_) => Err(jetdb_error(
            "INVALID_ARGUMENT",
            "option type must be one of Form, Report",
        )),
    }
}

fn properties_error(error: PropertiesError, name: &str) -> JsValue {
    match error {
        PropertiesError::File(e) => to_js_error(e),
        PropertiesError::NotFound => {
            jetdb_error("OBJECT_NOT_FOUND", &format!("object not found: {name}"))
        }
        PropertiesError::SeveralTypes(types) => {
            let names: Vec<String> = types.iter().map(ToString::to_string).collect();
            jetdb_error(
                "INVALID_ARGUMENT",
                &format!(
                    "objects of several types are named {name}: {}; choose one with the type option",
                    names.join(", ")
                ),
            )
        }
    }
}

fn macro_object(m: Macro) -> Object {
    let object = Object::new();
    set(&object, "source", source_object(m.def.source).into());
    set(
        &object,
        "statements",
        statement_array(m.def.statements).into(),
    );
    set(&object, "xml", m.def.xml.into());
    set(&object, "text", optional_string(m.text));
    object
}

fn source_object(source: MacroSource) -> Object {
    let object = Object::new();
    match source {
        MacroSource::Named { name } => {
            set(&object, "kind", "named".into());
            set(&object, "name", name.into());
        }
        MacroSource::Embedded {
            object_kind,
            object_name,
            control,
            event,
        } => {
            set(&object, "kind", "embedded".into());
            set(&object, "objectType", form_type_name(object_kind).into());
            set(&object, "objectName", object_name.into());
            set(&object, "control", optional_string(control));
            set(&object, "event", event.into());
        }
        MacroSource::Data {
            table,
            event,
            name,
            parameters,
        } => {
            set(&object, "kind", "data".into());
            set(&object, "table", table.into());
            set(&object, "event", optional_string(event));
            set(&object, "name", optional_string(name));
            let parameters: Array = parameters
                .into_iter()
                .map(|p| {
                    let parameter = Object::new();
                    set(&parameter, "name", p.name.into());
                    set(&parameter, "attributes", name_values(p.attributes).into());
                    parameter
                })
                .collect();
            set(&object, "parameters", parameters.into());
        }
    }
    object
}

fn statement_array(statements: Vec<MacroStatement>) -> Array {
    statements.into_iter().map(statement_object).collect()
}

fn statement_object(statement: MacroStatement) -> Object {
    let object = Object::new();
    match statement {
        MacroStatement::Action { name, arguments } => {
            set(&object, "kind", "action".into());
            set(&object, "name", name.into());
            let arguments = arguments.into_iter().map(|a| (a.name, a.value)).collect();
            set(&object, "arguments", name_values(arguments).into());
        }
        MacroStatement::Comment(text) => {
            set(&object, "kind", "comment".into());
            set(&object, "text", text.into());
        }
        MacroStatement::Conditional { branches } => {
            set(&object, "kind", "if".into());
            let branches: Array = branches
                .into_iter()
                .map(|b| {
                    let branch = Object::new();
                    set(&branch, "condition", optional_string(b.condition));
                    set(&branch, "statements", statement_array(b.statements).into());
                    branch
                })
                .collect();
            set(&object, "branches", branches.into());
        }
        MacroStatement::Group {
            description,
            statements,
        } => {
            set(&object, "kind", "group".into());
            set(&object, "description", description.into());
            set(&object, "statements", statement_array(statements).into());
        }
        MacroStatement::SubMacro { name, statements } => {
            set(&object, "kind", "submacro".into());
            set(&object, "name", name.into());
            set(&object, "statements", statement_array(statements).into());
        }
        MacroStatement::DataBlock {
            kind,
            data,
            statements,
        } => {
            set(&object, "kind", "dataBlock".into());
            set(&object, "type", kind.into());
            set(&object, "data", name_values(data).into());
            set(&object, "statements", statement_array(statements).into());
        }
        MacroStatement::Unknown(element) => {
            set(&object, "kind", "unknown".into());
            set(&object, "element", element_object(element).into());
        }
    }
    object
}

fn element_object(element: MacroXmlElement) -> Object {
    let object = Object::new();
    set(&object, "name", element.name.into());
    set(
        &object,
        "attributes",
        name_values(element.attributes).into(),
    );
    set(&object, "text", element.text.into());
    let children: Array = element.children.into_iter().map(element_object).collect();
    set(&object, "children", children.into());
    object
}

/// Name and value pairs as `{ name, value }` objects.
fn name_values(pairs: Vec<(String, String)>) -> Array {
    pairs
        .into_iter()
        .map(|(name, value)| {
            let object = Object::new();
            set(&object, "name", name.into());
            set(&object, "value", value.into());
            object
        })
        .collect()
}

fn optional_string(value: Option<String>) -> JsValue {
    value.map_or(JsValue::NULL, JsValue::from)
}
fn form_property_array(properties: Vec<FormProperty>) -> Array {
    properties
        .into_iter()
        .map(|p| {
            let object = Object::new();
            set(&object, "id", p.id.into());
            set(&object, "name", p.name.into());
            set(&object, "value", cell_value(p.value));
            object
        })
        .collect()
}

fn property_array(properties: Vec<Property>) -> Array {
    properties
        .into_iter()
        .map(|p| {
            let object = Object::new();
            set(&object, "name", p.name.into());
            set(&object, "value", cell_value(p.value));
            object
        })
        .collect()
}

fn group_array(groups: Vec<PropertyGroup>) -> Array {
    groups
        .into_iter()
        .map(|g| {
            let object = Object::new();
            set(&object, "name", g.name.into());
            set(&object, "properties", property_array(g.properties).into());
            object
        })
        .collect()
}

fn cell_value(cell: Cell) -> JsValue {
    match cell {
        Cell::Null => JsValue::NULL,
        Cell::Bool(v) => v.into(),
        Cell::Number(v) => v.into(),
        Cell::BigInt(v) => BigInt::from(v).into(),
        Cell::String(s) => s.into(),
        Cell::Bytes(bytes) => Uint8Array::from(bytes.as_slice()).into(),
        Cell::List(cells) => cells.into_iter().map(cell_value).collect::<Array>().into(),
        Cell::Record(fields) => {
            let object = Object::new();
            for (name, cell) in fields {
                set(&object, name, cell_value(cell));
            }
            object.into()
        }
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
