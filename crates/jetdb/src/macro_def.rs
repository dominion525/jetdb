//! Macro object extraction (Access "UI Macros" / named macro objects).
//!
//! Named macros live in the same `MSysAccessStorage` virtual-filesystem tree
//! that Forms/Reports use (see `storage.rs`), under a `Scripts` folder (the
//! historical DAO container name for macros) -- one numbered subfolder per
//! macro, each holding a `Blob` stream, discovered via the exact same
//! `DirData` mapping `form.rs`'s `list_forms` uses for Forms/Reports.
//!
//! Modern Access (2010+, the embedded/"UI Macro" redesign) additionally
//! mirrors a macro's full logic as a `UserInterfaceMacro` XML document inside
//! that same Blob, chunked into ~253-character pieces, each chunk re-prefixed
//! with a literal `_AXL:` marker -- this is also what `Application.SaveAsText`
//! reproduces as repeated `Comment ="_AXL:..."` blocks in its exported text.
//! `extract_axl_xml` locates those chunks by the 2-byte length field
//! immediately preceding each marker and reassembles the XML, which
//! [`read_macro`] turns into a [`MacroDef`].
//!
//! Embedded macros (set on a form, report, or control event) are stored the
//! same way inside the form's or report's own `Blob` stream, one XML document
//! per macro, each starting with its own `<?xml` declaration. The document
//! element's `For` attribute names the control (absent for the form or report
//! itself) and `Event` names the event. [`read_embedded_macros`] returns them.
//!
//! Data macros (Access 2010+) are not in `MSysAccessStorage`: a table's data
//! macros are an XML document in the `LvExtra` column of the table's
//! `MSysObjects` row, either a single `DataMacro` element or several wrapped in
//! `DataMacros`. [`read_data_macros`] returns them.
//!
//! Elements this module knows become dedicated [`MacroStatement`] variants;
//! any other element is kept as [`MacroStatement::Unknown`] with its name,
//! attributes, text, and children, so nothing in the XML is dropped.

use crate::catalog::{read_catalog, CatalogEntry};
use crate::data::{read_table_rows, Value};
use crate::encoding::TextEncoding;
use crate::file::{FileError, PageReader};
use crate::form::{self, FormObjectType, StreamKind};
use crate::format::{ObjectType, CATALOG_PAGE};
use crate::macro_action::{macro_action, macro_argument_value_name};
use crate::storage::{self, AccessStorage};
use crate::table::{column_index, read_table_def, TableDef};

/// One named macro, as listed under `MSysAccessStorage`'s `Scripts` folder.
///
/// Includes Access-internal artifacts (e.g. `~TMPCLPMacro`, a clipboard
/// scratch macro the macro designer creates on copy/paste) -- callers that
/// want the same visible set Access's Navigation Pane shows should filter
/// names starting with `~`, matching how callers already treat `~sq_...`
/// internal queries.
#[derive(Debug, Clone)]
pub struct MacroEntry {
    pub name: String,
}

/// A macro read from its XML definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDef {
    /// Where the macro comes from.
    pub source: MacroSource,
    /// The macro's statements in document order.
    ///
    /// For a macro stored without XML they are read from the grid: a row's
    /// comment becomes a [`MacroStatement::Comment`], its action a
    /// [`MacroStatement::Action`] named with [`macro_action`] and its argument
    /// names (a value that is one of named choices by
    /// [`macro_argument_value_name`]), a row with a macro name starts a
    /// [`MacroStatement::SubMacro`], and a condition starts a
    /// [`MacroStatement::Conditional`] that the
    /// following `...` rows continue. An action whose number is not in that
    /// table is named by its number, and so is an argument beyond its
    /// argument names.
    pub statements: Vec<MacroStatement>,
    /// The XML definition the statements were read from, or empty for a macro
    /// stored without XML.
    pub xml: String,
    /// The macro grid of a named or embedded macro (see [`MacroGrid`]), or
    /// `None` for data macros.
    pub grid: Option<MacroGrid>,
}

/// The rows of a named or embedded macro as stored in its binary grid.
///
/// Every named macro has this grid, including macros saved before Access 2010
/// that have no XML. It is the classic macro sheet: each row has an optional
/// macro name, condition, comment, and an action with up to ten arguments.
/// Macros written by newer Access versions store `If` blocks as conditions and
/// internal `SetLocalVar` rows here, and their XML as `_AXL:` comment rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroGrid {
    /// The first four bytes of the grid, which differ with the columns shown
    /// in the macro designer (0, 1, or 3 in the files examined).
    pub columns_shown: u32,
    /// The header string: `33` in grids with UTF-16LE row strings (`23` in the
    /// macro designer's clipboard macro), and `22` or empty in grids written
    /// by Access 97.
    pub header: String,
    pub rows: Vec<MacroGridRow>,
}

/// One row of a [`MacroGrid`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroGridRow {
    /// The row number in the macro sheet. Empty rows are not stored.
    pub row: u16,
    pub macro_name: Option<String>,
    pub condition: Option<String>,
    pub comment: Option<String>,
    /// The action number, or 0 for a row without an action.
    pub action_code: u16,
    /// The ten argument slots, `None` where an argument is empty.
    pub arguments: Vec<Option<String>>,
}

/// Where a [`MacroDef`] comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroSource {
    /// A named macro object.
    Named { name: String },
    /// A macro embedded in a form or report event.
    Embedded {
        object_kind: FormObjectType,
        object_name: String,
        /// The control the event belongs to, or `None` for the form or
        /// report itself.
        control: Option<String>,
        event: String,
    },
    /// A data macro of a table.
    Data {
        table: String,
        /// The table event (`Event` attribute), if any.
        event: Option<String>,
        /// The macro name (`Name` attribute), if any.
        name: Option<String>,
        /// The parameters of a named data macro (`Parameter` elements of
        /// `Parameters`), in document order.
        parameters: Vec<MacroParameter>,
    },
}

/// A parameter of a named data macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroParameter {
    /// The `Name` attribute.
    pub name: String,
    /// The other attributes of the `Parameter` element, in document order.
    pub attributes: Vec<(String, String)>,
}

/// One statement of a macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroStatement {
    /// An action and its arguments, in document order.
    Action {
        name: String,
        arguments: Vec<MacroArgument>,
    },
    /// A comment line.
    Comment(String),
    /// An `If` / `ElseIf` / `Else` block. The `Else` branch has no condition.
    Conditional { branches: Vec<MacroBranch> },
    /// A group of statements (`StatementGroup`).
    Group {
        description: String,
        statements: Vec<MacroStatement>,
    },
    /// A submacro (`SubMacro`).
    SubMacro {
        name: String,
        statements: Vec<MacroStatement>,
    },
    /// A record block of a data macro, such as `ForEachRecord` or
    /// `EditRecord`: an element whose children are an optional `Data` element
    /// and a `Statements` element. `data` holds the `Data` element's children
    /// as name and text pairs.
    DataBlock {
        kind: String,
        data: Vec<(String, String)>,
        statements: Vec<MacroStatement>,
    },
    /// An element this module does not interpret, kept as-is.
    Unknown(MacroXmlElement),
}

/// An argument of a [`MacroStatement::Action`]. The value is the XML text as
/// stored, including any leading or trailing whitespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroArgument {
    pub name: String,
    pub value: String,
}

/// One branch of a [`MacroStatement::Conditional`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroBranch {
    /// The branch condition, or `None` for `Else`.
    pub condition: Option<String>,
    pub statements: Vec<MacroStatement>,
}

/// A macro XML element kept without interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroXmlElement {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    /// The element's text. Whitespace-only text between child elements is
    /// not kept.
    pub text: String,
    pub children: Vec<MacroXmlElement>,
}

impl MacroXmlElement {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn child(&self, name: &str) -> Option<&MacroXmlElement> {
        self.children.iter().find(|c| c.name == name)
    }
}

/// List every named macro in the database.
///
/// Access 97 databases have no `Scripts` folder; their macros are the
/// macro rows of `MSysObjects`.
pub fn list_macros(reader: &mut PageReader) -> Result<Vec<MacroEntry>, FileError> {
    let storage = storage::read_access_storage(reader)?;
    list_macros_in(reader, &storage)
}

/// [`list_macros`] in the storage already read.
pub fn list_macros_in(
    reader: &mut PageReader,
    storage: &AccessStorage,
) -> Result<Vec<MacroEntry>, FileError> {
    if let Some(mapping) = scripts_dir_mapping(&storage.entries) {
        return Ok(mapping
            .into_iter()
            .map(|(name, _storage_num)| MacroEntry { name })
            .collect());
    }
    Ok(msysobjects_rows(reader, MSYSOBJECTS_TYPE_MACRO)?
        .into_iter()
        .map(|(name, _extra)| MacroEntry { name })
        .collect())
}

/// Read the named macro `name`.
///
/// A macro stored without XML (saved before Access 2010) is returned with an
/// empty [`MacroDef::xml`], its content in [`MacroDef::grid`], and statements
/// read from the grid (see [`MacroDef::statements`]).
///
/// Returns [`FileError::MacroNotFound`] if there is no such macro.
pub fn read_macro(reader: &mut PageReader, name: &str) -> Result<MacroDef, FileError> {
    let storage = storage::read_access_storage(reader)?;
    read_macro_in(reader, &storage, name)
}

/// [`read_macro`] in the storage already read.
pub fn read_macro_in(
    reader: &mut PageReader,
    storage: &AccessStorage,
    name: &str,
) -> Result<MacroDef, FileError> {
    let entries = &storage.entries;
    let source = MacroSource::Named {
        name: name.to_string(),
    };
    let not_found = || FileError::MacroNotFound {
        name: name.to_string(),
    };

    if scripts_dir_mapping(entries).is_none() {
        // Access 97: the grid is the LvExtra of the macro's MSysObjects row.
        let (_, extra) = msysobjects_rows(reader, MSYSOBJECTS_TYPE_MACRO)?
            .into_iter()
            .find(|(n, _)| n == name)
            .ok_or_else(not_found)?;
        let grid = parse_macro_grid(&extra.unwrap_or_default(), reader.text_encoding())?;
        return Ok(macro_of_grid(source, grid));
    }

    let blob = find_macro_blob(entries, name)?.ok_or_else(not_found)?;
    macro_of_blob(source, blob)
}

