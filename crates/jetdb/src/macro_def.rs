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
//! immediately preceding each marker and reassembles the XML; `format_macro_xml`
//! renders it as indented, diff-friendly text approximating the classic
//! macro-grid look.
//!
//! This does NOT reproduce `SaveAsText`'s literal legacy
//! `Condition=`/`Action=`/`Argument=` grid: that format is driven by an
//! internal numeric action-code table that isn't publicly documented and
//! can't be reconstructed from the XML mirror alone (arguments left at their
//! default value aren't serialized into the XML at all, so the argument
//! *count* wouldn't match). Rendering from the XML instead means every
//! action/argument is self-describing by name, and the same renderer handles
//! any action type without needing that table.
//!
//! Pre-2010 (.mdb, Jet3/Jet4) macros predate this XML mirror entirely --
//! `read_macro_text` returns an empty string for those, the same convention
//! `access.rs` already uses for a form/report with no code-behind module.

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

/// List every named macro in the database.
pub fn list_macros(reader: &mut PageReader) -> Result<Vec<MacroEntry>, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let Some(mapping) = scripts_dir_mapping(&entries) else {
        return Ok(Vec::new());
    };
    Ok(mapping.into_iter().map(|(name, _storage_num)| MacroEntry { name }).collect())
}

/// Rendered text for macro `name`'s logic (see module docs for the format),
/// or an empty string if the macro doesn't exist or has no embedded XML
/// mirror (pre-2010 `.mdb` format).
pub fn read_macro_text(reader: &mut PageReader, name: &str) -> Result<String, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let Some(blob) = find_macro_blob(&entries, name)? else {
        return Ok(String::new());
    };
    let Some(xml) = extract_axl_xml(blob) else {
        return Ok(String::new());
    };
    format_macro_xml(&xml)
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
    let Some(scripts_folder) =
        entries.iter().find(|e| e.parent_id == root_id && e.name == "Scripts" && storage::is_storage(e))
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
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

// ---------------------------------------------------------------------------
// Internal: XML -> readable text
// ---------------------------------------------------------------------------

/// A minimal in-memory XML tree node -- the macro XML schema is small and
/// flat enough that a full DOM-style tree isn't needed, but recursive
/// rendering (an `If`/`ElseIf`/`Else` group needing to close with one shared
/// `End If` after all of its siblings) is much simpler over a tree than over
/// raw `quick_xml` events directly.
struct XNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XNode>,
    text: String,
}

impl XNode {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn child_text(&self, name: &str) -> &str {
        self.children.iter().find(|c| c.name == name).map(|c| c.text.as_str()).unwrap_or("")
    }
}

fn parse_xml_tree(xml: &str) -> Result<XNode, FileError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut root = XNode { name: String::new(), attrs: Vec::new(), children: Vec::new(), text: String::new() };
    let mut stack: Vec<XNode> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_string();
                let attrs = read_attrs(&e)?;
                stack.push(XNode { name, attrs, children: Vec::new(), text: String::new() });
            }
            Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_string();
                let attrs = read_attrs(&e)?;
                let node = XNode { name, attrs, children: Vec::new(), text: String::new() };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root.children.push(node),
                }
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
                if let Some(node) = stack.pop() {
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => root.children.push(node),
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(FileError::InvalidMacroData { reason: format!("malformed macro XML: {e}") })
            }
        }
    }

    // The document element (UserInterfaceMacro) is `root`'s only child.
    root.children
        .pop()
        .ok_or_else(|| FileError::InvalidMacroData { reason: "empty macro XML document".to_string() })
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

fn format_macro_xml(xml: &str) -> Result<String, FileError> {
    let root = parse_xml_tree(xml)?;
    let mut out = String::new();
    if let Some(event) = root.attr("Event") {
        out.push_str(&format!("' Event: {event}\n"));
    }
    render_children(&root, 0, &mut out);
    Ok(out)
}

const INDENT: &str = "    ";

fn pad(indent: usize) -> String {
    INDENT.repeat(indent)
}

/// Renders every child of `node` in document order at the given indent
/// level. Used both for the document root and for any node whose own
/// children should appear without an extra wrapping header (`Statements`).
fn render_children(node: &XNode, indent: usize, out: &mut String) {
    for child in &node.children {
        render_node(child, indent, out);
    }
}

