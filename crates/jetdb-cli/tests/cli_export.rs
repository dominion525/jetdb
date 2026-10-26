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
// Complex columns
// ---------------------------------------------------------------------------

/// The fields of `column` in the export of complexDataTestV2007's Table1,
/// by the id of each row, with `extra` arguments.
fn complex_fields(path: &std::path::Path, column: &str, extra: &[&str]) -> Vec<(String, String)> {
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1"])
        .args(extra)
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut lines = stdout.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let i = header.iter().position(|c| *c == column).unwrap();
    // No field of these columns has a comma, so splitting at commas is
    // enough here.
    lines
        .map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            (fields[0].to_string(), fields[i].to_string())
        })
        .collect()
}

#[test]
fn export_complex_columns() {
    let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
    let fields = |column| complex_fields(&path, column, &[]);
    let expected = |values: [&str; 4]| -> Vec<(String, String)> {
        ["row1", "row2", "row3", "row4"]
            .iter()
            .zip(values)
            .map(|(id, v)| (format!("\"{id}\""), v.to_string()))
            .collect()
    };
    // Multiple values joined by ; as Access exports them.
    assert_eq!(
        fields("multi-value-data"),
        expected([
            "",
            "\"value1;value4\"",
            "\"value1;value2;value3;value4\"",
            ""
        ])
    );
    // The file names of attachments.
    assert_eq!(
        fields("attach-data"),
        expected([
            "",
            "\"test_data.txt;test_data2.txt\"",
            "",
            "\"test_data2.txt\""
        ])
    );
    // The version history keeps its numbers.
    assert_eq!(
        fields("VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880"),
        expected(["1", "2", "3", "4"])
    );
}

#[test]
fn export_attachments_to_a_directory() {
    let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
    let dir = std::env::temp_dir().join(format!("jetdb-export-attachments-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fields = complex_fields(
        &path,
        "attach-data",
        &["--attachments", dir.to_str().unwrap()],
    );
    assert_eq!(fields[1].1, "\"2/test_data.txt;2/test_data2.txt\"");
    assert_eq!(fields[3].1, "\"4/test_data2.txt\"");
    assert_eq!(
        std::fs::read_to_string(dir.join("2/test_data.txt")).unwrap(),
        "this is some test data for attachment."
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("4/test_data2.txt")).unwrap(),
        "this is some more test data for attachment."
    );
    std::fs::remove_dir_all(&dir).unwrap();
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

#[test]
fn export_wide_character_delimiter() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-d", "、"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("A、B、"),
        "header should be joined by the delimiter, got: {stdout}"
    );
}

/// A delimiter that is not one character is an error, and nothing is written.
fn assert_delimiter_rejected(delimiter: &str) {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["export", path.to_str().unwrap(), "Table1", "-d", delimiter])
        .output()
        .expect("failed to run jetdb");
    assert_eq!(output.status.code(), Some(1), "should exit with 1");
    assert!(output.stdout.is_empty(), "should write nothing");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr,
        format!("jetdb: delimiter must be one character, got '{delimiter}'\n")
    );
}

#[test]
fn export_empty_delimiter_is_an_error() {
    assert_delimiter_rejected("");
}

#[test]
fn export_two_character_delimiter_is_an_error() {
    assert_delimiter_rejected(";;");
}

#[test]
fn export_backslash_t_delimiter_is_an_error() {
    // What `-d "\t"` passes in a shell: a backslash and `t`.
    assert_delimiter_rejected("\\t");
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
