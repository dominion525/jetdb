#[macro_use]
mod common;
use common::jetdb_bin;

// ---------------------------------------------------------------------------
// Single table: -T Table1 (with column verification)
// ---------------------------------------------------------------------------

#[test]
fn schema_single_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Table: Table1"),
        "should contain table header, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Columns:"),
        "should contain Columns section, got:\n{stdout}"
    );
    // Verify specific columns and types
    assert!(
        stdout.contains("A  Text(50)"),
        "should contain column A with type"
    );
    assert!(stdout.contains("Text(100)"), "should contain Text(100)");
    assert!(stdout.contains("Long"), "should contain Long type");
    assert!(
        stdout.contains("Timestamp"),
        "should contain Timestamp type"
    );
}

// ---------------------------------------------------------------------------
// All tables (no -T)
// ---------------------------------------------------------------------------

#[test]
fn schema_all_tables() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap()])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain at least one "Table:" header
    assert!(
        stdout.contains("Table:"),
        "should contain at least one Table: header, got:\n{stdout}"
    );
    // Should not contain system tables
    assert!(
        !stdout.contains("Table: MSys"),
        "should not show system tables, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --no-indexes
// ---------------------------------------------------------------------------

#[test]
fn schema_no_indexes() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "Table1",
            "--no-indexes",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Columns:"),
        "should still show Columns section, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Indexes:"),
        "should not contain Indexes section with --no-indexes, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --no-relations (with a DB that actually has relationships)
// ---------------------------------------------------------------------------

#[test]
fn schema_no_relations() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "-T",
            "Table1",
            "--no-relations",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Relationships:"),
        "should not contain Relationships section with --no-relations, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Relationships section
// ---------------------------------------------------------------------------

#[test]
fn schema_with_relationships() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Relationships:"),
        "should contain Relationships section, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Table1.otherfk1 -> Table2.id"),
        "should contain Table1.otherfk1 -> Table2.id, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error: nonexistent file
// ---------------------------------------------------------------------------

#[test]
fn schema_nonexistent_file() {
    let output = jetdb_bin()
        .args(["schema", "/nonexistent/path/to/file.mdb"])
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
// Error: nonexistent table with specific message
// ---------------------------------------------------------------------------

#[test]
fn schema_nonexistent_table_msg() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "NoSuchTable"])
        .output()
        .expect("failed to run jetdb");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("table not found: NoSuchTable"),
        "stderr should contain specific table name, got: {stderr}"
    );
}

// ===========================================================================
// DDL output tests
// ===========================================================================

// ---------------------------------------------------------------------------
// --ddl sqlite basic
// ---------------------------------------------------------------------------

#[test]
fn ddl_sqlite_basic() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "--ddl", "sqlite"])
        .output()
        .expect("failed to run jetdb");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("CREATE TABLE"),
        "should contain CREATE TABLE, got:\n{stdout}"
    );
    // SQLite types
    assert!(
        stdout.contains("TEXT") || stdout.contains("INTEGER"),
        "should contain SQLite types, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl sqlite single table (-T)
// ---------------------------------------------------------------------------

#[test]
fn ddl_sqlite_single_table() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "--ddl",
            "sqlite",
            "-T",
            "Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"Table1\""),
        "should contain quoted Table1, got:\n{stdout}"
    );
    // Should contain only one CREATE TABLE
    assert_eq!(
        stdout.matches("CREATE TABLE").count(),
        1,
        "should have exactly one CREATE TABLE, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl postgres types
// ---------------------------------------------------------------------------

#[test]
fn ddl_postgres_types() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "--ddl",
            "postgres",
            "-T",
            "Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Table1 has Text columns -> VARCHAR
    assert!(
        stdout.contains("VARCHAR"),
        "should contain VARCHAR for text columns, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl mysql backtick quoting
// ---------------------------------------------------------------------------

#[test]
fn ddl_mysql_backtick() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "--ddl",
            "mysql",
            "-T",
            "Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("`Table1`"),
        "should use backtick quoting, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl access types
// ---------------------------------------------------------------------------

#[test]
fn ddl_access_types() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "--ddl",
            "access",
            "-T",
            "Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[Table1]"),
        "should use bracket quoting, got:\n{stdout}"
    );
    // Table1 has Text columns -> TEXT(n) in Access
    assert!(
        stdout.contains("TEXT("),
        "should contain TEXT(n) for text columns, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl sqlite --no-indexes
// ---------------------------------------------------------------------------

#[test]
fn ddl_no_indexes() {
    let path = skip_if_missing!("V2003/testV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "--ddl",
            "sqlite",
            "--no-indexes",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("CREATE INDEX") && !stdout.contains("CREATE UNIQUE INDEX"),
        "should not contain CREATE INDEX with --no-indexes, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl postgres --no-relations
// ---------------------------------------------------------------------------

#[test]
fn ddl_no_relations() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "--ddl",
            "postgres",
            "--no-relations",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("FOREIGN KEY"),
        "should not contain FOREIGN KEY with --no-relations, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl postgres with relationships (ALTER TABLE FK)
// ---------------------------------------------------------------------------

#[test]
fn ddl_with_relationships() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "--ddl", "postgres"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ALTER TABLE"),
        "should contain ALTER TABLE for FK, got:\n{stdout}"
    );
    assert!(
        stdout.contains("FOREIGN KEY"),
        "should contain FOREIGN KEY, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// --ddl sqlite with relationships (inline FK)
// ---------------------------------------------------------------------------

#[test]
fn ddl_sqlite_inline_fk() {
    let path = skip_if_missing!("V2003/indexTestV2003.mdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "--ddl", "sqlite"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // SQLite should have inline FOREIGN KEY inside CREATE TABLE
    assert!(
        stdout.contains("FOREIGN KEY"),
        "should contain inline FOREIGN KEY for SQLite, got:\n{stdout}"
    );
    // Should NOT have ALTER TABLE
    assert!(
        !stdout.contains("ALTER TABLE"),
        "SQLite should not use ALTER TABLE for FK, got:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Calculated columns: shown and written in DDL with the type of their result
// ---------------------------------------------------------------------------

#[test]
fn schema_calculated_columns() {
    // Access declares MonthlySalary (a Currency result) as Double.
    let path = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
    let output = jetdb_bin()
        .args(["schema", path.to_str().unwrap(), "-T", "Table1"])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("MonthlySalary "))
        .unwrap_or_else(|| panic!("no MonthlySalary line in:\n{stdout}"));
    assert_eq!(
        line.split_whitespace().collect::<Vec<_>>(),
        ["MonthlySalary", "Money", "CALC"],
        "got:\n{stdout}"
    );

    let output = jetdb_bin()
        .args([
            "schema",
            path.to_str().unwrap(),
            "--ddl",
            "postgres",
            "-T",
            "Table1",
        ])
        .output()
        .expect("failed to run jetdb");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"DecimalTest\" NUMERIC,"),
        "got:\n{stdout}"
    );
}
