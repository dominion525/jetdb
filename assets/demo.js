// デモのページ: 選んだファイルをブラウザの中の jetdb（WebAssembly 版）で開き、
// エンジンの版とテーブルの一覧を出す。ファイルはブラウザの外に送らない。
import init, { Database } from "/assets/wasm/jetdb_wasm.js";

const MESSAGES = {
  ja: {
    loading: "jetdb を読み込んでいます…",
    ready: "ファイルを選んでください。",
    initFailed: "jetdb を読み込めませんでした: ",
    opening: "開いています…",
    opened: "開きました。",
    openFailed: "開けませんでした: ",
    noTables: "テーブルはありません。",
  },
  en: {
    loading: "Loading jetdb…",
    ready: "Choose a file.",
    initFailed: "Could not load jetdb: ",
    opening: "Opening…",
    opened: "Opened.",
    openFailed: "Could not open the file: ",
    noTables: "There are no tables.",
  },
};

const lang = document.documentElement.lang.startsWith("ja") ? "ja" : "en";
const t = MESSAGES[lang];
const $ = (id) => document.getElementById(id);

const input = $("demo-file");
const status = $("demo-status");
const result = $("demo-result");

try {
  await init();
  input.disabled = false;
  status.textContent = t.ready;
} catch (e) {
  status.textContent = t.initFailed + e.message;
}

input.addEventListener("change", async () => {
  const file = input.files[0];
  if (!file) return;
  result.hidden = true;
  status.textContent = t.opening;
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const db = Database.open(bytes);
    // 読み取りに失敗しても、Wasm のメモリにあるファイルの写しを解放する
    try {
      const tables = db.tables();
      $("demo-name").textContent = file.name;
      $("demo-version").textContent = db.version();
      $("demo-count").textContent = String(tables.length);
      const items = tables.map((name) => {
        const li = document.createElement("li");
        li.textContent = name;
        return li;
      });
      if (items.length === 0) {
        const li = document.createElement("li");
        li.textContent = t.noTables;
        items.push(li);
      }
      $("demo-tables").replaceChildren(...items);
    } finally {
      db.free();
    }
    result.hidden = false;
    status.textContent = t.opened;
  } catch (e) {
    status.textContent = t.openFailed + e.message;
  }
});
