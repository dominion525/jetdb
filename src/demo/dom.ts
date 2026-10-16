// デモのページ: DOM を作る部品（要素、表、タブ、テキストの表示、値の出し方）。
// 画面に出す文字は、ページの言語の文字の表（t）を引数でもらう

import type {
  Attachment,
  ColumnVersion,
  Scalar,
  Value,
} from "/assets/wasm/jetdb_wasm.js";
import type { Messages } from "./messages.js";

/** 要素の中身にできるもの。null と undefined は無視する */
export type Child = Node | string | null | undefined;
type Attrs = Record<string, string | ((event: Event) => void)>;

// 要素を作る。children は文字列か要素（配列も 1 段だけ開く）
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: (Child | Child[])[]
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (typeof v === "function") e.addEventListener(k.slice(2), v);
    else if (k === "class") e.className = v;
    else e.setAttribute(k, v);
  }
  e.append(
    ...children
      .flat()
      .filter((c): c is Node | string => c !== null && c !== undefined),
  );
  return e;
}

/** 投げられたものの文 */
export const messageOf = (e: unknown): string =>
  e instanceof Error ? e.message : String(e);

/** jetdb のエラーなら、その code */
export const codeOf = (e: unknown): string | undefined =>
  e instanceof Error && "code" in e && typeof e.code === "string"
    ? e.code
    : undefined;

// 読めなかったときに、その場に出す一文
export const failure = (t: Messages, e: unknown) =>
  el("p", { class: "viewer-error" }, t.couldNotRead(messageOf(e)));

// 見出しの行と行の配列から表を作る。値は文字列か要素
export function table(headers: string[], rows: Child[][]) {
  return el(
    "div",
    { class: "viewer-table-wrap" },
    el(
      "table",
      { class: "viewer-table" },
      el(
        "thead",
        {},
        el(
          "tr",
          {},
          headers.map((h) => el("th", { scope: "col" }, h)),
        ),
      ),
      el(
        "tbody",
        {},
        rows.map((r) =>
          el(
            "tr",
            {},
            r.map((c) => el("td", {}, c)),
          ),
        ),
      ),
    ),
  );
}

// バイト列をファイルとして保存するボタン
export function saveButton(label: string, bytes: Uint8Array, filename: string) {
  return el(
    "button",
    {
      type: "button",
      class: "viewer-save",
      onclick: () => {
        const url = URL.createObjectURL(
          new Blob([bytes as Uint8Array<ArrayBuffer>]),
        );
        const a = el("a", { href: url, download: filename });
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      },
    },
    label,
  );
}

export const sizeOf = (t: Messages, n: number) =>
  n < 1024
    ? t.bytes(n)
    : n < 1024 * 1024
      ? `${(n / 1024).toFixed(1)} KB`
      : `${(n / 1024 / 1024).toFixed(1)} MB`;

// 添付ファイル・版の履歴を見分ける
const isAttachment = (v: unknown): v is Attachment =>
  v !== null &&
  typeof v === "object" &&
  !(v instanceof Uint8Array) &&
  "data" in v &&
  "name" in v;
const isVersion = (v: unknown): v is ColumnVersion =>
  v !== null &&
  typeof v === "object" &&
  !(v instanceof Uint8Array) &&
  "modified" in v &&
  "value" in v;

// 1 つの値を出す
export function value(t: Messages, v: Value | undefined): Child {
  if (v === null || v === undefined)
    return el("span", { class: "viewer-null" }, "NULL");
  if (typeof v === "boolean") return v ? "true" : "false";
  if (typeof v === "bigint" || typeof v === "number") return String(v);
  if (typeof v === "string") return v;
  if (v instanceof Uint8Array)
    return el(
      "span",
      { class: "viewer-binary" },
      t.binary(sizeOf(t, v.length)),
    );
  if (Array.isArray(v)) return complex(t, v);
  return JSON.stringify(v);
}

