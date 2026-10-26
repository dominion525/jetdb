// デモのページ: サンプルを開き、右の中身・タブ・URL の #・狭い幅・言語の切り替えを確かめる
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { needWasm, openDemo, openFile, testdata } from "./helpers.ts";

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

test("サンプルを取得している間に選んだファイルを開き、あとから届いたサンプルで上書きしない", async ({
  page,
}) => {
  const file = testdata("V2007/complexDataTestV2007.accdb");
  // サンプルの応答を、自分のファイルが開くまで止めておく
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/assets/jetdb-sample.accdb", async (route) => {
    await held;
    await route.continue();
  });
  await page.goto("/demo/");
  await expect(page.locator("#demo-file")).toBeEnabled();
  await expect(page.locator("#demo-status")).toHaveText("Opening the sample…");
  await openFile(page, file);
  await expect(page.locator("#demo-current")).toHaveText(
    "complexDataTestV2007.accdb",
  );
  const sample = page.waitForResponse("**/assets/jetdb-sample.accdb");
  release();
  await sample;
  await expect(page.locator("#demo-current")).toHaveText(
    "complexDataTestV2007.accdb",
  );
  await expect(page.locator("#demo-status")).toHaveText("");
});

test("サンプルだけ読めなかったら、そう書いて、自分のファイルは開ける", async ({
  page,
}) => {
  const file = testdata("V2007/complexDataTestV2007.accdb");
  await page.route("**/assets/jetdb-sample.accdb", (route) =>
    route.fulfill({ status: 404 }),
  );
  await page.goto("/ja/demo/");
  await expect(page.locator("#demo-status")).toHaveText(
    "サンプルを読み込めませんでした（HTTP 404）。自分のファイルは開けます。",
  );
  await expect(page.locator("#demo-file")).toBeEnabled();
  await expect(page.locator("#demo-result")).toBeHidden();
  await openFile(page, file);
  await expect(page.locator("#demo-current")).toHaveText(
    "complexDataTestV2007.accdb",
  );
  await expect(page.locator("#demo-result")).toBeVisible();
});

test("前のファイルにしか無い # に戻っても、履歴が足されずに抜け出せる", async ({
  page,
}) => {
  const file = testdata("V2007/complexDataTestV2007.accdb");
  // 紹介ページ → デモ → tblOrders → tblCustomers の順に履歴を作る
  await page.goto("/");
  await openDemo(page, "/demo/");
  await page.getByRole("button", { name: "tblOrders", exact: true }).click();
  await expect(detail(page).locator("h2")).toHaveText("tblOrders");
  await page.getByRole("button", { name: "tblCustomers", exact: true }).click();
  await expect(detail(page).locator("h2")).toHaveText("tblCustomers");
  // tblOrders の無いファイルを開く
  await openFile(page, file);
  await expect(page.locator("#demo-current")).toHaveText(
    "complexDataTestV2007.accdb",
  );
  await expect(page).toHaveURL(/#database$/);
  // 戻ると tblOrders の # になるが、このファイルに無いので #database に置き換わる（履歴は足さない）
  await page.goBack();
  await expect(page).toHaveURL(/#database$/);
  await expect(detail(page).locator("h2")).toHaveText("Database");
  // さらに 2 回戻ると、紹介ページまで戻れる
  await page.goBack();
  await expect(page).toHaveURL(/\/demo\/#database$/);
  await page.goBack();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.locator("h1")).not.toHaveText("Demo in the browser");
});

test("320px と 390px: ファイルを選ぶ欄がはみ出さず、ページャの前へ・次へは同じ行にある", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 800 });
  await openDemo(page, "/ja/demo/#table/tblOrders");
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(320);
  const input = page.locator("#demo-file");
  const card = page.locator(".demo-source");
  const inputBox = await input.boundingBox();
  const cardBox = await card.boundingBox();
  expect(
    inputBox && cardBox && inputBox.x + inputBox.width,
  ).toBeLessThanOrEqual((cardBox?.x ?? 0) + (cardBox?.width ?? 0) + 0.5);

  await page.setViewportSize({ width: 390, height: 800 });
  const buttons = detail(page).locator(".viewer-pager button");
  await expect(buttons).toHaveCount(2);
  const [previous, next] = await Promise.all([
    buttons.nth(0).boundingBox(),
    buttons.nth(1).boundingBox(),
  ]);
  expect(previous?.y).toBe(next?.y);
});

