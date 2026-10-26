// 紹介ページ（日英）: インストールのタブのキー操作と、コマンドのコピーボタン
import { expect, test } from "@playwright/test";

for (const [path, tabNames] of [
  ["/", ["Prebuilt binaries", "Homebrew", "cargo"]],
  ["/ja/", null],
] as const) {
  test(`${path}: インストールのタブを、押しても矢印・Home・End でも切り替える`, async ({
    page,
  }) => {
    await page.goto(path);
    const tabs = page.locator('[role="tablist"] [role="tab"]');
    await expect(tabs).toHaveCount(3);
    if (tabNames) await expect(tabs).toHaveText([...tabNames]);
    const panel = async (i: number) =>
      page.locator(
        `#${(await tabs.nth(i).getAttribute("aria-controls")) ?? ""}`,
      );

    await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
    await expect(await panel(1)).toBeHidden();

    await tabs.nth(2).click();
    await expect(tabs.nth(2)).toHaveAttribute("aria-selected", "true");
    await expect(await panel(2)).toBeVisible();
    await expect(await panel(0)).toBeHidden();

    await tabs.nth(2).focus();
    await page.keyboard.press("ArrowRight");
    await expect(tabs.nth(0)).toBeFocused();
    await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("End");
    await expect(tabs.nth(2)).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("Home");
    await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("ArrowLeft");
    await expect(tabs.nth(2)).toBeFocused();
  });

  test(`${path}: コマンドのある枠に、コピーボタンを出す`, async ({ page }) => {
    await page.goto(path);
    const buttons = page.locator(".cmd-block .copy-btn");
    expect(await buttons.count()).toBeGreaterThan(0);
    for (const button of await buttons.all()) {
      // 隠れたタブの中のボタンもあるので、hidden の属性で見る
      await expect(button).not.toHaveAttribute("hidden");
    }
  });
}

test("375px: 全項目の比較表は jetdb の列が枠に収まり、右にスクロールしても項目名が見える", async ({
  page,
}) => {
  await page.setViewportSize({ width: 375, height: 800 });
  await page.goto("/compare/");
  const wrap = page.locator(".table-wrap");
  const wrapBox = await wrap.boundingBox();
  const meBox = await page
    .locator("table.compare tbody td.me")
    .first()
    .boundingBox();
  expect(wrapBox && meBox && meBox.x + meBox.width).toBeLessThanOrEqual(
    (wrapBox?.x ?? 0) + (wrapBox?.width ?? 0),
  );
  await wrap.evaluate((e) => {
    e.scrollLeft = e.scrollWidth;
  });
  const label = page.locator("table.compare tbody th", { hasText: "Language" });
  const labelBox = await label.boundingBox();
  expect(labelBox && wrapBox && labelBox.x).toBeGreaterThanOrEqual(
    wrapBox?.x ?? 0,
  );
  expect(
    labelBox && wrapBox && labelBox.x + labelBox.width,
  ).toBeLessThanOrEqual((wrapBox?.x ?? 0) + (wrapBox?.width ?? 0));
});

test("1280px: カードの出力を開いても、同じ行のほかのカードは伸びない", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  const cards = page.locator(".reads > li");
  // カードの高さと、カードの中でのコマンドの枠の位置
  const shape = (i: number) =>
    cards.nth(i).evaluate((li) => ({
      height: Math.round(li.getBoundingClientRect().height),
      cmdTop: Math.round(
        (li.querySelector(".cmd")?.getBoundingClientRect().top ?? 0) -
          li.getBoundingClientRect().top,
      ),
    }));
  // 07 と 08 は同じ行で、閉じた高さとコマンドの枠の位置が同じ
  const before = await shape(7);
  expect(await shape(6)).toEqual(before);
  await cards.nth(6).locator("summary").click();
  await expect(cards.nth(6).locator("pre")).toBeVisible();
  expect((await shape(6)).height).toBeGreaterThan(before.height);
  expect(await shape(7)).toEqual(before);
});

for (const width of [1280, 375]) {
  test(`${width}px: カードの出力は枠の幅で折り返し、横にスクロールしない`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/");
    const card = page.locator(".reads > li").nth(3);
    await card.locator("summary").click();
    const pre = card.locator("pre");
    await expect(pre).toBeVisible();
    expect(await pre.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(
      true,
    );
    await expect(pre).toContainText("ORDER BY");
  });
}

test("比較表は、横に続きがある間だけ右端にかすむ帯を出す", async ({ page }) => {
  const band = (p: typeof page) =>
    p
      .locator("#compare .table-scroll")
      .evaluate((e) => getComputedStyle(e, "::after").opacity);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  expect(await band(page)).toBe("0");
  await page.setViewportSize({ width: 375, height: 800 });
  await expect.poll(() => band(page)).toBe("1");
  await page.locator("#compare .table-wrap").evaluate((e) => {
    e.scrollLeft = e.scrollWidth;
  });
  await expect.poll(() => band(page)).toBe("0");
});

// 見出しの各行の文字（1 文字ずつ位置を測って行に分ける）
const headingLines = (page: import("@playwright/test").Page) =>
  page.locator(".hero h1").evaluate((h) => {
    const lines = new Map<number, string>();
    const walker = document.createTreeWalker(h, NodeFilter.SHOW_TEXT);
    let node = walker.nextNode();
    while (node) {
      const text = node as Text;
      for (let i = 0; i < text.length; i++) {
        const range = document.createRange();
        range.setStart(text, i);
        range.setEnd(text, i + 1);
        const rect = range.getBoundingClientRect();
        if (rect.width === 0) continue;
        const top = Math.round(rect.top);
        lines.set(top, (lines.get(top) ?? "") + text.data[i]);
      }
      node = walker.nextNode();
    }
    return [...lines.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([, line]) => line.trim());
  });

for (const [path, lines] of [
  ["/", ["Read .mdb / .accdb", "files without", "Microsoft Access"]],
  ["/ja/", [".mdb / .accdb を、", "Access 無しで", "丸ごと読める"]],
] as const) {
  test(`${path}: 375px では見出しを語の途中で割らずに 3 行で出す`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 375, height: 800 });
    await page.goto(path);
    expect(await headingLines(page)).toEqual([...lines]);
  });
}

for (const [path, demo, label] of [
  ["/", "/demo/", "Demo"],
  ["/compare/", "/demo/", "Demo"],
  ["/demo/", "/demo/", "Demo"],
  ["/ja/", "/ja/demo/", "デモ"],
  ["/ja/compare/", "/ja/demo/", "デモ"],
  ["/ja/demo/", "/ja/demo/", "デモ"],
] as const) {
  test(`${path}: ヘッダーのナビからデモへ行ける`, async ({ page }) => {
    await page.goto(path);
    const link = page.locator(".header-nav").getByRole("link", { name: label });
    await expect(link).toHaveAttribute("href", demo);
    await link.click();
    await expect(page).toHaveURL(new RegExp(`${demo}(#.*)?$`));
    await expect(page.locator("#demo-file")).toBeAttached();
  });
}

for (const path of ["/", "/ja/"] as const) {
  test(`${path}: 375px でインストールのタブは 1 行に収まる`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 375, height: 800 });
    await page.goto(path);
    const tops = await page
      .locator('#install [role="tab"]')
      .evaluateAll((tabs) =>
        tabs.map((t) => Math.round(t.getBoundingClientRect().top)),
      );
    expect(new Set(tops).size).toBe(1);
  });
}
