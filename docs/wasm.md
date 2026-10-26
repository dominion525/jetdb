# JavaScript API (jetdb-wasm)

[日本語版 (Japanese)](wasm.ja.md)

jetdb-wasm is jetdb built as WebAssembly, for reading Microsoft Access databases (.mdb / .accdb) in the browser and Node.js. This document describes its `Database` class. The package includes TypeScript types, whose comments describe each field of the objects the methods return. The types need TypeScript 5.2 or later.

## Loading the Module

The package holds two builds and chooses one through the `exports` field of its package.json: CommonJS for Node.js, chosen by the `node` condition, and an ES module for everything else, such as browsers and bundlers that build for the browser.

With the ES module, call the default export once before using `Database`. It fetches and compiles the Wasm file next to the module:

```js
import init, { Database } from "jetdb-wasm";

await init();
```

Where the Wasm file cannot be fetched next to the module, such as in Cloudflare Workers, or where a bundler such as esbuild loads it, import the file as `jetdb-wasm/web/jetdb_wasm_bg.wasm` and pass it to `initSync`, as a `WebAssembly.Module` or as its bytes:

```js
import { initSync, Database } from "jetdb-wasm";
import wasm from "jetdb-wasm/web/jetdb_wasm_bg.wasm";

initSync({ module: wasm });
```

Node.js, and a bundle built for the server, such as for server-side rendering, get the CommonJS build. It loads its Wasm file when it is imported, so `Database` is ready to use without calling the default export. The default export and `initSync` are there and do nothing, so code that calls them, such as code shared with the browser, runs as well:

```js
import { Database } from "jetdb-wasm";
```

## Opening a Database

`Database.open(bytes, password)` opens a database from the bytes of its file, as a `Uint8Array` (a Node.js `Buffer` is one) or an `ArrayBuffer`. The password is that of a password-protected .accdb file (Agile Encryption, RC4 CryptoAPI, or Standard or NonStandard AES), and is not used for a file without one.

```js
const db = Database.open(bytes, "password");
console.log(db.version()); // "JET3", "JET4", "ACE12", "ACE14", "ACE15", "ACE16" or "ACE17"
```

A `Database` holds the bytes of the file in the memory of the Wasm module. Call `db.free()` when it is no longer needed, or, where JavaScript supports `using`, open it with `using`, which frees it at the end of its block:

```js
{
  using db = Database.open(bytes);
  // ...
}
```

`free()` may be called more than once, also on a database opened with `using`, and a method called after it throws `DATABASE_FREED`.

## Tables

| Method | Returns |
|--------|---------|
| `tables(options)` | The names of the user tables, sorted. With `{ system: true }`, the system and hidden tables too |
| `columns(table)` | The columns in the order Access shows them: name, type, size, precision and scale, AutoNumber, calculated |
| `indexes(table)` | The indexes: name, primary key, columns and their order, unique, ignore NULLs, required |
| `rows(table, options)` | The rows, each an object keyed by the column names |
| `csv(table, options)` | The rows as CSV, as `jetdb export` writes them |

`rows` gives the names of the columns in the order of the table (`columns`), the rows (`rows`), and the number of rows that could not be read and were left out (`skipped`). The keys of a row do not keep the order of the columns: JavaScript puts a key that looks like an integer, such as `"2019"`, before the others.

To read a large table a part at a time, give the largest number of rows to read as `limit`, and the `next` of the call before as `start`. `next` is null after the last rows:

```js
let start;
do {
  const part = db.rows("Orders", { start, limit: 1000 });
  for (const row of part.rows) {
    // ...
  }
  start = part.next;
} while (start !== null);
```

`next` is a position among the pages of the table, not a count of rows, so it is valid only for the same table of the same file. The `next` of another table given as `start` is not detected: `rows` reads that table from the same position, without an error. A `start` past the last row of the table gives no rows, and `next` is null.

The options of `csv` are those of `jetdb export`:

| Option | Default | Meaning |
|--------|---------|---------|
| `header` | `true` | Write the names of the columns as the first row |
| `delimiter` | `,` | The delimiter, one character |
| `dateFormat` | `%Y-%m-%d` | The format of a date without a time, a subset of strftime |
| `datetimeFormat` | `%Y-%m-%d %H:%M:%S` | The format of a date with a time |
| `bin` | `hex` | How Binary and OLE Object values are written: `strip`, `raw`, `octal` or `hex` |
| `null` | empty | The text of a NULL value |
| `booleanWords` | `false` | `TRUE` and `FALSE` rather than `1` and `0` for Yes/No values |
| `systemColumns` | `false` | Include the columns Access maintains and hides |

An attachment column gives the names of its files in the CSV.

## Relationships and DDL

`relationships(options)` gives the relationships between tables, sorted by name: the two tables, the pairs of columns, and whether Access keeps referential integrity and cascades updates and deletes. With `{ system: true }`, it includes those of system and hidden tables.

`ddl(dialect, options)` gives the DDL of the user tables, as `jetdb schema --ddl` writes it: CREATE TABLE, CREATE INDEX and the foreign keys. The dialect is `sqlite`, `postgres`, `mysql` or `access`.

| Option | Default | Meaning |
|--------|---------|---------|
| `table` | all user tables | Only this table, which may be a system table |
| `indexes` | `true` | Include the indexes |
| `relations` | `true` | Include the foreign keys. With `false`, the relationships are not read, so a file whose relationships cannot be read still gives the DDL of its tables |

## Queries

`queries(options)` gives the saved queries, sorted by name, with their types (`Select`, `MakeTable`, `Append`, `Update`, `Delete`, `Crosstab`, `Ddl`, `Passthrough` or `Union`). With `{ system: true }`, it includes the system and hidden queries, such as those Access makes for forms and reports.

