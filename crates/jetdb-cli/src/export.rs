use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use jetdb::timestamp;
use jetdb::{
    find_table, read_catalog, read_complex_column, read_table_def, read_table_rows, ComplexColumn,
    ComplexKind, ComplexValues, FileError, PageReader, Value,
};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ExportArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Table name to export
    pub table: String,

    /// Suppress header row
    #[arg(short = 'H', long = "no-header")]
    pub no_header: bool,

    /// Column delimiter (default: ",")
    #[arg(short = 'd', long = "delimiter", default_value = ",")]
    pub delimiter: String,

    /// Date format (strftime subset, default: "%Y-%m-%d")
    #[arg(short = 'D', long = "date-format", default_value = timestamp::DEFAULT_DATE_FORMAT)]
    pub date_format: String,

    /// Date-time format (strftime subset, default: "%Y-%m-%d %H:%M:%S")
    #[arg(
        short = 'T',
        long = "datetime-format",
        default_value = timestamp::DEFAULT_DATETIME_FORMAT
    )]
    pub datetime_format: String,

    /// Binary output mode
    #[arg(short = 'b', long = "bin", value_enum, default_value_t = BinMode::Hex)]
    pub bin_mode: BinMode,

    /// String to represent NULL values (default: "")
    #[arg(short = '0', long = "null", default_value = "")]
    pub null_string: String,

    /// Output booleans as TRUE/FALSE instead of 1/0
    #[arg(short = 'B', long = "boolean-words")]
    pub boolean_words: bool,

    /// Include the columns Access maintains and hides
    #[arg(short = 's', long = "system-columns")]
    pub system_columns: bool,

    /// Write the files of attachment columns to DIR/<number>/<file name>,
    /// giving their paths under DIR in the fields
    #[arg(long = "attachments", value_name = "DIR")]
    pub attachments: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum BinMode {
    Strip,
    Raw,
    Octal,
    Hex,
}

// ---------------------------------------------------------------------------
// Format options (collected from args for pure functions)
// ---------------------------------------------------------------------------

