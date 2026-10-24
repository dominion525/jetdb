// デモのページ（/demo/ と /ja/demo/）の入り口: jetdb（WebAssembly 版）でデータベースを開き、中身を見せる。
// 最初はサイトに置いたサンプルを開く。利用者が選んだファイルはブラウザの外に送らない。
// 紹介ページの動きは src/main.ts にある

import type { Database } from "/assets/wasm/jetdb_wasm.js";
import { codeOf, el, messageOf } from "./dom.js";
import { MESSAGES } from "./messages.js";
import { type DetailKind, detailView, hashOf, type Kind } from "./views.js";

// ページの言語の文字
const T = MESSAGES[document.documentElement.lang === "ja" ? "ja" : "en"];

const SAMPLE_URL = "/assets/jetdb-sample.accdb";
const SAMPLE_NAME = "jetdb-sample.accdb";

function byId<E extends HTMLElement>(id: string): E {
  const element = document.getElementById(id);
  if (!element) throw new Error(`no #${id} on the page`);
  return element as E;
}
const input = byId<HTMLInputElement>("demo-file");
const status = byId("demo-status");
const result = byId("demo-result");
const passwordForm = byId<HTMLFormElement>("demo-password");
const passwordInput = byId<HTMLInputElement>("demo-password-input");
const systemToggle = byId<HTMLInputElement>("demo-system");
const nav = byId("demo-nav");
const detail = byId("demo-detail");

// jetdb-wasm。サイトの /assets/wasm/ から、起動の中で読む。
// ファイルの頭の import で読むと、読めなかったときにモジュールごと止まり、エラーの一文も出せないため
let jetdb: typeof import("/assets/wasm/jetdb_wasm.js") | null = null;

// 今開いているデータベースと、パスワードを待っているファイル
let db: Database | null = null;
let pending: { bytes: Uint8Array; label: string; name: string } | null = null;

// 左の一覧に出す種類。read は名前の一覧を返す（system はシステムや隠しのものも含めるか）
const KIND_READERS: {
  kind: Kind;
  read: (db: Database, system: boolean) => string[];
}[] = [
  { kind: "table", read: (db, system) => db.tables({ system }) },
  {
    kind: "query",
    read: (db, system) => db.queries({ system }).map((q) => q.name),
  },
  {
    kind: "form",
    read: (db) =>
      db
        .forms()
        .filter((f) => f.type === "Form")
        .map((f) => f.name),
  },
  {
    kind: "report",
    read: (db) =>
      db
        .forms()
        .filter((f) => f.type === "Report")
        .map((f) => f.name),
  },
  { kind: "macro", read: (db, system) => db.macros({ system }) },
  { kind: "module", read: (db) => db.modules().map((m) => m.name) },
  {
    kind: "relationship",
    read: (db, system) => db.relationships({ system }).map((r) => r.name),
  },
];

interface KindList {
  kind: Kind;
  one: string;
  many: string;
  title: string;
  names?: string[];
  error?: string;
}

// 種類ごとに名前の一覧を読む。読めなかった種類は、名前の代わりにエラーを持つ
function listObjects(db: Database, system: boolean): KindList[] {
  return KIND_READERS.map(({ kind, read }) => {
    const [one, many, title] = T.kinds[kind];
    try {
      return { kind, one, many, title, names: read(db, system) };
    } catch (e) {
      return { kind, one, many, title, error: messageOf(e) };
    }
  });
}

// 開けたら、前のデータベースを解放して差し替える。パスワードが要るときは欄を出す
function open(
  bytes: Uint8Array,
  label: string,
  name: string,
  password: string | null,
) {
  if (!jetdb) return;
  let next: Database;
  try {
    next = jetdb.Database.open(bytes, password);
  } catch (e) {
    const code = codeOf(e);
    if (code === "PASSWORD_REQUIRED" || code === "INVALID_PASSWORD") {
      pending = { bytes, label, name };
      passwordForm.hidden = false;
      passwordInput.value = "";
      passwordInput.focus();
      status.textContent =
        code === "PASSWORD_REQUIRED"
          ? T.passwordRequired(name)
          : T.passwordWrong;
      return;
    }
    status.textContent =
      code === "INVALID_FILE"
        ? T.invalidFile(name, messageOf(e))
        : T.openFailed(name, messageOf(e));
    return;
  }
  pending = null;
  passwordForm.hidden = true;
  db?.free();
  db = next;
  byId("demo-current").textContent = label;
  showSummary(next, name);
  showNav(next);
  selectFromHash({ replace: true });
  status.textContent = "";
}

function showSummary(db: Database, name: string) {
  byId("demo-name").textContent = name;
  byId("demo-version").textContent = db.version();
  byId("demo-counts").replaceChildren(
    ...listObjects(db, false).map((k) => {
      const li = document.createElement("li");
      const value = document.createElement("strong");
      if (!k.names) {
        value.textContent = "—";
        li.append(value, ` ${k.many}`);
      } else {
        const [count, noun] = T.count(k.names.length, k.one, k.many);
        value.textContent = count;
        li.append(value, noun);
      }
      return li;
    }),
  );
  result.hidden = false;
}

