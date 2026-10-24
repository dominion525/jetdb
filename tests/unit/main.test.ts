import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// main.ts はモジュールではなく、読み込んだときにページの中のものを拾って動きを付ける。
// テストでは、ページの中身を作ってから毎回読み込み直す（? を付けて別のものとして読む）
let runs = 0;
async function loadMain(html: string, lang = "en"): Promise<void> {
  document.documentElement.lang = lang;
  document.body.innerHTML = html;
  runs += 1;
  await import(/* @vite-ignore */ `../../src/main.ts?run=${runs}`);
}

const $ = <T extends Element = HTMLElement>(selector: string) => {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`no ${selector}`);
  return element;
};

const COPY_HTML = `
  <div class="cmd-block">
    <pre><code>cargo add jetdb</code></pre>
    <button class="copy-btn" type="button" hidden>Copy</button>
  </div>
  <div class="cmd-block"><button class="copy-btn" id="lonely" hidden>Copy</button></div>
  <p class="copy-status" role="status"></p>`;

const TABS_HTML = `
  <div role="tablist" id="install">
    <button role="tab" id="t1" aria-controls="p1" aria-selected="false">A</button>
    <button role="tab" id="t2" aria-controls="p2" aria-selected="true">B</button>
    <button role="tab" id="t3" aria-controls="p3" aria-selected="false">C</button>
  </div>
  <div role="tabpanel" id="p1">a</div>
  <div role="tabpanel" id="p2">b</div>
  <div role="tabpanel" id="p3">c</div>
  <div role="tablist" id="demo">
    <button role="tab" id="d1" aria-selected="false">X</button>
    <button role="tab" id="d2" aria-selected="false">Y</button>
  </div>`;

const selected = () =>
  [...document.querySelectorAll('#install [role="tab"]')].map(
    (tab) => tab.getAttribute("aria-selected") === "true",
  );
const hidden = () =>
  ["p1", "p2", "p3"].map(
    (id) => (document.getElementById(id) as HTMLElement).hidden,
  );
const press = (key: string) =>
  document.activeElement?.dispatchEvent(
    new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
  );

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("ページを読み込んだとき", () => {
  it("html に js の印を付ける", async () => {
    document.documentElement.classList.remove("js");
    await loadMain("");
    expect(document.documentElement.classList.contains("js")).toBe(true);
  });
});

describe("コピーボタン", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  it("コマンドのある枠のボタンだけを出す", async () => {
    await loadMain(COPY_HTML);
    expect($<HTMLButtonElement>(".copy-btn").hidden).toBe(false);
    expect($<HTMLButtonElement>("#lonely").hidden).toBe(true);
  });

  it("押すとコマンドをコピーし、2 秒で元の文に戻す", async () => {
    const write = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue(undefined);
    await loadMain(COPY_HTML);
    const button = $<HTMLButtonElement>(".copy-btn");
    button.click();
    await vi.waitFor(() => expect(button.textContent).toBe("Copied"));
    expect(write).toHaveBeenCalledWith("cargo add jetdb");
    expect($(".copy-status").textContent).toBe("Copied");
    vi.advanceTimersByTime(2000);
    expect(button.textContent).toBe("Copy");
  });

  it("コピーできなかったら、文字を選んで手でコピーするよう伝える", async () => {
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(
      new Error("denied"),
    );
    await loadMain(COPY_HTML, "ja");
    const button = $<HTMLButtonElement>(".copy-btn");
    button.click();
    const message =
      "コピーできませんでした。選択した文字を手でコピーしてください";
    await vi.waitFor(() => expect(button.textContent).toBe(message));
    expect($(".copy-status").textContent).toBe(message);
    expect(window.getSelection()?.toString()).toBe("cargo add jetdb");
    vi.advanceTimersByTime(2000);
    expect(button.textContent).toBe("コピー");
  });
});

describe("インストール方法のタブ", () => {
  it("最初は aria-selected のタブを選び、ほかの中身を隠す", async () => {
    await loadMain(TABS_HTML);
    expect(selected()).toEqual([false, true, false]);
    expect(hidden()).toEqual([true, false, true]);
    expect(
      [...document.querySelectorAll<HTMLElement>('#install [role="tab"]')].map(
        (tab) => tab.tabIndex,
      ),
    ).toEqual([-1, 0, -1]);
  });

  it("押したタブに切り替える", async () => {
    await loadMain(TABS_HTML);
    $("#t3").click();
    expect(selected()).toEqual([false, false, true]);
    expect(hidden()).toEqual([true, true, false]);
  });

  it("矢印・Home・End のキーで切り替え、選んだタブに移る", async () => {
    await loadMain(TABS_HTML);
    $("#t2").focus();
    press("ArrowRight");
    expect(selected()).toEqual([false, false, true]);
    expect(document.activeElement?.id).toBe("t3");
    press("ArrowRight");
    expect(document.activeElement?.id).toBe("t1");
    press("ArrowLeft");
    expect(document.activeElement?.id).toBe("t3");
    press("Home");
    expect(selected()).toEqual([true, false, false]);
    press("End");
    expect(selected()).toEqual([false, false, true]);
    expect(hidden()).toEqual([true, true, false]);
  });

  it("aria-controls の無いタブの並び（デモのタブ）には触らない", async () => {
    await loadMain(TABS_HTML);
    const tabs = [
      ...document.querySelectorAll<HTMLElement>("#demo [role=tab]"),
    ];
    expect(tabs.map((tab) => tab.getAttribute("aria-selected"))).toEqual([
      "false",
      "false",
    ]);
    expect(tabs.map((tab) => tab.hasAttribute("tabindex"))).toEqual([
      false,
      false,
    ]);
  });
});
