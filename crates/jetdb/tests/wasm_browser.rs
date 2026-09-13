//! Tests for the browser Wasm target (`wasm32-unknown-unknown`), run on Node.js
//! with `wasm-bindgen-test-runner`.
//!
//! That target has no filesystem, so these tests embed their databases and open
//! them from memory through [`PageReader::open_reader`], the way a browser
//! caller would. The expected values are the ones the native tests in the
//! library check against the same files.
//!
//! `testdata/V1997/nwind.mdb` is not stored in the repository: run
//! `scripts/fetch-testdata.sh` before building these tests.
//!
//! On every other target this file compiles to nothing.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use std::io::Cursor;

use jetdb::{
    read_catalog, read_table_def, read_table_rows, read_vba_project, JetVersion, PageReader,
    TableDef, Value, VbaModuleType,
};
use wasm_bindgen_test::wasm_bindgen_test;

fn open(bytes: &'static [u8]) -> PageReader {
    PageReader::open_reader(Cursor::new(bytes)).unwrap()
}

fn read_table(reader: &mut PageReader, name: &str) -> (TableDef, Vec<Vec<Value>>) {
    let catalog = read_catalog(reader).unwrap();
    let entry = catalog
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("{name} not in catalog"));
    let table = read_table_def(reader, &entry.name, entry.table_page).unwrap();
    let result = read_table_rows(reader, &table).unwrap();
    assert_eq!(result.skipped_rows, 0, "rows skipped in {name}");
    (table, result.rows)
}

fn column(table: &TableDef, name: &str) -> usize {
    table
        .columns
        .iter()
        .position(|c| c.name == name)
        .unwrap_or_else(|| panic!("column {name} not found"))
}

// -- Long values spanning pages ------------------------------------------------
//
// MSP_PROJECTS holds a Memo long enough to be stored outside the row and a
// 22,970-byte OLE value. Reading them back byte for byte exercises the
// long-value page chains on each format.

const EXPECTED_AUTHOR: &str = "Jon Iles this is a a vawesrasoih aksdkl fas dlkjflkasjd flkjaslkdjflkajlksj dfl lkasjdf lkjaskldfj lkas dlk lkjsjdfkl; aslkdf lkasjkldjf lka skldf lka sdkjfl;kasjd falksjdfljaslkdjf laskjdfk jalskjd flkj aslkdjflkjkjasljdflkjas jf;lkasjd fjkas dasdf asd fasdf asdf asdmhf lksaiyudfoi jasodfj902384jsdf9 aw90se fisajldkfj lkasj dlkfslkd jflksjadf as";

const EXPECTED_BINARY: &[u8] = include_bytes!("../../../testdata/test2BinData.dat");

fn assert_long_values(bytes: &'static [u8], version: JetVersion) {
    let mut reader = open(bytes);
    assert_eq!(reader.header().version, version);

    let (table, rows) = read_table(&mut reader, "MSP_PROJECTS");
    let row = rows.first().expect("MSP_PROJECTS has a row");

    assert_eq!(
        row[column(&table, "PROJ_PROP_AUTHOR")],
        Value::Text(EXPECTED_AUTHOR.to_string())
    );
    assert_eq!(
        row[column(&table, "PROJ_PROP_TITLE")],
        Value::Text("Project1".to_string())
    );
    assert_eq!(
        row[column(&table, "RESERVED_BINARY_DATA")],
        Value::Binary(EXPECTED_BINARY.to_vec())
    );
}

#[wasm_bindgen_test]
fn jet3_long_values() {
    assert_long_values(
        include_bytes!("../../../testdata/V1997/test2V1997.mdb"),
        JetVersion::Jet3,
    );
}

#[wasm_bindgen_test]
fn jet4_long_values() {
    assert_long_values(
        include_bytes!("../../../testdata/V2003/test2V2003.mdb"),
        JetVersion::Jet4,
    );
}

#[wasm_bindgen_test]
fn ace14_long_values() {
    assert_long_values(
        include_bytes!("../../../testdata/V2010/test2V2010.accdb"),
        JetVersion::Ace14,
    );
}

// -- Encrypted database --------------------------------------------------------

#[wasm_bindgen_test]
fn agile_encrypted_database() {
    let bytes: &'static [u8] = include_bytes!("../../../testdata/db2013-enc.accdb");
    let mut reader = PageReader::open_reader_with_password(Cursor::new(bytes), Some("1234"))
        .expect("should open with correct password");

    let (_, rows) = read_table(&mut reader, "Customers");
    let field1: Vec<Option<&str>> = rows
        .iter()
        .map(|row| match &row[1] {
            Value::Text(s) => Some(s.as_str()),
            Value::Null => None,
            other => panic!("unexpected Field1 value: {other:?}"),
        })
        .collect();
    assert_eq!(
        field1,
        [
            Some("Test"),
            Some("Test2"),
            Some("a"),
            None,
            Some("c"),
            Some("d"),
            Some("f"),
        ]
    );
}

// -- VBA project ---------------------------------------------------------------
//
// VBA sits in a compound file read through `cfb`, whose timestamps go through
// `web-time` and, on this target only, through JavaScript.

#[wasm_bindgen_test]
fn vba_project() {
    let mut reader = open(include_bytes!("../../../testdata/vbaV2007.accdb"));
    let project = read_vba_project(&mut reader).expect("failed to read VBA project");

    let expected = [
        ("Class1", VbaModuleType::ClassOrDocument),
        ("Form_Form1", VbaModuleType::ClassOrDocument),
        ("Module1", VbaModuleType::Standard),
    ];
    let mut modules: Vec<_> = project.modules.iter().collect();
    modules.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(modules.len(), expected.len());
    for (module, (name, module_type)) in modules.iter().zip(expected) {
        assert_eq!(module.name, name);
        assert_eq!(module.module_type, module_type, "type mismatch for {name}");
        assert!(!module.source.is_empty(), "empty source for {name}");
    }
}

// -- Jet3 table with only fixed-length columns -----------------------------------

/// The regression from github.com/dominion525/jetdb/issues/12, read from memory.
#[wasm_bindgen_test]
fn jet3_all_fixed_column_table_reads_every_row() {
    let mut reader = open(include_bytes!("../../../testdata/V1997/nwind.mdb"));
    let (table, rows) = read_table(&mut reader, "Order Details");
    assert!(table.columns.iter().all(|c| c.is_fixed));
    assert_eq!(rows.len(), 2155);

    let discounted = rows
        .iter()
        .filter(|row| !matches!(row[4], Value::Float(d) if d == 0.0))
        .count();
    assert_eq!(discounted, 838);
}