// 複合列の値: 件数を出し、開くと中身を見られる
function complex(
  t: Messages,
  items: Scalar[] | Attachment[] | ColumnVersion[],
) {
  if (items.length === 0)
    return el("span", { class: "viewer-null" }, t.noneValue);
  const all: unknown[] = items;
  const files = all.every(isAttachment);
  const versions = all.every(isVersion);
  const noun = files
    ? t.complexNouns.file
    : versions
      ? t.complexNouns.version
      : t.complexNouns.value;
  const list = el(
    "ul",
    { class: "viewer-complex-list" },
    all.map((item) => {
      if (files && isAttachment(item)) {
        const name = item.name ?? t.noName;
        return el(
          "li",
          {},
          name,
          item.data
            ? [
                ` (${sizeOf(t, item.data.length)}) `,
                saveButton(t.save, item.data, name),
              ]
            : t.notDecoded,
        );
      }
      if (versions && isVersion(item))
        return el(
          "li",
          {},
          item.modified ?? t.noDate,
          ": ",
          value(t, item.value),
        );
      return el("li", {}, value(t, item as Scalar));
    }),
  );
  return el(
    "details",
    { class: "viewer-complex" },
    el("summary", {}, t.complexCount(items.length, noun[0], noun[1])),
    list,
  );
}

/** タブ: [名前, 中身を作る関数]。中身は選んだときに作る */
export type Tab = [label: string, render: () => Node];

export function tabs(t: Messages, items: Tab[]): [HTMLElement, HTMLElement] {
  const panel = el("div", { class: "viewer-panel", role: "tabpanel" });
  const buttons = items.map(([label], i) =>
    el(
      "button",
      {
        type: "button",
        class: "tab",
        role: "tab",
        "aria-selected": "false",
        onclick: () => select(i),
      },
      label,
    ),
  );
  function select(i: number) {
    buttons.forEach((b, j) => {
      b.setAttribute("aria-selected", String(i === j));
    });
    let content: Node;
    try {
      const render = items[i]?.[1];
      if (!render) return;
      content = render();
    } catch (e) {
      content = failure(t, e);
    }
    panel.replaceChildren(content);
  }
  const bar = el(
    "div",
    { class: "tabs viewer-tabs", role: "tablist" },
    buttons,
  );
  bar.addEventListener("keydown", (event) => {
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (i < 0) return;
    const next =
      event.key === "ArrowRight"
        ? (i + 1) % buttons.length
        : event.key === "ArrowLeft"
          ? (i - 1 + buttons.length) % buttons.length
          : null;
    if (next === null) return;
    event.preventDefault();
    buttons[next]?.focus();
    select(next);
  });
  select(0);
  return [bar, panel];
}

// テキストを等幅で出し、コピーとファイルとして保存のボタンを付ける
export function textView(t: Messages, text: string, filename: string) {
  const copy = el(
    "button",
    {
      type: "button",
      class: "viewer-save",
      onclick: async () => {
        try {
          await navigator.clipboard.writeText(text);
          copy.textContent = t.copied;
        } catch {
          copy.textContent = t.copyFailed;
        }
        setTimeout(() => {
          copy.textContent = t.copy;
        }, 2000);
      },
    },
    t.copy,
  );
  const save = saveButton(t.save, new TextEncoder().encode(text), filename);
  // 長い行を枠の中で折り返すかどうか。テキストは CLI と同じ形のまま
  const pre = el("pre", {}, el("code", {}, text));
  const wrapBox = el("input", { type: "checkbox" });
  wrapBox.addEventListener("change", () =>
    pre.classList.toggle("is-wrapped", wrapBox.checked),
  );
  return el(
    "div",
    { class: "viewer-code" },
    el(
      "div",
      { class: "viewer-code-actions" },
      el("label", { class: "viewer-wrap" }, wrapBox, t.wrapLines),
      copy,
      save,
    ),
    pre,
  );
}

// ファイル名に使えない文字を _ にする
export const fileName = (name: string, ext: string) =>
  `${name.replace(/[\\/:*?"<>|]/g, "_")}.${ext}`;
