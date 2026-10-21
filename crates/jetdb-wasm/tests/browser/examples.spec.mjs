// Checks the jetdb-wasm web build in real browsers through the developer page,
// crates/jetdb-wasm/examples/index.html: choose a file, open it, and read the
// version, the tables or the error shown on the page.

import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";

const testdata = (file) =>
  fileURLToPath(new URL(`../../../../testdata/${file}`, import.meta.url));

// Opens `file` on the page, with `password` and the system-table option.
async function open(page, file, { password = "", system = false } = {}) {
  await page.locator("#file").setInputFiles(testdata(file));
  await page.locator("#password").fill(password);
  await page.locator("#system").setChecked(system);
  await page.locator("#open").click();
}

async function tableNames(page) {
  await expect(page.locator("#result")).toBeVisible();
  return page.locator("#tables li").allTextContents();
}

test.beforeEach(async ({ page }) => {
  await page.goto("./");
  // The button is enabled once the Wasm is initialized.
  await expect(page.locator("#open")).toBeEnabled();
});

test("shows the version and the user tables", async ({ page }) => {
  await open(page, "V2003/testV2003.mdb");
  await expect(page.locator("#version")).toHaveText("JET4");
  const names = await tableNames(page);
  expect(names).toContain("Table1");
  expect(names.filter((name) => name.startsWith("MSys"))).toEqual([]);
});

test("includes the system tables when asked", async ({ page }) => {
  await open(page, "V2003/testV2003.mdb", { system: true });
  expect(await tableNames(page)).toContain("MSysObjects");
});

test("opens a password-protected database", async ({ page }) => {
  await open(page, "db2007-enc.accdb", { password: "Test123" });
  expect(await tableNames(page)).toEqual(["Table1"]);
});

test("shows an error for a wrong or missing password", async ({ page }) => {
  await open(page, "db2007-enc.accdb", { password: "wrong" });
  await expect(page.locator("#error")).toHaveText(/invalid password/);
  await expect(page.locator("#result")).toBeHidden();

  await open(page, "db2007-enc.accdb");
  await expect(page.locator("#error")).toHaveText(/password-protected/);
  await expect(page.locator("#result")).toBeHidden();
});

test("asks for a file when none is chosen", async ({ page }) => {
  await page.locator("#open").click();
  await expect(page.locator("#error")).toHaveText(
    "Choose a database file first.",
  );
});

// Runs `method` of the database open on the page with `argument` and
// `options` (JSON), where the method takes them, and returns what the page
// shows.
async function call(page, method, argument = "", options = "") {
  await page.locator("#method").selectOption(method);
  for (const [id, value] of [
    ["#argument", argument],
    ["#options", options],
  ]) {
    const input = page.locator(id);
    if (await input.isEnabled()) await input.fill(value);
  }
  await page.locator("#run").click();
  await expect(page.locator("#error")).toHaveText("");
  return page.locator("#output").textContent();
}

const callJson = async (...args) => JSON.parse(await call(...args));

test("lists every method of Database to call", async ({ page }) => {
  const methods = await page.evaluate(async () => {
    const { Database } = await import("../pkg/web/jetdb_wasm.js");
    // Leaving out free, and the methods wasm-bindgen makes for itself.
    return Object.getOwnPropertyNames(Database.prototype).filter(
      (name) => !["constructor", "free"].includes(name) && !name.startsWith("__"),
    );
  });
  const listed = await page.locator("#method option").allTextContents();
  expect(listed.toSorted()).toEqual(methods.toSorted());
});

test("calls the methods of tables", async ({ page }) => {
  await open(page, "V2003/testV2003.mdb");
  expect(await call(page, "version")).toBe("JET4");
  expect(await callJson(page, "tables", "", '{"system": true}')).toContain(
    "MSysObjects",
  );
  const columns = await callJson(page, "columns", "Table1");
  expect(columns.map((c) => c.name)).toContain("A");
  expect(Array.isArray(await callJson(page, "indexes", "Table1"))).toBe(true);
  const rows = await callJson(page, "rows", "Table1", '{"limit": 1}');
  expect(rows.columns).toEqual(columns.map((c) => c.name));
  expect(rows.rows).toHaveLength(1);
  expect((await call(page, "csv", "Table1")).split("\n").length).toBeGreaterThan(
    1,
  );
});

test("calls the methods of relationships, DDL and properties", async ({
  page,
}) => {
  await open(page, "V2003/indexTestV2003.mdb");
  expect((await callJson(page, "relationships")).length).toBeGreaterThan(0);
  expect(await call(page, "ddl", "postgres", '{"table": "Table2"}')).toMatch(
    /^CREATE TABLE "Table2" \(/,
  );
  await open(page, "V2010/calcFieldTestV2010.accdb");
  const props = await callJson(page, "properties", "Table1");
  expect(props.columns.length).toBeGreaterThan(0);
});

test("calls the methods of queries and modules", async ({ page }) => {
  await open(page, "V2003/queryTestV2003.mdb");
  expect((await callJson(page, "queries")).map((q) => q.name)).toContain(
    "DeleteQuery",
  );
  expect(await call(page, "querySql", "DeleteQuery")).toMatch(/^DELETE/);
  await open(page, "vbaV2007.accdb");
  expect((await callJson(page, "modules")).map((m) => m.name)).toContain(
    "Module1",
  );
  expect(await call(page, "moduleSource", "Module1")).toContain(
    'Attribute VB_Name = "Module1"',
  );
});

test("calls the methods of forms", async ({ page }) => {
  await open(page, "formPropTest.accdb");
  const form = "jp_フォーム_2";
  expect((await callJson(page, "forms")).map((f) => f.name)).toContain(form);
  const controls = await callJson(page, "formControls", form);
  expect(Object.keys(controls[0])).toEqual(["name", "type", "typeCode", "index"]);
  // The bytes of a stream are shown by their number and the first in hex.
  expect(await call(page, "formStream", form, '{"stream": "blob"}')).toMatch(
    /^"Uint8Array\(\d+\) 15 00 14 00 00 00 00 00 /,
  );
  const design = await callJson(page, "formProperties", form);
  expect(design.controls).toHaveLength(controls.length);
});

test("calls the methods of macros", async ({ page }) => {
  await open(page, "V2010/macroTestV2010.accdb");
  expect(await callJson(page, "macros")).toContain("mcrSimple");
  expect((await callJson(page, "macro", "mcrSimple")).source.name).toBe(
    "mcrSimple",
  );
  expect((await callJson(page, "embeddedMacros", "tblItems")).length).toBeGreaterThan(0);
  expect((await callJson(page, "dataMacros", "tblItems")).length).toBeGreaterThan(0);
  expect((await call(page, "dataMacrosText", "tblItems")).length).toBeGreaterThan(0);
});

test("shows the error of a call", async ({ page }) => {
  await open(page, "V2003/testV2003.mdb");
  await page.locator("#method").selectOption("columns");
  await page.locator("#argument").fill("NoSuchTable");
  await page.locator("#run").click();
  await expect(page.locator("#error")).toHaveText(/NoSuchTable/);
  await expect(page.locator("#output")).toHaveText("");

  await page.locator("#method").selectOption("tables");
  await page.locator("#options").fill("{system");
  await page.locator("#run").click();
  await expect(page.locator("#error")).not.toHaveText("");
});
