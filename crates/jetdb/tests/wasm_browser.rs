//! Tests for the browser Wasm target (`wasm32-unknown-unknown`), run on Node.js
//! with `wasm-bindgen-test-runner`.
//!
//! That target has no filesystem, so these tests embed their databases and open
//! them from memory through [`PageReader::open_reader`], the way a browser
//! caller would. On every other target this file compiles to nothing.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use std::io::Cursor;

use jetdb::{JetVersion, PageReader};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn open_reader_reads_a_jet4_database_from_memory() {
    let bytes: &'static [u8] = include_bytes!("../../../testdata/V2003/testV2003.mdb");
    let reader = PageReader::open_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(reader.header().version, JetVersion::Jet4);
}
