// デモのページ: サンプルを開き、右の中身・タブ・URL の #・狭い幅・言語の切り替えを確かめる
import { expect, test } from "@playwright/test";
import { needWasm, openDemo } from "./helpers.ts";

test.beforeEach(() => needWasm());

const detail = (page: import("@playwright/test").Page) =>
  page.locator("#demo-detail");

test("英語のデモ: サンプルを開き、概要と一覧を出す", async ({ page }) => {
  await openDemo(page, "/demo/");
  await expect(page.locator("#demo-current")).toHaveText(
    "Sample: jetdb-sample.accdb",
  );
  await expect(page.locator("#demo-version")).toHaveText("ACE12");
  await expect(page.locator("#demo-counts li")).toHaveText([
    "3 tables",
    "1 query",
    "1 form",
    "0 reports",
    "1 macro",
    "2 modules",
    "2 relationships",
  ]);
  await expect(page.locator(".viewer-group summary")).toHaveText([
    "Tables (3)",
    "Queries (1)",
    "Forms (1)",
    "Reports (0)",
    "Macros (1)",
    "Modules (2)",
    "Relationships (2)",
  ]);
  // # が無ければ、データベース全体を出す
  await expect(page).toHaveURL(/#database$/);
  await expect(detail(page).locator("h2")).toHaveText("Database");
  await expect(detail(page).locator("pre code")).toContainText(
    'CREATE TABLE "tblOrders"',
  );
});

test("日本語のデモ: ビューアの中も日本語", async ({ page }) => {
  await openDemo(page, "/ja/demo/");
  await expect(page.locator("#demo-current")).toHaveText(
    "サンプル: jetdb-sample.accdb",
  );
  await expect(page.locator("#demo-counts li").first()).toHaveText(
    "3 テーブル",
  );
  await page.getByRole("button", { name: "tblOrders", exact: true }).click();
  await expect(detail(page).getByRole("tab")).toHaveText([
    "データ",
    "CSV",
    "列",
    "索引",
    "プロパティ",
    "データマクロ",
    "DDL",
  ]);
  await expect(detail(page).locator(".viewer-pager span")).toHaveText(
    "1〜5 行目（全 5 行）",
  );
});

test("テーブル: タブを押しても、矢印のキーでも切り替わる", async ({ page }) => {
  await openDemo(page, "/demo/#table/tblOrders");
  await expect(detail(page).locator("h2")).toHaveText("tblOrders");
  await expect(detail(page).locator(".viewer-pager span")).toHaveText(
    "Rows 1–5 of 5",
  );
  await detail(page).getByRole("tab", { name: "Columns" }).click();
  // WebKit は押したボタンにフォーカスを移さないので、キーボードの人と同じくタブにフォーカスを移す
  await detail(page).getByRole("tab", { name: "Columns" }).focus();
  await expect(detail(page).locator("tbody tr").first()).toContainText(
    "AutoNumber",
  );
  await page.keyboard.press("ArrowRight");
  await expect(
    detail(page).getByRole("tab", { name: "Indexes" }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(detail(page).locator("tbody")).toContainText(
    "Primary key, Unique, Required",
  );
});

test("クエリ・フォーム・リレーションシップを右に出す", async ({ page }) => {
  await openDemo(page, "/demo/");
  await page
    .getByRole("button", { name: "qryMonthlySales", exact: true })
    .click();
  await expect(page).toHaveURL(/#query\/qryMonthlySales$/);
  await expect(detail(page).locator(".viewer-type")).toHaveText(
    "query · Select",
  );
  await expect(detail(page).locator("pre code")).toContainText("SELECT");

  await page.getByRole("button", { name: "frmOrders", exact: true }).click();
  await detail(page).getByRole("tab", { name: "Module" }).click();
  await detail(page).getByRole("link", { name: "Form_frmOrders" }).click();
  await expect(page).toHaveURL(/#module\/Form_frmOrders$/);
  await expect(detail(page).locator(".viewer-type")).toHaveText(
    "module · Document",
  );

  await page
    .getByRole("button", { name: "fkOrdersCustomers", exact: true })
    .click();
  await expect(detail(page).locator(".viewer-relation")).toHaveText(
    "tblOrders → tblCustomers",
  );
  await expect(detail(page).locator(".viewer-flags li")).toHaveText([
    "Referential integrity: yes",
    "Cascade update: no",
    "Cascade delete: no",
  ]);
});

test("フォーム: コントロールの一覧と、ストリームの保存", async ({ page }) => {
  await openDemo(page, "/demo/#form/frmOrders");
  await detail(page).getByRole("tab", { name: "Controls" }).click();
  const list = detail(page).locator("table").first();
  await expect(list.locator("th")).toHaveText(["Name", "Type", "Index"]);
  await expect(
    list.locator("tbody tr").first().locator("td").nth(2),
  ).toHaveText("0");
  await expect(detail(page).locator(".viewer-control").first()).toBeVisible();

  await detail(page).getByRole("tab", { name: "Streams" }).click();
  const blob = detail(page).locator("tbody tr").first();
  await expect(blob.locator("td").first()).toHaveText("blob");
  const download = page.waitForEvent("download");
  await blob.getByRole("button", { name: "Save" }).click();
  expect((await download).suggestedFilename()).toBe("frmOrders.blob.bin");
});

test("テーブルの CSV を出し、ファイルとして保存できる", async ({ page }) => {
  await openDemo(page, "/demo/#table/tblOrders");
  await detail(page).getByRole("tab", { name: "CSV" }).click();
  const csv = detail(page).locator("pre code");
  await expect(csv).toContainText(
    "OrderID,CustomerID,ProductID,Quantity,OrderDate,Shipped",
  );
  // 1 行目の列の名前と、5 行のデータ
  expect((await csv.textContent())?.trimEnd().split("\n")).toHaveLength(6);
  const download = page.waitForEvent("download");
  await detail(page).getByRole("button", { name: "Save" }).click();
  expect((await download).suggestedFilename()).toBe("tblOrders.csv");
});

test("DDL の方言を選び直せる", async ({ page }) => {
  await openDemo(page, "/demo/#database");
  await detail(page).getByLabel("SQL dialect").selectOption("postgres");
  await expect(detail(page).locator("pre code")).toContainText(
    "GENERATED ALWAYS AS IDENTITY",
  );
});

test("# で開き、読み込み直しても同じものを出す。無い名前は Database に戻す", async ({
  page,
}) => {
  await openDemo(page, "/demo/#macro/mcrOpenOrders");
  await expect(detail(page).locator("h2")).toHaveText("mcrOpenOrders");
  await expect(
    page.getByRole("button", { name: "mcrOpenOrders", exact: true }),
  ).toHaveAttribute("aria-current", "true");
  await page.reload();
  await expect(detail(page).locator("h2")).toHaveText("mcrOpenOrders");

  await openDemo(page, "/demo/#table/NoSuchTable");
  await expect(page).toHaveURL(/#database$/);
  await expect(detail(page).locator("h2")).toHaveText("Database");
});

test("システムのオブジェクトも出す", async ({ page }) => {
  await openDemo(page, "/demo/");
  await page.getByLabel("Show system objects").check();
  await expect(
    page.getByRole("button", { name: "MSysObjects", exact: true }),
  ).toBeVisible();
  await page.getByLabel("Show system objects").uncheck();
  await expect(
    page.getByRole("button", { name: "MSysObjects", exact: true }),
  ).toHaveCount(0);
});

test("狭い幅では選択の欄で選び、ページは横にはみ出さない", async ({ page }) => {
  await page.setViewportSize({ width: 500, height: 900 });
  await openDemo(page, "/demo/");
  const select = page.locator(".viewer-nav-select");
  await expect(select).toBeVisible();
  await expect(page.locator(".viewer-group").first()).toBeHidden();
  await select.selectOption("#form/frmOrders");
  await expect(detail(page).locator("h2")).toHaveText("frmOrders");
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(500);
});

test("日英のデモの間を、言語の切り替えで行き来する", async ({ page }) => {
  await openDemo(page, "/demo/");
  await page.getByRole("link", { name: "日本語" }).click();
  await expect(page).toHaveURL(/\/ja\/demo\//);
  await expect(page.locator("h1")).toHaveText("ブラウザで試せるデモ");
  await page.getByRole("link", { name: "English" }).click();
  await expect(page).toHaveURL(/\/demo\/$|\/demo\/#/);
  await expect(page.locator("h1")).toHaveText("Demo in the browser");
});
