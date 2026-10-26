//! The text `Application.SaveAsText` writes for a named macro.
//!
//! Each grid row becomes a `Begin` ... `End` block holding, in this order, the
//! row's `MacroName`, `Condition`, `Action`, `Comment`, and `Argument` lines.
//! Arguments are written up to the last non-empty one, empty arguments before
//! it as `Argument =""`. A value is quoted with `"` and `\` escaped by a
//! backslash and control characters as three-digit octal escapes (`\015`),
//! and is wrapped into quoted pieces of 80 characters of escaped text, each
//! piece extended to the end of an escape sequence it would otherwise split.
//!
//! [`embedded_macro_to_text`] gives the block an embedded macro takes in its
//! form's text, and [`data_macros_to_text`] the text `Application.SaveAsText` writes for
//! a table's data macros.

use crate::macro_action::macro_action;
use crate::macro_def::{MacroDef, MacroGrid, MacroGridRow, MacroSource};

/// The table events in the order `Application.SaveAsText` writes their data
/// macros.
const DATA_MACRO_EVENTS: [&str; 5] = [
    "AfterInsert",
    "AfterUpdate",
    "AfterDelete",
    "BeforeChange",
    "BeforeDelete",
];

/// The text `Application.SaveAsText` writes for the data macros of a table,
/// given as [`read_data_macros`](crate::read_data_macros) returns them, or an
/// empty string if there are none.
///
/// Access stores each data macro as its own XML document, and writes them as
/// one `DataMacros` document: the XML declaration and a CRLF, then a
/// `DataMacros` element carrying the namespace, holding each `DataMacro`
/// element without its own namespace attribute. The table events come first,
/// in the order AfterInsert, AfterUpdate, AfterDelete, BeforeChange,
/// BeforeDelete; the named data macros follow in their stored order.
pub fn data_macros_to_text(macros: &[MacroDef]) -> String {
    let mut namespace = None;
    let mut body = String::new();
    for (def, index) in ordered_data_macros(macros) {
        let Some(element) = data_macro_elements(&def.xml).into_iter().nth(index) else {
            continue;
        };
        push_data_macro(&mut body, &mut namespace, &def.xml, element);
    }
    match namespace {
        None if body.is_empty() => String::new(),
        namespace => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-16\" standalone=\"no\"?>\r\n<DataMacros{}>{body}</DataMacros>",
            namespace.unwrap_or_default()
        ),
    }
}

/// The order of a data macro: the table events in the order of
/// [`DATA_MACRO_EVENTS`], then the named data macros.
fn data_macro_rank(def: &MacroDef) -> usize {
    match &def.source {
        MacroSource::Data {
            event: Some(event), ..
        } => DATA_MACRO_EVENTS
            .iter()
            .position(|e| e == event)
            .unwrap_or(DATA_MACRO_EVENTS.len()),
        _ => DATA_MACRO_EVENTS.len() + 1,
    }
}

/// The data macros in the order SaveAsText writes them, each with the index
/// of its `DataMacro` element in its document.
fn ordered_data_macros(macros: &[MacroDef]) -> Vec<(&MacroDef, usize)> {
    // A document may hold several data macros, each read as a MacroDef whose
    // xml is the whole document: the n-th of the macros sharing a document is
    // its n-th DataMacro element.
    let mut seen: Vec<(&str, usize)> = Vec::new();
    let mut ordered: Vec<(&MacroDef, usize)> = Vec::new();
    for def in macros {
        let index = match seen.iter_mut().find(|(xml, _)| *xml == def.xml) {
            Some((_, count)) => {
                *count += 1;
                *count - 1
            }
            None => {
                seen.push((&def.xml, 1));
                0
            }
        };
        ordered.push((def, index));
    }
    ordered.sort_by_key(|(def, _)| data_macro_rank(def));
    ordered
}