test("続けて 2 つ選んだら、読み終わる順にかかわらず、最後に選んだ方が開いたままになる", async ({
  page,
}) => {
  const first = testdata("V2010/macroGeneratedTestV2010.accdb");
  const second = testdata("V2007/complexDataTestV2007.accdb");
  // 最初に選んだファイルの読み取りだけを遅らせ、2 つ目の方を先に読み終わらせる
  await page.addInitScript(() => {
    const original = File.prototype.arrayBuffer;
    let delayed = false;
    File.prototype.arrayBuffer = async function () {
      const buffer = await original.call(this);
      if (!delayed) {
        delayed = true;
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
      return buffer;
    };
  });
  await openDemo(page, "/demo/");
  await openFile(page, first);
  await openFile(page, second);
  await expect(page.locator("#demo-current")).toHaveText(
    "complexDataTestV2007.accdb",
  );
  // 最初のファイルの読み取りが終わっても、2 つ目のまま
  await page.waitForTimeout(1500);
  await expect(page.locator("#demo-current")).toHaveText(
    "complexDataTestV2007.accdb",
  );
  await expect(page.locator("#demo-status")).toHaveText("");
});

test("選んだら読み上げには見出しだけを伝え、右の中身は丸ごと読み上げの対象にしない", async ({
  page,
}) => {
  await openDemo(page, "/demo/");
  const announce = page.locator("#demo-announce");
  await expect(announce).toHaveText("Database");
  await page.getByRole("button", { name: "tblOrders", exact: true }).click();
  await expect(detail(page).locator("h2")).toHaveText("tblOrders");
  await expect(announce).toHaveText("tblOrders");
  expect(await detail(page).getAttribute("aria-live")).toBeNull();
  // タブと中身が id で結び付き、Tab キーで止まるのは選んでいるタブだけ
  const tabs = detail(page).getByRole("tab");
  const panel = detail(page).getByRole("tabpanel");
  await expect(tabs.first()).toHaveAttribute(
    "aria-controls",
    (await panel.getAttribute("id")) ?? "",
  );
  await expect(panel).toHaveAttribute(
    "aria-labelledby",
    (await tabs.first().getAttribute("id")) ?? "",
  );
  await expect(tabs.first()).toHaveAttribute("tabindex", "0");
  await expect(tabs.nth(1)).toHaveAttribute("tabindex", "-1");
  await tabs.first().focus();
  await page.keyboard.press("End");
  await expect(tabs.last()).toHaveAttribute("aria-selected", "true");
  await expect(tabs.last()).toBeFocused();
  await page.keyboard.press("Home");
  await expect(tabs.first()).toHaveAttribute("aria-selected", "true");
});

test("375px: ビューアのタブの代わりに選択の欄を出し、選ぶと中身が切り替わる", async ({
  page,
}) => {
  await page.setViewportSize({ width: 375, height: 800 });
  await openDemo(page, "/ja/demo/#table/tblOrders");
  const picker = detail(page).locator(".viewer-tabs-select");
  await expect(picker).toBeVisible();
  await expect(detail(page).getByRole("tablist")).toBeHidden();
  await expect(picker.locator("option")).toHaveText([
    "データ",
    "CSV",
    "列",
    "索引",
    "プロパティ",
    "データマクロ",
    "DDL",
  ]);
  await picker.selectOption({ label: "列" });
  await expect(detail(page).locator("tbody tr").first()).toContainText(
    "オートナンバー",
  );
  // 隠れているタブも、選んだものと同じ方に印が付く
  await expect(detail(page).locator('[role="tab"]').nth(2)).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

// デモの HTML の import map は、devDependency と同じ版の jetdb-wasm を jsDelivr から読む。
// 版を上げるときに 3 か所（HTML 2 つと package.json）をそろえ忘れないため
test("import map の jetdb-wasm は devDependency と同じ版の CDN を指す", () => {
  const version = (
    JSON.parse(readFileSync("package.json", "utf8")) as {
      devDependencies: Record<string, string>;
    }
  ).devDependencies["jetdb-wasm"];
  expect(version).toMatch(/^\d+\.\d+\.\d+$/);
  for (const html of ["demo/index.html", "ja/demo/index.html"]) {
    const map = /<script type="importmap">\s*(\{[\s\S]*?\})\s*<\/script>/.exec(
      readFileSync(html, "utf8"),
    );
    expect(map, html).not.toBeNull();
    const { imports } = JSON.parse(map?.[1] ?? "") as {
      imports: Record<string, string>;
    };
    expect(imports["jetdb-wasm"], html).toBe(
      `https://cdn.jsdelivr.net/npm/jetdb-wasm@${version}/web/jetdb_wasm.js`,
    );
  }
});
