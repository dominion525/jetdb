#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// Normal case: prop Table1
// ---------------------------------------------------------------------------

#[test]
fn prop_table1() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Object: Table1"),
        "should contain object header, got:\n{stdout}"
    );

    assert!(
        stdout.contains("Table Properties:") || stdout.contains("Column:"),
        "should contain property sections, got:\n{stdout}"
    );

    assert!(
        stdout.contains("GUID"),
        "should contain GUID property, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Nonexistent table: empty output
// ---------------------------------------------------------------------------

#[test]
fn prop_nonexistent_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "NoSuchTable_XYZ"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with empty output for nonexistent object"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "should produce empty output for nonexistent object, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Objects of several types sharing a name: --type chooses one
// ---------------------------------------------------------------------------

#[test]
fn prop_shared_name_needs_type() {
    // nwind.mdb has a table, a form and a macro all named Customers.
    let path = skip_if_missing!("V1997/nwind.mdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "Customers"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success(), "should fail without --type");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("objects of several types are named Customers: form, macro, table"),
        "got: {stderr}"
    );

    let output = jetdb_bin()
        .args([
            "prop",
            "--type",
            "table",
            path.to_str().unwrap(),
            "Customers",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Column: CustomerID"), "got:\n{stdout}");
    assert!(stdout.contains("  Table Properties:"), "got:\n{stdout}");

    // The heading names the type of the object.
    let output = jetdb_bin()
        .args([
            "prop",
            "--type",
            "form",
            path.to_str().unwrap(),
            "Customers",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("  Form Properties:"), "got:\n{stdout}");
    assert!(!stdout.contains("Table Properties:"), "got:\n{stdout}");
}

// ---------------------------------------------------------------------------
// Nonexistent file: error
// ---------------------------------------------------------------------------

#[test]
fn prop_nonexistent_file() {
    let output = jetdb_bin()
        .args(["prop", "/nonexistent/path/to/file.mdb", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success(), "should fail for nonexistent file");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
}
