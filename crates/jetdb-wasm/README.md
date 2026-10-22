# jetdb-wasm

Read Microsoft Access databases (.mdb / .accdb) in the browser and Node.js. This is [jetdb](https://github.com/dominion525/jetdb), a pure Rust reader of Access files, built as WebAssembly: no Access, ODBC driver or native module is needed. It reads Access 97 (Jet3) through Access 2019 (ACE17), including password-protected .accdb files.

## Installation

```bash
npm install jetdb-wasm
```

## Usage

In a browser, or with a bundler that builds for the browser, initialize the module once before opening a database:

```js
import init, { Database } from "jetdb-wasm";

await init();
document.querySelector("input[type=file]").addEventListener("change", async (event) => {
  const file = event.target.files[0];
  const db = Database.open(new Uint8Array(await file.arrayBuffer()));
  for (const table of db.tables()) {
    console.log(table, db.columns(table).map((c) => c.name));
  }
  db.free();
});
```

In Node.js, and in a bundle built for the server, the module is ready when it is imported, and has no default export to call:

```js
import { readFileSync } from "node:fs";
import { Database } from "jetdb-wasm";

const db = Database.open(readFileSync("database.accdb"), "password");
const { rows } = db.rows("Customers");
console.log(rows[0]);
db.free();
```

`Database.open` takes the bytes of the file, and the password of a password-protected .accdb file. Call `free()` when the database is no longer needed, or open it with `using` where JavaScript supports it, to release the memory it holds.

## What It Reads

| Object | Methods |
|--------|---------|
| Tables | `tables`, `columns`, `indexes`, `rows`, `csv` |
| Relationships and DDL | `relationships`, `ddl` (SQLite, PostgreSQL, MySQL, Access) |
| Queries | `queries`, `querySql` |
| VBA | `modules`, `moduleSource` |
| Forms and reports | `forms`, `formControls`, `formProperties`, `formStream` |
| Macros | `macros`, `macro`, `embeddedMacros`, `dataMacros`, `dataMacrosText` |
| Object properties | `properties` |

Values come as JavaScript values: numbers, bigints, booleans, strings (for text, dates, money and other values that a number would round), `Uint8Array` for binary data, and arrays for attachments and multiple values. A method that fails throws an `Error` named `JetdbError`, whose `code` tells what went wrong, such as `PASSWORD_REQUIRED` or `TABLE_NOT_FOUND`.

The package includes TypeScript types. See [the JavaScript API](https://github.com/dominion525/jetdb/blob/main/docs/wasm.md) for each method and its options.

## Limitations

- Read-only: jetdb does not write to Access files.
- The whole file is held in memory.
- In an Access 97 (Jet3) file, jetdb does not read the forms, reports and VBA.

## License

MIT OR Apache-2.0