fn render_node(node: &XNode, indent: usize, out: &mut String) {
    match node.name.as_str() {
        "Statements" => render_children(node, indent, out),
        "Comment" => {
            if !node.text.trim().is_empty() {
                out.push_str(&format!("{}' {}\n", pad(indent), node.text));
            }
        }
        "Action" => render_action(node, indent, out),
        "ConditionalBlock" => render_conditional_block(node, indent, out),
        "StatementGroup" => {
            let label = node.attr("Description").unwrap_or("");
            out.push_str(&format!("{}Group \"{label}\"\n", pad(indent)));
            if let Some(stmts) = node.children.iter().find(|c| c.name == "Statements") {
                render_children(stmts, indent + 1, out);
            }
            out.push_str(&format!("{}End Group\n", pad(indent)));
        }
        "SubMacro" => {
            let label = node.attr("Name").unwrap_or("");
            out.push_str(&format!("{}Sub {label}\n", pad(indent)));
            if let Some(stmts) = node.children.iter().find(|c| c.name == "Statements") {
                render_children(stmts, indent + 1, out);
            }
            out.push_str(&format!("{}End Sub\n", pad(indent)));
        }
        // Unrecognized element: still surface it (name + attributes) and
        // recurse into its children, rather than silently dropping data for
        // a macro XML schema variant this renderer hasn't been taught yet.
        other => {
            let attr_text: String =
                node.attrs.iter().map(|(k, v)| format!(" {k}=\"{v}\"")).collect();
            out.push_str(&format!("{}<{other}{attr_text}>\n", pad(indent)));
            render_children(node, indent + 1, out);
        }
    }
}

fn render_action(node: &XNode, indent: usize, out: &mut String) {
    let name = node.attr("Name").unwrap_or("");
    out.push_str(&format!("{}{name}\n", pad(indent)));
    for arg in node.children.iter().filter(|c| c.name == "Argument") {
        let arg_name = arg.attr("Name").unwrap_or("");
        out.push_str(&format!("{}{arg_name} =\"{}\"\n", pad(indent + 1), quote_escape(&arg.text)));
    }
}

/// A `ConditionalBlock` wraps one `If`, zero or more `ElseIf`, and an
/// optional `Else` -- rendered as a single `If`/`ElseIf`/`Else`/`End If`
/// group sharing one closing line, not one per branch.
fn render_conditional_block(node: &XNode, indent: usize, out: &mut String) {
    for branch in &node.children {
        match branch.name.as_str() {
            "If" => render_branch(branch, "If", indent, out),
            "ElseIf" => render_branch(branch, "ElseIf", indent, out),
            "Else" => {
                out.push_str(&format!("{}Else\n", pad(indent)));
                if let Some(stmts) = branch.children.iter().find(|c| c.name == "Statements") {
                    render_children(stmts, indent + 1, out);
                }
            }
            _ => render_node(branch, indent, out),
        }
    }
    out.push_str(&format!("{}End If\n", pad(indent)));
}

fn render_branch(node: &XNode, keyword: &str, indent: usize, out: &mut String) {
    let condition = node.child_text("Condition");
    out.push_str(&format!("{}{keyword} {condition}\n", pad(indent)));
    if let Some(stmts) = node.children.iter().find(|c| c.name == "Statements") {
        render_children(stmts, indent + 1, out);
    }
}

