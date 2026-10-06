use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use jetdb::{query_to_sql, read_queries, PageReader};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub command: QueryCommands,
}

#[derive(Subcommand)]
pub enum QueryCommands {
    /// List saved query names
    List(QueryListArgs),
    /// Show SQL of a saved query
    Show(QueryShowArgs),
}

#[derive(Args)]
pub struct QueryListArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Print one query name per line
    #[arg(short = '1', long = "newline", conflicts_with = "delimiter")]
    pub newline: bool,

    /// Delimiter between query names (default: space)
    #[arg(short = 'd', long = "delimiter")]
    pub delimiter: Option<String>,
}

#[derive(Args)]
pub struct QueryShowArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,

    /// Query name
    pub query_name: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn cmd_queries(args: QueryArgs, password: Option<&str>) -> ExitCode {
    match args.command {
        QueryCommands::List(a) => match run_list(&a, password) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                log::error!("{e}");
                ExitCode::FAILURE
            }
        },
        QueryCommands::Show(a) => match run_show(&a, password) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                log::error!("{e}");
                ExitCode::FAILURE
            }
        },
    }
}

fn run_list(args: &QueryListArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let queries = read_queries(&mut reader)?;

    let mut names: Vec<&str> = queries.iter().map(|q| q.name.as_str()).collect();
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

fn run_show(args: &QueryShowArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let queries = read_queries(&mut reader)?;

    let qdef = queries.iter().find(|q| q.name == args.query_name).ok_or(
        jetdb::FileError::QueryNotFound {
            name: args.query_name.clone(),
        },
    )?;
    if qdef.incomplete {
        return Err(jetdb::FileError::IncompleteQuery {
            name: qdef.name.clone(),
        });
    }
    let sql = query_to_sql(qdef);
    println!("{sql}");

    Ok(())
}
