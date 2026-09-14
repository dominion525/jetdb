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

use crate::data::{read_table_rows, Value};
use crate::file::{FileError, PageReader};
use crate::form::{self, FormObjectType, StreamKind};
use crate::format::CATALOG_PAGE;
use crate::macro_action::macro_action;
use crate::storage;
use crate::table::read_table_def;

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
    /// names, a row with a macro name starts a [`MacroStatement::SubMacro`],
    /// and a condition starts a [`MacroStatement::Conditional`] that the
    /// following `...` rows continue. An action whose number is not in that
    /// table is named by its number, and so is an argument beyond its
    /// argument names.
    pub statements: Vec<MacroStatement>,
    /// The XML definition the statements were read from, or empty for a macro
    /// stored without XML.
    pub xml: String,
    /// The macro grid of a named macro (see [`MacroGrid`]), or `None` for
    /// embedded and data macros.
    pub grid: Option<MacroGrid>,
}

/// The rows of a named macro as stored in its binary grid.
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
    let entries = storage::read_storage_entries(reader)?;
    if let Some(mapping) = scripts_dir_mapping(&entries) {
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
    let entries = storage::read_storage_entries(reader)?;
    let source = MacroSource::Named {
        name: name.to_string(),
    };
    let not_found = || FileError::MacroNotFound {
        name: name.to_string(),
    };

    if scripts_dir_mapping(&entries).is_none() {
        // Access 97: the grid is the LvExtra of the macro's MSysObjects row.
        let (_, extra) = msysobjects_rows(reader, MSYSOBJECTS_TYPE_MACRO)?
            .into_iter()
            .find(|(n, _)| n == name)
            .ok_or_else(not_found)?;
        let grid = parse_macro_grid(&extra.unwrap_or_default(), true)?;
        return Ok(MacroDef {
            source,
            statements: statements_from_grid(&grid),
            xml: String::new(),
            grid: Some(grid),
        });
    }

    let blob = find_macro_blob(&entries, name)?.ok_or_else(not_found)?;
    match extract_axl_xml(blob) {
        Some(xml) => {
            let root = parse_xml_tree(&xml)?;
            Ok(MacroDef {
                source,
                statements: statements_of(&root),
                grid: parse_macro_grid(blob, false).ok(),
                xml,
            })
        }
        None => {
            let grid = parse_macro_grid(blob, false)?;
            Ok(MacroDef {
                source,
                statements: statements_from_grid(&grid),
                xml: String::new(),
                grid: Some(grid),
            })
        }
    }
}

/// Read the embedded macros of the form or report `object_name`, in the order
/// they appear in its `Blob` stream and then its `BlobDelta` stream.
///
/// Databases in the Access 2000 format keep the macros in `BlobDelta`. A
/// document found in both streams is returned once.
///
/// Returns [`FileError::FormNotFound`] if there is no such form or report.
pub fn read_embedded_macros(
    reader: &mut PageReader,
    object_name: &str,
) -> Result<Vec<MacroDef>, FileError> {
    let stream = form::read_form_stream(reader, object_name, StreamKind::Blob)?;
    let mut documents = extract_axl_documents(&stream.data);
    if let Ok(delta) = form::read_form_stream(reader, object_name, StreamKind::BlobDelta) {
        for document in extract_axl_documents(&delta.data) {
            if !documents.contains(&document) {
                documents.push(document);
            }
        }
    }
    documents
        .into_iter()
        .map(|xml| {
            let root = parse_xml_tree(&xml)?;
            Ok(MacroDef {
                source: MacroSource::Embedded {
                    object_kind: stream.object_type,
                    object_name: object_name.to_string(),
                    control: root.attribute("For").map(str::to_string),
                    event: root.attribute("Event").unwrap_or("").to_string(),
                },
                statements: statements_of(&root),
                xml,
                grid: None,
            })
        })
        .collect()
}

/// `MSysObjects.Type` of a local table.
const MSYSOBJECTS_TYPE_TABLE: i16 = 1;
/// `MSysObjects.Type` of a macro.
const MSYSOBJECTS_TYPE_MACRO: i16 = -32766;

