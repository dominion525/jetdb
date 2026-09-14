//! The text `Application.SaveAsText` writes for a named macro.
//!
//! Each grid row becomes a `Begin` ... `End` block holding, in this order, the
//! row's `MacroName`, `Condition`, `Action`, `Comment`, and `Argument` lines.
//! Arguments are written up to the last non-empty one, empty arguments before
//! it as `Argument =""`. A value is quoted with `"` and `\` escaped by a
//! backslash and control characters as three-digit octal escapes (`\015`),
//! and is wrapped into quoted pieces of 80 characters of escaped text, each
//! piece extended to the end of an escape sequence it would otherwise split.

use crate::macro_action::macro_action;
use crate::macro_def::{MacroGrid, MacroGridRow};

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
            push_row(&mut out, row);
        }
        out
    }
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

fn push_row(out: &mut String, row: &MacroGridRow) {
    push_line(out, "Begin");
    if let Some(name) = &row.macro_name {
        push_value(out, "MacroName", name);
    }
    if let Some(condition) = &row.condition {
        push_value(out, "Condition", condition);
    }
    if row.action_code != 0 {
        let name = macro_action(row.action_code)
            .map_or_else(|| row.action_code.to_string(), |a| a.text_name.to_string());
        push_value(out, "Action", &name);
    }
    if let Some(comment) = &row.comment {
        push_value(out, "Comment", comment);
    }
    let slots: Vec<usize> = match macro_action(row.action_code).and_then(|a| a.text_slots) {
        Some(slots) => slots.to_vec(),
        None => (0..row.arguments.len()).collect(),
    };
    let arguments: Vec<Option<&str>> = slots
        .iter()
        .map(|&i| row.arguments.get(i).and_then(|a| a.as_deref()))
        .collect();
    if let Some(last) = arguments.iter().rposition(Option::is_some) {
        for argument in &arguments[..=last] {
            push_value(out, "Argument", argument.unwrap_or(""));
        }
    }
    push_line(out, "End");
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push_str("\r\n");
}

/// Writes `    key ="value"`, wrapping a long value onto `        "..."` lines.
fn push_value(out: &mut String, key: &str, value: &str) {
    let pieces = wrap(value);
    push_line(out, &format!("    {key} =\"{}\"", pieces[0]));
    for piece in &pieces[1..] {
        push_line(out, &format!("        \"{piece}\""));
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
