// 紹介ページの小さな動き: コマンドのコピーボタンと、インストール方法のタブ。
// HTML から <script defer> で読む（モジュールではない）ので、import / export は書かない。
// サイトの全部のページが読む（コピーボタンとタブがあるのは紹介ページ）。デモの動きは src/demo/ にある

document.documentElement.classList.add("js");

const SITE_MESSAGES = {
  ja: {
    copy: "コピー",
    copied: "コピーしました",
    copyFailed: "コピーできませんでした。選択した文字を手でコピーしてください",
  },
  en: {
    copy: "Copy",
    copied: "Copied",
    copyFailed: "Could not copy. Copy the selected text by hand",
  },
};

const siteText =
  SITE_MESSAGES[document.documentElement.lang.startsWith("ja") ? "ja" : "en"];

// ---- コマンドのコピーボタン ----

const copyStatus = document.querySelector(".copy-status");

// 読み上げ用の欄に伝える。同じ文を続けて出しても読み上げられるように、一度空にする
function announce(message: string): void {
  if (copyStatus) {
    copyStatus.textContent = "";
    copyStatus.textContent = message;
  }
}

function selectText(element: Element): void {
  const range = document.createRange();
  range.selectNodeContents(element);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

function setUpCopyButton(button: HTMLButtonElement): void {
  const code = button.parentElement?.querySelector("pre code");
  if (!code) return;
  button.hidden = false;
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(code.textContent ?? "");
      button.textContent = siteText.copied;
      announce(siteText.copied);
    } catch {
      selectText(code);
      button.textContent = siteText.copyFailed;
      announce(siteText.copyFailed);
    }
    setTimeout(() => {
      button.textContent = siteText.copy;
    }, 2000);
  });
}

// ---- インストール方法のタブ ----

// タブの並びを、矢印・Home・End のキーでも切り替えられるようにする。
// どのタブも aria-controls で中身を指している並びだけを扱う（デモのタブは src/demo/dom.ts が扱う）
function setUpTabs(tablist: Element): void {
  const tabs = [...tablist.querySelectorAll<HTMLElement>('[role="tab"]')];
  const panels = tabs.map((tab) => {
    const id = tab.getAttribute("aria-controls");
    return id ? document.getElementById(id) : null;
  });
  if (tabs.length === 0 || panels.some((panel) => panel === null)) return;

  function select(index: number, focus: boolean): void {
    tabs.forEach((tab, i) => {
      const selected = i === index;
      tab.setAttribute("aria-selected", String(selected));
      tab.tabIndex = selected ? 0 : -1;
      const panel = panels[i];
      if (panel) panel.hidden = !selected;
    });
    if (focus) tabs[index]?.focus();
  }

  tabs.forEach((tab, i) => {
    tab.addEventListener("click", () => select(i, false));
    tab.addEventListener("keydown", (event) => {
      let next: number | null = null;
      if (event.key === "ArrowRight") next = (i + 1) % tabs.length;
      if (event.key === "ArrowLeft") next = (i - 1 + tabs.length) % tabs.length;
      if (event.key === "Home") next = 0;
      if (event.key === "End") next = tabs.length - 1;
      if (next !== null) {
        event.preventDefault();
        select(next, true);
      }
    });
  });

  const initial = tabs.findIndex(
    (tab) => tab.getAttribute("aria-selected") === "true",
  );
  select(initial === -1 ? 0 : initial, false);
}

for (const button of document.querySelectorAll<HTMLButtonElement>(
  ".copy-btn",
)) {
  setUpCopyButton(button);
}
// ---- 読めるもののカードの高さ ----

// 同じ行のカードは、閉じた状態の高さを行の中でそろえる。
// 並びの行の高さをそろえる（グリッドの stretch）と、1 枚の出力を開いたときに同じ行のほかのカードまで伸び、
// 説明文とコマンドの間が空く。そこで並びは align-items: start にし、閉じた高さの最大値を min-height で与える
function equalizeCardRows(list: HTMLElement): void {
  const cards = [...list.querySelectorAll<HTMLElement>(":scope > li")];
  for (const card of cards) card.style.minHeight = "";
  const rows = new Map<number, HTMLElement[]>();
  for (const card of cards) {
    const row = rows.get(card.offsetTop) ?? [];
    row.push(card);
    rows.set(card.offsetTop, row);
  }
  for (const row of rows.values()) {
    const tallest = Math.max(...row.map(closedHeight));
    for (const card of row) card.style.minHeight = `${tallest}px`;
  }
}

// カードの閉じた状態の高さ。出力を開いていれば、その中身の高さ分を引く
function closedHeight(card: HTMLElement): number {
  const pre = card.querySelector<HTMLElement>("details[open] > pre");
  if (!pre) return card.offsetHeight;
  const style = getComputedStyle(pre);
  return (
    card.offsetHeight -
    pre.offsetHeight -
    Number.parseFloat(style.marginTop) -
    Number.parseFloat(style.marginBottom)
  );
}

function setUpCardRows(list: HTMLElement): void {
  equalizeCardRows(list);
  // フォントが遅れて読まれると高さが変わるので、読み終わったら測り直す
  document.fonts?.ready.then(() => equalizeCardRows(list));
  let pending = 0;
  window.addEventListener("resize", () => {
    cancelAnimationFrame(pending);
    pending = requestAnimationFrame(() => equalizeCardRows(list));
  });
}

for (const tablist of document.querySelectorAll('[role="tablist"]')) {
  setUpTabs(tablist);
}
for (const list of document.querySelectorAll<HTMLElement>(".reads")) {
  setUpCardRows(list);
}