/// Read the data macros of the local table `table`, in document order.
///
/// Returns an empty list for a table without data macros, and
/// [`FileError::TableNotFound`] if there is no such table. Each returned
/// [`MacroDef::xml`] is the whole XML document stored for the table.
pub fn read_data_macros(reader: &mut PageReader, table: &str) -> Result<Vec<MacroDef>, FileError> {
    let (_, extra) = msysobjects_rows(reader, MSYSOBJECTS_TYPE_TABLE)?
        .into_iter()
        .find(|(name, _)| name == table)
        .ok_or_else(|| FileError::TableNotFound {
            name: table.to_string(),
        })?;
    let Some(extra) = extra else {
        return Ok(Vec::new());
    };

    let mut macros = Vec::new();
    for xml in extract_utf16_xml_documents(&extra) {
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
        .map(|element| MacroDef {
            source: MacroSource::Data {
                table: table.to_string(),
                event: element.attribute("Event").map(str::to_string),
                name: element.attribute("Name").map(str::to_string),
                parameters: element
                    .child("Parameters")
                    .map(|p| {
                        p.children
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
                    })
                    .unwrap_or_default(),
            },
            statements: statements_of(element),
            xml: xml.clone(),
            grid: None,
        })
        .collect())
}

/// The name and `LvExtra` of every `MSysObjects` row of type `object_type`.
fn msysobjects_rows(
    reader: &mut PageReader,
    object_type: i16,
) -> Result<Vec<(String, Option<Vec<u8>>)>, FileError> {
    let tdef = read_table_def(reader, "MSysObjects", CATALOG_PAGE)?;
    let column = |name: &str| {
        tdef.columns
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| FileError::InvalidMacroData {
                reason: format!("MSysObjects has no {name} column"),
            })
    };
    let (name_idx, type_idx, extra_idx) = (column("Name")?, column("Type")?, column("LvExtra")?);
    Ok(read_table_rows(reader, &tdef)?
        .rows
        .into_iter()
        .filter(|row| row[type_idx] == Value::Int(object_type))
        .filter_map(|row| {
            let Value::Text(name) = &row[name_idx] else {
                return None;
            };
            let extra = match &row[extra_idx] {
                Value::Binary(b) => Some(b.clone()),
                _ => None,
            };
            Some((name.clone(), extra))
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Internal: macro grid
// ---------------------------------------------------------------------------

/// Parses a macro grid.
///
/// Layout, as observed in files from Access 97 through Microsoft 365:
/// - bytes 0..4: [`MacroGrid::columns_shown`]; bytes 4..0x20: not interpreted
/// - at 0x20: a 2-byte length `n`, `n` bytes of a UTF-16LE header string, and
///   2 zero bytes
/// - row strings: single-byte in Access 97 databases (`single_byte_file`),
///   whatever the header string (`22` or empty there). In later formats they
///   are UTF-16LE after the header string `33` (or `23` in the designer's
///   clipboard macro), and single-byte after an empty header string, which
///   macros converted from Access 97 keep.
/// - rows until the end, each: action number (u16), row number (u16), 4 field
///   offsets (u16: not interpreted, comment, condition, macro name), 10
///   argument offsets (u16), a 2-byte length `m`, `m` bytes of NUL-terminated
///   strings the offsets point into (0xFFFF for none), and 2 zero bytes
fn parse_macro_grid(bytes: &[u8], single_byte_file: bool) -> Result<MacroGrid, FileError> {
    let invalid = |what: &str| FileError::InvalidMacroData {
        reason: format!("macro grid: {what}"),
    };
    let read_u16 = |offset: usize| {
        bytes
            .get(offset..offset + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]))
            .ok_or_else(|| invalid("unexpected end of data"))
    };
    let columns_shown = bytes
        .get(0..4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| invalid("unexpected end of data"))?;
    let header_len = read_u16(0x20)? as usize;
    let header_units: Vec<u16> = bytes
        .get(0x22..0x22 + header_len)
        .ok_or_else(|| invalid("header string past the end of data"))?
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let header = String::from_utf16_lossy(&header_units);
    let single_byte = single_byte_file || header_len == 0;
    let mut pos = 0x22 + header_len + 2;

    let mut rows = Vec::new();
    while pos < bytes.len() {
        let action_code = read_u16(pos)?;
        let row = read_u16(pos + 2)?;
        let offsets = (0..14)
            .map(|k| read_u16(pos + 4 + 2 * k))
            .collect::<Result<Vec<u16>, FileError>>()?;
        let len = read_u16(pos + 32)? as usize;
        let block = bytes
            .get(pos + 34..pos + 34 + len)
            .ok_or_else(|| invalid("row strings past the end of data"))?;
        let string = |offset: u16| -> Result<Option<String>, FileError> {
            if offset == 0xFFFF {
                return Ok(None);
            }
            let rest = block
                .get(offset as usize..)
                .ok_or_else(|| invalid("string offset past the row"))?;
            Ok(Some(if single_byte {
                let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                crate::encoding::decode_latin1(&rest[..end])
            } else {
                let units: Vec<u16> = rest
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&u| u != 0)
                    .collect();
                String::from_utf16_lossy(&units)
            }))
        };
        rows.push(MacroGridRow {
            row,
            comment: string(offsets[1])?,
            condition: string(offsets[2])?,
            macro_name: string(offsets[3])?,
            action_code,
            arguments: offsets[4..]
                .iter()
                .map(|&o| string(o))
                .collect::<Result<Vec<_>, FileError>>()?,
        });
        pos += 34 + len + 2;
    }
    Ok(MacroGrid {
        columns_shown,
        header,
        rows,
    })
}