pub struct FormatOptions {
    pub delimiter: char,
    pub date_format: String,
    pub datetime_format: String,
    pub bin_mode: BinMode,
    pub null_string: String,
    pub boolean_words: bool,
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

/// Escape a string value for CSV output (RFC 4180).
///
/// - `always_quote=true`: always wrap in double quotes (for Text/Memo/Guid).
/// - `always_quote=false`: only quote if the value contains delimiter, newline, or `"`.
/// - Internal `"` is doubled to `""`.
pub fn csv_escape(value: &str, delimiter: char, always_quote: bool) -> String {
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
pub fn format_value(value: &Value, opts: &FormatOptions) -> String {
    match value {
        Value::Null => csv_escape(&opts.null_string, opts.delimiter, false),
        Value::Bool(b) => {
            if opts.boolean_words {
                if *b {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            } else if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::Byte(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::Long(v) => v.to_string(),
        Value::BigInt(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::Double(v) => v.to_string(),
        Value::Money(s) | Value::Numeric(s) => s.clone(),
        Value::Text(s) | Value::Guid(s) => csv_escape(s, opts.delimiter, true),
        Value::Binary(data) => {
            let s = format_binary(data, opts.bin_mode);
            if opts.bin_mode == BinMode::Raw && !s.is_empty() {
                csv_escape(&s, opts.delimiter, false)
            } else {
                s
            }
        }
        Value::Timestamp(ts) => {
            if timestamp::is_date_only(*ts) {
                timestamp::format_timestamp(*ts, &opts.date_format)
            } else {
                timestamp::format_timestamp(*ts, &opts.datetime_format)
            }
        }
        Value::DateTimeExtended(s) => csv_escape(s, opts.delimiter, true),
    }
}

/// A value as text, without the quotes of a CSV field: one of the values
/// [`complex_field`] joins.
fn plain_value(value: &Value, opts: &FormatOptions) -> String {
    match value {
        Value::Null => String::new(),
        Value::Text(s) | Value::Guid(s) | Value::DateTimeExtended(s) => s.clone(),
        Value::Binary(data) => format_binary(data, opts.bin_mode),
        other => format_value(other, opts),
    }
}

/// `items` joined by `;` as one quoted field, as Access exports the values
/// of a multivalued field, or an empty field for no items.
fn joined_field(items: &[String], opts: &FormatOptions) -> String {
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
/// attachments, written to `dir` and given as their paths under it when
/// there is one.
fn complex_field(
    values: ComplexValues,
    number: i32,
    opts: &FormatOptions,
    dir: Option<&Path>,
) -> io::Result<String> {
    let mut items = Vec::new();
    match values {
        ComplexValues::MultiValue(values) => {
            items.extend(values.iter().map(|v| plain_value(v, opts)));
        }
        ComplexValues::Attachment(files) => {
            for file in files {
                let name = safe_file_name(&plain_value(&file.name, opts));
                match (dir, &file.data) {
                    (Some(dir), Value::Binary(data)) => {
                        let folder = dir.join(number.to_string());
                        fs::create_dir_all(&folder)?;
                        fs::write(folder.join(&name), data)?;
                        items.push(format!("{number}/{name}"));
                    }
                    (Some(_), _) => {
                        log::warn!("the content of attachment {name} could not be read");
                        items.push(name);
                    }
                    (None, _) => items.push(name),
                }
            }
        }
        ComplexValues::VersionHistory(_) | ComplexValues::Unknown => {}
    }
    Ok(joined_field(&items, opts))
}

/// The field of `value`, with `column` its column if it is a complex column.
/// Multiple values and attachments give their values (see
/// [`complex_field`]); the version history and kinds jetdb does not know
/// keep the number.
fn field(
    reader: &mut PageReader,
    value: &Value,
    column: Option<&ComplexColumn>,
    opts: &FormatOptions,
    dir: Option<&Path>,
) -> Result<String, FileError> {
    if let (Some(column), Value::Long(n)) = (column, value) {
        if matches!(
            column.kind(),
            ComplexKind::MultiValue | ComplexKind::Attachment
        ) {
            let values = column.values(reader, *n)?;
            return complex_field(values, *n, opts, dir).map_err(FileError::Io);
        }
    }
    Ok(format_value(value, opts))
}

/// Format binary data according to the selected mode.
pub fn format_binary(data: &[u8], mode: BinMode) -> String {
    match mode {
        BinMode::Strip => String::new(),
        BinMode::Raw => String::from_utf8_lossy(data).into_owned(),
        BinMode::Octal => {
            let mut s = String::with_capacity(data.len() * 4);
            for b in data {
                s.push_str(&format!("\\{b:03o}"));
            }
            s
        }
        BinMode::Hex => {
            let mut s = String::with_capacity(data.len() * 2);
            for b in data {
                s.push_str(&format!("{b:02x}"));
            }
            s
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn cmd_export(args: ExportArgs, password: Option<&str>) -> ExitCode {
    match run_export(&args, password) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_export(args: &ExportArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let delimiter = args.delimiter.chars().next().unwrap_or(',');
    if args.delimiter.chars().count() > 1 {
        log::warn!("only the first character of delimiter is used");
    }
    let opts = FormatOptions {
        delimiter,
        date_format: args.date_format.clone(),
        datetime_format: args.datetime_format.clone(),
        bin_mode: args.bin_mode,
        null_string: args.null_string.clone(),
        boolean_words: args.boolean_words,
    };

    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let catalog = read_catalog(&mut reader)?;

    let entry = find_table(&catalog, &args.table)?;

    let tdef = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    // Column filtering: build indices of columns to include. Hidden columns
    // are left out unless asked for, except in system tables (see
    // `ColumnDef::is_shown`).
    let is_system_table = entry.is_system();
    let col_indices: Vec<usize> = tdef
        .columns
        .iter()
        .enumerate()
        .filter(|(_, col)| args.system_columns || col.is_shown(is_system_table))
        .map(|(i, _)| i)
        .collect();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let delim_str = delimiter.to_string();

    // Header
    if !args.no_header {
        let header: Vec<String> = col_indices
            .iter()
            .map(|&i| csv_escape(&tdef.columns[i].name, delimiter, false))
            .collect();
        writeln!(out, "{}", header.join(&delim_str)).map_err(jetdb::FileError::Io)?;
    }

    // Data rows
    let complex = col_indices
        .iter()
        .map(|&i| read_complex_column(&mut reader, &catalog, &tdef, &tdef.columns[i]))
        .collect::<Result<Vec<_>, _>>()?;
    let result = read_table_rows(&mut reader, &tdef)?;
    for row in &result.rows {
        let fields: Vec<String> = col_indices
            .iter()
            .zip(&complex)
            .map(|(&i, column)| {
                let dir = args.attachments.as_deref();
                field(&mut reader, &row[i], column.as_ref(), &opts, dir)
            })
            .collect::<Result<_, _>>()?;
        writeln!(out, "{}", fields.join(&delim_str)).map_err(jetdb::FileError::Io)?;
    }

    out.flush().map_err(jetdb::FileError::Io)?;

    result.warn_skipped(&args.table);

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_opts() -> FormatOptions {
        FormatOptions {
            delimiter: ',',
            date_format: timestamp::DEFAULT_DATE_FORMAT.to_string(),
            datetime_format: timestamp::DEFAULT_DATETIME_FORMAT.to_string(),
            bin_mode: BinMode::Hex,
            null_string: String::new(),
            boolean_words: false,
        }
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
        opts.null_string = "(null)".to_string();
        assert_eq!(format_value(&Value::Null, &opts), "(null)");
    }

    #[test]
    fn format_value_null_with_comma() {
        let mut opts = default_opts();
        opts.null_string = "N/A, unknown".to_string();
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
        opts.bin_mode = BinMode::Strip;
        assert_eq!(format_value(&Value::Binary(vec![0xde, 0xad]), &opts), "");
    }

    #[test]
    fn format_value_binary_octal() {
        let mut opts = default_opts();
        opts.bin_mode = BinMode::Octal;
        assert_eq!(
            format_value(&Value::Binary(vec![0x41, 0x42]), &opts),
            "\\101\\102"
        );
    }

    #[test]
    fn format_value_binary_raw_with_comma() {
        let mut opts = default_opts();
        opts.bin_mode = BinMode::Raw;
        assert_eq!(
            format_value(&Value::Binary(b"a,b".to_vec()), &opts),
            "\"a,b\""
        );
    }

    // -- format_binary --------------------------------------------------------

    #[test]
    fn format_binary_hex() {
        assert_eq!(format_binary(&[0xca, 0xfe], BinMode::Hex), "cafe");
    }

    #[test]
    fn format_binary_octal() {
        assert_eq!(format_binary(&[65, 10], BinMode::Octal), "\\101\\012");
    }

    #[test]
    fn format_binary_strip() {
        assert_eq!(format_binary(&[1, 2, 3], BinMode::Strip), "");
    }

    #[test]
    fn format_binary_empty() {
        assert_eq!(format_binary(&[], BinMode::Hex), "");
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
        opts.bin_mode = BinMode::Raw;
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
