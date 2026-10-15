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
const systemToggle = $("demo-system");
const nav = $("demo-nav");
const detail = $("demo-detail");

// 今開いているデータベースと、パスワードを待っているファイル
let db = null;
let pending = null;

// 左の一覧に出す種類。read は名前の一覧を返す（system はシステムや隠しのものも含めるか）
const KINDS = [
  {
    kind: "table",
    one: "table",
    many: "tables",
    title: "Tables",
    read: (system) => db.tables({ system }),
  },
  {
    kind: "query",
    one: "query",
    many: "queries",
    title: "Queries",
    read: (system) => db.queries({ system }).map((q) => q.name),
  },
  {
    kind: "form",
    one: "form",
    many: "forms",
    title: "Forms",
    read: () =>
      db
        .forms()
        .filter((f) => f.type === "Form")
        .map((f) => f.name),
  },
  {
    kind: "report",
    one: "report",
    many: "reports",
    title: "Reports",
    read: () =>
      db
        .forms()
        .filter((f) => f.type === "Report")
        .map((f) => f.name),
  },
  {
    kind: "macro",
    one: "macro",
    many: "macros",
    title: "Macros",
    read: (system) => db.macros({ system }),
  },
  {
    kind: "module",
    one: "module",
    many: "modules",
    title: "Modules",
    read: () => db.modules().map((m) => m.name),
  },
  {
    kind: "relationship",
    one: "relationship",
    many: "relationships",
    title: "Relationships",
    read: (system) => db.relationships({ system }).map((r) => r.name),
  },
];
const KIND_LABELS = Object.fromEntries(KINDS.map((k) => [k.kind, k.one]));

// 種類ごとに名前の一覧を読む。読めなかった種類は、名前の代わりにエラーを持つ
function listObjects(system) {
  return KINDS.map((k) => {
    try {
      return { ...k, names: k.read(system) };
    } catch (e) {
      return { ...k, error: e.message };
    }
  });
}

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
  showNav();
  selectFromHash({ replace: true });
  status.textContent = "";
}

function showSummary(name) {
  $("demo-name").textContent = name;
  $("demo-version").textContent = db.version();
  $("demo-counts").replaceChildren(
    ...listObjects(false).map((k) => {
      const n = k.names ? k.names.length : null;
      const li = document.createElement("li");
      const value = document.createElement("strong");
      value.textContent = n === null ? "—" : String(n);
      li.append(value, ` ${n === 1 ? k.one : k.many}`);
      return li;
    }),
  );
  result.hidden = false;
}

// URL の # の形: #database、#table/<名前> など
const hashOf = (kind, name) =>
  kind === "database" ? "#database" : `#${kind}/${encodeURIComponent(name)}`;

function navButton(kind, name, label) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "viewer-item";
  button.textContent = label;
  button.dataset.hash = hashOf(kind, name);
  button.addEventListener("click", () => {
    if (location.hash !== button.dataset.hash)
      location.hash = button.dataset.hash;
  });
  return button;
}

function showNav() {
  const groups = listObjects(systemToggle.checked).map((k) => {
    const group = document.createElement("details");
    group.className = "viewer-group";
    group.open = true;
    const summary = document.createElement("summary");
    summary.textContent = `${k.title} (${k.names ? k.names.length : "—"})`;
    group.append(summary);
    if (k.error) {
      const p = document.createElement("p");
      p.className = "viewer-error";
      p.textContent = `Could not read: ${k.error}`;
      group.append(p);
    } else {
      const ul = document.createElement("ul");
      for (const name of k.names) {
        const li = document.createElement("li");
        li.append(navButton(k.kind, name, name));
        ul.append(li);
      }
      group.append(ul);
    }
    return group;
  });
  nav.replaceChildren(navButton("database", null, "Database"), ...groups);
  markCurrent();
}

// 今選んでいるものに印を付ける
function markCurrent() {
  for (const button of nav.querySelectorAll(".viewer-item")) {
    if (button.dataset.hash === location.hash)
      button.setAttribute("aria-current", "true");
    else button.removeAttribute("aria-current");
  }
}

// # のものを右に出す。開いたファイルに無いもの、# が無いときは Database を出す
function selectFromHash({ replace = false } = {}) {
  if (!db) return;
  const known = [...nav.querySelectorAll(".viewer-item")].some(
    (b) => b.dataset.hash === location.hash,
  );
  if (!known) {
    if (replace) history.replaceState(null, "", "#database");
    else location.hash = "#database";
  }
  const [kind, ...rest] = location.hash.slice(1).split("/");
  const name = rest.length ? decodeURIComponent(rest.join("/")) : null;
  markCurrent();
  showDetail(kind, name);
}

function showDetail(kind, name) {
  const heading = document.createElement("h2");
  heading.className = "viewer-title";
  const type = document.createElement("p");
  type.className = "viewer-type";
  if (kind === "database") {
    heading.textContent = "Database";
    type.textContent = "The whole database";
  } else {
    heading.textContent = name;
    type.textContent = KIND_LABELS[kind];
  }
  const note = document.createElement("p");
  note.className = "note";
  note.textContent = "The contents come in a later step of this page.";
  detail.replaceChildren(heading, type, note);
}

window.addEventListener("hashchange", () => selectFromHash());
systemToggle.addEventListener("change", () => {
  if (!db) return;
  showNav();
  selectFromHash({ replace: true });
});

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
