#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// List forms
// ---------------------------------------------------------------------------

#[test]
fn form_list() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().any(|l| l == "Form1"),
        "should have Form1 on a line, got: {stdout}"
    );
}

#[test]
fn form_list_newline() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "list", "-1", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let default = jetdb_bin()
        .args(["form", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&default.stdout),
        "-1 should print what the default prints"
    );
    assert!(
        String::from_utf8_lossy(&default.stdout).lines().count() >= 2,
        "should have a name a line"
    );
}

// ---------------------------------------------------------------------------
// List forms - no forms in database
// ---------------------------------------------------------------------------

#[test]
fn form_list_no_forms() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["form", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim().is_empty(), "should be empty for no-forms DB");
}

// ---------------------------------------------------------------------------
// Dump form blob
// ---------------------------------------------------------------------------

#[test]
fn form_dump_blob() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "dump", path.to_str().unwrap(), "Form1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.len() >= 2,
        "Blob should be at least 2 bytes, got {}",
        output.stdout.len()
    );
    // Verify Blob header (common pattern)
    assert_eq!(
        output.stdout[0..2],
        [0x15, 0x00],
        "Blob should start with 0x15 0x00"
    );
}

// ---------------------------------------------------------------------------
// Dump form not found
// ---------------------------------------------------------------------------

#[test]
fn form_dump_not_found() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "dump", path.to_str().unwrap(), "NoSuchForm"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "should report not found, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Controls not found
// ---------------------------------------------------------------------------

#[test]
fn form_controls_not_found() {
    let path = skip_if_missing!("vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "controls", path.to_str().unwrap(), "NoSuchForm"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
}

// ---------------------------------------------------------------------------
// Controls success
// ---------------------------------------------------------------------------

#[test]
fn form_controls_success() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "controls", path.to_str().unwrap(), "F_Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    assert!(!trimmed.is_empty(), "controls output should not be empty");
    // Each line should be tab-separated 3 columns: name\ttype\tindex
    for line in trimmed.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            cols.len(),
            3,
            "expected 3 tab-separated columns, got: {line}"
        );
    }
}

// ---------------------------------------------------------------------------
// Props success + error
// ---------------------------------------------------------------------------

#[test]
fn form_props_success() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "F_Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Form: F_Table1"), "should have form header");
    assert!(
        stdout.contains("Form Properties:"),
        "should have properties section"
    );
    assert!(stdout.contains("Control:"), "should have control section");
}

#[test]
fn form_props_not_found() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "NoSuchForm"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "should report not found, got: {stderr}"
    );
}

#[test]
fn form_props_japanese_form() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "jp_フォーム_2"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Form: jp_フォーム_2"),
        "should have Japanese form header, got: {}",
        stdout.lines().next().unwrap_or("")
    );
}

// ---------------------------------------------------------------------------
// List flags: --forms-only, --reports-only, -d
// ---------------------------------------------------------------------------

#[test]
fn form_list_forms_only() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "list", "--forms-only", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "forms-only should list at least one form"
    );
}

#[test]
fn form_list_reports_only_empty() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "list", "--reports-only", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "reports-only should be empty (no reports), got: {}",
        stdout.trim()
    );
}

#[test]
fn form_list_delimiter() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args(["form", "list", "-d", "|", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('|'),
        "delimiter output should contain '|', got: {}",
        stdout.trim()
    );
}

// ---------------------------------------------------------------------------
// Dump stream variants: typeinfo, propdata
// ---------------------------------------------------------------------------

#[test]
fn form_dump_typeinfo() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args([
            "form",
            "dump",
            "-s",
            "typeinfo",
            path.to_str().unwrap(),
            "F_Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.len() >= 4,
        "TypeInfo should be at least 4 bytes, got {}",
        output.stdout.len()
    );
    // TypeInfo magic: 0xF7, 0xEA, 0xCD, 0xAC (little-endian ACCD_EAF7)
    assert_eq!(
        output.stdout[0..4],
        [0xF7, 0xEA, 0xCD, 0xAC],
        "TypeInfo should start with magic bytes"
    );
}

#[test]
fn form_dump_propdata() {
    let path = skip_if_missing!("formPropTest.accdb");
    let output = jetdb_bin()
        .args([
            "form",
            "dump",
            "-s",
            "propdata",
            path.to_str().unwrap(),
            "F_Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.is_empty(), "PropData should not be empty");
}

// ---------------------------------------------------------------------------
// A form and a report of the same name: --type chooses one
// ---------------------------------------------------------------------------

#[test]
fn form_props_shared_name_needs_type() {
    // sameNameTestV2007.accdb has a form and a report both named Shared.
    let path = skip_if_missing!("V2007/sameNameTestV2007.accdb");
    let output = jetdb_bin()
        .args(["form", "props", path.to_str().unwrap(), "Shared"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success(), "should fail without --type");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "objects of several types are named Shared: form, report; choose one with --type"
        ),
        "got: {stderr}"
    );

    for (object_type, header) in [("form", "Form: Shared"), ("report", "Report: Shared")] {
        let output = jetdb_bin()
            .args([
                "form",
                "props",
                "--type",
                object_type,
                path.to_str().unwrap(),
                "Shared",
            ])
            .output()
            .expect("failed to run jetdb");
        assert!(output.status.success(), "{object_type}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.starts_with(header), "{object_type}: {stdout}");
    }
}

// ---------------------------------------------------------------------------
// Access 97: forms and reports are not read
// ---------------------------------------------------------------------------

#[test]
fn form_access97_not_read() {
    // nwind.mdb has a form named Customers, in the format of Access 97.
    let path = skip_if_missing!("V1997/nwind.mdb");
    let output = jetdb_bin()
        .args(["form", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "should list nothing");

    for command in ["props", "controls", "dump"] {
        let output = jetdb_bin()
            .args(["form", command, path.to_str().unwrap(), "Customers"])
            .output()
            .expect("failed to run jetdb");
        assert!(!output.status.success(), "{command}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("the forms and reports of Access 97 are not read"),
            "{command}: {stderr}"
        );
    }
}
