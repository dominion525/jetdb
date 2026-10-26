use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use jetdb::{
    data_macros_to_text, embedded_macro_to_text, list_macros, read_data_macros,
    read_embedded_macros, read_embedded_macros_of_type, read_macro, PageReader,
};

use crate::form::FormTypeArg;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct MacroArgs {
    #[command(subcommand)]
    pub command: MacroCommands,
}

#[derive(Subcommand)]
pub enum MacroCommands {
    /// List named macro names
    List(MacroListArgs),
    /// Show a named macro as SaveAsText text or XML
    Show(MacroShowArgs),
    /// Show the embedded macros of a form or report as SaveAsText text or XML
    Embedded(MacroEmbeddedArgs),
    /// Show the data macros of a table as SaveAsText text or XML
    Data(MacroDataArgs),
}

#[derive(Args)]
pub struct MacroListArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Print one macro name per line
    #[arg(short = '1', long = "newline", conflicts_with = "delimiter")]
    pub newline: bool,

    /// Delimiter between macro names (default: space)
    #[arg(short = 'd', long = "delimiter")]
    pub delimiter: Option<String>,
}

#[derive(Args)]
pub struct MacroShowArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Macro name
    pub macro_name: String,

    /// Print the macro's XML instead of SaveAsText text
    #[arg(long = "xml")]
    pub xml: bool,
}

#[derive(Args)]
pub struct MacroEmbeddedArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Form or report name
    pub name: String,

    /// Print each embedded macro's XML instead of SaveAsText text
    #[arg(long = "xml")]
    pub xml: bool,

    /// Form or report, needed when a form and a report share the name
    #[arg(short = 't', long = "type")]
    pub object_type: Option<FormTypeArg>,
}

#[derive(Args)]
pub struct MacroDataArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Table name
    pub table: String,

    /// Print each data macro's XML instead of SaveAsText text
    #[arg(long = "xml")]
    pub xml: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn cmd_macro(args: MacroArgs, password: Option<&str>) -> ExitCode {
    match args.command {
        MacroCommands::List(a) => crate::exit_code(run_list(&a, password)),
        MacroCommands::Show(a) => crate::exit_code(run_show(&a, password)),
        MacroCommands::Embedded(a) => crate::exit_code(run_embedded(&a, password)),
        MacroCommands::Data(a) => crate::exit_code(run_data(&a, password)),
    }
}

fn run_list(args: &MacroListArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let mut names: Vec<String> = list_macros(&mut reader)?
        .into_iter()
        .map(|m| m.name)
        .collect();
    names.sort_unstable();

    crate::print_names(&names, args.newline, args.delimiter.as_deref());

    Ok(())
}

fn run_show(args: &MacroShowArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let def = read_macro(&mut reader, &args.macro_name)?;

    if args.xml {
        if !def.xml.is_empty() {
            println!("{}", def.xml);
        }
        return Ok(());
    }
    let grid = def.grid.ok_or_else(|| jetdb::FileError::InvalidMacroData {
        reason: format!("the macro grid of {} could not be read", args.macro_name),
    })?;
    print!("{}", grid.to_text());

    Ok(())
}

fn run_embedded(args: &MacroEmbeddedArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let macros = crate::form::read_named(
        &mut reader,
        &args.name,
        args.object_type,
        read_embedded_macros_of_type,
        read_embedded_macros,
    )?;
    for def in macros {
        if args.xml {
            println!("{}", def.xml);
        } else if let Some(text) = embedded_macro_to_text(&def) {
            print!("{text}");
        }
    }

    Ok(())
}

fn run_data(args: &MacroDataArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let macros = read_data_macros(&mut reader, &args.table)?;
    if args.xml {
        for def in &macros {
            println!("{}", def.xml);
        }
    } else {
        let text = data_macros_to_text(&macros);
        if !text.is_empty() {
            print!("{text}\r\n");
        }
    }

    Ok(())
}
