A pure Rust library for reading Microsoft Access database files (.mdb / .accdb).
Since it has no dependency on ODBC drivers or C libraries, you can read
Access databases directly on macOS or Linux without needing Windows.
Supports Access 97 (Jet3) through Access 2019 (ACE17).

# Quick Start

```toml
[dependencies]
jetdb = { git = "https://github.com/dominion525/jetdb" }
```

```rust,no_run
use jetdb::{PageReader, read_catalog};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    for entry in &catalog {
        println!("{}", entry.name);
    }
    Ok(())
}
```

# Data Flow

The jetdb API is designed around the page-based internal structure of Access databases.
To read data, call the functions in the following order:

```text
PageReader::open → read_catalog → read_table_def → read_table_rows → Value
```

1. [`PageReader::open`] opens the database file. It automatically detects the engine version from the file header and prepares RC4 decryption if needed.
2. [`read_catalog`] reads the system catalog (MSysObjects) and returns the list of objects in the database as a vector of [`CatalogEntry`].
3. [`read_table_def`] parses the table definition page (TDEF) and returns a [`TableDef`] containing column and index information.
4. [`read_table_rows`] scans the data pages and returns each row's values as [`Value`] enums.

# Reading Table Data

This is the most basic use case. Here is a complete example from opening a file to retrieving row data.

```rust,no_run
use jetdb::{PageReader, read_catalog, read_table_def, read_table_rows, Value};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;

    // Find a table in the catalog
    let catalog = read_catalog(&mut reader)?;
    let entry = catalog.iter()
        .find(|e| e.name == "Customers")
        .expect("Table not found");

    // Read the table definition
    let table_def = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    // Display column names
    for col in &table_def.columns {
        print!("{}\t", col.name);
    }
    println!();

    // Read row data
    let result = read_table_rows(&mut reader, &table_def)?;
    for row in &result.rows {
        for value in row {
            match value {
                Value::Text(s) => print!("{s}\t"),
                Value::Long(n) => print!("{n}\t"),
                Value::Double(f) => print!("{f}\t"),
                Value::Null => print!("(null)\t"),
                other => print!("{other:?}\t"),
            }
        }
        println!();
    }

    Ok(())
}
```

The `skipped_rows` field in [`ReadResult`] contains the number of rows that were skipped due to errors during reading.
Calling the `warn_skipped(table)` method logs a warning via `log::warn!` when there are skipped rows. Internal metadata reads (catalog, queries, relationships, properties, VBA) automatically emit this warning.

# The Value Type

The [`Value`] enum corresponds to each Access data type.

| Value Variant | Access Type | Notes |
|---------------|-------------|-------|
| `Null` | (NULL value for any type) | |
| `Bool(bool)` | Yes/No | |
| `Byte(u8)` | Byte | |
| `Int(i16)` | Integer | |
| `Long(i32)` | Long Integer | |
| `BigInt(i64)` | Large Number | ACE16 and later |
| `Float(f32)` | Single | |
| `Double(f64)` | Double | |
| `Text(String)` | Text / Memo | Memo is long text |
| `Binary(Vec<u8>)` | Binary / OLE Object | |
| `Money(String)` | Currency | Fixed-point string (4 decimal places) |
| `Numeric(String)` | Decimal | Variable-scale string |
| `Timestamp(f64)` | Date/Time | Days elapsed since 1899-12-30 |
| `Guid(String)` | Replication ID | `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` format |

`Money` and `Numeric` are returned as strings to preserve precision.
`Timestamp` returns the Access internal OLE Date format as-is (a floating-point number of days relative to the epoch 1899-12-30).

# Additional Features

## Listing Table Names

When you only need the names of user-created tables, [`table_names`] is convenient.
System tables and hidden tables are automatically excluded.

```rust,no_run
use jetdb::{PageReader, table_names};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let names = table_names(&mut reader)?;
    for name in &names {
        println!("{name}");
    }
    Ok(())
}
```

## Schema Information

[`TableDef`] contains column definitions ([`ColumnDef`]) and index definitions ([`IndexDef`]).

