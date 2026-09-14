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
    pub statements: Vec<MacroStatement>,
    /// The XML definition the statements were read from.
    pub xml: String,
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
    },
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
pub fn list_macros(reader: &mut PageReader) -> Result<Vec<MacroEntry>, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let Some(mapping) = scripts_dir_mapping(&entries) else {
        return Ok(Vec::new());
    };
    Ok(mapping
        .into_iter()
        .map(|(name, _storage_num)| MacroEntry { name })
        .collect())
}

/// Read the named macro `name`.
///
/// Returns [`FileError::MacroNotFound`] if there is no such macro, and
/// [`FileError::InvalidMacroData`] if its Blob has no XML definition.
pub fn read_macro(reader: &mut PageReader, name: &str) -> Result<MacroDef, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let Some(blob) = find_macro_blob(&entries, name)? else {
        return Err(FileError::MacroNotFound {
            name: name.to_string(),
        });
    };
    let Some(xml) = extract_axl_xml(blob) else {
        return Err(FileError::InvalidMacroData {
            reason: format!("macro {name} has no XML definition"),
        });
    };
    let root = parse_xml_tree(&xml)?;
    Ok(MacroDef {
        source: MacroSource::Named {
            name: name.to_string(),
        },
        statements: statements_of(&root),
        xml,
    })
}

/// Read the embedded macros of the form or report `object_name`, in the order
/// they appear in its `Blob` stream.
///
/// Returns [`FileError::FormNotFound`] if there is no such form or report.
pub fn read_embedded_macros(
    reader: &mut PageReader,
    object_name: &str,
) -> Result<Vec<MacroDef>, FileError> {
    let stream = form::read_form_stream(reader, object_name, StreamKind::Blob)?;
    extract_axl_documents(&stream.data)
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
            })
        })
        .collect()
}

/// `MSysObjects.Type` of a local table.
const MSYSOBJECTS_TYPE_TABLE: i16 = 1;

/// Read the data macros of the local table `table`, in document order.
///
/// Returns an empty list for a table without data macros, and
/// [`FileError::TableNotFound`] if there is no such table. Each returned
/// [`MacroDef::xml`] is the whole XML document stored for the table.
pub fn read_data_macros(reader: &mut PageReader, table: &str) -> Result<Vec<MacroDef>, FileError> {
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
    let rows = read_table_rows(reader, &tdef)?.rows;
    let Some(row) = rows.iter().find(|row| {
        row[name_idx] == Value::Text(table.to_string())
            && row[type_idx] == Value::Int(MSYSOBJECTS_TYPE_TABLE)
    }) else {
        return Err(FileError::TableNotFound {
            name: table.to_string(),
        });
    };
    let Value::Binary(extra) = &row[extra_idx] else {
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
        .map(|element| MacroDef {
            source: MacroSource::Data {
                table: table.to_string(),
                event: element.attribute("Event").map(str::to_string),
                name: element.attribute("Name").map(str::to_string),
            },
            statements: statements_of(element),
            xml: xml.clone(),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Internal: locating a macro's Blob stream
// ---------------------------------------------------------------------------

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
                    name: None
                },
                &MacroSource::Data {
                    table: "T".to_string(),
                    event: None,
                    name: Some("dmNamed".to_string())
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
    // macroTestV2010.accdb and its Access 2002-2003 copy macroTestV2003.mdb
    // hold four named macros created in Access (see testdata/SOURCES.md),
    // plus the designer's internal ~TMPCLPMacro.

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

    const MACRO_TEST_FILES: [&str; 2] = ["V2010/macroTestV2010.accdb", "V2003/macroTestV2003.mdb"];

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
        for file in [
            "V2010/macroTestV2010.accdb",
            "V2003/macroTestV2003.mdb",
            "V2000/macroTestV2000.mdb",
        ] {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let macros = read_data_macros(&mut reader, "tblItems").unwrap();
            assert_eq!(macros.len(), 1, "{file}");
            assert_eq!(
                macros[0].source,
                MacroSource::Data {
                    table: "tblItems".to_string(),
                    event: Some("BeforeChange".to_string()),
                    name: None
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
}