// ---------------------------------------------------------------------------
// Internal: locating a macro's Blob stream
// ---------------------------------------------------------------------------

/// The condition that continues the condition of the rows above it.
const CONTINUED_CONDITION: &str = "...";

/// The statements of a macro stored without XML, read from its grid rows.
fn statements_from_grid(grid: &MacroGrid) -> Vec<MacroStatement> {
    let mut top = Vec::new();
    // The submacro being read, and the open condition within it.
    let mut submacro: Option<(String, Vec<MacroStatement>)> = None;
    let mut branch: Option<MacroBranch> = None;

    for row in &grid.rows {
        if let Some(name) = &row.macro_name {
            finish_submacro(&mut top, &mut submacro, &mut branch);
            submacro = Some((name.clone(), Vec::new()));
        }
        let continues = row.condition.as_deref() == Some(CONTINUED_CONDITION) && branch.is_some();
        if !continues {
            let enclosing = submacro.as_mut().map_or(&mut top, |(_, s)| s);
            close_branch(enclosing, &mut branch);
            branch = row.condition.as_ref().map(|condition| MacroBranch {
                condition: Some(condition.clone()),
                statements: Vec::new(),
            });
        }
        let statements = match &mut branch {
            Some(b) => &mut b.statements,
            None => submacro.as_mut().map_or(&mut top, |(_, s)| s),
        };
        if let Some(comment) = &row.comment {
            statements.push(MacroStatement::Comment(comment.clone()));
        }
        if row.action_code != 0 {
            statements.push(grid_action(row));
        }
    }
    finish_submacro(&mut top, &mut submacro, &mut branch);
    top
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
                Some(MacroArgument {
                    name: argument_names
                        .get(i)
                        .map_or_else(|| i.to_string(), |n| n.to_string()),
                    value: value.clone()?,
                })
            })
            .collect(),
    }
}

fn scripts_dir_mapping(entries: &[storage::StorageEntry]) -> Option<Vec<(String, String)>> {
    let root_id = storage::find_root_id(entries);
    let scripts_folder = entries
        .iter()
        .find(|e| e.parent_id == root_id && e.name == "Scripts" && storage::is_storage(e))?;
    let dir_data = storage::find_dir_data(entries, scripts_folder.id)?;
    storage::parse_dir_data(&dir_data.data).ok()
}

