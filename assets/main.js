document.documentElement.classList.add("js");

const MESSAGES = {
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

const lang = document.documentElement.lang.startsWith("ja") ? "ja" : "en";
const t = MESSAGES[lang];

// コマンドのコピーボタン
const copyStatus = document.querySelector(".copy-status");

function announce(message) {
  if (copyStatus) {
    copyStatus.textContent = "";
    copyStatus.textContent = message;
  }
}

function selectText(element) {
  const range = document.createRange();
  range.selectNodeContents(element);
  const selection = window.getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
}

for (const button of document.querySelectorAll(".copy-btn")) {
  const code = button.parentElement.querySelector("pre code");
  if (!code) continue;
  button.hidden = false;
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(code.textContent);
      button.textContent = t.copied;
      announce(t.copied);
    } catch {
      selectText(code);
      button.textContent = t.copyFailed;
      announce(t.copyFailed);
    }
    setTimeout(() => {
      button.textContent = t.copy;
    }, 2000);
  });
}

// インストール方法のタブ
for (const tablist of document.querySelectorAll('[role="tablist"]')) {
  const tabs = [...tablist.querySelectorAll('[role="tab"]')];
  const panels = tabs.map((tab) =>
    document.getElementById(tab.getAttribute("aria-controls")),
  );

  function select(index, focus) {
    tabs.forEach((tab, i) => {
      const selected = i === index;
      tab.setAttribute("aria-selected", String(selected));
      tab.tabIndex = selected ? 0 : -1;
      panels[i].hidden = !selected;
    });
    if (focus) tabs[index].focus();
  }

  tabs.forEach((tab, i) => {
    tab.addEventListener("click", () => select(i, false));
    tab.addEventListener("keydown", (event) => {
      let next = null;
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

// まだ無いページへのダミーのリンク（公開の前に外す）
for (const link of document.querySelectorAll('a.dummy[aria-disabled="true"]')) {
  link.addEventListener("click", (event) => event.preventDefault());
}
