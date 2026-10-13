//! `form` subcommand — list forms/reports and dump binary streams.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use jetdb::{FormObjectType, PageReader, StreamKind};

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
    /// List control names from TypeInfo
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
        return Ok(Some(match given {
            FormTypeArg::Form => FormObjectType::Form,
            FormTypeArg::Report => FormObjectType::Report,
        }));
    }
    let mut types: Vec<FormObjectType> = Vec::new();
    for entry in jetdb::list_forms(reader).map_err(|e| e.to_string())? {
        if entry.name == name && !types.contains(&entry.object_type) {
            types.push(entry.object_type);
        }
    }
    match types.as_slice() {
        [_, _, ..] => Err(format!(
            "objects of several types are named {name}: form, report; choose one with --type"
        )),
        _ => Ok(types.first().copied()),
    }
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
        FormCommands::List(a) => match run_list(&a, password) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                log::error!("{e}");
                ExitCode::FAILURE
            }
        },
        FormCommands::Dump(a) => match run_dump(&a, password) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                log::error!("{e}");
                ExitCode::FAILURE
            }
        },
        FormCommands::Controls(a) => match run_controls(&a, password) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                log::error!("{e}");
                ExitCode::FAILURE
            }
        },
        FormCommands::Props(a) => match run_props(&a, password) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                log::error!("{e}");
                ExitCode::FAILURE
            }
        },
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
        .filter(|e| {
            if args.forms_only {
                e.object_type == FormObjectType::Form
            } else if args.reports_only {
                e.object_type == FormObjectType::Report
            } else {
                true
            }
        })
        .map(|e| e.name.as_str())
        .collect();
    names.sort_unstable();

    if names.is_empty() {
        return Ok(());
    }

    if args.newline {
        for name in &names {
            println!("{name}");
        }
    } else if let Some(delim) = &args.delimiter {
        println!("{}", names.join(delim));
    } else {
        println!("{}", names.join(" "));
    }

    Ok(())
}

fn run_dump(args: &FormDumpArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let kind = args.stream.to_stream_kind();
    let stream = match form_type_of(&mut reader, &args.name, args.object_type)? {
        Some(t) => jetdb::read_form_stream_of_type(&mut reader, &args.name, t, kind),
        None => jetdb::read_form_stream(&mut reader, &args.name, kind),
    }
    .map_err(|e| e.to_string())?;

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(&stream.data).map_err(|e| e.to_string())?;

    Ok(())
}

fn run_controls(args: &FormControlsArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let type_info = match form_type_of(&mut reader, &args.name, args.object_type)? {
        Some(t) => jetdb::read_form_type_info_of_type(&mut reader, &args.name, t),
        None => jetdb::read_form_type_info(&mut reader, &args.name),
    }
    .map_err(|e| e.to_string())?;

    for ctrl in &type_info.controls {
        let type_name = jetdb::control_type_name(ctrl.type_code)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("0x{:04X}", ctrl.type_code));
        println!("{}\t{}\t{}", ctrl.name, type_name, ctrl.index);
    }

    Ok(())
}

fn run_props(args: &FormPropsArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let form_props = match form_type_of(&mut reader, &args.name, args.object_type)? {
        Some(t) => jetdb::read_form_properties_of_type(&mut reader, &args.name, t),
        None => jetdb::read_form_properties(&mut reader, &args.name),
    }
    .map_err(|e| e.to_string())?;

    let type_label = match form_props.object_type {
        FormObjectType::Form => "Form",
        FormObjectType::Report => "Report",
    };
    println!("{type_label}: {}", form_props.form_name);

    if !form_props.properties.is_empty() {
        println!();
        println!("  {type_label} Properties:");
        print_properties(&form_props.properties);
    }

    for ctrl in &form_props.controls {
        println!();
        let type_label = jetdb::control_type_name(ctrl.type_code)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("0x{:04X}", ctrl.type_code));
        println!("  Control: {} ({})", ctrl.name, type_label);
        print_properties(&ctrl.properties);
    }

    Ok(())
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
        println!("    {label:<max_width$}  {}", prop.value);
    }
}
