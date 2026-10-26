# Changelog

## 0.4.0

### Added

- jetdb-wasm, an npm package that reads Access databases in the browser and Node.js, with TypeScript types. See [docs/wasm.md](docs/wasm.md).
- Opening a database from bytes in memory (`PageReader::open_reader`), which lets the library run on WebAssembly, including the browser target.
- Macros: named macros, macros embedded in forms and reports, and data macros, as statements, XML and the text of SaveAsText (`jetdb macro`).
- The values of complex columns: multiple values, attachments, and the history of versions of a Long Text column; `jetdb export --attachments` writes the attachments to files.
- The values of calculated columns, with the type of their result.
- Reading the rows of a table one at a time (`TableRows`, `read_row_at`), in `jetdb export` and jetdb-wasm too.
- Writing CSV in the library (`write_csv`).
- `--type` for `jetdb prop`, `jetdb form dump`, `jetdb form controls`, `jetdb form props` and `jetdb macro embedded`, to choose among objects of different types that share a name; without it, such a name is an error rather than one of them.

### Changed

- The minimum supported Rust version is 1.87.
- The public enums and the public structs whose fields are all public are `#[non_exhaustive]`; build `CsvOptions` from `CsvOptions::default()`.
- `ObjectType` follows the values Access stores in MSysObjects: `SystemTable` is gone, `Container`, `Database`, `LinkedOdbcTable` and `UserInfo` are new, and the values differ (`Form` is -32768, for example).
- `VbaModuleType::ClassOrDocument` is split into `Class` and `Document`.
- `encoding::decode_text` takes a `TextEncoding` rather than whether the file is Jet3.
- `DdlDialect` has three more required methods: `auto_increment_needs_key`, `supports_comments` and `index_names_per_table`.
- `column_flags::REPLICATION`, `is_replication_column` and `index_type::NORMAL` are deprecated: use `column_flags::HIDDEN`, `ColumnDef::is_hidden`, and `index_type::PRIMARY` or `index_type::ORDINARY`.
- The text of an Access 97 file is read in the code page of the database, rather than as Latin-1.
- Columns are listed in the order of Design View, and the size of a Text column is in characters, as Access shows it.
- DDL: a column that Access requires a value in is `NOT NULL`, a relationship without referential integrity is not a foreign key, a GUID AutoNumber column is a GUID column, a BigInt column is `BIGINT` in Access SQL, and index names are prefixed with their table in SQLite and PostgreSQL.
- Forms and reports: the Blob of a design is read as a tree, more properties are named after the keys of `Application.SaveAsText`, and the Access 2000 format is read too. `jetdb form controls` lists the sections and controls of the Blob, its third column being their position in that list.
- The forms and reports of Access 97 give `FileError::Jet3FormsNotRead` ("the forms and reports of Access 97 are not read") rather than not found.
- A 32-bit value of a form property is a signed number, as SaveAsText writes it (`BlobValue::Color` is gone), and its GUIDs are in upper case.
- A file without the signature of an Access database is not opened.
- No SQL is given for a saved query whose rows of MSysQueries could not all be read.
- The list subcommands (`queries list`, `vba list`, `form list`, `macro list`) print one name per line by default, as `tables` does; `-1` is accepted and does nothing.
- `jetdb prop` fails with "not found" for a missing object, heads the properties of an object with its type in words (`Query Properties:`, `Linked Table Properties:`) rather than always `Table Properties:`, and prints them before those of its columns.
- `jetdb prop` and `jetdb form props` quote text values and write a line break or another control character as `\r`, `\n`, `\t` or `\u{..}`, so that each property is on one line.
- `jetdb schema` and `jetdb prop` align names by their width in a terminal.
- `jetdb export -d` takes one character; an empty or longer delimiter is an error.
- `jetdb` without a subcommand prints its usage to stderr and exits with 2, and `jetdb --version` says `jetdb`.
- I/O errors name the path they failed on.

### Fixed

- The precision and scale of Jet4 Numeric columns were swapped.
- A hidden index could be taken for the primary key.
- Exports of system tables came out as empty rows.
- Timestamps before 1899-12-30 lost the time of day.
- A Jet3 row whose variable data ends on a 256-byte boundary.
- Broken data could make jetdb panic: property chunk lengths, Jet3 null masks, index counts, Decimal scales, form property lengths, deeply nested macro XML and form designs, the compressed data of a VBA project, and dates far out of range.
- An entry of a DirData stream of MSysAccessStorage without a null at the end of its name had its name read one byte off; it is taken as broken.
- The ON conditions of a join of two tables that were already joined inside another join were left out of the SQL of a query, or put on the outer join rather than where Access puts them.
- DDL: only a sole AutoNumber primary key is a SQLite `INTEGER PRIMARY KEY AUTOINCREMENT`, a MySQL `AUTO_INCREMENT` column that does not begin the primary key gets an index, MySQL GUID columns hold the braced values (`CHAR(38)`), and complex columns are not auto-increment columns or indexes.
- A CSV field of any type that contains the delimiter is quoted, not only text.
- The CLI panicked when the reader of its output closed the pipe, as in `jetdb schema ... | head`; it ends quietly.
- The crates and the archives of the CLI binaries hold the license files, and the links of the README work on crates.io.

For 0.2.0 to 0.3.3, see the [GitHub releases](https://github.com/dominion525/jetdb/releases).

## 0.1.1 — 2026-02-16

- Improve robustness of VBA extraction and data row reading.
- Add module-level documentation to all public modules.
- Clean up crate metadata (MSRV, exclude patterns).

## 0.1.0 — 2026-02-15

Initial release.
