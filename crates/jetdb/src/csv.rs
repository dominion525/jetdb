//! A table as CSV, as `jetdb export` writes it (RFC 4180).
//!
//! [`write_csv`] writes the header and then the rows one at a time, so only
//! the row being written is in memory. Complex columns of multiple values and
//! attachments give their values joined by `;` in one quoted field, as Access
//! exports a multivalued field; the version history keeps its number.

use std::io::{self, Write};

use crate::catalog::{find_table, CatalogEntry};
use crate::complex::{read_complex_column, Attachment, ComplexColumn, ComplexKind, ComplexValues};
use crate::data::{calculated_column_types_in, TableRows, Value};
use crate::file::{FileError, PageReader};
use crate::table::TableDef;
use crate::timestamp;

/// How [`write_csv`] writes the bytes of a Binary or OLE Object value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum BinaryFormat {
    /// Nothing (an empty field).
    Strip,
    /// The bytes as UTF-8, invalid sequences replaced.
    Raw,
    /// Each byte as `\NNN` in octal.
    Octal,
    /// Each byte as two lowercase hex digits.
    #[default]
    Hex,
}

/// The options of [`write_csv`], those of `jetdb export`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct CsvOptions {
    /// Write the names of the columns as the first row.
    pub header: bool,
    pub delimiter: char,
    /// The strftime-like format of a date without a time.
    pub date_format: String,
    /// The strftime-like format of a date with a time.
    pub datetime_format: String,
    pub binary: BinaryFormat,
    /// The text of a NULL value.
    pub null: String,
    /// `TRUE` and `FALSE` rather than `1` and `0` for Yes/No values.
    pub boolean_words: bool,
    /// Include the columns Access maintains and hides.
    pub system_columns: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            header: true,
            delimiter: ',',
            date_format: timestamp::DEFAULT_DATE_FORMAT.to_string(),
            datetime_format: timestamp::DEFAULT_DATETIME_FORMAT.to_string(),
            binary: BinaryFormat::Hex,
            null: String::new(),
            boolean_words: false,
            system_columns: false,
        }
    }
}

/// Writes the content of an attached file somewhere for [`write_csv`], given
/// the number the attachment column holds in the row, the file name (with
/// `/`, `\` and NUL made `_`) and the content, and gives the text its field
/// shows for the file.
pub type AttachmentWriter<'a> = dyn FnMut(i32, &str, &[u8]) -> io::Result<String> + 'a;

/// Write the table `table` to `out` as CSV, as `jetdb export` writes it, and
/// return the number of rows passed over because they could not be read.
///
/// The columns are those Access shows, as [`crate::ColumnDef::is_shown`]
/// tells, or all of them with [`CsvOptions::system_columns`]. An attachment
/// field gives the file names, or, with `attachments`, what it gives for each
/// file whose content could be read.
pub fn write_csv(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
    table: &TableDef,
    options: &CsvOptions,
    out: &mut dyn Write,
    mut attachments: Option<&mut AttachmentWriter<'_>>,
) -> Result<usize, FileError> {
    let system_table = find_table(catalog, &table.name)?.is_system();
    let columns = exported_columns(table, system_table, options);
    let delimiter = options.delimiter.to_string();
    if options.header {
        write_header(out, table, &columns, options)?;
    }
    let complex = complex_columns(reader, catalog, table, &columns)?;
    let calculated = calculated_column_types_in(catalog, table);
    let mut rows = TableRows::new(table, &calculated).with_columns(&columns);
    while let Some(row) = rows.next_row(reader)? {
        let fields = row_fields(
            reader,
            &row.values,
            &columns,
            &complex,
            options,
            &mut attachments,
        )?;
        writeln!(out, "{}", fields.join(&delimiter)).map_err(FileError::Io)?;
    }
    Ok(rows.skipped_rows())
}

