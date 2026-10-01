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

import { Database as NodeDatabase } from "jetdb-wasm";

const testdata = process.argv[2];
const read = (file) => readFileSync(join(testdata, file));

function check(Database, build) {
  checkTables(Database, build);
  checkColumns(Database, build);
  checkIndexes(Database, build);
  checkRows(Database, build);
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
  assert.throws(() => db.tables({ system: "yes" }), /option system must be a boolean/, build);
  assert.throws(() => db.tables(5), /cannot read option system/, build);
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
  assert.throws(() => db.columns("NoSuchTable"), /table not found: NoSuchTable/, build);
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
  assert.throws(() => db.indexes("NoSuchTable"), /table not found: NoSuchTable/, build);
}

function checkRows(Database, build) {
  const db = openTest(Database);
  assert.deepEqual(
    db.rows("Table1"),
    [
      { A: "abcdefg", B: "hijklmnop", C: 2, D: 222, E: 333333333, F: 444.555, G: "1974-09-21", H: "3.5000", I: true },
      { A: "a", B: "b", C: 0, D: 0, E: 0, F: 0, G: "1981-12-12", H: "0.0000", I: false },
    ],
    build,
  );
  assert.throws(() => db.rows("NoSuchTable"), /table not found: NoSuchTable/, build);

  const binary = Database.open(read("V2010/binIdxTestV2010.accdb")).rows("Test");
  assert.deepEqual(binary.find((row) => row.ID === 1).BinAsc, new Uint8Array([0x61, 0x62]), build);
  assert.equal(binary.find((row) => row.ID === 200).BinAsc, null, build);

  // 9007199254740993 is 2^53 + 1, which a number cannot hold exactly.
  assert.deepEqual(
    Database.open(read("V2016/bigIntTestV2016.accdb")).rows("BigIntTable"),
    [
      { ID: 1, Big: 9007199254740993n },
      { ID: 2, Big: -9007199254740993n },
      { ID: 3, Big: 0n },
      { ID: 4, Big: null },
    ],
    build,
  );
}

function checkPassword(Database, build) {
  const encrypted = read("db2007-enc.accdb");
  assert.throws(() => Database.open(encrypted), /password-protected/, build);
  assert.throws(() => Database.open(encrypted, "wrong"), /invalid password/, build);
  assert.deepEqual(Database.open(encrypted, "Test123").tables(), ["Table1"], build);
}

check(NodeDatabase, "node");

const packageDir = dirname(dirname(createRequire(import.meta.url).resolve("jetdb-wasm")));
const web = await import(pathToFileURL(join(packageDir, "web", "jetdb_wasm.js")).href);
web.initSync({ module: readFileSync(join(packageDir, "web", "jetdb_wasm_bg.wasm")) });
check(web.Database, "web");
