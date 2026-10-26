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

import init, { Database as NodeDatabase, initSync } from "jetdb-wasm";

const testdata = process.argv[2];
const read = (file) => readFileSync(join(testdata, file));

function check(Database, build) {
  checkOpen(Database, build);
  checkTables(Database, build);
  checkColumns(Database, build);
  checkIndexes(Database, build);
  checkRows(Database, build);
  checkRowsInParts(Database, build);
  checkCsv(Database, build);
  checkComplexRows(Database, build);
  checkBinaryAndBigIntRows(Database, build);
  checkSkippedRows(Database, build);
  checkRelationships(Database, build);
  checkDdl(Database, build);
  checkQueries(Database, build);
  checkModules(Database, build);
  checkForms(Database, build);
  checkFormControls(Database, build);
  checkFormStreams(Database, build);
  checkFormsOfTheSameName(Database, build);
  checkMacros(Database, build);
  checkProperties(Database, build);
  checkPropertiesOfAType(Database, build);
  checkErrors(Database, build);
  checkFree(Database, build);
  checkStringArguments(Database, build);
  checkPassword(Database, build);
  console.log(`ok: ${build}`);
}

const openTest = (Database) => Database.open(read("V2003/testV2003.mdb"));
const propertyValue = (properties, name) => properties.find((p) => p.name === name).value;

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
      ["A", "Text", 50],
      ["B", "Text", 100],
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
    { name: "A", type: "Text", size: 50, precision: 0, scale: 0, autoNumber: false, calculated: false },
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
      next: null,
    },
    build,
  );
  assert.throws(() => db.rows("NoSuchTable"), { name: "JetdbError", code: "TABLE_NOT_FOUND" }, build);

  // A table without rows still gives the names of its columns.
  const empty = db.rows("Table2");
  assert.deepEqual(empty.rows, [], build);
  assert.deepEqual(empty.columns, db.columns("Table2").map((c) => c.name), build);
  assert.ok(empty.columns.length > 0, build);
}

