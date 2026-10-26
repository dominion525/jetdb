A pure Rust library for reading Microsoft Access database files (.mdb / .accdb).
No ODBC drivers or C libraries required — read Access databases directly on macOS, Linux,
or any platform Rust supports. Covers Access 97 (Jet3) through Access 2019 (ACE17).

# Quick Start

```toml
[dependencies]
jetdb = "0.3"
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

The jetdb API mirrors the page-based internal structure of Access databases.
To read data, call functions in the following order:

```text
PageReader::open → read_catalog → read_table_def → read_table_rows → Value
```

1. [`PageReader::open`] opens the database file. It automatically detects the engine version from the file header and prepares RC4 decryption if needed. For password-protected .accdb files, use [`PageReader::open_with_password`] instead.
2. [`read_catalog`] reads the system catalog (MSysObjects) and returns a list of database objects as a vector of [`CatalogEntry`].
3. [`read_table_def`] parses the table definition page (TDEF) and returns a [`TableDef`] containing column and index information.
4. [`read_table_rows`] scans the data pages and returns each row's values as [`Value`] enums.

# Reading Table Data

This is the most common use case. Here is a complete example from opening a file to retrieving row data:

```rust,no_run
use jetdb::{PageReader, read_catalog, read_table_def, read_table_rows, Value};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;

    // Find a table in the catalog
    let catalog = read_catalog(&mut reader)?;
    let entry = catalog.iter()
        .find(|e| e.name == "Customers")
        .expect("table not found");

    // Read the table definition
    let table_def = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    // Print column names
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

The `skipped_rows` field of [`ReadResult`] indicates how many rows were skipped due to read errors.
Call `warn_skipped(table)` to emit a `log::warn!` message when any rows were skipped. Internal metadata readers (catalog, queries, relationships, properties, VBA) call this automatically.

# The Value Type

The [`Value`] enum maps to Access data types:

| Value Variant | Access Type | Notes |
|--------------|-------------|-------|
| `Null` | (NULL for any type) | |
| `Bool(bool)` | Yes/No | |
| `Byte(u8)` | Byte | |
| `Int(i16)` | Integer | |
| `Long(i32)` | Long Integer | |
| `BigInt(i64)` | Large Number | ACE16+ |
| `Float(f32)` | Single | |
| `Double(f64)` | Double | |
| `Text(String)` | Text / Memo | Memo for long text |
| `Binary(Vec<u8>)` | Binary / OLE Object | |
| `Money(String)` | Currency | Fixed-point string (4 decimal places) |
| `Numeric(String)` | Decimal | Variable-scale string |
| `Timestamp(f64)` | Date/Time | Days since 1899-12-30 |
| `Guid(String)` | Replication ID | `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` format |

`Money` and `Numeric` are returned as strings to preserve precision.
`Timestamp` is the raw OLE Date value (a floating-point day count from 1899-12-30).

# Other Features

## Listing Table Names

Use [`table_names`] when you only need the names of user-created tables.
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

## Saved Query SQL Recovery

Use [`read_queries`] to read saved query definitions and [`query_to_sql`] to reconstruct the SQL.

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

Use [`read_relationships`] to retrieve foreign key definitions between tables.

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

## Rows One at a Time

[`TableRows`] reads the rows of a table one at a time, so only the row being used is in memory; [`read_table_rows`] reads them all with it. The [`PageReader`] is passed to each call, so between rows the same reader can read other things, such as the values of a complex column. [`TableRows::with_columns`] reads only the columns asked for, without reading the long values of the others. [`TableRows::position`] gives where to continue, [`TableRows::starting_at`] starts again there, and [`read_row_at`] reads the row at one position.

```rust,no_run
use jetdb::{
    calculated_column_types_in, find_table, read_catalog, read_table_def, PageReader, TableRows,
};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = find_table(&catalog, "Orders")?;
    let table = read_table_def(&mut reader, &entry.name, entry.table_page)?;
    let calculated = calculated_column_types_in(&catalog, &table);
    let mut rows = TableRows::new(&table, &calculated);
    while let Some(row) = rows.next_row(&mut reader)? {
        println!("{:?}", row.values);
    }
    rows.warn_skipped(&table.name);
    Ok(())
}
```

## Complex Columns

A complex column (multiple values, attachments, or the version history of an append-only Long Text column, in Access 2007 and later) holds a number in each row, which [`read_table_rows`] returns as `Value::Long`. [`read_complex_column`] reads, of such a column, only where the values of each number are, and [`ComplexColumn::values`] reads the values of one number, so the values of the rows not asked for are never read. Attachments come with the content of each file, decompressed.

```rust,no_run
use jetdb::{
    calculated_column_types_in, find_table, read_catalog, read_complex_column, read_table_def,
    ComplexValues, PageReader, TableRows, Value,
};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.accdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = find_table(&catalog, "Products")?;
    let table = read_table_def(&mut reader, &entry.name, entry.table_page)?;
    let i = table.columns.iter().position(|c| c.name == "Tags").expect("column not found");
    let Some(tags) = read_complex_column(&mut reader, &catalog, &table, &table.columns[i])? else {
        return Ok(());
    };
    let calculated = calculated_column_types_in(&catalog, &table);
    let mut rows = TableRows::new(&table, &calculated);
    while let Some(row) = rows.next_row(&mut reader)? {
        if let Value::Long(n) = row.values[i] {
            if let ComplexValues::MultiValue(values) = tags.values(&mut reader, n)? {
                println!("{values:?}");
            }
        }
    }
    Ok(())
}
```

