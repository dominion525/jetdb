// デモのページ: jetdb（WebAssembly 版）でデータベースを開き、中身を見せる。
// 最初はサイトに置いたサンプルを開く。利用者が選んだファイルはブラウザの外に送らない。
import init, { Database } from "/assets/wasm/jetdb_wasm.js";

const SAMPLE_URL = "/assets/jetdb-sample.accdb";
const SAMPLE_NAME = "jetdb-sample.accdb";

const $ = (id) => document.getElementById(id);
const input = $("demo-file");
const status = $("demo-status");
const result = $("demo-result");
const passwordForm = $("demo-password");
const passwordInput = $("demo-password-input");

// 今開いているデータベースと、パスワードを待っているファイル
let db = null;
let pending = null;

// 開けたら、前のデータベースを解放して差し替える。パスワードが要るときは欄を出す
function open(bytes, label, name, password) {
  let next;
  try {
    next = Database.open(bytes, password);
  } catch (e) {
    if (e.code === "PASSWORD_REQUIRED" || e.code === "INVALID_PASSWORD") {
      pending = { bytes, label, name };
      passwordForm.hidden = false;
      passwordInput.value = "";
      passwordInput.focus();
      status.textContent =
        e.code === "PASSWORD_REQUIRED"
          ? `${name} is password-protected. Enter its password.`
          : "The password is wrong. Try again.";
      return;
    }
    status.textContent = `Could not open ${name}: ${e.message}`;
    return;
  }
  pending = null;
  passwordForm.hidden = true;
  db?.free();
  db = next;
  $("demo-current").textContent = label;
  showSummary(name);
  status.textContent = "";
}

// 種類ごとの数。読めなかった種類は — にする
function count(read) {
  try {
    return read();
  } catch {
    return null;
  }
}

function showSummary(name) {
  $("demo-name").textContent = name;
  $("demo-version").textContent = db.version();
  const counts = [
    ["table", "tables", () => db.tables().length],
    ["query", "queries", () => db.queries().length],
    ["form", "forms", () => db.forms().filter((f) => f.type === "Form").length],
    [
      "report",
      "reports",
      () => db.forms().filter((f) => f.type === "Report").length,
    ],
    ["macro", "macros", () => db.macros().length],
    ["module", "modules", () => db.modules().length],
    ["relationship", "relationships", () => db.relationships().length],
  ];
  $("demo-counts").replaceChildren(
    ...counts.map(([one, many, read]) => {
      const n = count(read);
      const li = document.createElement("li");
      const value = document.createElement("strong");
      value.textContent = n === null ? "—" : String(n);
      li.append(value, ` ${n === 1 ? one : many}`);
      return li;
    }),
  );
  result.hidden = false;
}

try {
  await init();
  input.disabled = false;
  status.textContent = "Opening the sample…";
  const response = await fetch(SAMPLE_URL);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  open(bytes, `Sample: ${SAMPLE_NAME}`, SAMPLE_NAME, null);
} catch (e) {
  status.textContent = `Could not load the demo: ${e.message}`;
}

input.addEventListener("change", async () => {
  const file = input.files[0];
  if (!file) return;
  status.textContent = `Opening ${file.name}…`;
  const bytes = new Uint8Array(await file.arrayBuffer());
  open(bytes, file.name, file.name, null);
});

passwordForm.addEventListener("submit", (event) => {
  event.preventDefault();
  if (!pending) return;
  open(pending.bytes, pending.label, pending.name, passwordInput.value);
});