/// The indexes of the columns of `table` to write: those Access shows, or all
/// of them with [`CsvOptions::system_columns`].
fn exported_columns(table: &TableDef, system_table: bool, options: &CsvOptions) -> Vec<usize> {
    (0..table.columns.len())
        .filter(|&i| options.system_columns || table.columns[i].is_shown(system_table))
        .collect()
}

/// The complex columns of the columns at the indexes in `columns`, `None`
/// for each that is not one.
fn complex_columns(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
    table: &TableDef,
    columns: &[usize],
) -> Result<Vec<Option<ComplexColumn>>, FileError> {
    columns
        .iter()
        .map(|&i| read_complex_column(reader, catalog, table, &table.columns[i]))
        .collect()
}

/// Write the line of the names of the columns at the indexes in `columns`.
fn write_header(
    out: &mut dyn Write,
    table: &TableDef,
    columns: &[usize],
    options: &CsvOptions,
) -> Result<(), FileError> {
    let header: Vec<String> = columns
        .iter()
        .map(|&i| csv_escape(&table.columns[i].name, options.delimiter, false))
        .collect();
    writeln!(out, "{}", header.join(&options.delimiter.to_string())).map_err(FileError::Io)
}

// ---------------------------------------------------------------------------
// Fields
// ---------------------------------------------------------------------------

/// The fields of the columns at the indexes in `columns` of a row whose
/// values are `values`, with `complex` their complex columns.
fn row_fields(
    reader: &mut PageReader,
    values: &[Value],
    columns: &[usize],
    complex: &[Option<ComplexColumn>],
    options: &CsvOptions,
    attachments: &mut Option<&mut AttachmentWriter<'_>>,
) -> Result<Vec<String>, FileError> {
    columns
        .iter()
        .zip(complex)
        .map(|(&i, column)| field(reader, &values[i], column.as_ref(), options, attachments))
        .collect()
}

