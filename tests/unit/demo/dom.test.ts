import { afterEach, describe, expect, it, vi } from "vitest";
import {
  codeOf,
  el,
  failure,
  fileName,
  messageOf,
  reasonOf,
  sizeOf,
  table,
  tabs,
  textView,
  value,
} from "../../../src/demo/dom.ts";
import { MESSAGES } from "../../../src/demo/messages.ts";

const t = MESSAGES.en;

// 値を出したものを、文字列のまま、または要素の文字として見る
const textOf = (child: ReturnType<typeof value>) =>
  child instanceof Node ? child.textContent : child;
const asElement = (child: ReturnType<typeof value>) => {
  if (!(child instanceof HTMLElement)) throw new Error("not an element");
  return child;
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("el", () => {
  it("class・属性・on で始まるものを分けて付ける", () => {
    const click = vi.fn();
    const button = el(
      "button",
      { class: "a b", type: "button", "aria-label": "x", onclick: click },
      "go",
    );
    expect(button.className).toBe("a b");
    expect(button.getAttribute("type")).toBe("button");
    expect(button.getAttribute("aria-label")).toBe("x");
    expect(button.hasAttribute("onclick")).toBe(false);
    button.click();
    expect(click).toHaveBeenCalledOnce();
  });

  it("中身の配列は 1 段だけ開き、null と undefined は飛ばす", () => {
    const span = el("span", {}, "a", null, ["b", undefined, el("i", {}, "c")]);
    expect(span.textContent).toBe("abc");
    expect(span.childNodes).toHaveLength(3);
  });
});

describe("エラーの文と code", () => {
  it("Error ならその文、そうでなければ文字列にしたもの", () => {
    expect(messageOf(new Error("broken"))).toBe("broken");
    expect(messageOf("plain")).toBe("plain");
  });

  it("jetdb のエラーの code を取り出す", () => {
    const e = Object.assign(new Error("x"), { code: "INVALID_FILE" });
    expect(codeOf(e)).toBe("INVALID_FILE");
    expect(codeOf(new Error("x"))).toBeUndefined();
    expect(codeOf({ code: "INVALID_FILE" })).toBeUndefined();
  });

  it("読めなかった理由: jetdb のエラーはその文、code の無いエラーは止まった知らせ", () => {
    const jetdbError = Object.assign(new Error("bad page"), {
      code: "INVALID_FILE",
    });
    expect(reasonOf(t, jetdbError)).toBe("bad page");
    // jetdb-wasm がパニックすると、code の無い RuntimeError や Error が来る
    expect(reasonOf(t, new WebAssembly.RuntimeError("unreachable"))).toBe(
      "jetdb stopped partway through this file. Open another file or reload the page (unreachable).",
    );
    expect(reasonOf(MESSAGES.ja, new Error("recursive use"))).toBe(
      "jetdb がこのファイルの途中で止まりました。別のファイルを開くか、ページを読み込み直してください（recursive use）。",
    );
  });

  it("読めなかったときの一文", () => {
    const p = failure(
      t,
      Object.assign(new Error("bad page"), { code: "INVALID_FILE" }),
    );
    expect(p.className).toBe("viewer-error");
    expect(p.textContent).toBe("Could not read: bad page");
    expect(failure(t, new Error("unreachable")).textContent).toBe(
      "Could not read: jetdb stopped partway through this file. Open another file or reload the page (unreachable).",
    );
  });
});

describe("table", () => {
  it("見出しの行と、値の行を作る", () => {
    const wrap = table(
      ["A", "B"],
      [
        ["1", el("b", {}, "2")],
        ["3", null],
      ],
    );
    expect(wrap.className).toBe("viewer-table-wrap");
    const heads = [...wrap.querySelectorAll("th")];
    expect(heads.map((h) => h.textContent)).toEqual(["A", "B"]);
    expect(heads.every((h) => h.getAttribute("scope") === "col")).toBe(true);
    expect(
      [...wrap.querySelectorAll("tbody tr")].map((r) =>
        [...r.querySelectorAll("td")].map((d) => d.textContent),
      ),
    ).toEqual([
      ["1", "2"],
      ["3", ""],
    ]);
  });
});

describe("大きさ", () => {
  it("1024 未満はバイト、そのあとは KB と MB", () => {
    expect(sizeOf(t, 1023)).toBe("1023 bytes");
    expect(sizeOf(t, 1024)).toBe("1.0 KB");
    expect(sizeOf(t, 1536 * 1024)).toBe("1.5 MB");
    expect(sizeOf(MESSAGES.ja, 10)).toBe("10 バイト");
  });
});

describe("値の出し方", () => {
  it("NULL・真偽値・数・文字", () => {
    const empty = asElement(value(t, null));
    expect(empty.className).toBe("viewer-null");
    expect(empty.textContent).toBe("NULL");
    expect(textOf(value(t, undefined))).toBe("NULL");
    expect(value(t, true)).toBe("true");
    expect(value(t, false)).toBe("false");
    expect(value(t, 1.5)).toBe("1.5");
    expect(value(t, 12345678901234567890n)).toBe("12345678901234567890");
    expect(value(t, "text")).toBe("text");
  });

  it("バイト列は大きさだけを出す", () => {
    const binary = asElement(value(t, new Uint8Array(2048)));
    expect(binary.className).toBe("viewer-binary");
    expect(binary.textContent).toBe("binary, 2.0 KB");
  });

  it("中身の無い複合列は none", () => {
    expect(textOf(value(t, []))).toBe("none");
  });

  it("複数の値は件数を出し、開くと値の一覧", () => {
    const details = asElement(value(t, ["a", null, 3]));
    expect(details.querySelector("summary")?.textContent).toBe("3 values");
    expect(
      [...details.querySelectorAll("li")].map((li) => li.textContent),
    ).toEqual(["a", "NULL", "3"]);
  });

  it("添付ファイルは名前と大きさと保存のボタン、読めないものはそう書く", () => {
    const attachment = (name: string | null, data: Uint8Array | null) => ({
      name,
      type: null,
      data,
      url: null,
      timestamp: null,
      flags: null,
    });
    const details = asElement(
      value(t, [
        attachment("a.txt", new Uint8Array(10)),
        attachment(null, new Uint8Array(1)),
        attachment("broken.bin", null),
      ]),
    );
    expect(details.querySelector("summary")?.textContent).toBe("3 files");
    const items = [...details.querySelectorAll("li")];
    expect(items.map((li) => li.textContent)).toEqual([
      "a.txt (10 bytes) Save",
      "(no name) (1 byte) Save",
      "broken.bin (could not be decoded)",
    ]);
    expect(items[2]?.querySelector("button")).toBeNull();
  });

  it("版の履歴は日時と値、日時の無いものはそう書く", () => {
    const details = asElement(
      value(t, [
        { value: "first", modified: "2024-01-02" },
        { value: null, modified: null },
      ]),
    );
    expect(details.querySelector("summary")?.textContent).toBe("2 versions");
    expect(
      [...details.querySelectorAll("li")].map((li) => li.textContent),
    ).toEqual(["2024-01-02: first", "(no date): NULL"]);
  });

  it("1 件のときは単数", () => {
    expect(
      asElement(value(t, [{ value: "v", modified: "d" }])).querySelector(
        "summary",
      )?.textContent,
    ).toBe("1 version");
  });
});

describe("保存のボタン", () => {
  it("中身を Blob にして、ファイル名を付けて保存させる", async () => {
    let saved: Blob | undefined;
    vi.spyOn(URL, "createObjectURL").mockImplementation((blob) => {
      saved = blob as Blob;
      return "blob:test";
    });
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const clicked: HTMLAnchorElement[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      clicked.push(this);
    });
    const view = textView(t, "SELECT 1;", "q.sql");
    const save = [...view.querySelectorAll("button")].find(
      (b) => b.textContent === "Save",
    );
    save?.click();
    expect(clicked[0]?.download).toBe("q.sql");
    expect(clicked[0]?.getAttribute("href")).toBe("blob:test");
    expect(await saved?.text()).toBe("SELECT 1;");
  });
});