/// The macro of `source` stored as a grid alone, without XML: its statements
/// are read from the grid.
fn macro_of_grid(source: MacroSource, grid: MacroGrid) -> MacroDef {
    MacroDef {
        source,
        statements: statements_from_grid(&grid),
        xml: String::new(),
        grid: Some(grid),
    }
}

/// The macro of `source` in `blob`: a grid that holds the XML of the macro as
/// `_AXL:` comment rows, or a grid alone.
fn macro_of_blob(source: MacroSource, blob: &[u8]) -> Result<MacroDef, FileError> {
    match extract_axl_xml(blob) {
        Some(xml) => {
            let root = parse_xml_tree(&xml)?;
            Ok(MacroDef {
                source,
                statements: statements_of(&root),
                grid: parse_macro_grid(blob, TextEncoding::Unicode).ok(),
                xml,
            })
        }
        None => Ok(macro_of_grid(
            source,
            parse_macro_grid(blob, TextEncoding::Unicode)?,
        )),
    }
}

/// Read the embedded macros of the form or report `object_name`, in the order
/// they appear in its `Blob` stream and then its `BlobDelta` stream.
///
/// Each embedded macro is a property of the form, report, or control whose
/// value is a macro grid (see [`MacroGrid`]) holding the macro's XML as
/// `_AXL:` comment rows. The XML's `For` and `Event` attributes give
/// [`MacroSource::Embedded`]'s control and event; a macro without XML has no
/// control and an empty event, and its statements are read from the grid.
///
/// Databases in the Access 2000 format keep the macros in `BlobDelta`. A
/// grid found in both streams is returned once.
///
/// Returns [`FileError::FormNotFound`] if there is no such form or report.
/// The form is read when a form and a report share the name.
pub fn read_embedded_macros(
    reader: &mut PageReader,
    object_name: &str,
) -> Result<Vec<MacroDef>, FileError> {
    embedded_macros(reader, object_name, None)
}

/// [`read_embedded_macros`] of the form or the report named `object_name`.
pub fn read_embedded_macros_of_type(
    reader: &mut PageReader,
    object_name: &str,
    object_type: FormObjectType,
) -> Result<Vec<MacroDef>, FileError> {
    embedded_macros(reader, object_name, Some(object_type))
}

fn embedded_macros(
    reader: &mut PageReader,
    object_name: &str,
    object_type: Option<FormObjectType>,
) -> Result<Vec<MacroDef>, FileError> {
    read_embedded_macros_in(
        &storage::read_access_storage(reader)?,
        object_name,
        object_type,
    )
}

/// [`read_embedded_macros`] in the storage already read: of `object_type`
/// when it is given, and otherwise the form when a form and a report share
/// the name.
pub fn read_embedded_macros_in(
    storage: &AccessStorage,
    object_name: &str,
    object_type: Option<FormObjectType>,
) -> Result<Vec<MacroDef>, FileError> {
    let stream = form::stream_in(storage, object_name, object_type, StreamKind::Blob)?;
    embedded_macro_grids_of(storage, &stream)
        .iter()
        .map(|bytes| embedded_macro(bytes, stream.object_type, object_name))
        .collect()
}

/// The embedded macro grids of the Blob `stream`, and then those of its
/// BlobDelta that the Blob does not have.
fn embedded_macro_grids_of(storage: &AccessStorage, stream: &form::FormStream) -> Vec<Vec<u8>> {
    let mut grids: Vec<Vec<u8>> = embedded_macro_grids(&stream.data)
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    // The BlobDelta of the object the Blob is of, even when a report shares
    // the name of a form.
    if let Ok(delta) = form::stream_in(
        storage,
        &stream.name,
        Some(stream.object_type),
        StreamKind::BlobDelta,
    ) {
        for grid in embedded_macro_grids(&delta.data) {
            if !grids.iter().any(|g| g == grid) {
                grids.push(grid.to_vec());
            }
        }
    }
    grids
}

/// The macro of the grid `bytes`, embedded in the form or report
/// `object_name` of `object_kind`: with the control and event its XML gives,
/// or with no control and an empty event, and the statements of the grid,
/// when it has no XML.
fn embedded_macro(
    bytes: &[u8],
    object_kind: FormObjectType,
    object_name: &str,
) -> Result<MacroDef, FileError> {
    let grid = parse_macro_grid(bytes, TextEncoding::Unicode)?;
    let (control, event, statements, xml) = match extract_axl_xml(bytes) {
        Some(xml) => {
            let root = parse_xml_tree(&xml)?;
            (
                root.attribute("For").map(str::to_string),
                root.attribute("Event").unwrap_or("").to_string(),
                statements_of(&root),
                xml,
            )
        }
        None => (
            None,
            String::new(),
            statements_from_grid(&grid),
            String::new(),
        ),
    };
    Ok(MacroDef {
        source: MacroSource::Embedded {
            object_kind,
            object_name: object_name.to_string(),
            control,
            event,
        },
        statements,
        xml,
        grid: Some(grid),
    })
}

/// The type of a form property entry whose value is an embedded macro grid.
const EMBEDDED_MACRO_PROPERTY_TYPE: [u8; 4] = [0x11, 0, 0, 0];

/// The macro grids stored as embedded macro properties in a form or report
/// stream, in order.
///
/// A property entry is a 2-byte property id, a 4-byte type, 4 bytes (4 in
/// these entries), and a 4-byte value length, followed by the value (see
/// `form.rs`). Embedded macro entries have type 0x11, and their value is a
/// grid whose header string length at offset 0x20 is 4 (`33`) or 0.
fn embedded_macro_grids(stream: &[u8]) -> Vec<&[u8]> {
    let mut grids = Vec::new();
    let mut search_from = 2;
    while let Some(rel) = find_bytes(
        stream.get(search_from..).unwrap_or(&[]),
        &EMBEDDED_MACRO_PROPERTY_TYPE,
    ) {
        let type_pos = search_from + rel;
        search_from = type_pos + 1;
        let Some(header) = stream.get(type_pos + 4..type_pos + 12) else {
            break;
        };
        if header[0..4] != [4, 0, 0, 0] {
            continue;
        }
        let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let start = type_pos + 12;
        let Some(grid) = stream.get(start..start + len) else {
            continue;
        };
        let header_len = grid
            .get(0x20..0x22)
            .map(|s| u16::from_le_bytes([s[0], s[1]]));
        if matches!(header_len, Some(0) | Some(4))
            && parse_macro_grid(grid, TextEncoding::Unicode).is_ok()
        {
            grids.push(grid);
            search_from = start + len;
        }
    }
    grids
}

/// `MSysObjects.Type` of a macro.
const MSYSOBJECTS_TYPE_MACRO: i16 = -32766;

/// Read the data macros of the local table `table`, in document order.
///
/// Returns an empty list for a table without data macros, and
/// [`FileError::TableNotFound`] if there is no such table. Each returned
/// [`MacroDef::xml`] is the whole XML document stored for the table.
pub fn read_data_macros(reader: &mut PageReader, table: &str) -> Result<Vec<MacroDef>, FileError> {
    read_data_macros_in(&read_catalog(reader)?, table)
}

/// [`read_data_macros`] with the catalog already read, whose entries keep
/// the data macros of each table ([`CatalogEntry::lv_extra`]).
pub fn read_data_macros_in(
    catalog: &[CatalogEntry],
    table: &str,
) -> Result<Vec<MacroDef>, FileError> {
    let entry = catalog
        .iter()
        .find(|e| e.object_type == ObjectType::Table && e.name == table)
        .ok_or_else(|| FileError::TableNotFound {
            name: table.to_string(),
        })?;
    let Some(extra) = &entry.lv_extra else {
        return Ok(Vec::new());
    };

    let mut macros = Vec::new();
    for xml in extract_utf16_xml_documents(extra) {
        macros.extend(data_macros_from_xml(table, xml)?);
    }
    Ok(macros)
}

/// The data macros in one XML document: the `DataMacro` children of a
/// `DataMacros` element, or a single `DataMacro` element.
fn data_macros_from_xml(table: &str, xml: String) -> Result<Vec<MacroDef>, FileError> {
    let root = parse_xml_tree(&xml)?;
    let elements: Vec<&MacroXmlElement> = match root.name.as_str() {
        "DataMacros" => root
            .children
            .iter()
            .filter(|c| c.name == "DataMacro")
            .collect(),
        "DataMacro" => vec![&root],
        _ => Vec::new(),
    };
    Ok(elements
        .into_iter()
        .map(|element| data_macro(table, element, &xml))
        .collect())
}

/// The data macro of `table` that the `DataMacro` element `element` of the
/// document `xml` is.
fn data_macro(table: &str, element: &MacroXmlElement, xml: &str) -> MacroDef {
    MacroDef {
        source: MacroSource::Data {
            table: table.to_string(),
            event: element.attribute("Event").map(str::to_string),
            name: element.attribute("Name").map(str::to_string),
            parameters: data_macro_parameters(element),
        },
        statements: statements_of(element),
        xml: xml.to_string(),
        grid: None,
    }
}

/// The `Parameter` elements of the `Parameters` of a data macro, each with
/// its name and its other attributes.
fn data_macro_parameters(element: &MacroXmlElement) -> Vec<MacroParameter> {
    let Some(parameters) = element.child("Parameters") else {
        return Vec::new();
    };
    parameters
        .children
        .iter()
        .filter(|c| c.name == "Parameter")
        .map(|c| MacroParameter {
            name: c.attribute("Name").unwrap_or_default().to_string(),
            attributes: c
                .attributes
                .iter()
                .filter(|(k, _)| k != "Name")
                .cloned()
                .collect(),
        })
        .collect()
}

/// An `MSysObjects` row's name and `LvExtra`.
type NamedLvExtra = (String, Option<Vec<u8>>);