fn find_macro_blob<'a>(
    entries: &'a [storage::StorageEntry],
    name: &str,
) -> Result<Option<&'a [u8]>, FileError> {
    let root_id = storage::find_root_id(entries);
    let Some(scripts_folder) = entries
        .iter()
        .find(|e| e.parent_id == root_id && e.name == "Scripts" && storage::is_storage(e))
    else {
        return Ok(None);
    };
    let Some(dir_data) = storage::find_dir_data(entries, scripts_folder.id) else {
        return Ok(None);
    };
    let mapping = storage::parse_dir_data(&dir_data.data)?;
    let Some((_, storage_num)) = mapping.iter().find(|(n, _)| n == name) else {
        return Ok(None);
    };
    let Some(macro_folder) = entries.iter().find(|e| {
        e.parent_id == scripts_folder.id && &e.name == storage_num && storage::is_storage(e)
    }) else {
        return Ok(None);
    };
    Ok(entries
        .iter()
        .find(|e| e.parent_id == macro_folder.id && e.name == "Blob" && !storage::is_storage(e))
        .map(|e| e.data.as_slice()))
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

/// Parses `xml` into an element tree and returns its document element.
fn parse_xml_tree(xml: &str) -> Result<MacroXmlElement, FileError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut root = MacroXmlElement {
        name: String::new(),
        attributes: Vec::new(),
        text: String::new(),
        children: Vec::new(),
    };
    let mut stack: Vec<MacroXmlElement> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                stack.push(MacroXmlElement {
                    name: e.name().as_ref().to_string(),
                    attributes: read_attrs(&e)?,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            Ok(Event::Empty(e)) => {
                let element = MacroXmlElement {
                    name: e.name().as_ref().to_string(),
                    attributes: read_attrs(&e)?,
                    text: String::new(),
                    children: Vec::new(),
                };
                stack.last_mut().unwrap_or(&mut root).children.push(element);
            }
            Ok(Event::Text(t)) => {
                let text = t.xml_content(quick_xml::XmlVersion::Explicit1_0);
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text);
                }
            }
            // Entity and character references (`&lt;`, `&#60;`) arrive as
            // separate events rather than inside the surrounding text.
            Ok(Event::GeneralRef(r)) => {
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(ch)) => ch.to_string(),
                    Ok(None) => quick_xml::escape::resolve_xml_entity(&r)
                        .ok_or_else(|| FileError::InvalidMacroData {
                            reason: format!("unknown entity in macro XML: &{};", &*r),
                        })?
                        .to_string(),
                    Err(e) => {
                        return Err(FileError::InvalidMacroData {
                            reason: format!("invalid character reference in macro XML: {e}"),
                        })
                    }
                };
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&resolved);
                }
            }
            Ok(Event::End(_)) => {
                if let Some(mut element) = stack.pop() {
                    // Indentation between child elements is not content.
                    if !element.children.is_empty() && element.text.trim().is_empty() {
                        element.text.clear();
                    }
                    stack.last_mut().unwrap_or(&mut root).children.push(element);
                }
            }
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
    if !element.attributes.is_empty()
        || !element.text.is_empty()
        || element.child("Statements").is_none()
        || element
            .children
            .iter()
            .any(|c| c.name != "Statements" && c.name != "Data")
    {
        return None;
    }
    let data = match element.child("Data") {
        Some(d) if !d.attributes.is_empty() || !d.text.is_empty() => return None,
        Some(d) => d
            .children
            .iter()
            .map(|c| {
                (c.attributes.is_empty() && c.children.is_empty())
                    .then(|| (c.name.clone(), c.text.clone()))
            })
            .collect::<Option<Vec<_>>>()?,
        None => Vec::new(),
    };
    Some(MacroStatement::DataBlock {
        kind: element.name.clone(),
        data,
        statements: statements_of(element),
    })
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

    const MACRO_TEST_FILES: [&str; 3] = [
        "V2010/macroTestV2010.accdb",
        "V2003/macroTestV2003.mdb",
        "V2000/macroTestV2000.mdb",
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

    #[test]
    fn read_embedded_macros_real_files() {
        // The form tblItems has a BeforeUpdate macro on the form itself and an
        // OnClick macro on the button btnHello.
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let macros = read_embedded_macros(&mut reader, "tblItems").unwrap();
            let embedded = |control: Option<&str>, event: &str| MacroSource::Embedded {
                object_kind: FormObjectType::Form,
                object_name: "tblItems".to_string(),
                control: control.map(str::to_string),
                event: event.to_string(),
            };
            assert_eq!(macros.len(), 2, "{file}");
            assert_eq!(macros[0].source, embedded(None, "BeforeUpdate"), "{file}");
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
                embedded(Some("btnHello"), "OnClick"),
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
        // tblItems has two table events and tblNamed one named data macro with
        // a parameter, in the accdb and its two .mdb copies.
        for file in [
            "V2010/macroGeneratedTestV2010.accdb",
            "V2003/macroGeneratedTestV2003.mdb",
            "V2000/macroGeneratedTestV2000.mdb",
        ] {
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
        // Loaded with the arguments 1 to 10.
        let def = read_macro(&mut reader, "oldarg_SendObject").unwrap();
        assert_eq!(
            def.statements,
            [action(
                "EMailDatabaseObject",
                &[
                    ("ObjectType", "1"),
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
        let grid = parse_macro_grid(&grid_bytes("33", b"F\0(\0)\0\0\0"), false).unwrap();
        assert_eq!(grid.rows[0].arguments[0].as_deref(), Some("F()"));
        // In an Access 97 database, row strings are single-byte after any header.
        let grid = parse_macro_grid(&grid_bytes("22", b"F()\xe9\0"), true).unwrap();
        assert_eq!(grid.rows[0].arguments[0].as_deref(), Some("F()é"));
        // Single-byte row strings after an empty header string.
        let grid = parse_macro_grid(&grid_bytes("", b"F()\xe9\0"), false).unwrap();
        assert_eq!(grid.rows[0].arguments[0].as_deref(), Some("F()é"));
    }

    #[test]
    fn parse_macro_grid_truncated_row_is_an_error() {
        let mut bytes = vec![0u8; 0x20];
        bytes.extend_from_slice(&[0, 0, 0, 0]); // empty header string
        bytes.extend_from_slice(&[0x16, 0x00, 0x01, 0x00]); // action, row, then nothing
        assert!(matches!(
            parse_macro_grid(&bytes, false),
            Err(FileError::InvalidMacroData { .. })
        ));
    }

    #[test]
    fn read_macro_grid_real_files() {
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();

            let grid = read_macro(&mut reader, "mcrSimple").unwrap().grid.unwrap();
            assert_eq!(grid.columns_shown, 0, "{file}");
            assert_eq!(
                grid.rows[..3],
                [
                    grid_row(
                        1,
                        0x17,
                        None,
                        None,
                        None,
                        &[
                            Some("frmEmbedded"),
                            Some("0"),
                            None,
                            None,
                            Some("-1"),
                            Some("0")
                        ]
                    ),
                    grid_row(
                        2,
                        0x16,
                        None,
                        None,
                        None,
                        &[Some("Hello"), Some("0"), Some("4"), Some("Title")]
                    ),
                    grid_row(
                        3,
                        0x16,
                        None,
                        None,
                        None,
                        &[Some("Default"), Some("-1"), Some("0")]
                    ),
                ],
                "{file}"
            );
            // The XML follows as comment rows.
            assert!(grid.rows[3..].iter().all(|r| r.action_code == 0
                && r.comment.as_deref().is_some_and(|c| c.starts_with("_AXL:"))));

            // An If block is stored as conditions on internal SetLocalVar rows.
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
                grid_row(
                    2,
                    0x52,
                    None,
                    None,
                    None,
                    &[Some("__*L0_"), Some("[TempVars]![x]=1")]
                ),
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

        let def = read_macro(&mut reader, "Customers").unwrap();
        assert_eq!(def.xml, "");
        let comment = |text: &str| MacroStatement::Comment(text.to_string());
        assert_eq!(
            def.statements,
            [
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
                                            ("Beep", "-1"),
                                            ("Type", "4"),
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
        );
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
