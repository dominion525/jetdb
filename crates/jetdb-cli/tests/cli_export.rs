#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// Basic export
// ---------------------------------------------------------------------------

#[test]
fn export_basic() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // Should have a header line
    assert!(!lines.is_empty(), "should have at least a header line");
    // Header should start with known column names
    assert!(
        lines[0].starts_with("A,B,"),
        "header should start with known column names, got: {}",
        lines[0]
    );
    // Should have data rows beyond the header
    assert!(lines.len() > 1, "should have data rows beyond header");
}

// ---------------------------------------------------------------------------
// System tables keep all columns
// ---------------------------------------------------------------------------

#[test]
fn export_system_table_keeps_all_columns() {
    // Every column of the system catalog tables carries the flag that marks
    // columns Access maintains and hides, so excluding those columns left
    // nothing to export.
    for file in ["V1997/testV1997.mdb", "V2003/testV2003.mdb"] {
        let path = skip_if_missing!(file);
        let output = jetdb_bin()
            .args(["export", path.to_str().unwrap(), "MSysObjects"])
            .output()
            .expect("failed to run jetdb");
        assert!(output.status.success(), "{file}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        assert!(
            lines[0].starts_with("Id,ParentId,Name,Type,"),
            "{file}: header: {}",
            lines[0]
        );
        assert!(
            lines[1..].iter().all(|l| !l.is_empty()),
            "{file}: rows should not be empty"
        );
    }
}

// ---------------------------------------------------------------------------
// --no-header (-H)
// ---------------------------------------------------------------------------

#[test]
fn export_no_header() {
    let path = skip_if_missing!("V2003/testV2003.mdb");

    // Get output with header
    let with_header = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .output()
        .expect("failed to run jetdb");
    let stdout_with = String::from_utf8_lossy(&with_header.stdout);
    let lines_with: Vec<&str> = stdout_with.lines().collect();

    // Get output without header
    let without_header = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-H"])
        .output()
        .expect("failed to run jetdb");
    assert!(without_header.status.success());
    let stdout_without = String::from_utf8_lossy(&without_header.stdout);
    let lines_without: Vec<&str> = stdout_without.lines().collect();

    // Without header should have one less line
    assert_eq!(
        lines_with.len(),
        lines_without.len() + 1,
        "no-header should have one fewer line"
    );
}

// ---------------------------------------------------------------------------
// Tab delimiter (-d)
// ---------------------------------------------------------------------------

#[test]
fn export_tab_delimiter() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-d", "\t"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let header = stdout
        .lines()
        .next()
        .expect("should have at least a header line");
    assert!(
        header.contains('\t'),
        "header should contain tabs with -d tab, got: {header}"
    );
}

// ---------------------------------------------------------------------------
// Date format (-D)
// ---------------------------------------------------------------------------

#[test]
fn export_date_format() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-D", "%d/%m/%Y"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with custom date format, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("21/09/1974"),
        "should contain date in d/m/Y format"
    );
}

// ---------------------------------------------------------------------------
// NULL string (-0)
// ---------------------------------------------------------------------------

#[test]
fn export_null_string() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-0", "(null)"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with custom null string, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ---------------------------------------------------------------------------
// Boolean words (-B)
// ---------------------------------------------------------------------------

#[test]
fn export_boolean_words() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-B"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed with boolean words, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TRUE") || stdout.contains("FALSE"),
        "should contain TRUE or FALSE with -B"
    );
}

// ---------------------------------------------------------------------------
// Encrypted .accdb — no password
// ---------------------------------------------------------------------------

#[test]
fn export_encrypted_no_password() {
    let path = skip_if_missing!("enc_vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "SomeTable"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("password-protected"),
        "stderr should mention password-protected, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn export_nonexistent_file() {
    let output = jetdb_bin()
        .args(["export", "/nonexistent/path/to/file.mdb", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent table
// ---------------------------------------------------------------------------

#[test]
fn export_nonexistent_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "NoSuchTable"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "should fail for nonexistent table"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("jetdb:"),
        "stderr should contain 'jetdb:' prefix, got: {stderr}"
    );
    assert!(
        stderr.contains("table not found: NoSuchTable"),
        "stderr should contain table name, got: {stderr}"
    );
}