function checkRowsInParts(Database, build) {
  const db = openTest(Database);
  // Read a part at a time, the parts join to all the rows, and next is null
  // after the last part.
  const whole = Database.open(read("V2007/complexDataTestV2007.accdb")).rows("Table1");
  for (const limit of [1, 3]) {
    const complexDb = Database.open(read("V2007/complexDataTestV2007.accdb"));
    let part = complexDb.rows("Table1", { limit });
    const joined = [...part.rows];
    while (part.next !== null) {
      assert.ok(part.rows.length === limit, build);
      part = complexDb.rows("Table1", { start: part.next, limit });
      joined.push(...part.rows);
    }
    assert.deepEqual(joined, whole.rows, build);
  }
  for (const options of [{ limit: 0 }, { limit: 1.5 }, { limit: "1" }, { start: -1 }, { start: 0.5 }, { start: "0" }]) {
    assert.throws(() => db.rows("Table1", options), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  }
}

function checkCsv(Database, build) {
  const db = openTest(Database);
  // The CSV of a table, as jetdb export writes it.
  assert.equal(
    db.csv("Table1"),
    'A,B,C,D,E,F,G,H,I\n"abcdefg","hijklmnop",2,222,333333333,444.555,1974-09-21,3.5000,1\n"a","b",0,0,0,0,1981-12-12,0.0000,0\n',
    build,
  );
  assert.equal(
    db.csv("Table1", { header: false, delimiter: "\t", dateFormat: "%d/%m/%Y", booleanWords: true }),
    '"abcdefg"\t"hijklmnop"\t2\t222\t333333333\t444.555\t21/09/1974\t3.5000\tTRUE\n"a"\t"b"\t0\t0\t0\t0\t12/12/1981\t0.0000\tFALSE\n',
    build,
  );
  const complexCsv = Database.open(read("V2007/complexDataTestV2007.accdb")).csv("Table1");
  assert.ok(complexCsv.includes(',"value1;value4","test_data.txt;test_data2.txt"'), build);
  for (const options of [{ delimiter: "" }, { delimiter: ",;" }, { bin: "HEX" }, { header: "no" }, { null: 0 }]) {
    assert.throws(() => db.csv("Table1", options), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  }
  assert.throws(() => db.csv("NoSuchTable"), { name: "JetdbError", code: "TABLE_NOT_FOUND" }, build);
}

function checkComplexRows(Database, build) {
  // A complex column gives an array of its values in the row.
  const complex = Database.open(read("V2007/complexDataTestV2007.accdb")).rows("Table1").rows;
  const complexRow = (id) => complex.find((row) => row.id === id);
  assert.deepEqual(complexRow("row1")["multi-value-data"], [], build);
  assert.deepEqual(complexRow("row2")["multi-value-data"], ["value1", "value4"], build);
  assert.deepEqual(
    complexRow("row4")["attach-data"],
    [
      {
        name: "test_data2.txt",
        type: "txt",
        data: new TextEncoder().encode("this is some more test data for attachment."),
        url: null,
        timestamp: null,
        flags: null,
      },
    ],
    build,
  );
  assert.deepEqual(
    complexRow("row3")["VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880"],
    [
      { value: "row3-memo", modified: "2011-09-12 21:21:19" },
      { value: "row3-memo-revised", modified: "2011-09-12 21:22:33" },
      { value: "row3-memo-again", modified: "2011-09-12 21:22:45" },
    ],
    build,
  );
}

function checkBinaryAndBigIntRows(Database, build) {
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

function checkModules(Database, build) {
  const db = Database.open(read("vbaV2007.accdb"));
  assert.deepEqual(
    db.modules(),
    [
      { name: "Class1", type: "Class" },
      { name: "Form_Form1", type: "Document" },
      { name: "Module1", type: "Standard" },
    ],
    build,
  );
  const source = db.moduleSource("Module1");
  assert.ok(source.startsWith('Attribute VB_Name = "Module1"'), `${build}: ${source}`);
  assert.ok(source.includes('Hello = "Hello, World!"'), `${build}: ${source}`);
  assert.throws(() => db.moduleSource("NoSuchModule"), { name: "JetdbError", code: "MODULE_NOT_FOUND" }, build);
  assert.deepEqual(openTest(Database).modules(), [], build);

  // Japanese names and source.
  const japanese = Database.open(read("vbaJapaneseV2007.accdb"));
  assert.ok(japanese.modules().some((m) => m.name === "顧客クラス" && m.type === "Class"), build);
  assert.ok(japanese.moduleSource("日本語モジュール").includes('あいさつ = "こんにちは、" & 名前 & "さん"'), build);
}

function checkForms(Database, build) {
  const db = Database.open(read("formPropTest.accdb"));
  assert.deepEqual(
    db.forms(),
    [
      { name: "F_Buttons", type: "Form" },
      { name: "F_Table0", type: "Form" },
      { name: "F_Table1", type: "Form" },
      { name: "jp_フォーム_2", type: "Form" },
    ],
    build,
  );
  const design = db.formProperties("jp_フォーム_2");
  assert.equal(design.name, "jp_フォーム_2", build);
  assert.equal(design.type, "Form", build);
  assert.equal(propertyValue(design.properties, "RecordSource"), "jp_クエリ_02", build);
  assert.equal(design.properties.find((p) => p.name === "RecordSource").id, 0x9c, build);
  // A property jetdb does not name is given by its number.
  assert.equal(design.properties.find((p) => p.name === "0x0250").id, 0x250, build);
  assert.equal(propertyValue(design.properties, "DividingLines"), "NotDefault", build);
  assert.equal(propertyValue(design.properties, "DefaultView"), 5, build);
  assert.equal(propertyValue(design.properties, "DatasheetGridlinesColor"), 0xe8e8e8, build);
  assert.ok(propertyValue(design.properties, "NameMap") instanceof Uint8Array, build);
  assert.equal(design.controls.length, 21, build);
  assert.deepEqual(
    { name: design.controls[0].name, type: design.controls[0].type, typeCode: design.controls[0].typeCode },
    { name: "フォームヘッダー", type: "FormHeader", typeCode: 0x1899 },
    build,
  );
}

function checkFormControls(Database, build) {
  const db = Database.open(read("formPropTest.accdb"));
  const design = db.formProperties("jp_フォーム_2");
  // The controls of a form, as jetdb form controls lists them: the names and
  // types of the controls of formProperties, each with its own index.
  const controls = db.formControls("jp_フォーム_2");
  assert.equal(controls.length, design.controls.length, build);
  assert.deepEqual(Object.keys(controls[0]), ["name", "type", "typeCode", "index"], build);
  assert.deepEqual(
    controls.map((c) => `${c.name} ${c.type} ${c.typeCode}`).sort(),
    design.controls.map((c) => `${c.name} ${c.type} ${c.typeCode}`).sort(),
    build,
  );
  const indexes = controls.map((c) => c.index);
  assert.ok(indexes.every((i) => Number.isInteger(i) && i >= 0), build);
  assert.equal(new Set(indexes).size, indexes.length, build);
  assert.throws(() => db.formControls("NoSuchForm"), { name: "JetdbError", code: "FORM_NOT_FOUND" }, build);
}

function checkFormStreams(Database, build) {
  const db = Database.open(read("formPropTest.accdb"));
  // The streams of a form, as jetdb form dump writes them; the blob starts
  // with the header every form design has.
  const blob = db.formStream("jp_フォーム_2");
  assert.ok(blob instanceof Uint8Array, build);
  assert.deepEqual([...blob.slice(0, 8)], [0x15, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00], build);
  assert.deepEqual(db.formStream("jp_フォーム_2", { stream: "blob" }), blob, build);
  assert.ok(db.formStream("jp_フォーム_2", { stream: "typeinfo" }).length > 0, build);
  assert.throws(() => db.formStream("jp_フォーム_2", { stream: "Blob" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.throws(() => db.formStream("NoSuchForm"), { name: "JetdbError", code: "FORM_NOT_FOUND" }, build);
  assert.throws(() => db.formProperties("NoSuchForm"), { name: "JetdbError", code: "FORM_NOT_FOUND" }, build);
  assert.deepEqual(openTest(Database).forms(), [], build);
}

function checkFormsOfTheSameName(Database, build) {
  // A form and a report both named Shared: the type option chooses one.
  const shared = Database.open(read("V2007/sameNameTestV2007.accdb"));
  assert.deepEqual(shared.forms(), [{ name: "Shared", type: "Form" }, { name: "Shared", type: "Report" }], build);
  assert.throws(() => shared.formProperties("Shared"), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.equal(shared.formProperties("Shared", { type: "Report" }).type, "Report", build);
  assert.throws(() => shared.formStream("Shared"), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  assert.throws(() => shared.formControls("Shared"), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  // Made by VBA, the report has an empty TypeInfo; its controls come from
  // the Blob.
  assert.deepEqual(
    shared.formControls("Shared", { type: "Report" }).map((c) => `${c.name} ${c.type} ${c.index}`),
    ["ページヘッダーセクション PageHeader 0", "詳細 Detail 1", "lblReportOnly Label 2", "ページフッターセクション PageFooter 3"],
    build,
  );
  assert.ok(shared.formStream("Shared", { type: "Report" }).length > 0, build);
  assert.equal(shared.embeddedMacros("Shared", { type: "Report" })[0].source.event, "OnOpen", build);
  assert.throws(() => shared.embeddedMacros("Shared", { type: "Page" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);

  const sports = Database.open(read("saveastext/SportsAdmin/Sports.accdb"));
  const report = sports.formProperties("Field (High Jump)");
  assert.equal(report.type, "Report", build);
  assert.equal(report.controls.length, 78, build);
}

function checkMacros(Database, build) {
  const db = Database.open(read("V2010/macroTestV2010.accdb"));
  assert.deepEqual(db.macros(), ["AutoExec", "mcrConditions", "mcrLongText", "mcrSimple"], build);
  assert.equal(db.macros({ system: true }).at(-1), "~TMPCLPMacro", build);
  assert.throws(() => db.macros({ system: "yes" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
  const simple = db.macro("mcrSimple");
  assert.deepEqual(simple.source, { kind: "named", name: "mcrSimple" }, build);
  assert.deepEqual(
    simple.statements[1],
    {
      kind: "action",
      name: "MessageBox",
      arguments: [
        { name: "Message", value: "Hello" },
        { name: "Beep", value: "No" },
        { name: "Type", value: "Information" },
        { name: "Title", value: "Title" },
      ],
    },
    build,
  );
  assert.ok(simple.xml.includes("<UserInterfaceMacro"), build);
  assert.ok(simple.text.startsWith("Version =196611"), `${build}: ${simple.text}`);
  assert.throws(() => db.macro("NoSuchMacro"), { name: "JetdbError", code: "MACRO_NOT_FOUND" }, build);

  const embedded = db.embeddedMacros("tblItems");
  assert.ok(embedded.length > 0, build);
  assert.equal(embedded[0].source.kind, "embedded", build);
  assert.equal(embedded[0].source.objectName, "tblItems", build);
  assert.equal(typeof embedded[0].text, "string", build);
  assert.throws(() => db.embeddedMacros("NoSuchForm"), { name: "JetdbError", code: "FORM_NOT_FOUND" }, build);

  const data = db.dataMacros("tblItems");
  assert.ok(data.length > 0, build);
  assert.equal(data[0].source.kind, "data", build);
  assert.equal(data[0].source.table, "tblItems", build);
  assert.equal(data[0].text, null, build);
  assert.ok(db.dataMacrosText("tblItems").length > 0, build);
  assert.throws(() => db.dataMacros("NoSuchTable"), { name: "JetdbError", code: "TABLE_NOT_FOUND" }, build);
  assert.deepEqual(openTest(Database).macros(), [], build);
}

function checkProperties(Database, build) {
  const db = Database.open(read("V2010/calcFieldTestV2010.accdb"));
  const props = db.properties("Table1");
  assert.match(propertyValue(props.object, "GUID"), /^\{[0-9A-F-]+\}$/, build);
  assert.equal(propertyValue(props.object, "TotalsRow"), false, build);
  assert.ok(propertyValue(props.object, "NameMap") instanceof Uint8Array, build);
  const firstName = props.columns.find((c) => c.name === "FirstName").properties;
  assert.equal(propertyValue(firstName, "AllowZeroLength"), true, build);
  assert.equal(propertyValue(firstName, "ColumnWidth"), 1380, build);
  assert.deepEqual(props.additional, [], build);

  assert.throws(() => db.properties("NoSuchObject"), { name: "JetdbError", code: "OBJECT_NOT_FOUND" }, build);
  assert.throws(() => db.properties(42), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
}

function checkPropertiesOfAType(Database, build) {
  // nwind.mdb has a form, a macro and a table all named Customers.
  const nwind = Database.open(read("V1997/nwind.mdb"));
  assert.throws(
    () => nwind.properties("Customers"),
    (e) => e.name === "JetdbError" && e.code === "INVALID_ARGUMENT" && e.message.includes("Form, Macro, Table"),
    build,
  );
  assert.ok(nwind.properties("Customers", { type: "Table" }).columns.some((c) => c.name === "CustomerID"), build);
  assert.match(propertyValue(nwind.properties("Customers", { type: "Form" }).object, "Description"), /Single-column form/, build);
  assert.throws(() => nwind.properties("Customers", { type: "Query" }), { name: "JetdbError", code: "OBJECT_NOT_FOUND" }, build);
  assert.throws(() => nwind.properties("Customers", { type: "View" }), { name: "JetdbError", code: "INVALID_ARGUMENT" }, build);
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

function checkFree(Database, build) {
  // free can be called again, and together with using, which calls it at the
  // end of its block; a method called after it is a DATABASE_FREED error
  // rather than an error of the glue wasm-bindgen generates.
  const freed = { name: "JetdbError", code: "DATABASE_FREED" };
  const db = openTest(Database);
  db.free();
  db.free();
  db[Symbol.dispose]();
  assert.throws(() => db.version(), freed, build);
  assert.throws(() => db.tables(), freed, build);
  assert.throws(() => db.rows("Table1"), freed, build);
  let kept;
  {
    using db = openTest(Database);
    db.free();
    kept = db;
  }
  assert.throws(() => kept.columns("Table1"), freed, build);
}

function checkStringArguments(Database, build) {
  // A value that is not a string, which TypeScript would not allow, is an
  // INVALID_ARGUMENT error rather than an error of the Wasm, and the database
  // can still be read after it.
  const db = openTest(Database);
  const invalid = { name: "JetdbError", code: "INVALID_ARGUMENT" };
  for (const method of ["columns", "indexes", "rows", "csv", "querySql", "moduleSource", "formProperties", "formStream", "formControls", "macro", "embeddedMacros", "dataMacros", "dataMacrosText"]) {
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

// The Node.js build is ready when it is imported, and its init and initSync,
// which initialize the web build, do nothing. A require gives the same build.
assert.equal(await init(), undefined);
assert.equal(initSync(), undefined);
const required = createRequire(import.meta.url)("jetdb-wasm");
assert.equal(required.Database, NodeDatabase);
assert.equal(await required.default(), undefined);
assert.equal(required.initSync(), undefined);
check(NodeDatabase, "node");

// The package exports its package.json and the Wasm files of its builds, for
// tools that read its version and for setups that pass the Wasm to initSync.
const require = createRequire(import.meta.url);
assert.equal(require("jetdb-wasm/package.json").name, "jetdb-wasm");
const packageDir = dirname(require.resolve("jetdb-wasm/package.json"));
const webWasm = require.resolve("jetdb-wasm/web/jetdb_wasm_bg.wasm");
assert.equal(webWasm, join(packageDir, "web", "jetdb_wasm_bg.wasm"));
assert.equal(require.resolve("jetdb-wasm/node/jetdb_wasm_bg.wasm"), join(packageDir, "node", "jetdb_wasm_bg.wasm"));
const web = await import(pathToFileURL(join(packageDir, "web", "jetdb_wasm.js")).href);
web.initSync({ module: readFileSync(webWasm) });
check(web.Database, "web");