`querySql(name)` gives the SQL of a saved query as Access shows it in SQL view.

## VBA

`modules()` gives the VBA modules, sorted by name, with their types (`Standard`, `Class` or `Document`, the module of a form or report). `moduleSource(name)` gives the source of a module, with the `Attribute` lines Access keeps before it, as `jetdb vba show` prints it.

## Forms and Reports

| Method | Returns |
|--------|---------|
| `forms()` | The forms and reports, sorted by name, with whether each is a form or a report |
| `formControls(name, options)` | The sections and controls, in the order of the design, with their types and places in the list |
| `formProperties(name, options)` | The design: the properties of the form or report, and those of each section and control |
| `formStream(name, options)` | The bytes of a stream of the form or report: `blob` (the design, the default), `typeinfo`, `propdata` or `blobdelta`, given as the `stream` option |

When a form and a report share the name, give `{ type: "Form" }` or `{ type: "Report" }` to `formControls`, `formProperties`, `formStream` and `embeddedMacros`; without it, they throw `INVALID_ARGUMENT`.

A property has its name as `name`, or its number as `0x....` for one jetdb does not know, and its number as `id`. A Yes/No property stored without a value, set to the value that is not its default, has the value `NotDefault`, as SaveAsText writes it. A color is a number, `0x00BBGGRR`, or negative for a system color.

## Macros

| Method | Returns |
|--------|---------|
| `macros(options)` | The names of the named macros, sorted. With `{ system: true }`, those Access makes for itself and names with `~` too |
| `macro(name)` | A named macro |
| `embeddedMacros(form, options)` | The macros embedded in the events of a form or report |
| `dataMacros(table)` | The data macros of a table |
| `dataMacrosText(table)` | The SaveAsText text of all the data macros of a table, as `jetdb macro data` prints it |

A macro has where it is (`source`), its statements (`statements`), the XML Access keeps for it (`xml`, empty for a macro saved before Access 2010), and its SaveAsText text (`text`), as `jetdb macro show` and `jetdb macro embedded` print it. `text` is null for a data macro, whose text `dataMacrosText` gives for its whole table; for a named macro whose grid of actions could not be read; and for an embedded macro stored without XML. For a data macro, `xml` is the whole XML document it was stored in, which may hold other data macros of the table too.

A statement is an action with its arguments, a comment, an `If` block, a group, a submacro, a record block of a data macro, or an element jetdb does not interpret, kept as it is.

## Object Properties

`properties(name, options)` gives the properties Access keeps for an object, such as the description or the format: those of the object itself (`object`), those of each column of a table (`columns`), and additional groups (`additional`). When objects of different types share the name, such as a table and a form both named `Customers`, give the type as `{ type: "Table" }`; without it, the method throws `INVALID_ARGUMENT`.

## Values

A value in a row, or of a property, is a JavaScript value by the type of its column:

| Access type | JavaScript value |
|-------------|------------------|
| Byte, Integer, Long Integer, Single, Double | `number`. A Single is the shortest decimal that gives back the stored value, as Access shows it; `Math.fround(value)` gives the stored value itself |
| Large Number (BigInt) | `bigint` |
| Yes/No | `boolean` |
| Short Text, Long Text, Replication ID (GUID), Currency, Decimal | `string`, so that no digits are lost |
| Date/Time | `string`, such as `2021-06-14`, or `2021-06-14 22:45:12` when the time is not midnight, rounded to the second as Access shows it |
| Date/Time Extended | `string` with all its digits, such as `2021-06-14 22:45:12.3456789`, or a `Uint8Array` of the stored bytes when they cannot be read as a date and time |
| Binary, OLE Object, and a type jetdb does not know | `Uint8Array` |
| Multiple values | An array of the values, empty when there are none |
| Attachment | An array of files, each with its name, type and content |
| Long Text with the history of versions | An array of the versions, oldest first |
| A complex column of a kind jetdb does not know | `number`, the ID of its values in a hidden table |
| NULL | `null` |

## Errors

A method that fails throws an `Error` whose `name` is `JetdbError` and whose `code` tells what went wrong. `JetdbError` is the TypeScript type of this error, not a class the package exports, so tell it by `name` and `code`, not by `instanceof`:

| Code | Meaning |
|------|---------|
| `PASSWORD_REQUIRED` | The file is password-protected, and no password was given |
| `INVALID_PASSWORD` | The password is wrong |
| `UNSUPPORTED_ENCRYPTION` | The file is encrypted in a way jetdb cannot read |
| `TABLE_NOT_FOUND`, `QUERY_NOT_FOUND`, `MODULE_NOT_FOUND`, `FORM_NOT_FOUND`, `MACRO_NOT_FOUND`, `OBJECT_NOT_FOUND` | No object of that name |
| `INVALID_FILE` | Not an Access database, or a broken one; the message says what is wrong |
| `NOT_SUPPORTED` | The database holds what jetdb does not read, such as the forms and reports of Access 97 |
| `INVALID_ARGUMENT` | A method was called with an argument or option that is not valid, such as a value of the wrong type or one outside its list, or with a name that objects of different types share and no `type` option to choose one |
| `DATABASE_FREED` | A method was called after `free()`, or after the end of the block of the `using` that opened the database |
| `IO` | Reading the bytes failed |

```js
try {
  db.rows("Customers");
} catch (e) {
  if (e.name === "JetdbError" && e.code === "TABLE_NOT_FOUND") {
    // ...
  }
}
```

## Limitations

- Read-only: jetdb does not write to Access files.
- The whole file is held in memory.
- In an Access 97 (Jet3) file, jetdb does not read the forms, reports and VBA: `forms()` and `modules()` give none, and reading a form or report by its name throws `NOT_SUPPORTED`.
