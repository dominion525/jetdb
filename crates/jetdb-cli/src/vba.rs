use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use jetdb::{read_vba_project, PageReader};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct VbaArgs {
    #[command(subcommand)]
    pub command: VbaCommands,
}

#[derive(Subcommand)]
pub enum VbaCommands {
    /// List VBA module names
    List(VbaListArgs),
    /// Show VBA module source code
    Show(VbaShowArgs),
}

#[derive(Args)]
pub struct VbaListArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Print one module name per line (the default; kept for compatibility, does nothing)
    #[arg(short = '1', long = "newline", conflicts_with = "delimiter")]
    pub newline: bool,

    /// Print the module names on one line, separated by this string
    #[arg(short = 'd', long = "delimiter")]
    pub delimiter: Option<String>,
}

#[derive(Args)]
pub struct VbaShowArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Module name
    pub module_name: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn cmd_vba(args: VbaArgs, password: Option<&str>) -> ExitCode {
    match args.command {
        VbaCommands::List(a) => crate::exit_code(run_list(&a, password)),
        VbaCommands::Show(a) => crate::exit_code(run_show(&a, password)),
    }
}

fn run_list(args: &VbaListArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let project = read_vba_project(&mut reader)?;

    let mut names: Vec<&str> = project.modules.iter().map(|m| m.name.as_str()).collect();
    names.sort_unstable();

    crate::print_names(&mut io::stdout().lock(), &names, args.delimiter.as_deref())?;

    Ok(())
}

fn run_show(args: &VbaShowArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let project = read_vba_project(&mut reader)?;

    let module = project
        .modules
        .iter()
        .find(|m| m.name == args.module_name)
        .ok_or(jetdb::FileError::ModuleNotFound {
            name: args.module_name.clone(),
        })?;
    writeln!(io::stdout(), "{}", module.source)?;

    Ok(())
}