/// The name and `LvExtra` of every `MSysObjects` row of type `object_type`.
fn msysobjects_rows(
    reader: &mut PageReader,
    object_type: i16,
) -> Result<Vec<NamedLvExtra>, FileError> {
    let tdef = read_table_def(reader, "MSysObjects", CATALOG_PAGE)?;
    let name_idx = msysobjects_column(&tdef, "Name")?;
    let type_idx = msysobjects_column(&tdef, "Type")?;
    let extra_idx = msysobjects_column(&tdef, "LvExtra")?;
    Ok(read_table_rows(reader, &tdef)?
        .rows
        .into_iter()
        .filter(|row| row[type_idx] == Value::Int(object_type))
        .filter_map(|row| named_lv_extra(&row, name_idx, extra_idx))
        .collect())
}

fn msysobjects_column(tdef: &TableDef, name: &str) -> Result<usize, FileError> {
    column_index(&tdef.columns, name).ok_or_else(|| FileError::InvalidMacroData {
        reason: format!("MSysObjects has no {name} column"),
    })
}

/// The name and `LvExtra` of a row of `MSysObjects`, or `None` when it has no
/// name.
fn named_lv_extra(row: &[Value], name_idx: usize, extra_idx: usize) -> Option<NamedLvExtra> {
    let name = row[name_idx].as_text()?.to_string();
    Some((name, row[extra_idx].as_binary().map(<[u8]>::to_vec)))
}

// ---------------------------------------------------------------------------
// Internal: macro grid
// ---------------------------------------------------------------------------

/// The string of single bytes at the start of `bytes`, up to the first 0: in
/// the code page of a Jet3 file, and as Latin-1 otherwise.
fn single_byte_string(bytes: &[u8], text: TextEncoding) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    match text.jet3_code_page() {
        Some(code_page) => crate::encoding::decode_code_page(&bytes[..end], code_page),
        None => crate::encoding::decode_latin1(&bytes[..end]),
    }
}

/// Parses a macro grid.
///
/// Layout, as observed in files from Access 97 through Microsoft 365:
/// - bytes 0..4: [`MacroGrid::columns_shown`]; bytes 4..0x20: not interpreted
/// - at 0x20: a 2-byte length `n`, `n` bytes of a UTF-16LE header string, and
///   2 zero bytes
/// - row strings: single-byte in Access 97 databases (`text` is Jet3),
///   whatever the header string (`22` or empty there). In later formats they
///   are UTF-16LE after the header string `33` (or `23` in the designer's
///   clipboard macro), and single-byte after an empty header string, which
///   macros converted from Access 97 keep.
/// - rows until the end, each: action number (u16), row number (u16), 4 field
///   offsets (u16: not interpreted, comment, condition, macro name), 10
///   argument offsets (u16), a 2-byte length `m`, `m` bytes of NUL-terminated
///   strings the offsets point into (0xFFFF for none), and 2 zero bytes
fn parse_macro_grid(bytes: &[u8], text: TextEncoding) -> Result<MacroGrid, FileError> {
    let grid = GridBytes {
        bytes,
        text,
        single_byte: false,
    };
    let columns_shown = grid.u32_at(0)?;
    let header_len = grid.u16_at(0x20)? as usize;
    let header = grid.header(header_len)?;
    let grid = GridBytes {
        single_byte: text.is_jet3() || header_len == 0,
        ..grid
    };

    let mut rows = Vec::new();
    let mut pos = 0x22 + header_len + 2;
    while pos < bytes.len() {
        let (row, next) = grid.row(pos)?;
        rows.push(row);
        pos = next;
    }
    Ok(MacroGrid {
        columns_shown,
        header,
        rows,
    })
}

fn grid_error(what: &str) -> FileError {
    FileError::InvalidMacroData {
        reason: format!("macro grid: {what}"),
    }
}

/// The bytes of a macro grid, and whether its row strings are single-byte,
/// in `text`.
struct GridBytes<'a> {
    bytes: &'a [u8],
    text: TextEncoding,
    single_byte: bool,
}

impl GridBytes<'_> {
    fn u16_at(&self, offset: usize) -> Result<u16, FileError> {
        self.bytes
            .get(offset..offset + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]))
            .ok_or_else(|| grid_error("unexpected end of data"))
    }

    fn u32_at(&self, offset: usize) -> Result<u32, FileError> {
        self.bytes
            .get(offset..offset + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or_else(|| grid_error("unexpected end of data"))
    }

    /// The UTF-16LE header string of `header_len` bytes at 0x22.
    fn header(&self, header_len: usize) -> Result<String, FileError> {
        let units: Vec<u16> = self
            .bytes
            .get(0x22..0x22 + header_len)
            .ok_or_else(|| grid_error("header string past the end of data"))?
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// The row at `pos`, and the position of the next row.
    fn row(&self, pos: usize) -> Result<(MacroGridRow, usize), FileError> {
        let action_code = self.u16_at(pos)?;
        let row = self.u16_at(pos + 2)?;
        let offsets = (0..14)
            .map(|k| self.u16_at(pos + 4 + 2 * k))
            .collect::<Result<Vec<u16>, FileError>>()?;
        let len = self.u16_at(pos + 32)? as usize;
        let block = self
            .bytes
            .get(pos + 34..pos + 34 + len)
            .ok_or_else(|| grid_error("row strings past the end of data"))?;
        let grid_row = self.row_of_strings(block, row, action_code, &offsets)?;
        Ok((grid_row, pos + 34 + len + 2))
    }

    /// The row `row` of the action `action_code`, with the strings in
    /// `block` at `offsets`: not interpreted, comment, condition, macro name,
    /// then the arguments.
    fn row_of_strings(
        &self,
        block: &[u8],
        row: u16,
        action_code: u16,
        offsets: &[u16],
    ) -> Result<MacroGridRow, FileError> {
        let string = |offset: u16| self.string(block, offset);
        Ok(MacroGridRow {
            row,
            comment: string(offsets[1])?,
            condition: string(offsets[2])?,
            macro_name: string(offsets[3])?,
            action_code,
            arguments: offsets[4..]
                .iter()
                .map(|&o| string(o))
                .collect::<Result<Vec<_>, FileError>>()?,
        })
    }

    /// The string at `offset` of the strings `block` of a row, or `None` for
    /// 0xFFFF.
    fn string(&self, block: &[u8], offset: u16) -> Result<Option<String>, FileError> {
        if offset == 0xFFFF {
            return Ok(None);
        }
        let rest = block
            .get(offset as usize..)
            .ok_or_else(|| grid_error("string offset past the row"))?;
        if self.single_byte {
            return Ok(Some(single_byte_string(rest, self.text)));
        }
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        Ok(Some(String::from_utf16_lossy(&units)))
    }
}

// ---------------------------------------------------------------------------
// Internal: locating a macro's Blob stream
// ---------------------------------------------------------------------------

/// The condition that continues the condition of the rows above it.
const CONTINUED_CONDITION: &str = "...";

/// The statements of a macro stored without XML, read from its grid rows.
fn statements_from_grid(grid: &MacroGrid) -> Vec<MacroStatement> {
    let mut statements = GridStatements {
        top: Vec::new(),
        submacro: None,
        branch: None,
    };
    for row in &grid.rows {
        statements.read_row(row);
    }
    statements.finish_submacro();
    statements.top
}

/// The statements read from the rows of a grid so far.
struct GridStatements {
    top: Vec<MacroStatement>,
    /// The submacro being read, and the open condition within it.
    submacro: Option<(String, Vec<MacroStatement>)>,
    branch: Option<MacroBranch>,
}

impl GridStatements {
    fn read_row(&mut self, row: &MacroGridRow) {
        if let Some(name) = &row.macro_name {
            self.finish_submacro();
            self.submacro = Some((name.clone(), Vec::new()));
        }
        self.open_branch(row.condition.as_deref());
        let statements = self.statements();
        if let Some(comment) = &row.comment {
            statements.push(MacroStatement::Comment(comment.clone()));
        }
        if row.action_code != 0 {
            statements.push(grid_action(row));
        }
    }

    /// Close the open condition and open one for `condition`, unless it
    /// continues the open one.
    fn open_branch(&mut self, condition: Option<&str>) {
        if condition == Some(CONTINUED_CONDITION) && self.branch.is_some() {
            return;
        }
        let enclosing = self.submacro.as_mut().map_or(&mut self.top, |(_, s)| s);
        close_branch(enclosing, &mut self.branch);
        self.branch = condition.map(|condition| MacroBranch {
            condition: Some(condition.to_string()),
            statements: Vec::new(),
        });
    }

    /// The statements a row goes in: those of the open condition, or of the
    /// submacro or the macro.
    fn statements(&mut self) -> &mut Vec<MacroStatement> {
        match &mut self.branch {
            Some(b) => &mut b.statements,
            None => self.submacro.as_mut().map_or(&mut self.top, |(_, s)| s),
        }
    }

    fn finish_submacro(&mut self) {
        finish_submacro(&mut self.top, &mut self.submacro, &mut self.branch);
    }
}

fn close_branch(statements: &mut Vec<MacroStatement>, branch: &mut Option<MacroBranch>) {
    if let Some(b) = branch.take() {
        statements.push(MacroStatement::Conditional { branches: vec![b] });
    }
}

fn finish_submacro(
    top: &mut Vec<MacroStatement>,
    submacro: &mut Option<(String, Vec<MacroStatement>)>,
    branch: &mut Option<MacroBranch>,
) {
    match submacro.take() {
        Some((name, mut statements)) => {
            close_branch(&mut statements, branch);
            top.push(MacroStatement::SubMacro { name, statements });
        }
        None => close_branch(top, branch),
    }
}

/// The action of a grid row with its non-empty arguments.
fn grid_action(row: &MacroGridRow) -> MacroStatement {
    let action = macro_action(row.action_code);
    let argument_names = action.map_or(&[][..], |a| a.arguments);
    MacroStatement::Action {
        name: action.map_or_else(|| row.action_code.to_string(), |a| a.name.to_string()),
        arguments: row
            .arguments
            .iter()
            .enumerate()
            .filter_map(|(i, value)| {
                let value = value.as_deref()?;
                let name = argument_names
                    .get(i)
                    .map_or_else(|| i.to_string(), |n| n.to_string());
                let value = macro_argument_value_name(row.action_code, &name, value)
                    .unwrap_or(value)
                    .to_string();
                Some(MacroArgument { name, value })
            })
            .collect(),
    }
}

fn scripts_dir_mapping(entries: &[storage::StorageEntry]) -> Option<Vec<(String, String)>> {
    let scripts_folder = storage::find_top_folder(entries, "Scripts")?;
    let dir_data = storage::find_dir_data(entries, scripts_folder.id)?;
    storage::parse_dir_data(&dir_data.data).ok()
}

fn find_macro_blob<'a>(
    entries: &'a [storage::StorageEntry],
    name: &str,
) -> Result<Option<&'a [u8]>, FileError> {
    let Some(scripts_folder) = storage::find_top_folder(entries, "Scripts") else {
        return Ok(None);
    };
    Ok(storage::object_stream(entries, scripts_folder, name, "Blob")?.map(|e| e.data.as_slice()))
}

