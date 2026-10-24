// デモで jetdb 本体の testdata のファイルを開く: パスワード付き、Access でないもの、
// 添付ファイルなどの複合列、マクロの多いファイル
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { needWasm, openDemo, openFile, testdata } from "./helpers.ts";

test.beforeEach(() => needWasm());

test("パスワード付きのファイル: 無し → 違う → 正しい の順に開く", async ({
  page,
}) => {
  const file = testdata("db2007-enc.accdb");
  await openDemo(page, "/demo/");
  await openFile(page, file);
  const status = page.locator("#demo-status");
  await expect(status).toHaveText(
    "db2007-enc.accdb is password-protected. Enter its password.",
  );
  const password = page.getByLabel("Password");
  await expect(password).toBeFocused();
  await password.fill("wrong");
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await expect(status).toHaveText("The password is wrong. Try again.");
  await password.fill("Test123");
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.locator("#demo-current")).toHaveText("db2007-enc.accdb");
  await expect(page.locator("#demo-password")).toBeHidden();
  await expect(
    page.getByRole("button", { name: "Table1", exact: true }),
  ).toBeVisible();
});

test("Access でないファイルは開かず、前のファイルを出したままにする", async ({
  page,
}) => {
  const file = testdata("sample-input.tab");
  await openDemo(page, "/ja/demo/");
  await openFile(page, file);
  await expect(page.locator("#demo-status")).toHaveText(
    /^sample-input\.tab を開けませんでした。Access のデータベースではないか、壊れています（.+）。$/,
  );
  await expect(page.locator("#demo-current")).toHaveText(
    "サンプル: jetdb-sample.accdb",
  );
});

test("複合列: 版の履歴・複数の値・添付ファイルを出し、添付ファイルを保存できる", async ({
  page,
}) => {
  const file = testdata("V2007/complexDataTestV2007.accdb");
  await openDemo(page, "/demo/");
  await openFile(page, file);
  await page.getByRole("button", { name: "Table1", exact: true }).click();
  const row2 = page.locator("#demo-detail tbody tr").nth(1);
  const cells = row2.locator(":scope > td");
  await expect(cells.nth(1).locator("summary")).toHaveText("1 version");
  await expect(cells.nth(4).locator("summary")).toHaveText("2 values");
  await expect(cells.nth(5).locator("summary")).toHaveText("2 files");

  await cells.nth(5).locator("summary").click();
  await expect(cells.nth(5).locator("li")).toHaveText([
    "test_data.txt (38 bytes) Save",
    "test_data2.txt (43 bytes) Save",
  ]);
  const download = page.waitForEvent("download");
  await cells.nth(5).getByRole("button", { name: "Save" }).first().click();
  const saved = await download;
  expect(saved.suggestedFilename()).toBe("test_data.txt");
  const path = await saved.path();
  expect(readFileSync(path).length).toBe(38);
});

test("マクロが 464 個あるファイル: そのまとまりだけ最初は閉じ、選ぶと開く", async ({
  page,
}) => {
  const file = testdata("V2010/macroGeneratedTestV2010.accdb");
  await openDemo(page, "/demo/");
  await openFile(page, file);
  const macros = page.locator(".viewer-group", {
    has: page.locator("summary", { hasText: "Macros (464)" }),
  });
  await expect(macros).not.toHaveAttribute("open");
  await expect(
    page.locator(".viewer-group", {
      has: page.locator("summary", { hasText: "Tables (" }),
    }),
  ).toHaveAttribute("open");
  // 読み込み直すとサンプルに戻るので、同じページの中で # を変える
  await page.evaluate(() => {
    location.hash = "#macro/n1_RunCode";
  });
  await expect(page.locator("#demo-detail h2")).toHaveText("n1_RunCode");
  await expect(macros).toHaveAttribute("open");
  await expect(
    page.getByRole("button", { name: "n1_RunCode", exact: true }),
  ).toHaveAttribute("aria-current", "true");
});

test("日本語の Access 97: データベースのコードページで読み、日本語が化けない", async ({
  page,
}) => {
  const file = testdata("V1997/japaneseTestV1997.mdb");
  await openDemo(page, "/ja/demo/");
  await openFile(page, file);
  await expect(page.locator("#demo-current")).toHaveText(
    "japaneseTestV1997.mdb",
  );
  await expect(page.locator("#demo-version")).toHaveText("JET3");
  await expect(
    page.getByRole("button", { name: "顧客一覧", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "顧客", exact: true }).click();
  const cells = page.locator("#demo-detail tbody td");
  await expect(cells.filter({ hasText: "山田太郎" })).toHaveCount(1);
  await expect(cells.filter({ hasText: "①～髙" })).toHaveCount(1);
});

test("Access 2000 の形式のフォーム: プロパティを名前で出し、コントロールの一覧も出す", async ({
  page,
}) => {
  const file = testdata("V2000/propertyIdTestV2000.mdb");
  await openDemo(page, "/demo/");
  await openFile(page, file);
  await expect(page.locator("#demo-current")).toHaveText(
    "propertyIdTestV2000.mdb",
  );
  await expect(page.locator("#demo-version")).toHaveText("JET4");
  await expect(page.locator("#demo-counts li").nth(2)).toHaveText("87 forms");
  await expect(page.locator("#demo-counts li").nth(3)).toHaveText("19 reports");
  // 50 個を超えるまとまりは最初は閉じているので、開いてから選ぶ
  const forms = page.locator(".viewer-group", {
    has: page.locator("summary", { hasText: "Forms (87)" }),
  });
  await expect(forms).not.toHaveAttribute("open");
  await forms.locator("summary").click();
  await forms.locator(".viewer-item").first().click();
  const detail = page.locator("#demo-detail");
  await expect(detail.locator(".viewer-type")).toHaveText("form");
  await expect(detail.locator("tbody")).toContainText("DividingLines");
  await detail.getByRole("tab", { name: "Controls" }).click();
  const list = detail.locator("table").first();
  await expect(list.locator("th")).toHaveText(["Name", "Type", "Index"]);
  expect(await list.locator("tbody tr").count()).toBeGreaterThan(0);
});