/// Escapes embedded backslashes/double-quotes so an argument's raw
/// (already XML-unescaped) value stays unambiguous inside this renderer's
/// own `Name ="value"` quoting -- the same convention a real
/// `Application.SaveAsText` export uses for its `Argument ="..."` lines.
fn quote_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axl_chunk_header(chunk_including_prefix_and_marker: &str) -> Vec<u8> {
        let text_bytes: Vec<u8> =
            chunk_including_prefix_and_marker.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
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
    fn format_macro_xml_action_with_arguments() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><Action Name="OpenForm"><Argument Name="FormName">frmStartup</Argument></Action></Statements></UserInterfaceMacro>"#;
        let text = format_macro_xml(xml).unwrap();
        assert_eq!(text, "OpenForm\n    FormName =\"frmStartup\"\n");
    }

    #[test]
    fn format_macro_xml_conditional_block_if_elseif_else() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><ConditionalBlock><If><Condition>A</Condition><Statements><Action Name="X"/></Statements></If><ElseIf><Condition>B</Condition><Statements><Action Name="Y"/></Statements></ElseIf><Else><Statements><Action Name="Z"/></Statements></Else></ConditionalBlock></Statements></UserInterfaceMacro>"#;
        let text = format_macro_xml(xml).unwrap();
        assert_eq!(
            text,
            "If A\n    X\nElseIf B\n    Y\nElse\n    Z\nEnd If\n"
        );
    }

    #[test]
    fn format_macro_xml_comment_and_event_attribute() {
        let xml = r#"<UserInterfaceMacro Event="OnUnload" xmlns="ns"><Statements><Comment>hello</Comment><Action Name="StopMacro"/></Statements></UserInterfaceMacro>"#;
        let text = format_macro_xml(xml).unwrap();
        assert_eq!(text, "' Event: OnUnload\n' hello\nStopMacro\n");
    }

    #[test]
    fn format_macro_xml_resolves_references_in_conditions() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><ConditionalBlock><If><Condition>[MacroError]&lt;&gt;0 And [x]&#62;1</Condition><Statements><Action Name="X"/></Statements></If></ConditionalBlock></Statements></UserInterfaceMacro>"#;
        let text = format_macro_xml(xml).unwrap();
        assert_eq!(text, "If [MacroError]<>0 And [x]>1\n    X\nEnd If\n");
    }

    #[test]
    fn format_macro_xml_statement_group() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><StatementGroup Description="Foo"><Statements><Action Name="X"/></Statements></StatementGroup></Statements></UserInterfaceMacro>"#;
        let text = format_macro_xml(xml).unwrap();
        assert_eq!(text, "Group \"Foo\"\n    X\nEnd Group\n");
    }

    #[test]
    fn format_macro_xml_escapes_quotes_and_backslashes_in_argument_values() {
        let xml = r#"<UserInterfaceMacro xmlns="ns"><Statements><Action Name="SetValue"><Argument Name="Expression">say "hi" \ bye</Argument></Action></Statements></UserInterfaceMacro>"#;
        let text = format_macro_xml(xml).unwrap();
        assert_eq!(text, "SetValue\n    Expression =\"say \\\"hi\\\" \\\\ bye\"\n");
    }

    #[test]
    fn format_macro_xml_real_autoexec_dev_matches_known_actions() {
        // Reconstructed from MS NorthwindDev.accdb's AutoExec macro Blob
        // (see module docs) -- both actions and both conditions round-trip.
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-16" standalone="no"?>"#,
            r#"<UserInterfaceMacro MinimumClientDesignVersion="14.0.0000.0000" xmlns="http://schemas.microsoft.com/office/accessservices/2009/11/application">"#,
            r#"<Statements><ConditionalBlock><If><Condition>Not [CurrentProject].[IsTrusted]</Condition>"#,
            r#"<Statements><Action Name="OpenForm"><Argument Name="FormName">frmStartup</Argument></Action></Statements></If>"#,
            r#"</ConditionalBlock><ConditionalBlock><If><Condition>[CurrentProject].[IsTrusted]</Condition>"#,
            r#"<Statements><Action Name="RunCode"><Argument Name="FunctionName">Startup()</Argument></Action></Statements></If>"#,
            r#"</ConditionalBlock></Statements></UserInterfaceMacro>"#,
        );
        let text = format_macro_xml(xml).unwrap();
        assert_eq!(
            text,
            "If Not [CurrentProject].[IsTrusted]\n    OpenForm\n        FormName =\"frmStartup\"\nEnd If\nIf [CurrentProject].[IsTrusted]\n    RunCode\n        FunctionName =\"Startup()\"\nEnd If\n"
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

    fn expected_macro_texts() -> Vec<(&'static str, String)> {
        vec![
            (
                "mcrSimple",
                "OpenForm\n    FormName =\"frmEmbedded\"\nMessageBox\n    Message =\"Hello\"\n    Beep =\"No\"\n    Type =\"Information\"\n    Title =\"Title\"\nMessageBox\n    Message =\"Default\"\n".to_string(),
            ),
            (
                "mcrConditions",
                "' first comment\nIf [TempVars]![x]=1\n    MessageBox\n        Message =\"one\"\nElseIf [TempVars]![x]<10\n    MessageBox\n        Message =\"small\"\nElse\n    MessageBox\n        Message =\"other\"\nEnd If\nStopMacro\n".to_string(),
            ),
            (
                "mcrLongText",
                // Access stores at most 255 characters of the 300-character message.
                format!(
                    "MessageBox\n    Message =\"{}\"\nMessageBox\n    Message =\"日本語のメッセージ\"\n    Title =\"確認\"\n",
                    &"0123456789".repeat(26)[..255]
                ),
            ),
            ("AutoExec", "MessageBox\n    Message =\"autoexec\"\n".to_string()),
        ]
    }

    #[test]
    fn list_macros_real_files() {
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let mut names: Vec<String> = list_macros(&mut reader).unwrap().into_iter().map(|m| m.name).collect();
            names.sort();
            assert_eq!(
                names,
                ["AutoExec", "mcrConditions", "mcrLongText", "mcrSimple", "~TMPCLPMacro"],
                "{file}"
            );
        }
    }

    #[test]
    fn read_macro_text_real_files() {
        for file in MACRO_TEST_FILES {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            for (name, expected) in expected_macro_texts() {
                let text = read_macro_text(&mut reader, name).unwrap();
                assert_eq!(text, expected, "{file}: {name}");
            }
        }
    }
}
