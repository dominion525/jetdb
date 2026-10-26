use std::process::Command;

fn jetdb_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jetdb"))
}

// ---------------------------------------------------------------------------
// No subcommand
// ---------------------------------------------------------------------------

#[test]
fn no_arguments_prints_help_to_stderr() {
    let output = jetdb_bin().output().expect("failed to run jetdb");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage: jetdb [OPTIONS] [COMMAND]"),
        "stderr should contain the usage, got: {stderr}"
    );
}

#[test]
fn options_without_subcommand_is_error() {
    let output = jetdb_bin()
        .args(["--password", "1234567890"])
        .output()
        .expect("failed to run jetdb");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires a subcommand"),
        "stderr should say a subcommand is required, got: {stderr}"
    );
    assert!(
        stderr.contains("Usage: jetdb [OPTIONS] [COMMAND]"),
        "stderr should contain the usage, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Top-level options
// ---------------------------------------------------------------------------

#[test]
fn help_skill() {
    let output = jetdb_bin()
        .arg("--help-skill")
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("The jetdb Claude Code Skill has moved"),
        "stdout should tell where the skill has moved, got: {stdout}"
    );
}

#[test]
fn version() {
    let output = jetdb_bin()
        .arg("--version")
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!("jetdb ", env!("CARGO_PKG_VERSION"), "\n")
    );
}