```rust,no_run
use jetdb::{PageReader, read_catalog, read_table_def};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = catalog.iter().find(|e| e.name == "Customers").unwrap();
    let table_def = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    for col in &table_def.columns {
        println!("{}: {:?}", col.name, col.col_type);
    }
    for idx in &table_def.indexes {
        println!("INDEX {}: {:?}", idx.name, idx.columns);
    }
    Ok(())
}
```

## Restoring Saved Query SQL

[`read_queries`] reads saved query definitions, and [`query_to_sql`] restores them to SQL strings.

```rust,no_run
use jetdb::{PageReader, read_queries, query_to_sql};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let queries = read_queries(&mut reader)?;
    for qdef in &queries {
        println!("-- {} ({:?})", qdef.name, qdef.query_type);
        println!("{}", query_to_sql(qdef));
    }
    Ok(())
}
```

## Relationships

[`read_relationships`] retrieves foreign key definitions between tables.

```rust,no_run
use jetdb::{PageReader, read_relationships};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let rels = read_relationships(&mut reader)?;
    for rel in &rels {
        println!("{}: {} -> {}", rel.name, rel.from_table, rel.to_table);
    }
    Ok(())
}
```

## VBA Source Code

[`read_vba_project`] extracts VBA module source code.
For databases that do not contain VBA, an empty [`VbaProject`] is returned.

```rust,no_run
use jetdb::{PageReader, read_vba_project};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let project = read_vba_project(&mut reader)?;
    for module in &project.modules {
        println!("--- {} ({:?}) ---", module.name, module.module_type);
        println!("{}", module.source);
    }
    Ok(())
}
```

## DDL Generation

The [`ddl`] module generates DDL in SQLite / PostgreSQL / MySQL / Access SQL from table definitions.

```rust,no_run
use jetdb::{PageReader, read_catalog, read_table_def, read_relationships};
use jetdb::ddl::{generate_ddl, Sqlite};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;

    let mut tables = Vec::new();
    for entry in &catalog {
        if entry.object_type == jetdb::ObjectType::Table
            && !entry.name.starts_with("MSys")
        {
            tables.push(read_table_def(&mut reader, &entry.name, entry.table_page)?);
        }
    }
    let rels = read_relationships(&mut reader)?;
    let sql = generate_ddl(&Sqlite, &tables, &rels, true, true);
    println!("{sql}");
    Ok(())
}
```

Available dialects are [`ddl::Sqlite`], [`ddl::Postgres`], [`ddl::Mysql`], and [`ddl::Access`].

## Opening from Memory (WebAssembly)

[`PageReader::open_reader`] opens a database from an in-memory source such as a `std::io::Cursor<Vec<u8>>` instead of a file path. This is how the library is used on WebAssembly, including the browser target `wasm32-unknown-unknown`, which has no filesystem. Password-protected files use [`PageReader::open_reader_with_password`].

```rust,no_run
use std::io::Cursor;
use jetdb::{PageReader, table_names};

fn list_tables(bytes: Vec<u8>) -> Result<Vec<String>, jetdb::FileError> {
    let mut reader = PageReader::open_reader(Cursor::new(bytes))?;
    table_names(&mut reader)
}
```

The source must satisfy [`file::DataSource`] (`Read + Seek + Send + Sync + UnwindSafe + RefUnwindSafe`), so a reader holding an `Rc` or a `RefCell` is not accepted.

# Error Handling

All public functions return `Result<T,` [`FileError`]`>`.
[`FileError`] is an enum with variants for I/O errors, format violations, object-not-found conditions, and more. It can be propagated directly with the `?` operator.

# Supported Versions

| Engine | Access Version | File Format |
|--------|---------------|-------------|
| Jet3 | Access 97 | .mdb |
| Jet4 | Access 2000/2003 | .mdb |
| ACE12 | Access 2007 | .accdb |
| ACE14 | Access 2010 | .accdb |
| ACE15 | Access 2013 | .accdb |
| ACE16 | Access 2016 | .accdb |
| ACE17 | Access 2019 | .accdb |

# Limitations

- Read-only (writing is not supported)
- Index-based lookups are not supported (sequential scan only)
- [`read_table_rows`] loads all rows of a table into memory. Be mindful of memory usage for tables with a very large number of rows
- Password-protected databases are not supported
