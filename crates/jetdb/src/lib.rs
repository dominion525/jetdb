#![doc = include_str!("lib_doc.md")]

pub mod catalog;
pub mod complex;
pub(crate) mod crypto;
pub mod csv;
pub mod data;
pub mod ddl;
pub mod encoding;
pub mod file;
pub mod form;
pub mod form2000;
pub mod format;
pub mod macro_action;
pub mod macro_def;
mod macro_text;
pub mod map;
pub mod money;
pub mod prop;
pub mod query;
pub mod relationship;
#[cfg(test)]
mod save_as_text;
pub(crate) mod storage;
pub mod table;
pub mod timestamp;
pub mod vba;

pub use catalog::{find_table, read_catalog, table_names, CatalogEntry};
pub use complex::{
    read_complex_column, Attachment, ComplexColumn, ComplexKind, ComplexValues, Version,
};
pub use csv::{write_csv, AttachmentWriter, BinaryFormat, CsvOptions};
pub use encoding::TextEncoding;
pub use file::{find_row, DbHeader, FileError, PageReader};
pub use format::{
    catalog_flags, column_flags, index_flags, index_type, ColumnType, FormatError, JetFormat,
    JetVersion, ObjectType, PageType, JET3, JET4,
};
pub use relationship::{
    read_relationships, read_relationships_in, relationship_flags, Relationship, RelationshipColumn,
};
pub use table::{
    read_table_def, ColumnDef, ForeignKeyReference, IndexColumn, IndexColumnOrder, IndexDef,
    TableDef,
};
// Kept so that code using the former name still compiles, with a warning.
#[allow(deprecated)]
pub use table::is_replication_column;

pub use data::{
    calculated_column_types, calculated_column_types_in, read_row_at, read_table_rows,
    read_table_rows_with, ReadResult, RowPosition, TableRow, TableRows, Value,
};
pub use form::{
    control_type_label, control_type_name, list_forms, list_forms_in, read_form_properties,
    read_form_properties_in, read_form_properties_of_type, read_form_stream, read_form_stream_in,
    read_form_stream_of_type, read_form_type_info, read_form_type_info_in,
    read_form_type_info_of_type, BlobProperty, BlobValue, ControlInfo, ControlProperties,
    FormEntry, FormObjectType, FormProperties, FormStream, FormTypeInfo, StreamKind,
};
pub use macro_action::{macro_action, macro_argument_value_name, MacroAction, MACRO_ACTIONS};
pub use macro_def::{
    list_macros, list_macros_in, read_data_macros, read_data_macros_in, read_embedded_macros,
    read_embedded_macros_in, read_embedded_macros_of_type, read_macro, read_macro_in,
    MacroArgument, MacroBranch, MacroDef, MacroEntry, MacroGrid, MacroGridRow, MacroParameter,
    MacroSource, MacroStatement, MacroXmlElement,
};
pub use macro_text::{data_macros_to_text, embedded_macro_to_text};
pub use prop::{
    entry_properties, read_object_properties, read_object_properties_of_type, ObjectProperties,
    PropMapType, Property, PropertyMap,
};
pub use query::{query_to_sql, read_queries, read_queries_in, QueryDef, QueryType};
pub use storage::{read_access_storage, AccessStorage};
pub use vba::{read_vba_project, read_vba_project_in, VbaModule, VbaModuleType, VbaProject};

#[cfg(test)]
mod tests {
    #[test]
    fn docs_lib_md_is_the_crate_documentation() {
        // docs/lib.md, read on GitHub, is a copy of lib_doc.md. It is outside
        // the crate, so the published package has no docs/lib.md, and the
        // test is skipped there.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/lib.md");
        let Ok(docs_lib_md) = std::fs::read_to_string(&path) else {
            eprintln!("SKIP: docs/lib.md not found: {}", path.display());
            return;
        };
        assert_eq!(docs_lib_md, include_str!("lib_doc.md"));
    }
}