/// Adds the `DataMacro` `element` of the document `xml` to `body` without
/// its namespace attribute, and keeps the namespace, of the element or of its
/// `DataMacros` root, in `namespace` if none is kept yet.
fn push_data_macro(body: &mut String, namespace: &mut Option<String>, xml: &str, element: &str) {
    let tag_end = element.find('>').unwrap_or(element.len());
    match find_attribute(&element[..tag_end], " xmlns=\"") {
        Some((from, to)) => {
            namespace.get_or_insert_with(|| element[from..to].to_string());
            body.push_str(&element[..from]);
            body.push_str(&element[to..]);
        }
        None => {
            // In a DataMacros document, the namespace is on DataMacros.
            if let Some(root) = xml.find("<DataMacros") {
                let root_tag = &xml[root..];
                let root_end = root_tag.find('>').unwrap_or(root_tag.len());
                if let Some((from, to)) = find_attribute(&root_tag[..root_end], " xmlns=\"") {
                    namespace.get_or_insert_with(|| root_tag[from..to].to_string());
                }
            }
            body.push_str(element);
        }
    }
}

/// The `DataMacro` elements of a data macro document, in document order:
/// the root element, or the children of a `DataMacros` root. A `DataMacro`
/// element does not hold another, so each runs to the first `</DataMacro>`
/// after it, or ends its own tag with `/>`.
fn data_macro_elements(xml: &str) -> Vec<&str> {
    let mut elements = Vec::new();
    let mut from = 0;
    while let Some(found) = xml[from..].find("<DataMacro") {
        let start = from + found;
        let after = start + "<DataMacro".len();
        // Not DataMacros.
        if !matches!(xml.as_bytes().get(after), Some(b' ' | b'>' | b'/')) {
            from = after;
            continue;
        }
        let tag_end = xml[start..].find('>').map_or(xml.len(), |i| start + i + 1);
        let end = if xml[start..tag_end].ends_with("/>") {
            tag_end
        } else {
            xml[tag_end..]
                .find("</DataMacro>")
                .map_or(xml.len(), |i| tag_end + i + "</DataMacro>".len())
        };
        elements.push(&xml[start..end]);
        from = end;
    }
    elements
}

/// The byte range of the attribute starting with `prefix` (such as
/// ` xmlns="`) in `tag`, through its closing quote.
fn find_attribute(tag: &str, prefix: &str) -> Option<(usize, usize)> {
    let from = tag.find(prefix)?;
    let value_start = from + prefix.len();
    let to = value_start + tag[value_start..].find('"')? + 1;
    Some((from, to))
}

/// The width of the pieces a long value is wrapped into.
const PIECE_WIDTH: usize = 80;

impl MacroGrid {
    /// The macro in the text format `Application.SaveAsText` writes, with
    /// CRLF line endings.
    ///
    /// The `Version` line is `196611` for the header string `33` and `0` for
    /// an empty header string. An action whose number is not in
    /// [`macro_action`]'s table is written by its number.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        push_line(&mut out, &format!("Version ={}", version(&self.header)));
        push_line(&mut out, "PublishOption =1");
        push_line(&mut out, &format!("ColumnsShown ={}", self.columns_shown));
        for row in &self.rows {
            push_row(&mut out, "", row);
        }
        out
    }
}

/// The text `Application.SaveAsText` writes for an embedded macro, as part of
/// its form's or report's text: an `<event>EmMacro = Begin` line, the grid's
/// `Version` and `ColumnsShown` lines and rows indented by four spaces, and an
/// `End` line, with CRLF line endings. The form's text indents this block by
/// the depth of the form, section, or control it belongs to.
///
/// Returns `None` for a macro that is not embedded, has no grid, or has no
/// event name.
pub fn embedded_macro_to_text(def: &MacroDef) -> Option<String> {
    let MacroSource::Embedded { event, .. } = &def.source else {
        return None;
    };
    let grid = def.grid.as_ref()?;
    if event.is_empty() {
        return None;
    }
    let mut out = String::new();
    push_line(&mut out, &format!("{event}EmMacro = Begin"));
    push_line(&mut out, &format!("    Version ={}", version(&grid.header)));
    push_line(
        &mut out,
        &format!("    ColumnsShown ={}", grid.columns_shown),
    );
    for row in &grid.rows {
        push_row(&mut out, "    ", row);
    }
    push_line(&mut out, "End");
    Some(out)
}

