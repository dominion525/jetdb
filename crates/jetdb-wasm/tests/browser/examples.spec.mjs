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
