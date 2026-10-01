#![doc = include_str!("lib_doc.md")]

pub mod catalog;
pub(crate) mod crypto;
pub mod data;
pub mod ddl;
pub mod encoding;
pub mod file;
pub mod form;
pub mod format;
pub mod macro_action;
pub mod macro_def;
mod macro_text;
pub mod map;
pub mod money;
pub mod prop;
pub mod query;
pub mod relationship;
pub(crate) mod storage;
pub mod table;
pub mod timestamp;
pub mod vba;

pub use catalog::{read_catalog, table_names, CatalogEntry};
pub use file::{find_row, DbHeader, FileError, PageReader};
pub use format::{
    catalog_flags, column_flags, index_flags, index_type, ColumnType, FormatError, JetFormat,
    JetVersion, ObjectType, PageType, JET3, JET4,
};
pub use relationship::{read_relationships, relationship_flags, Relationship, RelationshipColumn};
pub use table::{
    is_replication_column, read_table_def, ColumnDef, ForeignKeyReference, IndexColumn,
    IndexColumnOrder, IndexDef, TableDef,
};

pub use data::{calculated_column_types, read_table_rows, ReadResult, Value};
pub use form::{
    control_type_name, list_forms, read_form_properties, read_form_stream, read_form_type_info,
    BlobProperty, BlobValue, ControlInfo, ControlProperties, FormEntry, FormObjectType,
    FormProperties, FormStream, FormTypeInfo, StreamKind,
};
pub use macro_action::{macro_action, macro_argument_value_name, MacroAction, MACRO_ACTIONS};
pub use macro_def::{
    list_macros, read_data_macros, read_embedded_macros, read_macro, MacroArgument, MacroBranch,
    MacroDef, MacroEntry, MacroGrid, MacroGridRow, MacroParameter, MacroSource, MacroStatement,
    MacroXmlElement,
};
pub use macro_text::{data_macros_to_text, embedded_macro_to_text};
pub use prop::{read_object_properties, ObjectProperties, PropMapType, Property, PropertyMap};
pub use query::{query_to_sql, read_queries, QueryDef, QueryType};
pub use vba::{read_vba_project, VbaModule, VbaModuleType, VbaProject};