/// The `Version` value for a grid header string of two digits `ab`, which is
/// `a * 0x10000 + b` (`33` is 196611), or 0 for any other header string.
fn version(header: &str) -> u32 {
    let digits: Vec<u32> = header.chars().filter_map(|c| c.to_digit(10)).collect();
    match (header.chars().count(), digits.as_slice()) {
        (2, [a, b]) => (a << 16) | b,
        _ => 0,
    }
}

/// Writes a row as a `Begin` ... `End` block at `indent`.
fn push_row(out: &mut String, indent: &str, row: &MacroGridRow) {
    push_line(out, &format!("{indent}Begin"));
    if let Some(name) = &row.macro_name {
        push_value(out, indent, "MacroName", name);
    }
    if let Some(condition) = &row.condition {
        push_value(out, indent, "Condition", condition);
    }
    if row.action_code != 0 {
        push_value(out, indent, "Action", &action_text_name(row.action_code));
    }
    if let Some(comment) = &row.comment {
        push_value(out, indent, "Comment", comment);
    }
    for argument in written_arguments(row) {
        push_value(out, indent, "Argument", argument.unwrap_or(""));
    }
    push_line(out, &format!("{indent}End"));
}

/// The name SaveAsText writes for the action `action_code`, or its number
/// for one that is not in [`macro_action`]'s table.
fn action_text_name(action_code: u16) -> String {
    macro_action(action_code).map_or_else(|| action_code.to_string(), |a| a.text_name.to_string())
}

/// The arguments of `row` that SaveAsText writes: in the slots of its
/// action, up to the last one that has a value.
fn written_arguments(row: &MacroGridRow) -> Vec<Option<&str>> {
    let slots: Vec<usize> = match macro_action(row.action_code).and_then(|a| a.text_slots) {
        Some(slots) => slots.to_vec(),
        None => (0..row.arguments.len()).collect(),
    };
    let mut arguments: Vec<Option<&str>> = slots
        .iter()
        .map(|&i| row.arguments.get(i).and_then(|a| a.as_deref()))
        .collect();
    let written = arguments
        .iter()
        .rposition(Option::is_some)
        .map_or(0, |last| last + 1);
    arguments.truncate(written);
    arguments
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push_str("\r\n");
}

/// Writes `key ="value"` four spaces deeper than `indent`, wrapping a long
/// value onto `"..."` lines four spaces deeper still.
fn push_value(out: &mut String, indent: &str, key: &str, value: &str) {
    let pieces = wrap(value);
    push_line(out, &format!("{indent}    {key} =\"{}\"", pieces[0]));
    for piece in &pieces[1..] {
        push_line(out, &format!("{indent}        \"{piece}\""));
    }
}

/// The escaped value split into pieces of at least [`PIECE_WIDTH`] characters,
/// except the last.
fn wrap(value: &str) -> Vec<String> {
    let mut pieces = vec![String::new()];
    for c in value.chars() {
        let unit = escape(c);
        let current = pieces.last_mut().expect("pieces is never empty");
        if current.chars().count() >= PIECE_WIDTH {
            pieces.push(unit);
        } else {
            current.push_str(&unit);
        }
    }
    pieces
}

