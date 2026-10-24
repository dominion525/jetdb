# Changelog

## 0.4.0

### Added

- jetdb-wasm, an npm package that reads Access databases in the browser and Node.js, with TypeScript types. See [docs/wasm.md](docs/wasm.md).
- Opening a database from bytes in memory (`PageReader::open_reader`), which lets the library run on WebAssembly, including the browser target.
- Forms and reports: the properties of a form or report and of each section and control, named after the keys of `Application.SaveAsText`, also for the Access 2000 format (`jetdb form props`, `jetdb form controls`).
- Macros: named macros, macros embedded in forms and reports, and data macros, as statements, XML and the text of SaveAsText (`jetdb macro`).
- The values of complex columns: multiple values, attachments, and the history of versions of a Long Text column.
- The values of calculated columns, with the type of their result.
- Reading the rows of a table one at a time (`TableRows`, `read_row_at`), in `jetdb export` and jetdb-wasm too.
- Writing CSV in the library (`write_csv`).

### Changed

- The minimum supported Rust version is 1.86.
- The text of an Access 97 file is read in the code page of the database, rather than as Latin-1.
- Columns are listed in the order of Design View, and the size of a Text column is in characters, as Access shows it.
- DDL: a column that Access requires a value in is `NOT NULL`, a relationship without referential integrity is not a foreign key, a GUID AutoNumber column is a GUID column, a BigInt column is `BIGINT` in Access SQL, and index names are prefixed with their table in SQLite and PostgreSQL.
- A 32-bit value of a form property is a signed number, as SaveAsText writes it (`BlobValue::Color` is gone), and its GUIDs are in upper case.
- A file without the signature of an Access database is not opened.
- No SQL is given for a saved query whose rows of MSysQueries could not all be read.
- `jetdb schema` and `jetdb prop` align names by their width in a terminal.

### Fixed

- The precision and scale of Jet4 Numeric columns were swapped.
- A hidden index could be taken for the primary key.
- Object types of MSysObjects as real databases store them.
- Exports of system tables came out as empty rows.
- Timestamps before 1899-12-30 lost the time of day.
- A Jet3 row whose variable data ends on a 256-byte boundary.
- Broken data could make jetdb panic: property chunk lengths, Jet3 null masks, index counts, Decimal scales, form property lengths, and deeply nested macro XML.

For 0.2.0 to 0.3.3, see the [GitHub releases](https://github.com/dominion525/jetdb/releases).

## 0.1.1 — 2026-02-16

- Improve robustness of VBA extraction and data row reading.
- Add module-level documentation to all public modules.
- Clean up crate metadata (MSRV, exclude patterns).

## 0.1.0 — 2026-02-15

Initial release.