// ---------------------------------------------------------------------------
// Internal: `_AXL:`-chunked XML reassembly
// ---------------------------------------------------------------------------

/// "_AXL:" encoded as UTF-16LE bytes.
const AXL_MARKER: &[u8] = &[b'_', 0, b'A', 0, b'X', 0, b'L', 0, b':', 0];

/// Reassembles the `UserInterfaceMacro` XML document mirrored inside a
/// macro's Blob stream, or `None` if the Blob has no `_AXL:`-marked chunks
/// (pre-2010 macros predate this mirror).
fn extract_axl_xml(blob: &[u8]) -> Option<String> {
    extract_axl_documents(blob).into_iter().next()
}

/// Reassembles every XML document mirrored inside a Blob stream, in order. A
/// chunk whose text starts with `<?xml` begins a new document, so a form's
/// Blob with several embedded macros yields one document per macro.
///
/// Verified against 5 real macros (334-5317 XML chars, from
/// `MS NorthwindDev.accdb`/`MS NorthwindStarter.accdb`'s `AutoExec`,
/// `macFilterOrders`, `macOrderDetails_SetColor`, `macMainMenu_UpdateSubs`,
/// and the internal `~TMPCLPMacro`, cross-checked against a real
/// `Application.SaveAsText` export of `AutoExec`): each chunk is
/// `[18 bytes 0xFF][2-byte LE length L]["_AXL:" + chunk text, UTF-16LE][2-byte
/// NUL terminator]`, where `L` is the byte length of `"_AXL:" + chunk text`
/// (i.e. not counting the terminator). Chunks are ~253 characters each except
/// the last, but this reads `L` directly rather than assuming a fixed size --
/// distance-to-next-marker undercounts when a chunk ends short of 253 chars,
/// leaking header bytes from the next chunk into the decoded text.
fn extract_axl_documents(blob: &[u8]) -> Vec<String> {
    let mut documents: Vec<String> = Vec::new();
    let mut search_from = 0usize;

    while let Some(rel) = find_bytes(&blob[search_from..], AXL_MARKER) {
        let pos = search_from + rel;
        if pos < 2 {
            break;
        }
        let declared_len = u16::from_le_bytes([blob[pos - 2], blob[pos - 1]]) as usize;
        if declared_len < AXL_MARKER.len() + 2 || pos + declared_len - 2 > blob.len() {
            break;
        }
        let chunk_end = pos + declared_len - 2;
        let chunk_text = decode_utf16le_lossy(&blob[pos..chunk_end]);
        let chunk = chunk_text.strip_prefix("_AXL:").unwrap_or(&chunk_text);
        match documents.last_mut() {
            Some(document) if !chunk.starts_with("<?xml") => document.push_str(chunk),
            _ => documents.push(chunk.to_string()),
        }
        search_from = chunk_end;
    }

    documents
        .into_iter()
        .map(|d| d.trim_end_matches('\u{0}').to_string())
        .collect()
}

/// "<?xml" encoded as UTF-16LE bytes.
const XML_DECLARATION_START: &[u8] = &[b'<', 0, b'?', 0, b'x', 0, b'm', 0, b'l', 0];

/// Extracts the UTF-16LE XML documents stored in `bytes` after a binary
/// header, as in `MSysObjects.LvExtra`. Each document runs from a `<?xml`
/// declaration to the last `>` before the next declaration or the end.
fn extract_utf16_xml_documents(bytes: &[u8]) -> Vec<String> {
    let mut starts = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = find_bytes(&bytes[search_from..], XML_DECLARATION_START) {
        starts.push(search_from + rel);
        search_from += rel + XML_DECLARATION_START.len();
    }
    starts
        .iter()
        .enumerate()
        .filter_map(|(i, &start)| {
            let end = starts.get(i + 1).copied().unwrap_or(bytes.len());
            let text = decode_utf16le_lossy(&bytes[start..end]);
            let last = text.rfind('>')?;
            Some(text[..=last].to_string())
        })
        .collect()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn decode_utf16le_lossy(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

// ---------------------------------------------------------------------------
// Internal: XML -> element tree -> statements
// ---------------------------------------------------------------------------

/// How deep the elements of macro XML may nest. The statements, and the
/// copies and drops of the tree, recur on each level, and a file can nest
/// them deep enough to use up the stack, which is 1 MB in Wasm: about 2,000
/// levels of statement groups do there. Macros Access makes nest a few
/// levels.
const MAX_XML_DEPTH: usize = 256;

/// Parses `xml` into an element tree and returns its document element; an
/// error when its elements nest deeper than [`MAX_XML_DEPTH`].
fn parse_xml_tree(xml: &str) -> Result<MacroXmlElement, FileError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut root = new_element(String::new(), Vec::new());
    let mut stack: Vec<MacroXmlElement> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => open_element(&mut stack, &e)?,
            Ok(Event::Empty(e)) => {
                let element = element_of(&e)?;
                stack.last_mut().unwrap_or(&mut root).children.push(element);
            }
            Ok(Event::Text(t)) => {
                push_text(
                    &mut stack,
                    &t.xml_content(quick_xml::XmlVersion::Explicit1_0),
                );
            }
            // A CDATA section is text as it is, without references to resolve.
            Ok(Event::CData(t)) => {
                push_text(
                    &mut stack,
                    &t.xml_content(quick_xml::XmlVersion::Explicit1_0),
                );
            }
            // Entity and character references (`&lt;`, `&#60;`) arrive as
            // separate events rather than inside the surrounding text.
            Ok(Event::GeneralRef(r)) => push_text(&mut stack, &resolve_reference(&r)?),
            Ok(Event::End(_)) => close_element(&mut stack, &mut root),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(FileError::InvalidMacroData {
                    reason: format!("malformed macro XML: {e}"),
                })
            }
        }
    }

    root.children
        .pop()
        .ok_or_else(|| FileError::InvalidMacroData {
            reason: "empty macro XML document".to_string(),
        })
}

/// Adds `text` to the text of the element being read, if any.
fn push_text(stack: &mut [MacroXmlElement], text: &str) {
    if let Some(top) = stack.last_mut() {
        top.text.push_str(text);
    }
}

/// Closes the element being read, and adds it to the children of the one
/// around it, or of `root`.
fn close_element(stack: &mut Vec<MacroXmlElement>, root: &mut MacroXmlElement) {
    if let Some(mut element) = stack.pop() {
        // Indentation between child elements is not content.
        if !element.children.is_empty() && element.text.trim().is_empty() {
            element.text.clear();
        }
        stack.last_mut().unwrap_or(root).children.push(element);
    }
}

/// Opens the element `e` on `stack`, the elements being read; an error when
/// that would nest them deeper than [`MAX_XML_DEPTH`].
fn open_element(
    stack: &mut Vec<MacroXmlElement>,
    e: &quick_xml::events::BytesStart<'_>,
) -> Result<(), FileError> {
    if stack.len() == MAX_XML_DEPTH {
        return Err(FileError::InvalidMacroData {
            reason: format!("macro XML nested deeper than {MAX_XML_DEPTH} elements"),
        });
    }
    stack.push(element_of(e)?);
    Ok(())
}

/// The element of the tag `e`, with its name and attributes.
fn element_of(e: &quick_xml::events::BytesStart<'_>) -> Result<MacroXmlElement, FileError> {
    Ok(new_element(e.name().as_ref().to_string(), read_attrs(e)?))
}

/// An element without text or children yet.
fn new_element(name: String, attributes: Vec<(String, String)>) -> MacroXmlElement {
    MacroXmlElement {
        name,
        attributes,
        text: String::new(),
        children: Vec::new(),
    }
}

/// The text of the entity or character reference `r`, such as `&lt;` or
/// `&#60;`.
fn resolve_reference(r: &quick_xml::events::BytesRef<'_>) -> Result<String, FileError> {
    match r.resolve_char_ref() {
        Ok(Some(ch)) => Ok(ch.to_string()),
        Ok(None) => quick_xml::escape::resolve_xml_entity(r)
            .map(str::to_string)
            .ok_or_else(|| FileError::InvalidMacroData {
                reason: format!("unknown entity in macro XML: &{};", &**r),
            }),
        Err(e) => Err(FileError::InvalidMacroData {
            reason: format!("invalid character reference in macro XML: {e}"),
        }),
    }
}

fn read_attrs(e: &quick_xml::events::BytesStart<'_>) -> Result<Vec<(String, String)>, FileError> {
    let mut out = Vec::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|e| FileError::InvalidMacroData {
            reason: format!("invalid macro XML attribute: {e}"),
        })?;
        let key = attr.key.as_ref().to_string();
        let val = attr
            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
            .map_err(|e| FileError::InvalidMacroData {
                reason: format!("invalid macro XML attribute value: {e}"),
            })?
            .into_owned();
        out.push((key, val));
    }
    Ok(out)
}