describe("テキストの表示", () => {
  const copyButton = (view: HTMLElement) =>
    [...view.querySelectorAll("button")].find((b) => b.textContent === "Copy");

  it("テキストを等幅で出し、折り返しを切り替えられる", () => {
    const view = textView(t, "a very long line", "x.txt");
    // ページに置いていないチェックボックスは、押しても change を送らないので、ページに置く
    document.body.replaceChildren(view);
    expect(view.querySelector("pre code")?.textContent).toBe(
      "a very long line",
    );
    const wrap = view.querySelector<HTMLInputElement>(".viewer-wrap input");
    const pre = view.querySelector("pre");
    expect(view.querySelector(".viewer-wrap")?.textContent).toBe("Wrap lines");
    expect(pre?.classList.contains("is-wrapped")).toBe(false);
    wrap?.click();
    expect(pre?.classList.contains("is-wrapped")).toBe(true);
    wrap?.click();
    expect(pre?.classList.contains("is-wrapped")).toBe(false);
  });

  it("コピーできたら Copied、2 秒で戻す", async () => {
    vi.useFakeTimers();
    const write = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue(undefined);
    const view = textView(t, "SELECT 1;", "q.sql");
    const copy = copyButton(view);
    copy?.click();
    await vi.waitFor(() => expect(copy?.textContent).toBe("Copied"));
    expect(write).toHaveBeenCalledWith("SELECT 1;");
    vi.advanceTimersByTime(2000);
    expect(copy?.textContent).toBe("Copy");
  });

  it("コピーできなかったらそう書く", async () => {
    vi.useFakeTimers();
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(
      new Error("denied"),
    );
    const view = textView(MESSAGES.ja, "x", "x.txt");
    const copy = [...view.querySelectorAll("button")].find(
      (b) => b.textContent === "コピー",
    );
    copy?.click();
    await vi.waitFor(() =>
      expect(copy?.textContent).toBe("コピーできませんでした"),
    );
    vi.advanceTimersByTime(2000);
    expect(copy?.textContent).toBe("コピー");
  });
});

