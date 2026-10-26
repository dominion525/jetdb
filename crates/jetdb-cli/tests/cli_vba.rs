#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// List VBA modules (default: one per line)
// ---------------------------------------------------------------------------

#[test]
fn vba_list() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in ["Class1", "Form_Form1", "Module1"] {
        assert!(
            stdout.lines().any(|l| l == name),
            "should have {name} on a line, got: {stdout}"
        );
    }
}

// ---------------------------------------------------------------------------
// List VBA modules with -1: the same as the default
// ---------------------------------------------------------------------------

#[test]
fn vba_list_newline() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap(), "-1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let default = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&default.stdout),
        "-1 should print what the default prints"
    );
}

// ---------------------------------------------------------------------------
// List VBA modules with custom delimiter (-d)
// ---------------------------------------------------------------------------

#[test]
fn vba_list_delimiter() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap(), "-d", "|"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('|'),
        "should contain | delimiter, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Show VBA module source code
// ---------------------------------------------------------------------------

#[test]
fn vba_show_source() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "show", path.to_str().unwrap(), "Module1"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Function"),
        "should contain VBA keyword 'Function', got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent module
// ---------------------------------------------------------------------------

#[test]
fn vba_nonexistent_module() {
    let path = skip_if_missing!("vbaV2003.mdb");
    let output = jetdb_bin()
        .args(["vba", "show", path.to_str().unwrap(), "NoSuch"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        !output.status.success(),
        "should fail for nonexistent module"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("VBA module not found: NoSuch"),
        "stderr should contain module name, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn vba_nonexistent_file() {
    let output = jetdb_bin()
        .args(["vba", "list", "/nonexistent/path/to/file.mdb"])
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
// Encrypted .accdb — no password / wrong password
// ---------------------------------------------------------------------------

#[test]
fn vba_list_encrypted_no_password() {
    let path = skip_if_missing!("enc_vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["vba", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("password-protected"),
        "stderr should mention password-protected, got: {stderr}"
    );
}

#[test]
fn vba_list_encrypted_wrong_password() {
    let path = skip_if_missing!("enc_vbaV2007.accdb");
    let output = jetdb_bin()
        .args(["--password", "wrong", "vba", "list", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid password"),
        "stderr should mention invalid password, got: {stderr}"
    );
}
