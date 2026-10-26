/// Filter rust-code-analysis NDJSON output for high-complexity functions.
///
/// Reads NDJSON from stdin, recursively walks the `spaces` tree, and prints
/// a table of functions exceeding the thresholds:
///   - Cyclomatic Complexity (CC) >= 10
///   - Cognitive Complexity (Cog) >= 10
///   - Source Lines of Code (SLOC) >= 50
use std::io::{self, BufRead};

use serde::Deserialize;

const THRESHOLD_CC: i64 = 10;
const THRESHOLD_COG: i64 = 10;
const THRESHOLD_SLOC: i64 = 50;

#[derive(Deserialize)]
struct FileEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    spaces: Vec<Space>,
}

#[derive(Deserialize)]
struct Space {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    start_line: i64,
    #[serde(default)]
    end_line: i64,
    #[serde(default)]
    metrics: Metrics,
    #[serde(default)]
    spaces: Vec<Space>,
}

#[derive(Deserialize, Default)]
struct Metrics {
    #[serde(default)]
    cyclomatic: SumMetric,
    #[serde(default)]
    cognitive: SumMetric,
    #[serde(default)]
    loc: LocMetric,
}

#[derive(Deserialize, Default)]
struct SumMetric {
    #[serde(default)]
    sum: f64,
}

#[derive(Deserialize, Default)]
struct LocMetric {
    #[serde(default)]
    sloc: f64,
}

struct FunctionEntry {
    file: String,
    name: String,
    line: String,
    cc: i64,
    cog: i64,
    sloc: i64,
}

fn extract_functions(space: &Space, filepath: &str, results: &mut Vec<FunctionEntry>) {
    if space.kind == "function" {
        let cc = space.metrics.cyclomatic.sum as i64;
        let cog = space.metrics.cognitive.sum as i64;
        let sloc = space.metrics.loc.sloc as i64;

        if cc >= THRESHOLD_CC || cog >= THRESHOLD_COG || sloc >= THRESHOLD_SLOC {
            results.push(FunctionEntry {
                file: filepath.to_string(),
                name: space.name.clone(),
                line: format!("{}-{}", space.start_line, space.end_line),
                cc,
                cog,
                sloc,
            });
        }
    }

    for child in &space.spaces {
        extract_functions(child, filepath, results);
    }
}

fn main() {
    let functions = functions_above_threshold(io::stdin().lock());

    println!("Total: {} functions above threshold", functions.len());

    if functions.is_empty() {
        return;
    }
    print_table(&functions);
}

/// The functions above the thresholds in the JSON lines of
/// rust-code-analysis that `input` reads, most complex first. Lines that are
/// not JSON of a file are passed over.
fn functions_above_threshold(input: impl BufRead) -> Vec<FunctionEntry> {
    let mut functions = Vec::new();
    for line in input.lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<FileEntry>(trimmed) else {
            continue;
        };
        for space in &entry.spaces {
            extract_functions(space, &entry.name, &mut functions);
        }
    }
    functions.sort_by(|a, b| b.cc.cmp(&a.cc).then(b.cog.cmp(&a.cog)));
    functions
}

/// Print `functions` as a table, each column as wide as its widest value.
fn print_table(functions: &[FunctionEntry]) {
    let w_file = functions.iter().map(|f| f.file.len()).max().unwrap().max(4);
    let w_name = functions.iter().map(|f| f.name.len()).max().unwrap().max(8);
    let w_line = functions.iter().map(|f| f.line.len()).max().unwrap().max(5);

    println!(
        "{:<w_file$}  {:<w_name$}  {:<w_line$}  {:>4}  {:>4}  {:>5}",
        "File", "Function", "Lines", "CC", "Cog", "SLOC",
    );
    println!(
        "{:<w_file$}  {:<w_name$}  {:<w_line$}  {:>4}  {:>4}  {:>5}",
        "-".repeat(w_file),
        "-".repeat(w_name),
        "-".repeat(w_line),
        "----",
        "----",
        "-----",
    );
    for f in functions {
        println!(
            "{:<w_file$}  {:<w_name$}  {:<w_line$}  {:>4}  {:>4}  {:>5}",
            f.file, f.name, f.line, f.cc, f.cog, f.sloc,
        );
    }
}