/// Escape a string value for CSV output (RFC 4180).
///
/// - `always_quote=true`: always wrap in double quotes (for Text/Memo/Guid).
/// - `always_quote=false`: only quote if the value contains delimiter, newline, or `"`.
/// - Internal `"` is doubled to `""`.
fn csv_escape(value: &str, delimiter: char, always_quote: bool) -> String {
    let needs_quote = always_quote
        || value.contains(delimiter)
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r');
    if !needs_quote {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Format a `Value` as a CSV field string.
fn format_value(value: &Value, opts: &CsvOptions) -> String {
    match value {
        Value::Null => csv_escape(&opts.null, opts.delimiter, false),
        Value::Text(s) | Value::Guid(s) | Value::DateTimeExtended(s) => {
            csv_escape(s, opts.delimiter, true)
        }
        // Quoted, as text is, when the delimiter, a quote or a line break is
        // in it: a minus with `-d '-'`, a digit of hex with `-d 'a'`, a
        // date with a comma in its format.
        other => csv_escape(&plain_value(other, opts), opts.delimiter, false),
    }
}

/// The text of a Yes/No value: `1` or `0`, or `TRUE` or `FALSE` with
/// [`CsvOptions::boolean_words`].
fn bool_text(b: bool, opts: &CsvOptions) -> &'static str {
    match (b, opts.boolean_words) {
        (true, true) => "TRUE",
        (false, true) => "FALSE",
        (true, false) => "1",
        (false, false) => "0",
    }
}

/// The text of a Date/Time value, in the date format when it has no time of
/// day.
fn timestamp_text(ts: f64, opts: &CsvOptions) -> String {
    let format = if timestamp::is_date_only(ts) {
        &opts.date_format
    } else {
        &opts.datetime_format
    };
    timestamp::format_timestamp(ts, format)
}

/// A value as text, without the quotes of a CSV field: the text of a field,
/// and one of the values [`complex_field`] joins.
fn plain_value(value: &Value, opts: &CsvOptions) -> String {
    match value {
        Value::Null => String::new(),
        Value::Text(s) | Value::Guid(s) | Value::DateTimeExtended(s) => s.clone(),
        Value::Money(s) | Value::Numeric(s) => s.clone(),
        Value::Bool(b) => bool_text(*b, opts).to_string(),
        Value::Byte(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::Long(v) => v.to_string(),
        Value::BigInt(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::Double(v) => v.to_string(),
        Value::Timestamp(ts) => timestamp_text(*ts, opts),
        Value::Binary(data) => format_binary(data, opts.binary),
    }
}

/// `items` joined by `;` as one quoted field, as Access exports the values
/// of a multivalued field, or an empty field for no items.
fn joined_field(items: &[String], opts: &CsvOptions) -> String {
    if items.is_empty() {
        String::new()
    } else {
        csv_escape(&items.join(";"), opts.delimiter, true)
    }
}

/// `name` made safe to use as a file name in a directory: `/`, `\` and NUL
/// become `_`, and an empty name, `.` or `..` becomes `_`.
fn safe_file_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    match name.as_str() {
        "" | "." | ".." => "_".to_string(),
        _ => name,
    }
}

/// The field of `values`, the values of the complex column that holds
/// `number` in a row: the values for multiple values, and the file names for
/// attachments, or what `attachments` gives for each file it is given.
fn complex_field(
    values: ComplexValues,
    number: i32,
    opts: &CsvOptions,
    attachments: &mut Option<&mut AttachmentWriter<'_>>,
) -> io::Result<String> {
    let mut items = Vec::new();
    match values {
        ComplexValues::MultiValue(values) => {
            items.extend(values.iter().map(|v| plain_value(v, opts)));
        }
        ComplexValues::Attachment(files) => {
            items = attachment_items(files, number, opts, attachments)?;
        }
        ComplexValues::VersionHistory(_) | ComplexValues::Unknown => {}
    }
    Ok(joined_field(&items, opts))
}

/// The items of the field of the attached `files` of the attachment column
/// that holds `number` in a row: their file names, or what `attachments`
/// gives for each file whose content could be read.
fn attachment_items(
    files: Vec<Attachment>,
    number: i32,
    opts: &CsvOptions,
    attachments: &mut Option<&mut AttachmentWriter<'_>>,
) -> io::Result<Vec<String>> {
    let mut items = Vec::new();
    for file in files {
        let name = safe_file_name(&plain_value(&file.name, opts));
        match (attachments.as_mut(), &file.data) {
            (Some(write), Value::Binary(data)) => items.push(write(number, &name, data)?),
            (Some(_), _) => {
                log::warn!("the content of attachment {name} could not be read");
                items.push(name);
            }
            (None, _) => items.push(name),
        }
    }
    Ok(items)
}

/// The field of `value`, with `column` its column if it is a complex column.
/// Multiple values and attachments give their values (see
/// [`complex_field`]); the version history and kinds jetdb does not know
/// keep the number.
fn field(
    reader: &mut PageReader,
    value: &Value,
    column: Option<&ComplexColumn>,
    opts: &CsvOptions,
    attachments: &mut Option<&mut AttachmentWriter<'_>>,
) -> Result<String, FileError> {
    if let (Some(column), Value::Long(n)) = (column, value) {
        if matches!(
            column.kind(),
            ComplexKind::MultiValue | ComplexKind::Attachment
        ) {
            let values = column.values(reader, *n)?;
            return complex_field(values, *n, opts, attachments).map_err(FileError::Io);
        }
    }
    Ok(format_value(value, opts))
}

/// Format binary data according to the selected mode.
fn format_binary(data: &[u8], mode: BinaryFormat) -> String {
    match mode {
        BinaryFormat::Strip => String::new(),
        BinaryFormat::Raw => String::from_utf8_lossy(data).into_owned(),
        BinaryFormat::Octal => {
            let mut s = String::with_capacity(data.len() * 4);
            for b in data {
                s.push_str(&format!("\\{b:03o}"));
            }
            s
        }
        BinaryFormat::Hex => {
            let mut s = String::with_capacity(data.len() * 2);
            for b in data {
                s.push_str(&format!("{b:02x}"));
            }
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_opts() -> CsvOptions {
        CsvOptions::default()
    }

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(relative);
        path.exists().then_some(path)
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

    /// The CSV of the table `table` in `path`, with the files `attachments`
    /// is given, if any.
    fn csv_of(
        path: &std::path::Path,
        table: &str,
        options: &CsvOptions,
        attachments: Option<&mut AttachmentWriter<'_>>,
    ) -> String {
        let mut reader = PageReader::open(path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = find_table(&catalog, table).unwrap();
        let tdef =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let mut out = Vec::new();
        let skipped =
            write_csv(&mut reader, &catalog, &tdef, options, &mut out, attachments).unwrap();
        assert_eq!(skipped, 0);
        String::from_utf8(out).unwrap()
    }

    // -- write_csv --------------------------------------------------------------

    #[test]
    fn write_csv_of_a_table() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        assert_eq!(
            csv_of(&path, "Table1", &CsvOptions::default(), None),
            "A,B,C,D,E,F,G,H,I\n\
             \"abcdefg\",\"hijklmnop\",2,222,333333333,444.555,1974-09-21,3.5000,1\n\
             \"a\",\"b\",0,0,0,0,1981-12-12,0.0000,0\n"
        );
    }

    #[test]
    fn write_csv_with_options() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let options = CsvOptions {
            header: false,
            delimiter: '\t',
            date_format: "%d/%m/%Y".to_string(),
            boolean_words: true,
            ..CsvOptions::default()
        };
        assert_eq!(
            csv_of(&path, "Table1", &options, None),
            "\"abcdefg\"\t\"hijklmnop\"\t2\t222\t333333333\t444.555\t21/09/1974\t3.5000\tTRUE\n\
             \"a\"\t\"b\"\t0\t0\t0\t0\t12/12/1981\t0.0000\tFALSE\n"
        );
    }

    #[test]
    fn write_csv_quotes_numbers_and_dates_with_the_delimiter() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let options = CsvOptions {
            header: false,
            date_format: "%d,%m,%Y".to_string(),
            ..CsvOptions::default()
        };
        assert_eq!(
            csv_of(&path, "Table1", &options, None),
            "\"abcdefg\",\"hijklmnop\",2,222,333333333,444.555,\"21,09,1974\",3.5000,1\n\
             \"a\",\"b\",0,0,0,0,\"12,12,1981\",0.0000,0\n"
        );
        let options = CsvOptions {
            header: false,
            delimiter: '.',
            ..CsvOptions::default()
        };
        assert_eq!(
            csv_of(&path, "Table1", &options, None),
            "\"abcdefg\".\"hijklmnop\".2.222.333333333.\"444.555\".1974-09-21.\"3.5000\".1\n\
             \"a\".\"b\".0.0.0.0.1981-12-12.\"0.0000\".0\n"
        );
        let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
        let options = CsvOptions {
            delimiter: ' ',
            ..CsvOptions::default()
        };
        let csv = csv_of(&path, "Table1", &options, None);
        assert!(csv.contains(
            "\n3 \"row3\" \"2021-06-14 12:45:00\" \"2021-06-14 12:45:00\" \
             \"6/14/2021 12:45:00.0000000 PM\" \"2021-06-14 12:45:00\"\n"
        ));
        assert!(csv.contains("\n2 \"row2\" \"2021-06-14\" 2021-06-14 "));
    }

    #[test]
    fn write_csv_of_complex_columns() {
        // Multiple values and attachment file names joined by ;, and the
        // files given to attachments with the numbers of their rows.
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        let options = CsvOptions::default();
        let plain = csv_of(&path, "Table1", &options, None);
        assert!(plain.contains(",\"value1;value4\",\"test_data.txt;test_data2.txt\""));
        let mut files: Vec<(i32, String, Vec<u8>)> = Vec::new();
        let mut write = |n: i32, name: &str, data: &[u8]| -> io::Result<String> {
            files.push((n, name.to_string(), data.to_vec()));
            Ok(format!("{n}:{name}"))
        };
        let written = csv_of(&path, "Table1", &options, Some(&mut write));
        assert!(written.contains(",\"2:test_data.txt;2:test_data2.txt\""));
        assert!(written.contains(",\"4:test_data2.txt\""));
        let more = b"this is some more test data for attachment.".to_vec();
        assert_eq!(
            files,
            [
                (
                    2,
                    "test_data.txt".to_string(),
                    b"this is some test data for attachment.".to_vec()
                ),
                (2, "test_data2.txt".to_string(), more.clone()),
                (4, "test_data2.txt".to_string(), more),
            ]
        );
    }

    #[test]
    fn write_csv_leaves_out_hidden_columns() {
        // Every column of a system table is hidden, so they are all written.
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let header = |options: &CsvOptions| {
            csv_of(&path, "MSysObjects", options, None)
                .lines()
                .next()
                .unwrap()
                .to_string()
        };
        let all = header(&CsvOptions {
            system_columns: true,
            ..CsvOptions::default()
        });
        assert_eq!(header(&CsvOptions::default()), all);
        assert!(all.starts_with("Id,ParentId,Name,"));
    }

    // -- csv_escape -----------------------------------------------------------

    #[test]
    fn csv_escape_plain_text() {
        assert_eq!(csv_escape("hello", ',', true), "\"hello\"");
    }

    #[test]
    fn csv_escape_with_comma() {
        assert_eq!(csv_escape("a,b", ',', false), "\"a,b\"");
    }

    #[test]
    fn csv_escape_with_quote() {
        assert_eq!(csv_escape("say \"hi\"", ',', false), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn csv_escape_with_newline() {
        assert_eq!(csv_escape("line1\nline2", ',', false), "\"line1\nline2\"");
    }

    #[test]
    fn csv_escape_empty() {
        assert_eq!(csv_escape("", ',', true), "\"\"");
    }

    #[test]
    fn csv_escape_no_quote_needed() {
        assert_eq!(csv_escape("hello", ',', false), "hello");
    }

    #[test]
    fn csv_escape_tab_delimiter() {
        assert_eq!(csv_escape("a\tb", '\t', false), "\"a\tb\"");
    }

    #[test]
    fn csv_escape_header_with_comma() {
        assert_eq!(csv_escape("Amount, USD", ',', false), "\"Amount, USD\"");
    }

    // -- format_value ---------------------------------------------------------

    #[test]
    fn format_value_null_default() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::Null, &opts), "");
    }

    #[test]
    fn format_value_null_custom() {
        let mut opts = default_opts();
        opts.null = "(null)".to_string();
        assert_eq!(format_value(&Value::Null, &opts), "(null)");
    }

    #[test]
    fn format_value_null_with_comma() {
        let mut opts = default_opts();
        opts.null = "N/A, unknown".to_string();
        assert_eq!(format_value(&Value::Null, &opts), "\"N/A, unknown\"");
    }

    #[test]
    fn format_value_bool_numeric() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::Bool(true), &opts), "1");
        assert_eq!(format_value(&Value::Bool(false), &opts), "0");
    }

    #[test]
    fn format_value_bool_words() {
        let mut opts = default_opts();
        opts.boolean_words = true;
        assert_eq!(format_value(&Value::Bool(true), &opts), "TRUE");
        assert_eq!(format_value(&Value::Bool(false), &opts), "FALSE");
    }

    #[test]
    fn format_value_int() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::Int(42), &opts), "42");
    }

    #[test]
    fn format_value_long() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::Long(123456), &opts), "123456");
    }

    #[test]
    fn format_value_double() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::Double(3.125), &opts), "3.125");
    }

    #[test]
    fn format_value_text() {
        let opts = default_opts();
        assert_eq!(
            format_value(&Value::Text("hello".to_string()), &opts),
            "\"hello\""
        );
    }

    #[test]
    fn format_value_text_with_comma() {
        let opts = default_opts();
        assert_eq!(
            format_value(&Value::Text("a,b".to_string()), &opts),
            "\"a,b\""
        );
    }

    #[test]
    fn format_value_guid() {
        let opts = default_opts();
        let guid = "{12345678-1234-1234-1234-123456789ABC}".to_string();
        let result = format_value(&Value::Guid(guid), &opts);
        assert!(result.starts_with('"') && result.ends_with('"'));
    }

    #[test]
    fn format_value_money() {
        let opts = default_opts();
        assert_eq!(
            format_value(&Value::Money("12.3400".to_string()), &opts),
            "12.3400"
        );
    }

    #[test]
    fn format_value_timestamp_date() {
        let opts = default_opts();
        // 37623.0 = 2003-01-02
        assert_eq!(
            format_value(&Value::Timestamp(37623.0), &opts),
            "2003-01-02"
        );
    }

    #[test]
    fn format_value_timestamp_datetime() {
        let opts = default_opts();
        // 37623.5 = 2003-01-02 12:00:00
        assert_eq!(
            format_value(&Value::Timestamp(37623.5), &opts),
            "2003-01-02 12:00:00"
        );
    }

    #[test]
    fn format_value_timestamp_out_of_range() {
        // A date out of the years 100 to 9999 is the epoch, as a NaN is.
        let opts = default_opts();
        for ts in [1e300, -1e300, 3e18, 1e15 + 0.5] {
            assert_eq!(
                format_value(&Value::Timestamp(ts), &opts),
                "1899-12-30",
                "{ts}"
            );
        }
    }

    #[test]
    fn format_value_timestamp_with_delimiter() {
        let mut opts = default_opts();
        opts.delimiter = ' ';
        assert_eq!(
            format_value(&Value::Timestamp(37623.5), &opts),
            "\"2003-01-02 12:00:00\""
        );
        assert_eq!(
            format_value(&Value::Timestamp(37623.0), &opts),
            "2003-01-02"
        );
    }

    #[test]
    fn format_value_numbers_with_delimiter() {
        let mut opts = default_opts();
        opts.delimiter = '.';
        assert_eq!(format_value(&Value::Float(1.5), &opts), "\"1.5\"");
        assert_eq!(format_value(&Value::Double(3.125), &opts), "\"3.125\"");
        assert_eq!(
            format_value(&Value::Money("12.3400".to_string()), &opts),
            "\"12.3400\""
        );
        assert_eq!(
            format_value(&Value::Numeric("123.45".to_string()), &opts),
            "\"123.45\""
        );
        assert_eq!(format_value(&Value::Double(3.0), &opts), "3");
    }

    #[test]
    fn format_value_integers_bools_and_binary_with_delimiter() {
        let mut opts = default_opts();
        opts.delimiter = '-';
        assert_eq!(format_value(&Value::Long(-1), &opts), "\"-1\"");
        assert_eq!(format_value(&Value::BigInt(-2), &opts), "\"-2\"");
        assert_eq!(format_value(&Value::Int(3), &opts), "3");
        opts.delimiter = 'a';
        assert_eq!(
            format_value(&Value::Binary(vec![0xde, 0xad]), &opts),
            "\"dead\""
        );
        assert_eq!(format_value(&Value::Binary(vec![0x12]), &opts), "12");
        opts.delimiter = 'R';
        opts.boolean_words = true;
        assert_eq!(format_value(&Value::Bool(true), &opts), "\"TRUE\"");
        assert_eq!(format_value(&Value::Bool(false), &opts), "FALSE");
    }

    #[test]
    fn format_value_binary_hex() {
        let opts = default_opts();
        assert_eq!(
            format_value(&Value::Binary(vec![0xde, 0xad]), &opts),
            "dead"
        );
    }

    #[test]
    fn format_value_binary_strip() {
        let mut opts = default_opts();
        opts.binary = BinaryFormat::Strip;
        assert_eq!(format_value(&Value::Binary(vec![0xde, 0xad]), &opts), "");
    }

    #[test]
    fn format_value_binary_octal() {
        let mut opts = default_opts();
        opts.binary = BinaryFormat::Octal;
        assert_eq!(
            format_value(&Value::Binary(vec![0x41, 0x42]), &opts),
            "\\101\\102"
        );
    }

    #[test]
    fn format_value_binary_raw_with_comma() {
        let mut opts = default_opts();
        opts.binary = BinaryFormat::Raw;
        assert_eq!(
            format_value(&Value::Binary(b"a,b".to_vec()), &opts),
            "\"a,b\""
        );
    }

    // -- format_binary --------------------------------------------------------

    #[test]
    fn format_binary_hex() {
        assert_eq!(format_binary(&[0xca, 0xfe], BinaryFormat::Hex), "cafe");
    }

    #[test]
    fn format_binary_octal() {
        assert_eq!(format_binary(&[65, 10], BinaryFormat::Octal), "\\101\\012");
    }

    #[test]
    fn format_binary_strip() {
        assert_eq!(format_binary(&[1, 2, 3], BinaryFormat::Strip), "");
    }

    #[test]
    fn format_binary_empty() {
        assert_eq!(format_binary(&[], BinaryFormat::Hex), "");
    }

    #[test]
    fn format_value_bigint() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::BigInt(42), &opts), "42");
        assert_eq!(format_value(&Value::BigInt(-1), &opts), "-1");
    }

    #[test]
    fn format_value_float() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::Float(1.5), &opts), "1.5");
    }

    #[test]
    fn format_value_numeric() {
        let opts = default_opts();
        assert_eq!(
            format_value(&Value::Numeric("123.45".to_string()), &opts),
            "123.45"
        );
    }

    #[test]
    fn format_value_byte() {
        let opts = default_opts();
        assert_eq!(format_value(&Value::Byte(255), &opts), "255");
    }

    #[test]
    fn format_value_binary_raw_empty() {
        let mut opts = default_opts();
        opts.binary = BinaryFormat::Raw;
        assert_eq!(format_value(&Value::Binary(vec![]), &opts), "");
    }

    #[test]
    fn joined_field_quotes_the_values() {
        let opts = default_opts();
        let items = ["a".to_string(), "b, c".to_string()];
        assert_eq!(joined_field(&items, &opts), "\"a;b, c\"");
        assert_eq!(joined_field(&[], &opts), "");
    }

    #[test]
    fn plain_value_has_no_quotes() {
        let opts = default_opts();
        assert_eq!(plain_value(&Value::Text("a\"b".into()), &opts), "a\"b");
        assert_eq!(plain_value(&Value::Long(3), &opts), "3");
        assert_eq!(plain_value(&Value::Null, &opts), "");
    }

    #[test]
    fn plain_value_of_numbers_and_dates_has_no_quotes() {
        let mut opts = default_opts();
        opts.delimiter = '.';
        assert_eq!(plain_value(&Value::Double(3.125), &opts), "3.125");
        assert_eq!(plain_value(&Value::Money("1.50".into()), &opts), "1.50");
        opts.delimiter = ' ';
        assert_eq!(
            plain_value(&Value::Timestamp(37623.5), &opts),
            "2003-01-02 12:00:00"
        );
    }

    #[test]
    fn safe_file_name_stays_in_its_directory() {
        assert_eq!(safe_file_name("a.txt"), "a.txt");
        assert_eq!(safe_file_name("../a/b\\c.txt"), ".._a_b_c.txt");
        assert_eq!(safe_file_name(".."), "_");
        assert_eq!(safe_file_name("."), "_");
        assert_eq!(safe_file_name(""), "_");
    }

    #[test]
    fn csv_escape_carriage_return() {
        assert_eq!(csv_escape("a\rb", ',', false), "\"a\rb\"");
    }
}
