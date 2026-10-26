// ページを通すテストの道具: jetdb 本体の testdata の場所、Wasm があるか、デモが開くのを待つこと
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, type Page, test } from "@playwright/test";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

/**
 * jetdb 本体の testdata。環境変数 JETDB_TESTDATA で指せる。
 * 無ければ、このリポジトリ（gh-pages の worktree）の隣にある本体の testdata
 */
export const TESTDATA = process.env.JETDB_TESTDATA
  ? resolve(process.env.JETDB_TESTDATA)
  : resolve(root, "../jetdb/testdata");

/** testdata のファイルの場所。無ければ、そのテストを理由を付けて飛ばす */
export function testdata(file: string): string {
  const path = join(TESTDATA, file);
  test.skip(
    !existsSync(path),
    `${path} is missing: set JETDB_TESTDATA to the testdata directory of jetdb`,
  );
  return path;
}

/** デモのテストの最初に呼ぶ。jetdb-wasm（devDependency。serve.ts --local-wasm が配る）が無ければ、理由を付けて飛ばす */
export function needWasm(): void {
  test.skip(
    !existsSync(join(root, "node_modules/jetdb-wasm/web/jetdb_wasm_bg.wasm")),
    "node_modules/jetdb-wasm/ is missing: run npm ci",
  );
}

/** デモのページを開き、サンプルが開いて右に中身が出るまで待つ（狭い幅では左の一覧が隠れるので、右の見出しで見る） */
export async function openDemo(page: Page, path: string): Promise<void> {
  await page.goto(path);
  await expect(page.locator("#demo-detail h2")).toBeVisible();
}

/** デモで testdata のファイルを選ぶ。開けたかどうかは、呼んだテストの方で確かめる */
export async function openFile(page: Page, path: string): Promise<void> {
  await page.locator("#demo-file").setInputFiles(path);
}
