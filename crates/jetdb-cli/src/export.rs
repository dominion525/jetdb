use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use jetdb::timestamp;
use jetdb::{
    find_table, read_catalog, read_table_def, write_csv, BinaryFormat, CsvOptions, PageReader,
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

impl From<BinMode> for BinaryFormat {
    fn from(mode: BinMode) -> Self {
        match mode {
            BinMode::Strip => BinaryFormat::Strip,
            BinMode::Raw => BinaryFormat::Raw,
            BinMode::Octal => BinaryFormat::Octal,
            BinMode::Hex => BinaryFormat::Hex,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn cmd_export(args: ExportArgs, password: Option<&str>) -> ExitCode {
    crate::exit_code(run_export(&args, password))
}

/// The options of `write_csv` that the arguments give.
fn csv_options(args: &ExportArgs) -> CsvOptions {
    if args.delimiter.chars().count() > 1 {
        log::warn!("only the first character of delimiter is used");
    }
    let mut options = CsvOptions::default();
    options.header = !args.no_header;
    options.delimiter = args.delimiter.chars().next().unwrap_or(',');
    options.date_format = args.date_format.clone();
    options.datetime_format = args.datetime_format.clone();
    options.binary = args.bin_mode.into();
    options.null = args.null_string.clone();
    options.boolean_words = args.boolean_words;
    options.system_columns = args.system_columns;
    options
}

/// Write the file `name` of the attachment that holds `number` in a row to
/// `dir/<number>/<name>`, and give that path under `dir`.
fn write_attachment(dir: &Path, number: i32, name: &str, data: &[u8]) -> io::Result<String> {
    let folder = dir.join(number.to_string());
    fs::create_dir_all(&folder)?;
    fs::write(folder.join(name), data)?;
    Ok(format!("{number}/{name}"))
}

fn run_export(args: &ExportArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let options = csv_options(args);

    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let catalog = read_catalog(&mut reader)?;
    let entry = find_table(&catalog, &args.table)?;
    let tdef = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    // With --attachments, each file goes to DIR/<number>/<file name>, and its
    // field gives that path under DIR.
    let mut write_file = |number: i32, name: &str, data: &[u8]| {
        let dir = args
            .attachments
            .as_deref()
            .expect("only called with a directory");
        write_attachment(dir, number, name, data)
    };
    let attachments = args
        .attachments
        .is_some()
        .then_some(&mut write_file as &mut jetdb::AttachmentWriter<'_>);

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let skipped = write_csv(
        &mut reader,
        &catalog,
        &tdef,
        &options,
        &mut out,
        attachments,
    )?;
    out.flush().map_err(jetdb::FileError::Io)?;
    if skipped > 0 {
        log::warn!(
            "{}: {skipped} row(s) skipped due to parse errors",
            args.table
        );
    }
    Ok(())
}