describe("タブ", () => {
  const setUp = (render2: () => Node = () => el("p", {}, "two")) => {
    const rendered: string[] = [];
    const [bar, panel] = tabs(t, [
      [
        "One",
        () => {
          rendered.push("one");
          return el("p", {}, "one");
        },
      ],
      [
        "Two",
        () => {
          rendered.push("two");
          return render2();
        },
      ],
      ["Three", () => el("p", {}, "three")],
    ]);
    document.body.replaceChildren(bar, panel);
    const buttons = [...bar.querySelectorAll<HTMLButtonElement>("button")];
    const selected = () =>
      buttons.map((b) => b.getAttribute("aria-selected") === "true");
    return { bar, panel, buttons, selected, rendered };
  };
  const press = (key: string) =>
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    );

  it("最初のタブを選び、その中身だけを作る", () => {
    const { bar, panel, selected, rendered } = setUp();
    expect(bar.getAttribute("role")).toBe("tablist");
    expect(panel.getAttribute("role")).toBe("tabpanel");
    expect(selected()).toEqual([true, false, false]);
    expect(panel.textContent).toBe("one");
    expect(rendered).toEqual(["one"]);
  });

  it("押したタブの中身を、そのときに作る", () => {
    const { panel, buttons, selected, rendered } = setUp();
    buttons[1]?.click();
    expect(selected()).toEqual([false, true, false]);
    expect(panel.textContent).toBe("two");
    expect(rendered).toEqual(["one", "two"]);
  });

  it("左右の矢印で切り替え、端では反対の端へ回る", () => {
    const { panel, buttons, selected } = setUp();
    buttons[0]?.focus();
    press("ArrowLeft");
    expect(selected()).toEqual([false, false, true]);
    expect(document.activeElement).toBe(buttons[2]);
    press("ArrowRight");
    expect(selected()).toEqual([true, false, false]);
    expect(panel.textContent).toBe("one");
    press("Enter");
    expect(selected()).toEqual([true, false, false]);
  });

  it("タブと中身を id で結び付け、Tab キーでは選んでいるタブにだけ止まる", () => {
    const { panel, buttons } = setUp();
    const ids = buttons.map((b) => b.id);
    expect(new Set(ids).size).toBe(3);
    expect(panel.id).not.toBe("");
    for (const b of buttons) {
      expect(b.getAttribute("aria-controls")).toBe(panel.id);
    }
    expect(panel.getAttribute("aria-labelledby")).toBe(ids[0]);
    expect(buttons.map((b) => b.tabIndex)).toEqual([0, -1, -1]);
    buttons[2]?.click();
    expect(panel.getAttribute("aria-labelledby")).toBe(ids[2]);
    expect(buttons.map((b) => b.tabIndex)).toEqual([-1, -1, 0]);
  });

  it("別の組とは id が重ならない", () => {
    const first = setUp();
    const second = setUp();
    expect(second.panel.id).not.toBe(first.panel.id);
    expect(second.buttons[0]?.id).not.toBe(first.buttons[0]?.id);
  });

  it("Home と End で先頭と末尾へ移る", () => {
    const { buttons, selected } = setUp();
    buttons[1]?.focus();
    press("End");
    expect(selected()).toEqual([false, false, true]);
    expect(document.activeElement).toBe(buttons[2]);
    press("Home");
    expect(selected()).toEqual([true, false, false]);
    expect(document.activeElement).toBe(buttons[0]);
  });

  it("中身を作れなかったら、その場に読めなかった一文を出す", () => {
    const { panel, buttons } = setUp(() => {
      throw Object.assign(new Error("no such table"), {
        code: "TABLE_NOT_FOUND",
      });
    });
    buttons[1]?.click();
    expect(panel.textContent).toBe("Could not read: no such table");
  });
});

describe("ファイル名", () => {
  it("ファイル名に使えない文字を _ にする", () => {
    expect(fileName('a/b\\c:d*e?f"g<h>i|j', "sql")).toBe(
      "a_b_c_d_e_f_g_h_i_j.sql",
    );
    expect(fileName("Form1.OnClick", "txt")).toBe("Form1.OnClick.txt");
  });
});
