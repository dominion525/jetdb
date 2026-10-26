//! `form` subcommand — list forms/reports and dump binary streams.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use jetdb::{BlobValue, FormObjectType, PageReader, StreamKind};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct FormArgs {
    #[command(subcommand)]
    pub command: FormCommands,
}

#[derive(Subcommand)]
pub enum FormCommands {
    /// List form and report names
    List(FormListArgs),
    /// Dump a binary stream from a form or report
    Dump(FormDumpArgs),
    /// List the sections and controls of a form or report
    Controls(FormControlsArgs),
    /// Show form/report and control properties parsed from Blob
    Props(FormPropsArgs),
}

#[derive(Args)]
pub struct FormListArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Print one name per line
    #[arg(short = '1', long = "newline", conflicts_with = "delimiter")]
    pub newline: bool,

    /// Custom delimiter between names
    #[arg(short = 'd', long = "delimiter")]
    pub delimiter: Option<String>,

    /// Show only forms
    #[arg(long = "forms-only", conflicts_with = "reports_only")]
    pub forms_only: bool,

    /// Show only reports
    #[arg(long = "reports-only", conflicts_with = "forms_only")]
    pub reports_only: bool,
}

#[derive(Args)]
pub struct FormDumpArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Form or report name
    pub name: String,

    /// Which stream to dump
    #[arg(short = 's', long = "stream", default_value = "blob")]
    pub stream: StreamArg,

    /// Form or report, needed when a form and a report share the name
    #[arg(short = 't', long = "type")]
    pub object_type: Option<FormTypeArg>,
}

#[derive(Args)]
pub struct FormControlsArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Form or report name
    pub name: String,

    /// Form or report, needed when a form and a report share the name
    #[arg(short = 't', long = "type")]
    pub object_type: Option<FormTypeArg>,
}

#[derive(Args)]
pub struct FormPropsArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Form or report name
    pub name: String,

    /// Form or report, needed when a form and a report share the name
    #[arg(short = 't', long = "type")]
    pub object_type: Option<FormTypeArg>,
}

/// The type a `--type` option names.
#[derive(Clone, Copy, ValueEnum)]
pub enum FormTypeArg {
    Form,
    Report,
}

/// The type of the form or report named `name`: the one `given`, or the
/// only one of that name, or `None` when there is none. A form and a report
/// of the same name need `--type`.
pub(crate) fn form_type_of(
    reader: &mut PageReader,
    name: &str,
    given: Option<FormTypeArg>,
) -> Result<Option<FormObjectType>, String> {
    if let Some(given) = given {
        return Ok(Some(given.object_type()));
    }
    let types = types_named(&jetdb::list_forms(reader).map_err(|e| e.to_string())?, name);
    match types.as_slice() {
        [_, _, ..] => Err(format!(
            "objects of several types are named {name}: form, report; choose one with --type"
        )),
        _ => Ok(types.first().copied()),
    }
}

impl FormTypeArg {
    fn object_type(self) -> FormObjectType {
        match self {
            FormTypeArg::Form => FormObjectType::Form,
            FormTypeArg::Report => FormObjectType::Report,
        }
    }
}

/// The types of the forms and reports of `entries` named `name`, each once.
fn types_named(entries: &[jetdb::FormEntry], name: &str) -> Vec<FormObjectType> {
    let mut types: Vec<FormObjectType> = Vec::new();
    for entry in entries {
        if entry.name == name && !types.contains(&entry.object_type) {
            types.push(entry.object_type);
        }
    }
    types
}

/// What `of_type` reads of the form or report `name` of the type `given`
/// or of the only type of that name, or what `of_name` reads when there is
/// none of that name, with the error as a message.
pub(crate) fn read_named<T>(
    reader: &mut PageReader,
    name: &str,
    given: Option<FormTypeArg>,
    of_type: impl FnOnce(&mut PageReader, &str, FormObjectType) -> Result<T, jetdb::FileError>,
    of_name: impl FnOnce(&mut PageReader, &str) -> Result<T, jetdb::FileError>,
) -> Result<T, String> {
    match form_type_of(reader, name, given)? {
        Some(t) => of_type(reader, name, t),
        None => of_name(reader, name),
    }
    .map_err(|e| e.to_string())
}

