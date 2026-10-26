#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// Normal case: prop Table1
// ---------------------------------------------------------------------------

#[test]
fn prop_puts_the_properties_of_the_object_before_those_of_its_columns() {
    // tblProducts stores the map of its column ProductName before its own.
    let path = skip_if_missing!("jetdb-sample.accdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "tblProducts"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let headings: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("  ") && !l.starts_with("    "))
        .collect();
    assert_eq!(
        headings,
        [
            "  Table Properties:",
            "  Column: ProductName",
            "  Column: UnitPrice",
            "  Column: Discontinued",
        ],
        "got:\n{stdout}"
    );
}

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
// Nonexistent object: error
// ---------------------------------------------------------------------------

#[test]
fn prop_nonexistent_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "NoSuchTable_XYZ"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "should fail for nonexistent object"
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb: object not found: NoSuchTable_XYZ"),
        "got: {stderr}"
    );
}

#[test]
fn prop_name_of_another_case_is_not_found() {
    let path = skip_if_missing!("V2010/macroGeneratedTestV2010.accdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "TBLITEMS"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success(), "should fail for TBLITEMS");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb: object not found: TBLITEMS"),
        "got: {stderr}"
    );
}

#[test]
fn prop_object_of_another_type_is_not_found() {
    // frmEmbedded is a form, not a query.
    let path = skip_if_missing!("V2010/macroGeneratedTestV2010.accdb");
    let output = jetdb_bin()
        .args([
            "prop",
            "--type",
            "query",
            path.to_str().unwrap(),
            "frmEmbedded",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success(), "should fail for a query");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb: query not found: frmEmbedded"),
        "got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Object without properties: empty output
// ---------------------------------------------------------------------------

#[test]
fn prop_object_without_properties() {
    // MSysObjects has no properties.
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["prop", path.to_str().unwrap(), "MSysObjects"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "got:\n{stdout}");
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
// The heading writes the type in words
// ---------------------------------------------------------------------------

#[test]
fn prop_heading_writes_the_type_in_words() {
    let path = skip_if_missing!("V2007/linkerTestV2007.accdb");
    for (name, heading) in [
        ("Table2", "  Linked Table Properties:"),
        ("SummaryInfo", "  Database Property Properties:"),
    ] {
        let output = jetdb_bin()
            .args(["prop", path.to_str().unwrap(), name])
            .output()
            .expect("failed to run jetdb");
        assert!(output.status.success(), "{name}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.lines().any(|l| l == heading),
            "{name}, got:\n{stdout}"
        );
    }
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