/// The statements inside `element`'s `Statements` child, or none if it has
/// no such child.
fn statements_of(element: &MacroXmlElement) -> Vec<MacroStatement> {
    element
        .child("Statements")
        .map(|s| s.children.iter().map(to_statement).collect())
        .unwrap_or_default()
}

fn to_statement(element: &MacroXmlElement) -> MacroStatement {
    let converted = match element.name.as_str() {
        "Action" => to_action(element),
        "Comment" => Some(MacroStatement::Comment(element.text.clone())),
        "ConditionalBlock" => to_conditional(element),
        "StatementGroup" => Some(MacroStatement::Group {
            description: element.attribute("Description").unwrap_or("").to_string(),
            statements: statements_of(element),
        }),
        "SubMacro" => Some(MacroStatement::SubMacro {
            name: element.attribute("Name").unwrap_or("").to_string(),
            statements: statements_of(element),
        }),
        _ => to_data_block(element),
    };
    converted.unwrap_or_else(|| MacroStatement::Unknown(element.clone()))
}

/// An element without attributes whose children are a `Statements` element
/// and optionally a `Data` element whose own children are plain text
/// elements; anything else is left to [`MacroStatement::Unknown`].
fn to_data_block(element: &MacroXmlElement) -> Option<MacroStatement> {
    if !is_data_block_shape(element) {
        return None;
    }
    let data = match element.child("Data") {
        Some(d) => data_of_block(d)?,
        None => Vec::new(),
    };
    Some(MacroStatement::DataBlock {
        kind: element.name.clone(),
        data,
        statements: statements_of(element),
    })
}

/// Whether `element` has no attributes or text, and a `Statements` child and
/// no children but `Statements` and `Data`.
fn is_data_block_shape(element: &MacroXmlElement) -> bool {
    element.attributes.is_empty()
        && element.text.is_empty()
        && element.child("Statements").is_some()
        && element
            .children
            .iter()
            .all(|c| c.name == "Statements" || c.name == "Data")
}

/// The names and texts of the children of the `Data` element `data`, when it
/// has no attributes or text and each child is a plain text element.
fn data_of_block(data: &MacroXmlElement) -> Option<Vec<(String, String)>> {
    if !data.attributes.is_empty() || !data.text.is_empty() {
        return None;
    }
    data.children
        .iter()
        .map(|c| {
            (c.attributes.is_empty() && c.children.is_empty())
                .then(|| (c.name.clone(), c.text.clone()))
        })
        .collect()
}

