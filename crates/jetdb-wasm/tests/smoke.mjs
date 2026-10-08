// Checks the installed jetdb-wasm package: the Node.js build through the
// package name, and the web build initialized with its Wasm.
//
// Run by scripts/test-wasm-package.sh, which installs the package next to
// this file and passes the testdata directory as the first argument.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";
import { runInNewContext } from "node:vm";

import { Database as NodeDatabase } from "jetdb-wasm";

const testdata = process.argv[2];
const read = (file) => readFileSync(join(testdata, file));

function check(Database, build) {
  checkOpen(Database, build);
  checkTables(Database, build);
  checkColumns(Database, build);
  checkIndexes(Database, build);
  checkRows(Database, build);
  checkSkippedRows(Database, build);
  checkRelationships(Database, build);
  checkDdl(Database, build);
  checkQueries(Database, build);
  checkProperties(Database, build);
  checkErrors(Database, build);
  checkStringArguments(Database, build);
  checkPassword(Database, build);
  console.log(`ok: ${build}`);
}

const openTest = (Database) => Database.open(read("V2003/testV2003.mdb"));

function checkTables(Database, build) {
  const db = openTest(Database);
  assert.equal(db.version(), "JET4", build);
  const tables = db.tables();
  assert.ok(tables.includes("Table1"), `${build}: ${tables}`);
  assert.ok(!tables.some((name) => name.startsWith("MSys")), `${build}: ${tables}`);
  assert.ok(db.tables({ system: true }).includes("MSysObjects"), build);
  assert.throws(() => db.tables({ system: "yes" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.throws(() => db.tables(5), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
}

function checkColumns(Database, build) {
  const db = openTest(Database);
  const columns = db.columns("Table1");
  assert.deepEqual(
    columns.map((c) => [c.name, c.type, c.size]),
    [
      ["A", "Text", 100],
      ["B", "Text", 200],
      ["C", "Byte", 1],
      ["D", "Int", 2],
      ["E", "Long", 4],
      ["F", "Double", 8],
      ["G", "Timestamp", 8],
      ["H", "Money", 8],
      ["I", "Boolean", 1],
    ],
    build,
  );
  assert.deepEqual(
    columns[0],
    { name: "A", type: "Text", size: 100, precision: 0, scale: 0, autoNumber: false, calculated: false },
    build,
  );
  assert.throws(() => db.columns("NoSuchTable"), { name: "JetdbError", code: "TABLE_NOT_FOUND" }, build);
}

function checkIndexes(Database, build) {
  const db = openTest(Database);
  assert.deepEqual(
    db.indexes("Table1"),
    [
      {
        name: "B",
        primaryKey: false,
        columns: [{ name: "B", descending: false }],
        unique: false,
        ignoreNulls: false,
        required: false,
      },
      {
        name: "PrimaryKey",
        primaryKey: true,
        columns: [{ name: "A", descending: false }],
        unique: true,
        ignoreNulls: false,
        required: true,
      },
    ],
    build,
  );
  assert.throws(() => db.indexes("NoSuchTable"), { name: "JetdbError", code: "TABLE_NOT_FOUND" }, build);
}

function checkRows(Database, build) {
  const db = openTest(Database);
  assert.deepEqual(
    db.rows("Table1"),
    {
      columns: ["A", "B", "C", "D", "E", "F", "G", "H", "I"],
      rows: [
        { A: "abcdefg", B: "hijklmnop", C: 2, D: 222, E: 333333333, F: 444.555, G: "1974-09-21", H: "3.5000", I: true },
        { A: "a", B: "b", C: 0, D: 0, E: 0, F: 0, G: "1981-12-12", H: "0.0000", I: false },
      ],
      skipped: 0,
    },
    build,
  );
  assert.throws(() => db.rows("NoSuchTable"), { name: "JetdbError", code: "TABLE_NOT_FOUND" }, build);
  // A table without rows still gives the names of its columns.
  const empty = db.rows("Table2");
  assert.deepEqual(empty.rows, [], build);
  assert.deepEqual(empty.columns, db.columns("Table2").map((c) => c.name), build);
  assert.ok(empty.columns.length > 0, build);

  const binary = Database.open(read("V2010/binIdxTestV2010.accdb")).rows("Test").rows;
  assert.deepEqual(binary.find((row) => row.ID === 1).BinAsc, new Uint8Array([0x61, 0x62]), build);
  assert.equal(binary.find((row) => row.ID === 200).BinAsc, null, build);

  // 9007199254740993 is 2^53 + 1, which a number cannot hold exactly.
  assert.deepEqual(
    Database.open(read("V2016/bigIntTestV2016.accdb")).rows("BigIntTable").rows,
    [
      { ID: 1, Big: 9007199254740993n },
      { ID: 2, Big: -9007199254740993n },
      { ID: 3, Big: 0n },
      { ID: 4, Big: null },
    ],
    build,
  );
}

function checkSkippedRows(Database, build) {
  // Table1's two rows are on page 27 of the 4096-byte pages. Giving the
  // second row the offset of the first leaves it no bytes, so it cannot be
  // read.
  const damaged = new Uint8Array(read("V2003/testV2003.mdb"));
  const pos = 27 * 4096 + 16;
  assert.equal(damaged[pos] | (damaged[pos + 1] << 8), 0x0f89, build);
  damaged.set([0xb8, 0x0f], pos);

  const { rows, skipped } = Database.open(damaged).rows("Table1");
  assert.deepEqual(
    rows.map((row) => row.A),
    ["abcdefg"],
    build,
  );
  assert.equal(skipped, 1, build);
}

function checkRelationships(Database, build) {
  const db = Database.open(read("V2003/indexTestV2003.mdb"));
  assert.deepEqual(
    db.relationships(),
    [
      {
        name: "Table2Table1",
        fromTable: "Table1",
        toTable: "Table2",
        columns: [{ from: "otherfk1", to: "id" }],
        referentialIntegrity: true,
        cascadeUpdate: false,
        cascadeDelete: true,
      },
      {
        name: "Table3Table1",
        fromTable: "Table1",
        toTable: "Table3",
        columns: [{ from: "otherfk2", to: "id" }],
        referentialIntegrity: true,
        cascadeUpdate: true,
        cascadeDelete: false,
      },
    ],
    build,
  );
  assert.throws(() => db.relationships({ system: "yes" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);

  // Access adds relationships between its own system tables from Access 2000.
  const v2000 = Database.open(read("V2000/indexTestV2000.mdb"));
  assert.equal(v2000.relationships().length, 2, build);
  assert.equal(v2000.relationships({ system: true }).length, 4, build);
  assert.deepEqual(openTest(Database).relationships(), [], build);
}

function checkDdl(Database, build) {
  const db = Database.open(read("V2003/indexTestV2003.mdb"));
  assert.equal(
    db.ddl("postgres", { table: "Table2" }),
    'CREATE TABLE "Table2" (\n    "id" INTEGER,\n    "data" VARCHAR(50),\n    PRIMARY KEY ("id")\n);\n\n' +
      'CREATE INDEX "Table2_id_idx" ON "Table2" ("id");\n',
    build,
  );
  const all = db.ddl("postgres");
  for (const expected of ['CREATE TABLE "Table1"', 'CREATE TABLE "Table3"', "ON DELETE CASCADE", "ON UPDATE CASCADE"]) {
    assert.ok(all.includes(expected), `${build}: ${expected} in ${all}`);
  }
  const bare = db.ddl("postgres", { indexes: false, relations: false });
  assert.ok(!bare.includes("CREATE INDEX") && !bare.includes("FOREIGN KEY"), `${build}: ${bare}`);
  assert.ok(db.ddl("sqlite", { table: "Table1" }).includes("    FOREIGN KEY (\"otherfk1\")"), build);
  assert.ok(db.ddl("mysql").startsWith("CREATE TABLE `Table1` ("), build);
  assert.ok(db.ddl("access").startsWith("CREATE TABLE [Table1] ("), build);
  assert.throws(() => db.ddl("oracle"), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.throws(() => db.ddl("mysql", { table: 2 }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.throws(() => db.ddl("mysql", { table: "NoSuchTable" }), { name: "JetdbError", code: "TABLE_NOT_FOUND" }, build);
}

function checkQueries(Database, build) {
  const db = Database.open(read("V2003/queryTestV2003.mdb"));
  assert.deepEqual(
    db.queries(),
    [
      { name: "AppendQuery", type: "Append" },
      { name: "CrosstabQuery", type: "Crosstab" },
      { name: "DataDefinitionQuery", type: "Ddl" },
      { name: "DeleteQuery", type: "Delete" },
      { name: "MakeTableQuery", type: "MakeTable" },
      { name: "PassthroughQuery", type: "Passthrough" },
      { name: "SelectQuery", type: "Select" },
      { name: "UnionQuery", type: "Union" },
      { name: "UpdateQuery", type: "Update" },
    ],
    build,
  );
  assert.equal(
    db.querySql("DeleteQuery"),
    'DELETE Table1.col1, Table1.col2, Table1.col3\nFROM Table1\nWHERE (((Table1.col1)>"blah"));',
    build,
  );
  assert.throws(() => db.queries({ system: "yes" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.throws(() => db.querySql("NoSuchQuery"), { name: "JetdbError", code: "QUERY_NOT_FOUND" }, build);
  assert.deepEqual(openTest(Database).queries(), [], build);

  // Access made ~sq_rStatistics-byPlace for a report.
  const sports = Database.open(read("saveastext/SportsAdmin/Sports.accdb"));
  const hidden = "~sq_rStatistics-byPlace";
  assert.ok(!sports.queries().some((q) => q.name === hidden), build);
  assert.deepEqual(sports.queries({ system: true }).find((q) => q.name === hidden), { name: hidden, type: "Crosstab" }, build);
  assert.ok(sports.querySql(hidden).startsWith("TRANSFORM Count("), build);
}

function checkProperties(Database, build) {
  const db = Database.open(read("V2010/calcFieldTestV2010.accdb"));
  const props = db.properties("Table1");
  const value = (properties, name) => properties.find((p) => p.name === name).value;
  assert.match(value(props.object, "GUID"), /^\{[0-9A-F-]+\}$/, build);
  assert.equal(value(props.object, "TotalsRow"), false, build);
  assert.ok(value(props.object, "NameMap") instanceof Uint8Array, build);
  const firstName = props.columns.find((c) => c.name === "FirstName").properties;
  assert.equal(value(firstName, "AllowZeroLength"), true, build);
  assert.equal(value(firstName, "ColumnWidth"), 1380, build);
  assert.deepEqual(props.additional, [], build);

  // nwind.mdb has a form, a macro and a table all named Customers.
  const nwind = Database.open(read("V1997/nwind.mdb"));
  assert.throws(
    () => nwind.properties("Customers"),
    (e) => e.name === "JetdbError" && e.code === "INVALID_ARGUMENT" && e.message.includes("Form, Macro, Table"),
    build,
  );
  assert.ok(nwind.properties("Customers", { type: "Table" }).columns.some((c) => c.name === "CustomerID"), build);
  assert.match(value(nwind.properties("Customers", { type: "Form" }).object, "Description"), /Single-column form/, build);
  assert.throws(() => nwind.properties("Customers", { type: "Query" }), { name: "JetdbError", code: "OBJECT_NOT_FOUND" }, build);
  assert.throws(() => nwind.properties("Customers", { type: "View" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.throws(() => db.properties("NoSuchObject"), { name: "JetdbError", code: "OBJECT_NOT_FOUND" }, build);
  assert.throws(() => db.properties(42), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
}

function checkOpen(Database, build) {
  // read gives a Node.js Buffer, which is a Uint8Array.
  const buffer = read("V2003/testV2003.mdb");
  const arrayBuffer = buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
  assert.equal(Database.open(arrayBuffer).version(), "JET4", build);
  assert.equal(Database.open(new Uint8Array(arrayBuffer)).version(), "JET4", build);
  assert.throws(() => Database.open("testV2003.mdb"), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);

  // Bytes made in another realm, such as an iframe or a vm context, are not
  // instances of this realm's Uint8Array and ArrayBuffer.
  const foreign = runInNewContext("new Uint8Array(bytes)", { bytes: buffer });
  assert.ok(!(foreign instanceof Uint8Array), build);
  assert.equal(Database.open(foreign).version(), "JET4", build);
  assert.equal(Database.open(foreign.buffer).version(), "JET4", build);
  const notBytes = runInNewContext("new Int16Array(4)");
  assert.throws(() => Database.open(notBytes), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
}

function checkErrors(Database, build) {
  // The other checks match each error by its code; this one checks that an
  // error is an Error, and the code of bytes that are not a database.
  assert.throws(
    () => Database.open(new Uint8Array(10)),
    (e) => e instanceof Error && e.name === "JetdbError" && e.code === "INVALID_FILE",
    build,
  );
}

function checkStringArguments(Database, build) {
  // A value that is not a string, which TypeScript would not allow, is an
  // INVALID_ARGUMENT error rather than an error of the Wasm, and the database
  // can still be read after it.
  const db = openTest(Database);
  const invalid = { name: "JetdbError", code: "INVALID_ARGUMENT" };
  for (const method of ["columns", "indexes", "rows", "querySql"]) {
    for (const value of [undefined, null, 42, {}]) {
      assert.throws(() => db[method](value), invalid, `${build}: ${method}(${value})`);
    }
  }
  assert.equal(db.columns("Table1").length, 9, build);

  const bytes = read("V2003/testV2003.mdb");
  for (const password of [42, {}, true]) {
    assert.throws(() => Database.open(bytes, password), invalid, `${build}: password ${password}`);
  }
  assert.equal(Database.open(bytes, undefined).version(), "JET4", build);
  assert.equal(Database.open(bytes, null).version(), "JET4", build);
}

function checkPassword(Database, build) {
  const encrypted = read("db2007-enc.accdb");
  assert.throws(() => Database.open(encrypted), { name: "JetdbError", code: "PASSWORD_REQUIRED" }, build);
  assert.throws(() => Database.open(encrypted, "wrong"), { name: "JetdbError", code: "INVALID_PASSWORD" }, build);
  assert.deepEqual(Database.open(encrypted, "Test123").tables(), ["Table1"], build);
}

check(NodeDatabase, "node");

const packageDir = dirname(dirname(createRequire(import.meta.url).resolve("jetdb-wasm")));
const web = await import(pathToFileURL(join(packageDir, "web", "jetdb_wasm.js")).href);
web.initSync({ module: readFileSync(join(packageDir, "web", "jetdb_wasm_bg.wasm")) });
check(web.Database, "web");