## CSV

[`write_csv`] writes a table as CSV, as `jetdb export` writes it, one row at a time. [`CsvOptions`] holds the options of `jetdb export`. An attachment field gives the file names, or, with a function given for `attachments`, what that function returns for each file it is given.

```rust,no_run
use jetdb::{find_table, read_catalog, read_table_def, write_csv, CsvOptions, PageReader};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = find_table(&catalog, "Orders")?;
    let table = read_table_def(&mut reader, &entry.name, entry.table_page)?;
    let mut options = CsvOptions::default();
    options.delimiter = '\t';
    let mut out = std::io::stdout().lock();
    write_csv(&mut reader, &catalog, &table, &options, &mut out, None)?;
    Ok(())
}
```

## VBA Source Code

Use [`read_vba_project`] to extract VBA module source code.
For databases without VBA, an empty [`VbaProject`] is returned.

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

## Forms and Reports

Use [`list_forms`] to list the forms and reports, and [`read_form_properties_of_type`] to read the design of one: the properties of the form or report itself, and those of each section and control.
A property is named by [`BlobProperty::display_name`], which gives `0xXXXX` for a property whose name is not known, and its value ([`BlobValue`]) prints as text.

```rust,no_run
use jetdb::{PageReader, control_type_label, list_forms, read_form_properties_of_type};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.accdb")?;
    for entry in list_forms(&mut reader)? {
        let form = read_form_properties_of_type(&mut reader, &entry.name, entry.object_type)?;
        println!("{:?} {}", form.object_type, form.form_name);
        for p in &form.properties {
            println!("  {} = {}", p.display_name(), p.value);
        }
        for control in &form.controls {
            println!("  {} ({})", control.name, control_type_label(control.type_code));
        }
    }
    Ok(())
}
```

[`read_form_type_info`] reads the names and types of the controls from the TypeInfo stream, and [`read_form_stream`] the raw bytes of a stream ([`StreamKind`]).

## Macros

Macros of three kinds are read as [`MacroDef`]: named macros with [`list_macros`] and [`read_macro`], the macros embedded in the events of a form or report with [`read_embedded_macros`], and the data macros of a table with [`read_data_macros`].
A [`MacroDef`] holds the statements of the macro ([`MacroStatement`]), the XML it was read from, and the macro grid of a named or embedded macro.

```rust,no_run
use jetdb::{MacroStatement, PageReader, list_macros, read_macro};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.accdb")?;
    for entry in list_macros(&mut reader)? {
        let def = read_macro(&mut reader, &entry.name)?;
        println!("{}", entry.name);
        for statement in &def.statements {
            if let MacroStatement::Action { name, .. } = statement {
                println!("  {name}");
            }
        }
    }
    Ok(())
}
```

[`data_macros_to_text`] and [`embedded_macro_to_text`] give the text `Application.SaveAsText` writes for data macros and an embedded macro.

## Object Properties

Use [`read_object_properties`] to read the properties of an object, such as the description or the format of a table and its columns. They come in groups ([`PropertyMap`]): one for the object itself, and one for each column.

```rust,no_run
use jetdb::{PageReader, read_object_properties};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let props = read_object_properties(&mut reader, "Customers")?;
    for map in &props.maps {
        println!("{:?} {}", map.map_type, map.name);
        for p in &map.properties {
            println!("  {} = {:?}", p.name, p.value);
        }
    }
    Ok(())
}
```

## DDL Generation

The [`ddl`] module generates DDL for SQLite, PostgreSQL, MySQL, or Access SQL from table definitions.

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

Available dialects: [`ddl::Sqlite`], [`ddl::Postgres`], [`ddl::Mysql`], [`ddl::Access`].

## Password-Protected Databases

Use [`PageReader::open_with_password`] to open password-protected .accdb files, encrypted with Agile Encryption, RC4 CryptoAPI, or Standard or NonStandard AES.

```rust,no_run
use jetdb::{PageReader, table_names};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password("protected.accdb", Some("secret"))?;
    let names = table_names(&mut reader)?;
    for name in &names {
        println!("{name}");
    }
    Ok(())
}
```

For non-encrypted files, `open_with_password` works the same as `open` (the password is ignored).

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
[`FileError`] is an enum with variants for I/O errors, format errors, and missing objects,
and can be propagated with the `?` operator.

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

ACE15 is the name of the version byte 0x04, after Access 2013. Access 2013 writes the format of Access 2010 (ACE14), and no file with the byte 0x04 is known; jetdb reads such a file as ACE15.

A Jet3 (Access 97) file stores text in the code page of the database, such as 1252 for Western European languages or 932 for Japanese. jetdb reads it in the code page in the header of the file ([`PageReader::text_encoding`]), and as Latin-1 when it does not know the code page.

# Limitations

- Read-only (no write support)
- No index-based lookups (full table scan only)
- Password-protected .accdb files require [`PageReader::open_with_password`] (Agile Encryption, RC4 CryptoAPI, Standard/NonStandard AES)