/// An `Action` whose children are all `Argument`s; anything else is left to
/// [`MacroStatement::Unknown`] so no child is dropped.
fn to_action(element: &MacroXmlElement) -> Option<MacroStatement> {
    let arguments = element
        .children
        .iter()
        .map(|c| {
            (c.name == "Argument").then(|| MacroArgument {
                name: c.attribute("Name").unwrap_or("").to_string(),
                value: c.text.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(MacroStatement::Action {
        name: element.attribute("Name").unwrap_or("").to_string(),
        arguments,
    })
}

/// A `ConditionalBlock` made of `If` / `ElseIf` / `Else` children; anything
/// else is left to [`MacroStatement::Unknown`].
fn to_conditional(element: &MacroXmlElement) -> Option<MacroStatement> {
    let branches = element
        .children
        .iter()
        .map(|c| match c.name.as_str() {
            "If" | "ElseIf" => Some(MacroBranch {
                condition: Some(
                    c.child("Condition")
                        .map(|x| x.text.clone())
                        .unwrap_or_default(),
                ),
                statements: statements_of(c),
            }),
            "Else" => Some(MacroBranch {
                condition: None,
                statements: statements_of(c),
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(MacroStatement::Conditional { branches })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axl_chunk_header(chunk_including_prefix_and_marker: &str) -> Vec<u8> {
        let text_bytes: Vec<u8> = chunk_including_prefix_and_marker
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let declared_len = (text_bytes.len() + 2) as u16;
        let mut out = vec![0xFFu8; 18];
        out.extend_from_slice(&declared_len.to_le_bytes());
        out
    }

    fn build_blob(chunks: &[&str]) -> Vec<u8> {
        let mut blob = Vec::new();
        for chunk in chunks {
            let full = format!("_AXL:{chunk}");
            blob.extend(axl_chunk_header(&full));
            blob.extend(full.encode_utf16().flat_map(|u| u.to_le_bytes()));
            blob.extend_from_slice(&[0x00, 0x00]); // NUL terminator
        }
        blob
    }

    #[test]
    fn extract_axl_xml_single_chunk() {
        let blob = build_blob(&["<a>hi</a>"]);
        assert_eq!(extract_axl_xml(&blob).as_deref(), Some("<a>hi</a>"));
    }

    #[test]
    fn extract_axl_xml_multiple_chunks_reassembles_in_order() {
        let blob = build_blob(&["<a>", "hi", "</a>"]);
        assert_eq!(extract_axl_xml(&blob).as_deref(), Some("<a>hi</a>"));
    }

    #[test]
    fn extract_axl_xml_no_marker_returns_none() {
        let blob = vec![0u8; 32];
        assert_eq!(extract_axl_xml(&blob), None);
    }

    #[test]
    fn extract_axl_xml_short_chunk_does_not_leak_next_headers() {
        // A chunk shorter than the ~253-char nominal size must not pull in
        // the following chunk's 0xFF header bytes -- this is exactly the bug
        // distance-to-next-marker had (verified against real macro data).
        let blob = build_blob(&["short", "second chunk"]);
        assert_eq!(extract_axl_xml(&blob).as_deref(), Some("shortsecond chunk"));
    }

    #[test]
    fn extract_axl_documents_splits_at_xml_declarations() {
        let blob = build_blob(&[
            "<?xml version=\"1.0\"?><a>",
            "1</a>",
            "<?xml version=\"1.0\"?><b/>",
        ]);
        assert_eq!(
            extract_axl_documents(&blob),
            [
                "<?xml version=\"1.0\"?><a>1</a>",
                "<?xml version=\"1.0\"?><b/>"
            ]
        );
    }

    // -- XML -> statements ------------------------------------------------------

    fn statements(xml: &str) -> Vec<MacroStatement> {
        statements_of(&parse_xml_tree(xml).unwrap())
    }

    /// A macro whose elements nest `depth` levels: the document element,
    /// its `Statements`, and `X` elements in them.
    fn nested_xml(depth: usize) -> String {
        let inner = depth - 2;
        format!(
            "<UserInterfaceMacro xmlns=\"ns\"><Statements>{}{}</Statements></UserInterfaceMacro>",
            "<X>".repeat(inner),
            "</X>".repeat(inner)
        )
    }

    #[test]
    fn elements_nest_up_to_the_limit() {
        // Within the limit, the tree gives its statements: the elements are
        // unknown statements copied with the tree in them.
        let tree = parse_xml_tree(&nested_xml(MAX_XML_DEPTH)).unwrap();
        assert_eq!(statements_of(&tree).len(), 1);
        let deeper = parse_xml_tree(&nested_xml(MAX_XML_DEPTH + 1));
        assert!(
            matches!(&deeper, Err(FileError::InvalidMacroData { reason }) if reason.contains("deeper")),
            "{deeper:?}"
        );
    }

    #[test]
    fn deep_nesting_is_an_error_rather_than_a_stack_overflow() {
        // 100,000 levels, which would use up the stack building the
        // statements of the tree and dropping it.
        assert!(parse_xml_tree(&nested_xml(100_000)).is_err());
    }

    fn action(name: &str, arguments: &[(&str, &str)]) -> MacroStatement {
        MacroStatement::Action {
            name: name.to_string(),
            arguments: arguments
                .iter()
                .map(|(n, v)| MacroArgument {
                    name: n.to_string(),
                    value: v.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn action_with_arguments() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><Action Name="OpenForm"><Argument Name="FormName">frmStartup</Argument></Action><Action Name="Beep"/></Statements></UserInterfaceMacro>"#;
        assert_eq!(
            statements(xml),
            [
                action("OpenForm", &[("FormName", "frmStartup")]),
                action("Beep", &[])
            ]
        );
    }

    #[test]
    fn conditional_block_if_elseif_else() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><ConditionalBlock><If><Condition>A</Condition><Statements><Action Name="X"/></Statements></If><ElseIf><Condition>B</Condition><Statements><Action Name="Y"/></Statements></ElseIf><Else><Statements><Action Name="Z"/></Statements></Else></ConditionalBlock></Statements></UserInterfaceMacro>"#;
        let branch = |condition: Option<&str>, name: &str| MacroBranch {
            condition: condition.map(str::to_string),
            statements: vec![action(name, &[])],
        };
        assert_eq!(
            statements(xml),
            [MacroStatement::Conditional {
                branches: vec![
                    branch(Some("A"), "X"),
                    branch(Some("B"), "Y"),
                    branch(None, "Z")
                ]
            }]
        );
    }

    #[test]
    fn references_in_conditions_are_resolved() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><ConditionalBlock><If><Condition>[MacroError]&lt;&gt;0 And [x]&#62;1</Condition><Statements/></If></ConditionalBlock></Statements></UserInterfaceMacro>"#;
        let MacroStatement::Conditional { branches } = &statements(xml)[0] else {
            panic!("expected a conditional block");
        };
        assert_eq!(
            branches[0].condition.as_deref(),
            Some("[MacroError]<>0 And [x]>1")
        );
    }

    #[test]
    fn comment_group_and_submacro() {
        // Element names as written by Access in published SaveAsText exports.
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><Comment>hello</Comment><StatementGroup Description="Group A"><Statements><Action Name="X"/></Statements></StatementGroup><SubMacro Name="SubOne"><Statements><Action Name="Y"/></Statements></SubMacro></Statements></UserInterfaceMacro>"#;
        assert_eq!(
            statements(xml),
            [
                MacroStatement::Comment("hello".to_string()),
                MacroStatement::Group {
                    description: "Group A".to_string(),
                    statements: vec![action("X", &[])]
                },
                MacroStatement::SubMacro {
                    name: "SubOne".to_string(),
                    statements: vec![action("Y", &[])]
                },
            ]
        );
    }

    #[test]
    fn unknown_elements_are_kept() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><Mystery Kind="k"><Part>p</Part></Mystery></Statements></UserInterfaceMacro>"#;
        assert_eq!(
            statements(xml),
            [MacroStatement::Unknown(MacroXmlElement {
                name: "Mystery".to_string(),
                attributes: vec![("Kind".to_string(), "k".to_string())],
                text: String::new(),
                children: vec![MacroXmlElement {
                    name: "Part".to_string(),
                    attributes: Vec::new(),
                    text: "p".to_string(),
                    children: Vec::new(),
                }],
            })]
        );
    }

    #[test]
    fn action_with_a_non_argument_child_is_kept_unknown() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><Action Name="X"><Other/></Action></Statements></UserInterfaceMacro>"#;
        assert!(matches!(
            &statements(xml)[0],
            MacroStatement::Unknown(e) if e.name == "Action"
        ));
    }

    #[test]
    fn argument_values_keep_whitespace_and_indentation_is_ignored() {
        let xml = "<UserInterfaceMacro xmlns=\"ns\">\n  <Statements>\n    <Action Name=\"X\">\n      <Argument Name=\"Description\"> negative qty</Argument>\n    </Action>\n  </Statements>\n</UserInterfaceMacro>";
        assert_eq!(
            statements(xml),
            [action("X", &[("Description", " negative qty")])]
        );
    }

    #[test]
    fn cdata_is_text() {
        // A CDATA section, alone or next to text and references, is part of
        // the text of its element, with < and & as they are.
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><Action Name="X"><Argument Name="Description"><![CDATA[negative qty]]></Argument><Argument Name="Where">[a] <![CDATA[< 1 & ]]>&lt; 2</Argument></Action></Statements></UserInterfaceMacro>"#;
        assert_eq!(
            statements(xml),
            [action(
                "X",
                &[("Description", "negative qty"), ("Where", "[a] < 1 & < 2")]
            )]
        );
    }

    #[test]
    fn data_macros_wrapped_in_data_macros_element() {
        let xml = r#"<?xml version="1.0"?><DataMacros xmlns="ns"><DataMacro Event="AfterInsert"><Statements><Action Name="A"/></Statements></DataMacro><DataMacro Name="dmNamed"><Statements><Action Name="B"/></Statements></DataMacro></DataMacros>"#;
        let macros = data_macros_from_xml("T", xml.to_string()).unwrap();
        let sources: Vec<&MacroSource> = macros.iter().map(|m| &m.source).collect();
        assert_eq!(
            sources,
            [
                &MacroSource::Data {
                    table: "T".to_string(),
                    event: Some("AfterInsert".to_string()),
                    name: None,
                    parameters: vec![]
                },
                &MacroSource::Data {
                    table: "T".to_string(),
                    event: None,
                    name: Some("dmNamed".to_string()),
                    parameters: vec![]
                },
            ]
        );
        assert_eq!(macros[0].statements, [action("A", &[])]);
        assert_eq!(macros[1].statements, [action("B", &[])]);
        assert!(macros.iter().all(|m| m.xml == xml));
    }

    #[test]
    fn data_blocks() {
        let xml = r#"<DataMacro Event="AfterUpdate" xmlns="ns"><Statements><ForEachRecord><Data><Reference>tblLog</Reference><WhereCondition>[ID]=1</WhereCondition></Data><Statements><EditRecord><Data/><Statements><Action Name="SetField"><Argument Name="Field">Note</Argument></Action></Statements></EditRecord></Statements></ForEachRecord></Statements></DataMacro>"#;
        assert_eq!(
            statements(xml),
            [MacroStatement::DataBlock {
                kind: "ForEachRecord".to_string(),
                data: vec![
                    ("Reference".to_string(), "tblLog".to_string()),
                    ("WhereCondition".to_string(), "[ID]=1".to_string()),
                ],
                statements: vec![MacroStatement::DataBlock {
                    kind: "EditRecord".to_string(),
                    data: Vec::new(),
                    statements: vec![action("SetField", &[("Field", "Note")])],
                }],
            }]
        );
    }

    #[test]
    fn data_blocks_of_other_shapes_are_unknown() {
        // A block is a DataBlock only without attributes or text, with a
        // Statements child and no children but Statements and Data, and with
        // a Data of plain text elements, without attributes or text itself.
        for block in [
            r#"<ForEachRecord Alias="x"><Statements/></ForEachRecord>"#,
            r#"<ForEachRecord>text<Statements/></ForEachRecord>"#,
            r#"<ForEachRecord><Data/></ForEachRecord>"#,
            r#"<ForEachRecord><Statements/><Other/></ForEachRecord>"#,
            r#"<ForEachRecord><Data Alias="x"/><Statements/></ForEachRecord>"#,
            r#"<ForEachRecord><Data>text<Reference>t</Reference></Data><Statements/></ForEachRecord>"#,
            r#"<ForEachRecord><Data><Reference Alias="x">t</Reference></Data><Statements/></ForEachRecord>"#,
            r#"<ForEachRecord><Data><Reference><Inner/></Reference></Data><Statements/></ForEachRecord>"#,
        ] {
            let xml = format!(
                r#"<DataMacro Event="AfterUpdate"><Statements>{block}</Statements></DataMacro>"#
            );
            assert!(
                matches!(statements(&xml)[..], [MacroStatement::Unknown(_)]),
                "{block}"
            );
        }
    }

    #[test]
    fn extract_utf16_xml_documents_skips_the_header() {
        let mut bytes = vec![0x01, 0x02, 0x03, 0x04];
        let xml = "<?xml version=\"1.0\"?><DataMacro/>";
        bytes.extend(xml.encode_utf16().flat_map(|u| u.to_le_bytes()));
        bytes.extend_from_slice(&[0, 0]);
        assert_eq!(extract_utf16_xml_documents(&bytes), [xml]);
    }

    // -- Real files ---------------------------------------------------------
    //
    // macroTestV2010.accdb and its Access 2002-2003 and Access 2000 copies
    // (macroTestV2003.mdb, macroTestV2000.mdb) hold four named macros created
    // in Access (see testdata/SOURCES.md), plus the designer's internal
    // ~TMPCLPMacro. The Access 2000 copy stores them in MSysAccessObjects.

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::PathBuf::from(manifest_dir)
            .join("../../testdata")
            .join(relative);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    macro_rules! skip_if_missing {
        ($path:expr) => {
            match test_data_path($path) {
                Some(p) => p,
                None => {
                    eprintln!("SKIP: test data not found: {}", $path);
                    return;
                }
            }
        };
    }

    #[test]
    fn embedded_macros_of_a_form_and_a_report_of_the_same_name() {
        // Made by testdata/form-generators/same-name-generator.bas: the form
        // Shared has an OnLoad macro and the report Shared an OnOpen macro.
        // The Access 2000 format keeps them in BlobDelta.
        for file in [
            "V2007/sameNameTestV2007.accdb",
            "V2000/sameNameTestV2000.mdb",
        ] {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let events = |macros: Vec<MacroDef>| -> Vec<(FormObjectType, String)> {
                macros
                    .into_iter()
                    .map(|m| match m.source {
                        MacroSource::Embedded {
                            object_kind, event, ..
                        } => (object_kind, event),
                        other => panic!("{other:?}"),
                    })
                    .collect()
            };
            let form = read_embedded_macros_of_type(&mut reader, "Shared", FormObjectType::Form);
            assert_eq!(
                events(form.unwrap()),
                [(FormObjectType::Form, "OnLoad".to_string())],
                "{file}"
            );
            let report =
                read_embedded_macros_of_type(&mut reader, "Shared", FormObjectType::Report);
            assert_eq!(
                events(report.unwrap()),
                [(FormObjectType::Report, "OnOpen".to_string())],
                "{file}"
            );
            // Without a type, the form, as before.
            assert_eq!(
                events(read_embedded_macros(&mut reader, "Shared").unwrap()),
                [(FormObjectType::Form, "OnLoad".to_string())],
                "{file}"
            );
        }
    }

    const MACRO_TEST_FILES: [&str; 3] = [
        "V2010/macroTestV2010.accdb",
        "V2003/macroTestV2003.mdb",
        "V2000/macroTestV2000.mdb",
    ];

    /// The databases that testdata/macro-generators made: the accdb and its
    /// two .mdb copies.
    const MACRO_GENERATED_FILES: [&str; 3] = [
        "V2010/macroGeneratedTestV2010.accdb",
        "V2003/macroGeneratedTestV2003.mdb",
        "V2000/macroGeneratedTestV2000.mdb",
    ];

    fn expected_macro_statements() -> Vec<(&'static str, Vec<MacroStatement>)> {
        let message = |text: &str| action("MessageBox", &[("Message", text)]);
        vec![
            (
                "mcrSimple",
                vec![
                    action("OpenForm", &[("FormName", "frmEmbedded")]),
                    action(
                        "MessageBox",
                        &[
                            ("Message", "Hello"),
                            ("Beep", "No"),
                            ("Type", "Information"),
                            ("Title", "Title"),
                        ],
                    ),
                    message("Default"),
                ],
            ),
            (
                "mcrConditions",
                vec![
                    MacroStatement::Comment("first comment".to_string()),
                    MacroStatement::Conditional {
                        branches: vec![
                            MacroBranch {
                                condition: Some("[TempVars]![x]=1".to_string()),
                                statements: vec![message("one")],
                            },
                            MacroBranch {
                                condition: Some("[TempVars]![x]<10".to_string()),
                                statements: vec![message("small")],
                            },
                            MacroBranch {
                                condition: None,
                                statements: vec![message("other")],
                            },
                        ],
                    },
                    action("StopMacro", &[]),
                ],
            ),
            (
                "mcrLongText",
                vec![
                    // Access stores at most 255 characters of the 300-character message.
                    message(&"0123456789".repeat(26)[..255]),
                    action(
                        "MessageBox",
                        &[("Message", "日本語のメッセージ"), ("Title", "確認")],
                    ),
                ],
            ),
            ("AutoExec", vec![message("autoexec")]),
        ]
    }

    #[test]
    fn list_macros_real_files() {
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let mut names: Vec<String> = list_macros(&mut reader)
                .unwrap()
                .into_iter()
                .map(|m| m.name)
                .collect();
            names.sort();
            assert_eq!(
                names,
                [
                    "AutoExec",
                    "mcrConditions",
                    "mcrLongText",
                    "mcrSimple",
                    "~TMPCLPMacro"
                ],
                "{file}"
            );
        }
    }

    #[test]
    fn read_macro_real_files() {
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            for (name, expected) in expected_macro_statements() {
                let def = read_macro(&mut reader, name).unwrap();
                assert_eq!(
                    def.source,
                    MacroSource::Named {
                        name: name.to_string()
                    },
                    "{file}: {name}"
                );
                assert_eq!(def.statements, expected, "{file}: {name}");
                assert!(def.xml.contains("<UserInterfaceMacro"), "{file}: {name}");
            }
        }
    }

    #[test]
    fn read_macro_not_found() {
        let path = skip_if_missing!("V2010/macroTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert!(matches!(
            read_macro(&mut reader, "NoSuchMacro"),
            Err(FileError::MacroNotFound { name }) if name == "NoSuchMacro"
        ));
    }

    /// The source of a macro embedded in the form tblItems.
    fn embedded_in_tbl_items(control: Option<&str>, event: &str) -> MacroSource {
        MacroSource::Embedded {
            object_kind: FormObjectType::Form,
            object_name: "tblItems".to_string(),
            control: control.map(str::to_string),
            event: event.to_string(),
        }
    }

    #[test]
    fn read_embedded_macros_real_files() {
        // The form tblItems has a BeforeUpdate macro on the form itself and an
        // OnClick macro on the button btnHello.
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let macros = read_embedded_macros(&mut reader, "tblItems").unwrap();
            assert_eq!(macros.len(), 2, "{file}");
            assert_eq!(
                macros[0].source,
                embedded_in_tbl_items(None, "BeforeUpdate"),
                "{file}"
            );
            assert_eq!(
                macros[0].statements,
                [MacroStatement::Conditional {
                    branches: vec![MacroBranch {
                        condition: Some("[Qty]<0".to_string()),
                        statements: vec![action("Beep", &[])],
                    }]
                }],
                "{file}"
            );
            assert_eq!(
                macros[1].source,
                embedded_in_tbl_items(Some("btnHello"), "OnClick"),
                "{file}"
            );
            assert_eq!(
                macros[1].statements,
                [action("MessageBox", &[("Message", "button clicked")])],
                "{file}"
            );
        }
    }

    #[test]
    fn grids_of_embedded_macros_real_files() {
        // Each macro's grid has its compiled rows, then its XML as comment rows.
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let macros = read_embedded_macros(&mut reader, "tblItems").unwrap();
            let grids: Vec<&MacroGrid> = macros.iter().map(|m| m.grid.as_ref().unwrap()).collect();
            assert_eq!(
                grids[0].rows[0],
                grid_row(1, 4, None, Some("[Qty]<0"), None, &[]),
                "{file}"
            );
            assert_eq!(
                grids[1].rows[0],
                action_row(1, 22, &[Some("button clicked"), Some("-1"), Some("0")]),
                "{file}"
            );
            assert!(grids.iter().all(|g| g.header == "33"
                && g.rows[1..].iter().all(|r| r.action_code == 0
                    && r.comment.as_deref().is_some_and(|c| c.starts_with("_AXL:")))));
        }
    }

    #[test]
    fn read_embedded_macros_generated_files() {
        // frmEmbedded has an OnLoad macro and an OnClick macro on btnHello, and
        // rptEmbedded an OnOpen macro. They were loaded from text holding only
        // their XML, so their grids have only comment rows.
        for file in MACRO_GENERATED_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let mut found = Vec::new();
            for object in ["frmEmbedded", "rptEmbedded"] {
                for m in read_embedded_macros(&mut reader, object).unwrap() {
                    let MacroSource::Embedded { control, event, .. } = &m.source else {
                        panic!("{:?}", m.source);
                    };
                    let grid = m.grid.as_ref().unwrap();
                    assert!(grid.rows.iter().all(|r| r.action_code == 0), "{file}");
                    found.push((object, control.clone(), event.clone()));
                }
            }
            assert_eq!(
                found,
                [
                    ("frmEmbedded", None, "OnLoad".to_string()),
                    (
                        "frmEmbedded",
                        Some("btnHello".to_string()),
                        "OnClick".to_string()
                    ),
                    ("rptEmbedded", None, "OnOpen".to_string()),
                ],
                "{file}"
            );
        }
    }

    #[test]
    fn read_embedded_macros_form_without_macros() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert_eq!(read_embedded_macros(&mut reader, "Form1").unwrap(), []);
    }

    #[test]
    fn read_embedded_macros_not_found() {
        let path = skip_if_missing!("V2010/macroTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert!(matches!(
            read_embedded_macros(&mut reader, "NoSuchForm"),
            Err(FileError::FormNotFound { .. })
        ));
    }

    #[test]
    fn read_data_macros_real_files() {
        // tblItems has one BeforeChange data macro in all three copies.
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let macros = read_data_macros(&mut reader, "tblItems").unwrap();
            assert_eq!(macros.len(), 1, "{file}");
            assert_eq!(
                macros[0].source,
                MacroSource::Data {
                    table: "tblItems".to_string(),
                    event: Some("BeforeChange".to_string()),
                    name: None,
                    parameters: vec![]
                },
                "{file}"
            );
            assert_eq!(
                macros[0].statements,
                [MacroStatement::Conditional {
                    branches: vec![MacroBranch {
                        condition: Some("[Qty]<0".to_string()),
                        statements: vec![action(
                            "RaiseError",
                            &[("Number", "1"), ("Description", " negative qty")]
                        )],
                    }]
                }],
                "{file}"
            );
        }
    }

    #[test]
    fn read_data_macros_generated_files() {
        // tblItems has two table events, in the accdb and its two .mdb copies.
        for file in MACRO_GENERATED_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let events: Vec<Option<String>> = read_data_macros(&mut reader, "tblItems")
                .unwrap()
                .into_iter()
                .map(|m| match m.source {
                    MacroSource::Data { event, .. } => event,
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(
                events,
                [
                    Some("BeforeChange".to_string()),
                    Some("AfterInsert".to_string())
                ],
                "{file}"
            );
        }
    }

    #[test]
    fn read_named_data_macros_generated_files() {
        // tblNamed has one named data macro with a parameter.
        for file in MACRO_GENERATED_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let macros = read_data_macros(&mut reader, "tblNamed").unwrap();
            assert_eq!(macros.len(), 1, "{file}");
            assert_eq!(
                macros[0].source,
                MacroSource::Data {
                    table: "tblNamed".to_string(),
                    event: None,
                    name: Some("dmLog".to_string()),
                    parameters: vec![MacroParameter {
                        name: "msg".to_string(),
                        attributes: vec![],
                    }],
                },
                "{file}"
            );
            assert_eq!(
                macros[0].statements,
                [MacroStatement::DataBlock {
                    kind: "CreateRecord".to_string(),
                    data: vec![("Reference".to_string(), "tblAudit".to_string())],
                    statements: vec![action("SetField", &[("Field", "Note"), ("Value", "[msg]")])],
                }],
                "{file}"
            );
        }
    }

    #[test]
    fn read_data_macros_table_without_macros() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert_eq!(read_data_macros(&mut reader, "Table1").unwrap(), []);
    }

    #[test]
    fn read_data_macros_not_found() {
        let path = skip_if_missing!("V2010/macroTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert!(matches!(
            read_data_macros(&mut reader, "NoSuchTable"),
            Err(FileError::TableNotFound { name }) if name == "NoSuchTable"
        ));
    }

    // -- Macro grid -------------------------------------------------------------

    fn grid_row(
        row: u16,
        action_code: u16,
        macro_name: Option<&str>,
        condition: Option<&str>,
        comment: Option<&str>,
        arguments: &[Option<&str>],
    ) -> MacroGridRow {
        let mut args: Vec<Option<String>> =
            arguments.iter().map(|a| a.map(str::to_string)).collect();
        args.resize(10, None);
        MacroGridRow {
            row,
            macro_name: macro_name.map(str::to_string),
            condition: condition.map(str::to_string),
            comment: comment.map(str::to_string),
            action_code,
            arguments: args,
        }
    }

    #[test]
    fn statements_from_grid_rows() {
        let grid = MacroGrid {
            columns_shown: 3,
            header: String::new(),
            rows: vec![
                grid_row(1, 22, None, Some("[x]=1"), None, &[Some("one")]),
                grid_row(2, 4, None, Some("..."), None, &[]),
                // A new condition closes the open one.
                grid_row(3, 4, None, Some("[x]=2"), None, &[]),
                // A row without a condition closes it too.
                grid_row(4, 0, None, None, Some("note"), &[]),
                // `...` without an open condition is a condition of its own.
                grid_row(5, 4, None, Some("..."), None, &[]),
                grid_row(6, 999, Some("Sub"), None, None, &[None, Some("b")]),
                grid_row(7, 3, None, None, None, &[None, None, None, Some("extra")]),
            ],
        };
        let branch =
            |condition: &str, statements: Vec<MacroStatement>| MacroStatement::Conditional {
                branches: vec![MacroBranch {
                    condition: Some(condition.to_string()),
                    statements,
                }],
            };
        assert_eq!(
            statements_from_grid(&grid),
            [
                branch(
                    "[x]=1",
                    vec![
                        action("MessageBox", &[("Message", "one")]),
                        action("Beep", &[])
                    ]
                ),
                branch("[x]=2", vec![action("Beep", &[])]),
                MacroStatement::Comment("note".to_string()),
                branch("...", vec![action("Beep", &[])]),
                MacroStatement::SubMacro {
                    name: "Sub".to_string(),
                    statements: vec![
                        action("999", &[("1", "b")]),
                        action("ApplyFilter", &[("3", "extra")]),
                    ],
                },
            ]
        );
    }

    #[test]
    fn read_macro_without_xml_in_storage() {
        // One-row macros loaded from `Action ="..."` text have a grid and no XML.
        let path = skip_if_missing!("V2010/macroGeneratedTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let def = read_macro(&mut reader, "old_MsgBox").unwrap();
        assert_eq!(def.xml, "");
        assert_eq!(def.statements, [action("MessageBox", &[])]);
        // Loaded with the arguments 1 to 10. ObjectType 1 is named as a choice.
        let def = read_macro(&mut reader, "oldarg_SendObject").unwrap();
        assert_eq!(
            def.statements,
            [action(
                "EMailDatabaseObject",
                &[
                    ("ObjectType", "Query"),
                    ("ObjectName", "2"),
                    ("OutputFormat", "3"),
                    ("To", "4"),
                    ("Cc", "5"),
                    ("Bcc", "6"),
                    ("Subject", "7"),
                    ("MessageText", "8"),
                    ("EditMessage", "9"),
                    ("TemplateFile", "10"),
                ]
            )]
        );
    }

    #[test]
    fn embedded_macro_without_xml_reads_the_grid() {
        // A grid without _AXL: rows has no control and an empty event, and
        // its statements are those of its rows.
        let bytes = grid_bytes("33", &[0x78, 0x00, 0x00, 0x00]);
        let def = embedded_macro(&bytes, FormObjectType::Report, "rptOld").unwrap();
        let grid = parse_macro_grid(&bytes, TextEncoding::Unicode).unwrap();
        assert_eq!(
            def.source,
            MacroSource::Embedded {
                object_kind: FormObjectType::Report,
                object_name: "rptOld".to_string(),
                control: None,
                event: String::new(),
            }
        );
        assert_eq!(def.xml, "");
        assert!(!def.statements.is_empty());
        assert_eq!(def.statements, statements_from_grid(&grid));
        assert_eq!(def.grid, Some(grid));
    }

    /// A grid with the header string `header` (UTF-16LE) and one row: action
    /// 33 with its first argument stored as `argument`.
    fn grid_bytes(header: &str, argument: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x20];
        let header: Vec<u8> = header.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes.extend_from_slice(&(header.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&[33, 0, 1, 0]);
        let mut offsets = [0xFFFFu16; 14];
        offsets[4] = 0;
        for o in offsets {
            bytes.extend_from_slice(&o.to_le_bytes());
        }
        bytes.extend_from_slice(&(argument.len() as u16).to_le_bytes());
        bytes.extend_from_slice(argument);
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    #[test]
    fn parse_macro_grid_string_encoding_follows_the_header() {
        // UTF-16LE row strings after the header string "33".
        let grid =
            parse_macro_grid(&grid_bytes("33", b"F\0(\0)\0\0\0"), TextEncoding::Unicode).unwrap();
        assert_eq!(grid.rows[0].arguments[0].as_deref(), Some("F()"));
        // In an Access 97 database, row strings are single-byte after any header.
        let grid = parse_macro_grid(
            &grid_bytes("22", b"F()\xe9\0"),
            TextEncoding::Jet3 { code_page: 1252 },
        )
        .unwrap();
        assert_eq!(grid.rows[0].arguments[0].as_deref(), Some("F()é"));
        // Single-byte row strings after an empty header string.
        let grid = parse_macro_grid(&grid_bytes("", b"F()\xe9\0"), TextEncoding::Unicode).unwrap();
        assert_eq!(grid.rows[0].arguments[0].as_deref(), Some("F()é"));
    }

    #[test]
    fn parse_macro_grid_truncated_row_is_an_error() {
        let mut bytes = vec![0u8; 0x20];
        bytes.extend_from_slice(&[0, 0, 0, 0]); // empty header string
        bytes.extend_from_slice(&[0x16, 0x00, 0x01, 0x00]); // action, row, then nothing
        assert!(matches!(
            parse_macro_grid(&bytes, TextEncoding::Unicode),
            Err(FileError::InvalidMacroData { .. })
        ));
    }

    /// A row of the grid of an action alone, without a macro name, a
    /// condition or a comment.
    fn action_row(row: u16, action_code: u16, arguments: &[Option<&str>]) -> MacroGridRow {
        grid_row(row, action_code, None, None, None, arguments)
    }

    #[test]
    fn read_macro_grid_real_files() {
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let grid = read_macro(&mut reader, "mcrSimple").unwrap().grid.unwrap();
            assert_eq!(grid.columns_shown, 0, "{file}");
            let open_form = [
                Some("frmEmbedded"),
                Some("0"),
                None,
                None,
                Some("-1"),
                Some("0"),
            ];
            assert_eq!(
                grid.rows[..3],
                [
                    action_row(1, 0x17, &open_form),
                    action_row(
                        2,
                        0x16,
                        &[Some("Hello"), Some("0"), Some("4"), Some("Title")]
                    ),
                    action_row(3, 0x16, &[Some("Default"), Some("-1"), Some("0")]),
                ],
                "{file}"
            );
            // The XML follows as comment rows.
            assert!(grid.rows[3..].iter().all(|r| r.action_code == 0
                && r.comment.as_deref().is_some_and(|c| c.starts_with("_AXL:"))));
        }
    }

    #[test]
    fn read_macro_grid_of_conditions_real_files() {
        // An If block is stored as conditions on internal SetLocalVar rows.
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let grid = read_macro(&mut reader, "mcrConditions")
                .unwrap()
                .grid
                .unwrap();
            assert_eq!(
                grid.rows[0],
                grid_row(1, 0, None, None, Some("first comment"), &[]),
                "{file}"
            );
            assert_eq!(
                grid.rows[1],
                action_row(2, 0x52, &[Some("__*L0_"), Some("[TempVars]![x]=1")]),
                "{file}"
            );
            assert_eq!(
                grid.rows[3],
                grid_row(
                    4,
                    0x16,
                    None,
                    Some("[LocalVars]![__*L0C_]"),
                    None,
                    &[Some("one"), Some("-1"), Some("0")]
                ),
                "{file}"
            );
        }
    }

    #[test]
    fn access_97_macros() {
        // nwind.mdb is fetched by scripts/fetch-testdata.sh. Its macros have no
        // XML; the grid is the LvExtra of each MSysObjects macro row.
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        let mut names: Vec<String> = list_macros(&mut reader)
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "Customer Labels Dialog",
                "Customer Phone List",
                "Customers",
                "Employees (page break)",
                "Sales Totals by Amount",
                "Sample Autokeys",
                "Suppliers"
            ]
        );
    }

    /// The statements of the macro Customers of nwind.mdb.
    fn customers_statements() -> Vec<MacroStatement> {
        let comment = |text: &str| MacroStatement::Comment(text.to_string());
        vec![
            comment("Attached to the Customers form."),
            comment("Attached to the BeforeUpdate event of the CustomerID field."),
            MacroStatement::SubMacro {
                name: "ValidateID".to_string(),
                statements: vec![
                    MacroStatement::Conditional {
                        branches: vec![MacroBranch {
                            condition: Some("DLookUp(\"[CustomerID]\",\"[Customers]\",\"[CustomerID] = Form.[CustomerID] \") Is Not Null".to_string()),
                            statements: vec![
                                comment("If the value of CustomerID is not unique, display a message."),
                                action(
                                    "MessageBox",
                                    &[
                                        ("Message", "The Customer ID you entered already exists. Enter a unique ID."),
                                        ("Beep", "Yes"),
                                        ("Type", "Information"),
                                        ("Title", "Duplicate Customer ID"),
                                    ]
                                ),
                                comment("Return to the CustomerID control."),
                                action("CancelEvent", &[]),
                            ],
                        }],
                    },
                    comment("Attached to the AfterUpdate event of the form."),
                ],
            },
            MacroStatement::SubMacro {
                name: "Update Country List".to_string(),
                statements: vec![
                    comment("Requery the Country control."),
                    action("Requery", &[("ControlName", "Country")]),
                ],
            },
        ]
    }

    #[test]
    fn access_97_macro_statements() {
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let def = read_macro(&mut reader, "Customers").unwrap();
        assert_eq!(def.xml, "");
        assert_eq!(def.statements, customers_statements());
    }

    #[test]
    fn access_97_macro_grid() {
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let def = read_macro(&mut reader, "Customers").unwrap();
        let grid = def.grid.unwrap();
        assert_eq!(grid.columns_shown, 3);
        assert_eq!(
            grid.rows,
            [
                grid_row(1, 0, None, None, Some("Attached to the Customers form."), &[]),
                grid_row(3, 0, None, None, Some("Attached to the BeforeUpdate event of the CustomerID field."), &[]),
                grid_row(
                    4,
                    0x16,
                    Some("ValidateID"),
                    Some("DLookUp(\"[CustomerID]\",\"[Customers]\",\"[CustomerID] = Form.[CustomerID] \") Is Not Null"),
                    Some("If the value of CustomerID is not unique, display a message."),
                    &[
                        Some("The Customer ID you entered already exists. Enter a unique ID."),
                        Some("-1"),
                        Some("4"),
                        Some("Duplicate Customer ID"),
                    ],
                ),
                grid_row(5, 0x05, None, Some("..."), Some("Return to the CustomerID control."), &[]),
                grid_row(7, 0, None, None, Some("Attached to the AfterUpdate event of the form."), &[]),
                grid_row(8, 0x1C, Some("Update Country List"), None, Some("Requery the Country control."), &[Some("Country")]),
            ]
        );
    }

    #[test]
    fn access_97_macro_grid_of_autokeys() {
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let grid = read_macro(&mut reader, "Sample Autokeys")
            .unwrap()
            .grid
            .unwrap();
        assert_eq!(grid.columns_shown, 1);
        assert_eq!(
            grid.rows[1],
            grid_row(
                2,
                0x22,
                Some("^p"),
                None,
                Some("Run the Customer Phone List.Print macro when Ctrl+P is pressed."),
                &[Some("Customer Phone List.Print")],
            )
        );
    }
}