fn escape(c: char) -> String {
    match c {
        '"' => "\\\"".to_string(),
        '\\' => "\\\\".to_string(),
        c if (c as u32) < 0x20 => format!("\\{:03o}", c as u32),
        c => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(action_code: u16, arguments: &[Option<&str>]) -> MacroGridRow {
        let mut arguments: Vec<Option<String>> =
            arguments.iter().map(|a| a.map(str::to_string)).collect();
        arguments.resize(10, None);
        MacroGridRow {
            row: 1,
            macro_name: None,
            condition: None,
            comment: None,
            action_code,
            arguments,
        }
    }

    #[test]
    fn matches_access_output_in_generated_test_data() {
        // tblGenExport holds Access's SaveAsText output of each macro the
        // generator created (kind `macro`) or loaded from `Action ="..."` rows
        // (kind `old-macro`).
        use crate::catalog::read_catalog;
        use crate::data::{read_table_rows, Value};
        use crate::file::PageReader;
        use crate::macro_def::read_macro;
        use crate::table::read_table_def;

        let relative = "V2010/macroGeneratedTestV2010.accdb";
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(relative);
        if !path.exists() {
            eprintln!("SKIP: test data not found: {relative}");
            return;
        }
        let mut reader = PageReader::open(&path).unwrap();
        let page = read_catalog(&mut reader)
            .unwrap()
            .into_iter()
            .find(|e| e.name == "tblGenExport")
            .unwrap()
            .table_page;
        let tdef = read_table_def(&mut reader, "tblGenExport", page).unwrap();
        let mut compared = 0;
        for row in read_table_rows(&mut reader, &tdef).unwrap().rows {
            let (Value::Text(kind), Value::Text(name), Value::Text(content)) =
                (&row[1], &row[2], &row[3])
            else {
                continue;
            };
            if kind != "macro" && kind != "old-macro" {
                continue;
            }
            let grid = read_macro(&mut reader, name).unwrap().grid.unwrap();
            assert_eq!(grid.to_text(), *content, "{name}");
            compared += 1;
        }
        assert_eq!(compared, 87 + 56 + 1);
    }

    /// Compares each macro of `database` with the SaveAsText file of the same
    /// name in `macros_dir`, decoded with `encoding`. The files were written by
    /// a tool that removes `PublishOption =1` lines, so that line is left out
    /// of the comparison, and line endings are compared as `\n`.
    fn compare_with_saveastext_files(
        database: &str,
        macros_dir: &str,
        encoding: &'static encoding_rs::Encoding,
    ) {
        use crate::file::PageReader;
        use crate::macro_def::read_macro;

        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
        if !base.join(database).exists() {
            eprintln!("SKIP: test data not found: {database}");
            return;
        }
        let normalize = |text: &str| {
            text.trim_start_matches('\u{feff}')
                .lines()
                .filter(|line| *line != "PublishOption =1")
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut reader = PageReader::open(base.join(database)).unwrap();
        let mut compared = 0;
        for entry in std::fs::read_dir(base.join(macros_dir)).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let bytes = std::fs::read(&path).unwrap();
            let (expected, _, _) = encoding.decode(&bytes);
            let grid = read_macro(&mut reader, &name).unwrap().grid.unwrap();
            assert_eq!(
                normalize(&grid.to_text()),
                normalize(&expected),
                "{database}: {name}"
            );
            compared += 1;
        }
        assert!(compared > 0, "{macros_dir}");
    }

    #[test]
    fn matches_saveastext_files_of_public_databases() {
        // Fetched by scripts/fetch-testdata.sh. Sports.accdb has macros
        // converted from Access 97 (`Version =0`), macro names, conditions,
        // escapes, and DoMenuItem; the Strings.mdb files are in Windows-1251.
        compare_with_saveastext_files(
            "saveastext/SportsAdmin/Sports.accdb",
            "saveastext/SportsAdmin/macros",
            encoding_rs::UTF_8,
        );
        compare_with_saveastext_files(
            "saveastext/Strings/Strings.mdb",
            "saveastext/Strings/macros",
            encoding_rs::WINDOWS_1251,
        );
    }

    #[test]
    fn data_macros_match_access_output_in_generated_test_data() {
        // tblGenExport holds Access's SaveAsText output of the data macros of
        // tblItems and tblNamed (kind `datamacro`), and of tblOrderA and
        // tblOrderB (kind `datamacro-order`), whose data macros were loaded in
        // scrambled orders.
        use crate::catalog::read_catalog;
        use crate::data::{read_table_rows, Value};
        use crate::file::PageReader;
        use crate::macro_def::read_data_macros;
        use crate::table::read_table_def;

        let relative = "V2010/macroGeneratedTestV2010.accdb";
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(relative);
        if !path.exists() {
            eprintln!("SKIP: test data not found: {relative}");
            return;
        }
        let mut reader = PageReader::open(&path).unwrap();
        let page = read_catalog(&mut reader)
            .unwrap()
            .into_iter()
            .find(|e| e.name == "tblGenExport")
            .unwrap()
            .table_page;
        let tdef = read_table_def(&mut reader, "tblGenExport", page).unwrap();
        let mut compared = Vec::new();
        for row in read_table_rows(&mut reader, &tdef).unwrap().rows {
            let (Value::Text(kind), Value::Text(table), Value::Text(content)) =
                (&row[1], &row[2], &row[3])
            else {
                continue;
            };
            if kind != "datamacro" && kind != "datamacro-order" {
                continue;
            }
            let macros = read_data_macros(&mut reader, table).unwrap();
            assert_eq!(data_macros_to_text(&macros), *content, "{table}");
            compared.push(table.clone());
        }
        compared.sort();
        assert_eq!(compared, ["tblItems", "tblNamed", "tblOrderA", "tblOrderB"]);
    }

    /// The `<event>EmMacro = Begin` blocks of SaveAsText output `content`,
    /// each with its event and its text without its indentation.
    fn saved_embedded_macros(content: &str) -> Vec<(String, String)> {
        let lines: Vec<&str> = content.split("\r\n").collect();
        let mut blocks = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let Some(key) = line.trim_start().strip_suffix("EmMacro = Begin") else {
                continue;
            };
            let indent = line.len() - line.trim_start().len();
            let end = (i..lines.len())
                .find(|&j| lines[j] == format!("{}End", &line[..indent]))
                .unwrap();
            let text: String = lines[i..=end]
                .iter()
                .map(|l| format!("{}\r\n", &l[indent..]))
                .collect();
            blocks.push((key.to_string(), text));
        }
        blocks
    }

    /// The SaveAsText output of the forms and reports in tblGenExport, each
    /// with the name of its object.
    fn saved_forms_and_reports(reader: &mut crate::file::PageReader) -> Vec<(String, String)> {
        use crate::catalog::read_catalog;
        use crate::data::{read_table_rows, Value};
        use crate::table::read_table_def;

        let page = read_catalog(reader)
            .unwrap()
            .into_iter()
            .find(|e| e.name == "tblGenExport")
            .unwrap()
            .table_page;
        let tdef = read_table_def(reader, "tblGenExport", page).unwrap();
        let mut saved = Vec::new();
        for row in read_table_rows(reader, &tdef).unwrap().rows {
            if let (Value::Text(kind), Value::Text(object), Value::Text(content)) =
                (&row[1], &row[2], &row[3])
            {
                if kind == "form" || kind == "report" {
                    saved.push((object.clone(), content.clone()));
                }
            }
        }
        saved
    }

    #[test]
    fn embedded_macros_match_access_output_in_generated_test_data() {
        // tblGenExport holds Access's SaveAsText output of frmEmbedded (kind
        // `form`) and rptEmbedded (kind `report`). Each `<event>EmMacro = Begin`
        // block in it, without its indentation, is compared with the text of
        // the embedded macro for that event.
        use crate::file::PageReader;
        use crate::macro_def::read_embedded_macros;

        let relative = "V2010/macroGeneratedTestV2010.accdb";
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(relative);
        if !path.exists() {
            eprintln!("SKIP: test data not found: {relative}");
            return;
        }
        let mut reader = PageReader::open(&path).unwrap();
        let mut compared = Vec::new();
        for (object, content) in saved_forms_and_reports(&mut reader) {
            let macros = read_embedded_macros(&mut reader, &object).unwrap();
            for (key, expected) in saved_embedded_macros(&content) {
                let def = macros
                    .iter()
                    .find(|m| matches!(&m.source, MacroSource::Embedded { event, .. } if *event == key))
                    .unwrap_or_else(|| panic!("{object}: no {key} macro"));
                assert_eq!(
                    embedded_macro_to_text(def).unwrap(),
                    expected,
                    "{object} {key}"
                );
                compared.push(format!("{object}.{key}"));
            }
        }
        compared.sort();
        assert_eq!(
            compared,
            [
                "frmEmbedded.OnClick",
                "frmEmbedded.OnLoad",
                "rptEmbedded.OnOpen"
            ]
        );
    }

    #[test]
    fn data_macros_to_text_without_macros_is_empty() {
        assert_eq!(data_macros_to_text(&[]), "");
    }

    #[test]
    fn data_macros_of_one_data_macros_document_are_written_once() {
        // A table may keep its data macros in one DataMacros document, which
        // read_data_macros returns as one MacroDef for each, all with the whole
        // document as their xml. Each is written once, the named one after the
        // table event, under the namespace of DataMacros.
        let xml = r#"<?xml version="1.0"?><DataMacros xmlns="ns"><DataMacro Name="dmNamed"><Statements><Action Name="B"/></Statements></DataMacro><DataMacro Event="AfterInsert"><Statements><Action Name="A"/></Statements></DataMacro></DataMacros>"#;
        let data = |event: Option<&str>, name: Option<&str>| MacroDef {
            source: MacroSource::Data {
                table: "t".to_string(),
                event: event.map(str::to_string),
                name: name.map(str::to_string),
                parameters: Vec::new(),
            },
            statements: Vec::new(),
            xml: xml.to_string(),
            grid: None,
        };
        let macros = [data(None, Some("dmNamed")), data(Some("AfterInsert"), None)];
        assert_eq!(
            data_macros_to_text(&macros),
            "<?xml version=\"1.0\" encoding=\"UTF-16\" standalone=\"no\"?>\r\n\
             <DataMacros xmlns=\"ns\">\
             <DataMacro Event=\"AfterInsert\"><Statements><Action Name=\"A\"/></Statements></DataMacro>\
             <DataMacro Name=\"dmNamed\"><Statements><Action Name=\"B\"/></Statements></DataMacro>\
             </DataMacros>"
        );
    }

    #[test]
    fn arguments_in_text_slot_order() {
        // DoMenuItem, as in a macro converted from Access 97.
        let grid = MacroGrid {
            columns_shown: 0,
            header: String::new(),
            rows: vec![row(
                8,
                &[
                    Some("1"),
                    Some("4"),
                    Some("4"),
                    Some("0"),
                    Some("43"),
                    Some("121"),
                    Some("20"),
                ],
            )],
        };
        assert!(grid.to_text().ends_with(
            "    Action =\"DoMenuItem\"\r\n    Argument =\"20\"\r\n    Argument =\"1\"\r\n\
             \x20   Argument =\"4\"\r\n    Argument =\"4\"\r\n    Argument =\"0\"\r\nEnd\r\n"
        ));
    }

    #[test]
    fn version_from_header() {
        assert_eq!(version("33"), 196611);
        assert_eq!(version(""), 0);
    }

    #[test]
    fn row_lines_in_order() {
        let grid = MacroGrid {
            columns_shown: 3,
            header: String::new(),
            rows: vec![MacroGridRow {
                macro_name: Some("&Setup".to_string()),
                condition: Some("CurrentUser()=\"Owner\"".to_string()),
                comment: Some("note".to_string()),
                ..row(
                    23,
                    &[
                        Some("Setup Carnival"),
                        Some("0"),
                        None,
                        None,
                        Some("1"),
                        Some("0"),
                    ],
                )
            }],
        };
        assert_eq!(
            grid.to_text(),
            "Version =0\r\nPublishOption =1\r\nColumnsShown =3\r\nBegin\r\n\
             \x20   MacroName =\"&Setup\"\r\n\
             \x20   Condition =\"CurrentUser()=\\\"Owner\\\"\"\r\n\
             \x20   Action =\"OpenForm\"\r\n\
             \x20   Comment =\"note\"\r\n\
             \x20   Argument =\"Setup Carnival\"\r\n\
             \x20   Argument =\"0\"\r\n\
             \x20   Argument =\"\"\r\n\
             \x20   Argument =\"\"\r\n\
             \x20   Argument =\"1\"\r\n\
             \x20   Argument =\"0\"\r\n\
             End\r\n"
        );
    }

    #[test]
    fn wrap_does_not_split_escapes() {
        let value = format!("{}\r\n{}", "a".repeat(75), "b".repeat(10));
        assert_eq!(
            wrap(&value),
            [format!("{}\\015\\012", "a".repeat(75)), "b".repeat(10)]
        );
        assert_eq!(wrap(&"c".repeat(81)), ["c".repeat(80), "c".to_string()]);
        assert_eq!(wrap(""), [""]);
    }
}