#[derive(Clone, ValueEnum)]
pub enum StreamArg {
    Blob,
    Typeinfo,
    Propdata,
    Blobdelta,
}

impl StreamArg {
    fn to_stream_kind(&self) -> StreamKind {
        match self {
            Self::Blob => StreamKind::Blob,
            Self::Typeinfo => StreamKind::TypeInfo,
            Self::Propdata => StreamKind::PropData,
            Self::Blobdelta => StreamKind::BlobDelta,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn cmd_form(args: FormArgs, password: Option<&str>) -> ExitCode {
    match args.command {
        FormCommands::List(a) => crate::exit_code(run_list(&a, password)),
        FormCommands::Dump(a) => crate::exit_code(run_dump(&a, password)),
        FormCommands::Controls(a) => crate::exit_code(run_controls(&a, password)),
        FormCommands::Props(a) => crate::exit_code(run_props(&a, password)),
    }
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn run_list(args: &FormListArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let entries = jetdb::list_forms(&mut reader)?;

    let mut names: Vec<&str> = entries
        .iter()
        .filter(|e| is_listed(args, e.object_type))
        .map(|e| e.name.as_str())
        .collect();
    names.sort_unstable();

    crate::print_names(&names, args.newline, args.delimiter.as_deref());

    Ok(())
}

/// Whether `form list` lists an object of `object_type`.
fn is_listed(args: &FormListArgs, object_type: FormObjectType) -> bool {
    if args.forms_only {
        object_type == FormObjectType::Form
    } else if args.reports_only {
        object_type == FormObjectType::Report
    } else {
        true
    }
}

fn run_dump(args: &FormDumpArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let kind = args.stream.to_stream_kind();
    let stream = read_named(
        &mut reader,
        &args.name,
        args.object_type,
        |reader, name, t| jetdb::read_form_stream_of_type(reader, name, t, kind),
        |reader, name| jetdb::read_form_stream(reader, name, kind),
    )?;

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(&stream.data).map_err(|e| e.to_string())?;

    Ok(())
}

fn run_controls(args: &FormControlsArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    // The sections and controls of the Blob, which TypeInfo can lack.
    let form_props = read_named(
        &mut reader,
        &args.name,
        args.object_type,
        jetdb::read_form_properties_of_type,
        jetdb::read_form_properties,
    )?;

    for (index, ctrl) in form_props.controls.iter().enumerate() {
        let type_name = jetdb::control_type_label(ctrl.type_code);
        println!("{}\t{}\t{}", ctrl.name, type_name, index);
    }

    Ok(())
}

fn run_props(args: &FormPropsArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let form_props = read_named(
        &mut reader,
        &args.name,
        args.object_type,
        jetdb::read_form_properties_of_type,
        jetdb::read_form_properties,
    )?;

    let type_label = form_type_label(form_props.object_type);
    println!("{type_label}: {}", form_props.form_name);

    if !form_props.properties.is_empty() {
        println!();
        println!("  {type_label} Properties:");
        print_properties(&form_props.properties);
    }

    for ctrl in &form_props.controls {
        println!();
        let type_label = jetdb::control_type_label(ctrl.type_code);
        println!("  Control: {} ({})", ctrl.name, type_label);
        print_properties(&ctrl.properties);
    }

    Ok(())
}

fn form_type_label(object_type: FormObjectType) -> &'static str {
    match object_type {
        FormObjectType::Form => "Form",
        FormObjectType::Report => "Report",
    }
}

fn print_properties(props: &[jetdb::BlobProperty]) {
    // Calculate max label width for alignment.
    let max_width = props
        .iter()
        .map(|p| p.display_name().len())
        .max()
        .unwrap_or(0);

    for prop in props {
        let label = prop.display_name();
        println!("    {label:<max_width$}  {}", format_value(&prop.value));
    }
}

/// Format a property value for display: text quoted on one line, the others
/// as they are.
fn format_value(value: &BlobValue) -> String {
    match value {
        BlobValue::Text(s) => crate::quoted(s),
        _ => value.to_string(),
    }
}
