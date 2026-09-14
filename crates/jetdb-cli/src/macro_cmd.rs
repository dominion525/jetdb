use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use jetdb::{list_macros, read_data_macros, read_embedded_macros, read_macro, PageReader};

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
    /// Show the embedded macros of a form or report as XML
    Embedded(MacroEmbeddedArgs),
    /// Show the data macros of a table as XML
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
}

#[derive(Args)]
pub struct MacroDataArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Table name
    pub table: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn cmd_macro(args: MacroArgs, password: Option<&str>) -> ExitCode {
    let result = match args.command {
        MacroCommands::List(a) => run_list(&a, password),
        MacroCommands::Show(a) => run_show(&a, password),
        MacroCommands::Embedded(a) => run_embedded(&a, password),
        MacroCommands::Data(a) => run_data(&a, password),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_list(args: &MacroListArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let mut names: Vec<String> = list_macros(&mut reader)?
        .into_iter()
        .map(|m| m.name)
        .collect();
    names.sort_unstable();

    if names.is_empty() {
        return Ok(());
    }
    if args.newline {
        for name in &names {
            println!("{name}");
        }
    } else if let Some(ref delim) = args.delimiter {
        println!("{}", names.join(delim));
    } else {
        println!("{}", names.join(" "));
    }

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

fn run_embedded(args: &MacroEmbeddedArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    for def in read_embedded_macros(&mut reader, &args.name)? {
        println!("{}", def.xml);
    }

    Ok(())
}

fn run_data(args: &MacroDataArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    for def in read_data_macros(&mut reader, &args.table)? {
        println!("{}", def.xml);
    }

    Ok(())
}
