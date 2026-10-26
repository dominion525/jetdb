#[macro_use]
mod common;
use common::jetdb_bin;

use std::sync::Mutex;

const MACRO_FILE: &str = "V2010/macroGeneratedTestV2010.accdb";

/// Held while a test makes its pipe and runs jetdb: a process another test
/// starts at the same time can hold the reader of the pipe for a moment, and
/// jetdb could then write without an error.
static SPAWN: Mutex<()> = Mutex::new(());

/// Run jetdb with `args`, its standard output a pipe whose reader has
/// closed, and check that it ends quietly with success.
fn assert_quiet_when_stdout_closed(args: &[&str]) {
    let output = {
        let _spawn = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let (reader, writer) = std::io::pipe().expect("failed to create a pipe");
        drop(reader);
        jetdb_bin()
            .args(args)
            .stdout(writer)
            .output()
            .expect("failed to run jetdb")
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{args:?} should succeed, stderr: {stderr}"
    );
    assert!(stderr.is_empty(), "{args:?} should be quiet, got: {stderr}");
}

#[test]
fn ver() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    assert_quiet_when_stdout_closed(&["ver", path.to_str().unwrap()]);
}

#[test]
fn tables() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    assert_quiet_when_stdout_closed(&["tables", path.to_str().unwrap()]);
}

#[test]
fn schema() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    assert_quiet_when_stdout_closed(&["schema", path.to_str().unwrap()]);
}

#[test]
fn schema_ddl() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    assert_quiet_when_stdout_closed(&["schema", path.to_str().unwrap(), "--ddl", "sqlite"]);
}

#[test]
fn export() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    assert_quiet_when_stdout_closed(&["export", path.to_str().unwrap(), "Table1"]);
}

#[test]
fn queries_list() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    assert_quiet_when_stdout_closed(&["queries", "list", path.to_str().unwrap()]);
}

#[test]
fn queries_show() {
    let path = skip_if_missing!("V2003/queryTestV2003.mdb");
    assert_quiet_when_stdout_closed(&["queries", "show", path.to_str().unwrap(), "DeleteQuery"]);
}

#[test]
fn prop() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    assert_quiet_when_stdout_closed(&["prop", path.to_str().unwrap(), "Table1"]);
}

#[test]
fn vba_list() {
    let path = skip_if_missing!("vbaV2003.mdb");
    assert_quiet_when_stdout_closed(&["vba", "list", path.to_str().unwrap()]);
}

#[test]
fn vba_show() {
    let path = skip_if_missing!("vbaV2003.mdb");
    assert_quiet_when_stdout_closed(&["vba", "show", path.to_str().unwrap(), "Module1"]);
}

#[test]
fn form_list() {
    let path = skip_if_missing!("formPropTest.accdb");
    assert_quiet_when_stdout_closed(&["form", "list", path.to_str().unwrap()]);
}

#[test]
fn form_dump() {
    let path = skip_if_missing!("formPropTest.accdb");
    assert_quiet_when_stdout_closed(&["form", "dump", path.to_str().unwrap(), "F_Table1"]);
}

#[test]
fn form_controls() {
    let path = skip_if_missing!("formPropTest.accdb");
    assert_quiet_when_stdout_closed(&["form", "controls", path.to_str().unwrap(), "F_Table1"]);
}

#[test]
fn form_props() {
    let path = skip_if_missing!("formPropTest.accdb");
    assert_quiet_when_stdout_closed(&["form", "props", path.to_str().unwrap(), "F_Table1"]);
}

#[test]
fn macro_list() {
    let path = skip_if_missing!(MACRO_FILE);
    assert_quiet_when_stdout_closed(&["macro", "list", path.to_str().unwrap()]);
}

#[test]
fn macro_show() {
    let path = skip_if_missing!(MACRO_FILE);
    assert_quiet_when_stdout_closed(&["macro", "show", path.to_str().unwrap(), "old_MsgBox"]);
}

#[test]
fn macro_show_xml() {
    let path = skip_if_missing!(MACRO_FILE);
    assert_quiet_when_stdout_closed(&[
        "macro",
        "show",
        "--xml",
        path.to_str().unwrap(),
        "mcrStructure",
    ]);
}

#[test]
fn macro_embedded() {
    let path = skip_if_missing!(MACRO_FILE);
    assert_quiet_when_stdout_closed(&["macro", "embedded", path.to_str().unwrap(), "frmEmbedded"]);
}

#[test]
fn macro_data() {
    let path = skip_if_missing!(MACRO_FILE);
    assert_quiet_when_stdout_closed(&["macro", "data", path.to_str().unwrap(), "tblNamed"]);
}

#[test]
fn help_skill() {
    assert_quiet_when_stdout_closed(&["--help-skill"]);
}
