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
//! Elements this module knows become dedicated [`MacroStatement`] variants;
//! any other element is kept as [`MacroStatement::Unknown`] with its name,
//! attributes, text, and children, so nothing in the XML is dropped.

use crate::file::{FileError, PageReader};
use crate::storage;

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
fn extract_axl_xml(blob: &[u8]) -> Option<String> {
    let mut xml = String::new();
    let mut search_from = 0usize;
    let mut found_any = false;

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
        xml.push_str(chunk_text.strip_prefix("_AXL:").unwrap_or(&chunk_text));
        found_any = true;
        search_from = chunk_end;
    }

    if !found_any {
        return None;
    }
    Some(xml.trim_end_matches('\u{0}').to_string())
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
        _ => None,
    };
    converted.unwrap_or_else(|| MacroStatement::Unknown(element.clone()))
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
}
