// 紹介ページの小さな動き: コマンドのコピーボタン、インストール方法のタブ、ダミーのリンク。
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
// どのタブも aria-controls で中身を指している並びだけを扱う（デモのタブは demo.js が扱う）
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

// ---- まだ無いページへのダミーのリンク（公開の前に外す）----

function disableDummyLink(link: Element): void {
  link.addEventListener("click", (event) => event.preventDefault());
}

for (const button of document.querySelectorAll<HTMLButtonElement>(
  ".copy-btn",
)) {
  setUpCopyButton(button);
}
for (const tablist of document.querySelectorAll('[role="tablist"]')) {
  setUpTabs(tablist);
}
for (const link of document.querySelectorAll('a.dummy[aria-disabled="true"]')) {
  disableDummyLink(link);
}