function navButton(kind: DetailKind, name: string | null, label: string) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "viewer-item";
  button.textContent = label;
  // 長い名前は 1 行で「…」と切るので、全部の名前を title に持たせる
  button.title = label;
  const hash = hashOf(kind, name);
  button.dataset.hash = hash;
  button.addEventListener("click", () => {
    if (location.hash !== hash) location.hash = hash;
  });
  return button;
}

// この数を超えるまとまりは、最初は閉じておく（選んでいるものがあれば開く）
const OPEN_LIMIT = 50;

function showNav(db: Database) {
  const kinds = listObjects(db, systemToggle.checked);
  const groups = kinds.map((k) => {
    const group = document.createElement("details");
    group.className = "viewer-group";
    const current = (k.names ?? []).some(
      (n) => hashOf(k.kind, n) === location.hash,
    );
    group.open = !k.names || k.names.length <= OPEN_LIMIT || current;
    const summary = document.createElement("summary");
    summary.textContent = `${k.title} (${k.names ? k.names.length : "—"})`;
    group.append(summary);
    if (k.error !== undefined || !k.names) {
      const p = document.createElement("p");
      p.className = "viewer-error";
      p.textContent = T.couldNotRead(k.error ?? "");
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
  nav.replaceChildren(
    navSelect(kinds),
    navButton("database", null, T.database),
    ...groups,
  );
  markCurrent();
}

// 狭い幅で一覧の代わりに出す選択の欄。種類ごとにまとめる
function navSelect(kinds: KindList[]) {
  const select = el(
    "select",
    {
      class: "viewer-select viewer-nav-select",
      "aria-label": T.objectToShow,
    },
    el("option", { value: "#database" }, T.database),
    kinds.flatMap(({ kind, title, names }) =>
      names && names.length > 0
        ? [
            el(
              "optgroup",
              { label: `${title} (${names.length})` },
              names.map((n) => el("option", { value: hashOf(kind, n) }, n)),
            ),
          ]
        : [],
    ),
  );
  select.addEventListener("change", () => {
    location.hash = select.value;
  });
  return select;
}

// 今選んでいるものに印を付ける
function markCurrent() {
  for (const button of nav.querySelectorAll<HTMLElement>(".viewer-item")) {
    if (button.dataset.hash === location.hash) {
      button.setAttribute("aria-current", "true");
      // 閉じているまとまりの中のものなら、まとまりを開く
      const group = button.closest("details");
      if (group) group.open = true;
    } else button.removeAttribute("aria-current");
  }
  const select = nav.querySelector<HTMLSelectElement>(".viewer-nav-select");
  if (select) select.value = location.hash;
}

// # のものを右に出す。開いたファイルに無いもの、# が無いときは Database を出す
function selectFromHash({ replace = false } = {}) {
  if (!db) return;
  const known = [...nav.querySelectorAll<HTMLElement>(".viewer-item")].some(
    (b) => b.dataset.hash === location.hash,
  );
  if (!known) {
    if (replace) history.replaceState(null, "", "#database");
    else location.hash = "#database";
  }
  const [kind, ...rest] = location.hash.slice(1).split("/");
  const name = rest.length ? decodeURIComponent(rest.join("/")) : "";
  markCurrent();
  detail.replaceChildren(
    ...detailView({ db, t: T }, kind as DetailKind, name).filter(
      (c): c is Node | string => c !== null && c !== undefined,
    ),
  );
}

window.addEventListener("hashchange", () => selectFromHash());
systemToggle.addEventListener("change", () => {
  if (!db) return;
  showNav(db);
  selectFromHash({ replace: true });
});

try {
  jetdb = await import("/assets/wasm/jetdb_wasm.js");
  await jetdb.default();
  input.disabled = false;
  status.textContent = T.openingSample;
  const response = await fetch(SAMPLE_URL);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  open(bytes, T.sample(SAMPLE_NAME), SAMPLE_NAME, null);
} catch (e) {
  status.textContent = T.loadFailed(messageOf(e));
}

input.addEventListener("change", async () => {
  const file = input.files?.[0];
  if (!file) return;
  status.textContent = T.opening(file.name);
  const bytes = new Uint8Array(await file.arrayBuffer());
  // 開いている間は画面が止まるので、先に「開いています」を描かせる
  await new Promise((resolve) =>
    requestAnimationFrame(() => setTimeout(resolve)),
  );
  open(bytes, file.name, file.name, null);
});

passwordForm.addEventListener("submit", (event) => {
  event.preventDefault();
  if (!pending) return;
  open(pending.bytes, pending.label, pending.name, passwordInput.value);
});
