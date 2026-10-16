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
