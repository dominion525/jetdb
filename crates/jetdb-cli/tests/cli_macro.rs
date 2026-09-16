#[macro_use]
mod common;
use common::jetdb_bin;

const MACRO_FILE: &str = "V2010/macroGeneratedTestV2010.accdb";

fn run(args: &[&str]) -> std::process::Output {
    jetdb_bin()
        .args(args)
        .output()
        .expect("failed to run jetdb")
}

fn stdout_of(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// List macros (default: space-separated, -1, -d)
// ---------------------------------------------------------------------------

#[test]
fn macro_list() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&["macro", "list", path.to_str().unwrap()]));
    let names: Vec<&str> = stdout.trim().split(' ').collect();
    assert!(names.contains(&"mcrStructure"), "got: {stdout}");
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "names should be sorted");
}

#[test]
fn macro_list_newline() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&["macro", "list", path.to_str().unwrap(), "-1"]));
    assert!(stdout.lines().any(|l| l == "mcrErrors"), "got: {stdout}");
}

#[test]
fn macro_list_delimiter() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&["macro", "list", path.to_str().unwrap(), "-d", "|"]));
    assert!(stdout.contains("|mcrStructure|"), "got: {stdout}");
}

// ---------------------------------------------------------------------------
// Show a named macro (SaveAsText text / --xml)
// ---------------------------------------------------------------------------

#[test]
fn macro_show_text() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&[
        "macro",
        "show",
        path.to_str().unwrap(),
        "old_MsgBox",
    ]));
    assert_eq!(
        stdout,
        "Version =196611\r\nPublishOption =1\r\nColumnsShown =0\r\nBegin\r\n    Action =\"MsgBox\"\r\nEnd\r\n"
    );
}

#[test]
fn macro_show_xml() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&[
        "macro",
        "show",
        "--xml",
        path.to_str().unwrap(),
        "mcrStructure",
    ]));
    assert!(stdout.starts_with("<?xml"), "got: {stdout}");
    assert!(stdout.contains("<SubMacro Name='SubOne'>"), "got: {stdout}");
}

#[test]
fn macro_show_xml_without_xml_prints_nothing() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&[
        "macro",
        "show",
        "--xml",
        path.to_str().unwrap(),
        "old_MsgBox",
    ]));
    assert_eq!(stdout, "");
}

// ---------------------------------------------------------------------------
// Embedded macros and data macros
// ---------------------------------------------------------------------------

#[test]
fn macro_embedded() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&[
        "macro",
        "embedded",
        path.to_str().unwrap(),
        "frmEmbedded",
    ]));
    assert!(
        stdout.starts_with("OnLoadEmMacro = Begin\r\n"),
        "got: {stdout}"
    );
    assert!(
        stdout.contains("\r\nOnClickEmMacro = Begin\r\n"),
        "got: {stdout}"
    );
}

#[test]
fn macro_embedded_xml() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&[
        "macro",
        "embedded",
        "--xml",
        path.to_str().unwrap(),
        "frmEmbedded",
    ]));
    assert_eq!(stdout.lines().count(), 2, "got: {stdout}");
    assert!(stdout.contains("For='btnHello'"), "got: {stdout}");
}

#[test]
fn macro_data() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&["macro", "data", path.to_str().unwrap(), "tblNamed"]));
    assert!(stdout.contains("<DataMacros xmlns="), "got: {stdout}");
    assert!(
        stdout.contains("<DataMacro Name=\"dmLog\">"),
        "got: {stdout}"
    );
}

#[test]
fn macro_data_xml() {
    let path = skip_if_missing!(MACRO_FILE);
    let stdout = stdout_of(&run(&[
        "macro",
        "data",
        "--xml",
        path.to_str().unwrap(),
        "tblItems",
    ]));
    assert_eq!(stdout.matches("<?xml").count(), 2, "got: {stdout}");
    assert!(!stdout.contains("<DataMacros"), "got: {stdout}");
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

fn assert_fails_with(args: &[&str], message: &str) {
    let output = run(args);
    assert!(!output.status.success(), "should fail: {args:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(message),
        "expected {message}, got: {stderr}"
    );
}

#[test]
fn macro_not_found_errors() {
    let path = skip_if_missing!(MACRO_FILE);
    let file = path.to_str().unwrap();
    assert_fails_with(
        &["macro", "show", file, "NoSuch"],
        "macro not found: NoSuch",
    );
    assert_fails_with(
        &["macro", "embedded", file, "NoSuch"],
        "form/report not found: NoSuch",
    );
    assert_fails_with(
        &["macro", "data", file, "NoSuch"],
        "table not found: NoSuch",
    );
}

#[test]
fn macro_nonexistent_file() {
    assert_fails_with(
        &["macro", "list", "/nonexistent/path/to/file.mdb"],
        "jetdb:",
    );
}
