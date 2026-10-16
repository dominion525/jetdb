// デモのページ: jetdb（WebAssembly 版）でデータベースを開き、中身を見せる。
// 最初はサイトに置いたサンプルを開く。利用者が選んだファイルはブラウザの外に送らない。
import init, { Database } from "/assets/wasm/jetdb_wasm.js";
import { MESSAGES } from "/assets/demo-messages.js";

// ページの言語の文字
const T = MESSAGES[document.documentElement.lang === "ja" ? "ja" : "en"];

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
const KIND_READERS = [
  {
    kind: "table",
    read: (system) => db.tables({ system }),
  },
  {
    kind: "query",
    read: (system) => db.queries({ system }).map((q) => q.name),
  },
  {
    kind: "form",
    read: () =>
      db
        .forms()
        .filter((f) => f.type === "Form")
        .map((f) => f.name),
  },
  {
    kind: "report",
    read: () =>
      db
        .forms()
        .filter((f) => f.type === "Report")
        .map((f) => f.name),
  },
  {
    kind: "macro",
    read: (system) => db.macros({ system }),
  },
  {
    kind: "module",
    read: () => db.modules().map((m) => m.name),
  },
  {
    kind: "relationship",
    read: (system) => db.relationships({ system }).map((r) => r.name),
  },
];
const KINDS = KIND_READERS.map((k) => {
  const [one, many, title] = T.kinds[k.kind];
  return { ...k, one, many, title };
});
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
          ? T.passwordRequired(name)
          : T.passwordWrong;
      return;
    }
    status.textContent =
      e.code === "INVALID_FILE"
        ? T.invalidFile(name, e.message)
        : T.openFailed(name, e.message);
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
      if (n === null) {
        value.textContent = "—";
        li.append(value, ` ${k.many}`);
      } else {
        const [count, noun] = T.count(n, k.one, k.many);
        value.textContent = count;
        li.append(value, noun);
      }
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
  // 長い名前は 1 行で「…」と切るので、全部の名前を title に持たせる
  button.title = label;
  button.dataset.hash = hashOf(kind, name);
  button.addEventListener("click", () => {
    if (location.hash !== button.dataset.hash)
      location.hash = button.dataset.hash;
  });
  return button;
}

// この数を超えるまとまりは、最初は閉じておく（選んでいるものがあれば開く）
const OPEN_LIMIT = 50;

function showNav() {
  const kinds = listObjects(systemToggle.checked);
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
    if (k.error) {
      const p = document.createElement("p");
      p.className = "viewer-error";
      p.textContent = T.couldNotRead(k.error);
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
function navSelect(kinds) {
  const select = el(
    "select",
    {
      class: "viewer-select viewer-nav-select",
      "aria-label": T.objectToShow,
    },
    el("option", { value: "#database" }, T.database),
    kinds
      .filter((k) => k.names && k.names.length > 0)
      .map((k) =>
        el(
          "optgroup",
          { label: `${k.title} (${k.names.length})` },
          k.names.map((n) => el("option", { value: hashOf(k.kind, n) }, n)),
        ),
      ),
  );
  select.addEventListener("change", () => {
    location.hash = select.value;
  });
  return select;
}

// 今選んでいるものに印を付ける
function markCurrent() {
  for (const button of nav.querySelectorAll(".viewer-item")) {
    if (button.dataset.hash === location.hash) {
      button.setAttribute("aria-current", "true");
      // 閉じているまとまりの中のものなら、まとまりを開く
      const group = button.closest("details");
      if (group) group.open = true;
    } else button.removeAttribute("aria-current");
  }
  const select = nav.querySelector(".viewer-nav-select");
  if (select) select.value = location.hash;
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

// ---- 部品 ----

// 要素を作る。children は文字列か要素
function el(tag, attrs = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v);
  }
  e.append(...children.flat().filter((c) => c !== null && c !== undefined));
  return e;
}

// 読めなかったときに、その場に出す一文
const failure = (e) =>
  el("p", { class: "viewer-error" }, T.couldNotRead(e.message));

// 見出しの行と行の配列から表を作る。値は文字列か要素
function table(headers, rows) {
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
function saveButton(label, bytes, filename) {
  return el(
    "button",
    {
      type: "button",
      class: "viewer-save",
      onclick: () => {
        const url = URL.createObjectURL(new Blob([bytes]));
        const a = el("a", { href: url, download: filename });
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      },
    },
    label,
  );
}

const sizeOf = (n) =>
  n < 1024
    ? T.bytes(n)
    : n < 1024 * 1024
      ? `${(n / 1024).toFixed(1)} KB`
      : `${(n / 1024 / 1024).toFixed(1)} MB`;

// 添付ファイル・版の履歴・複数の値を見分ける
const isAttachment = (v) =>
  v &&
  typeof v === "object" &&
  !(v instanceof Uint8Array) &&
  "data" in v &&
  "name" in v;
const isVersion = (v) =>
  v &&
  typeof v === "object" &&
  !(v instanceof Uint8Array) &&
  "modified" in v &&
  "value" in v;

// 1 つの値を出す
function value(v) {
  if (v === null || v === undefined)
    return el("span", { class: "viewer-null" }, "NULL");
  if (typeof v === "boolean") return v ? "true" : "false";
  if (typeof v === "bigint" || typeof v === "number") return String(v);
  if (typeof v === "string") return v;
  if (v instanceof Uint8Array)
    return el(
      "span",
      { class: "viewer-binary" },
      T.binary(sizeOf(v.length)),
    );
  if (Array.isArray(v)) return complex(v);
  return JSON.stringify(v);
}

// 複合列の値: 件数を出し、開くと中身を見られる
function complex(items) {
  if (items.length === 0)
    return el("span", { class: "viewer-null" }, T.noneValue);
  const files = items.every(isAttachment);
  const versions = items.every(isVersion);
  const noun = files
    ? T.complexNouns.file
    : versions
      ? T.complexNouns.version
      : T.complexNouns.value;
  const list = el(
    "ul",
    { class: "viewer-complex-list" },
    items.map((item) => {
      if (files) {
        const name = item.name ?? T.noName;
        return el(
          "li",
          {},
          name,
          item.data
            ? [
                ` (${sizeOf(item.data.length)}) `,
                saveButton(T.save, item.data, name),
              ]
            : T.notDecoded,
        );
      }
      if (versions)
        return el(
          "li",
          {},
          item.modified ?? T.noDate,
          ": ",
          value(item.value),
        );
      return el("li", {}, value(item));
    }),
  );
  return el(
    "details",
    { class: "viewer-complex" },
    el(
      "summary",
      {},
      T.complexCount(items.length, noun[0], noun[1]),
    ),
    list,
  );
}

// タブ。tabs は [名前, 中身を作る関数]。中身は選んだときに作る
function tabs(items) {
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
  function select(i) {
    buttons.forEach((b, j) => b.setAttribute("aria-selected", String(i === j)));
    let content;
    try {
      content = items[i][1]();
    } catch (e) {
      content = failure(e);
    }
    panel.replaceChildren(content);
  }
  const bar = el(
    "div",
    { class: "tabs viewer-tabs", role: "tablist" },
    buttons,
  );
  bar.addEventListener("keydown", (event) => {
    const i = buttons.indexOf(document.activeElement);
    if (i < 0) return;
    const next =
      event.key === "ArrowRight"
        ? (i + 1) % buttons.length
        : event.key === "ArrowLeft"
          ? (i - 1 + buttons.length) % buttons.length
          : null;
    if (next === null) return;
    event.preventDefault();
    buttons[next].focus();
    select(next);
  });
  select(0);
  return [bar, panel];
}

// ---- テーブル ----

const PAGE_SIZE = 100;

// 100 行ずつ読む。starts[i] は i ページ目を読み始める位置。最後まで読んだら総数がわかる
function tableData(name) {
  const box = el("div");
  const starts = [undefined];
  let total = null;
  function show(page) {
    const rows = db.rows(name, { start: starts[page], limit: PAGE_SIZE });
    if (rows.next !== null) starts[page + 1] = rows.next;
    else total = page * PAGE_SIZE + rows.rows.length;
    const first = page * PAGE_SIZE + 1;
    const last = page * PAGE_SIZE + rows.rows.length;
    const range =
      rows.rows.length === 0
        ? T.noRows
        : T.rows(first, last, total);
    const pager = el(
      "div",
      { class: "viewer-pager" },
      el("span", {}, range),
      el(
        "button",
        {
          type: "button",
          class: "viewer-save",
          onclick: () => show(page - 1),
          ...(page === 0 ? { disabled: "" } : {}),
        },
        T.previous,
      ),
      el(
        "button",
        {
          type: "button",
          class: "viewer-save",
          onclick: () => show(page + 1),
          ...(rows.next === null ? { disabled: "" } : {}),
        },
        T.next,
      ),
    );
    const skipped =
      rows.skipped > 0
        ? el(
            "p",
            { class: "note" },
            T.skipped(rows.skipped),
          )
        : null;
    box.replaceChildren(
      pager,
      skipped ?? "",
      table(
        rows.columns,
        rows.rows.map((row) => rows.columns.map((c) => value(row[c]))),
      ),
    );
  }
  show(0);
  return box;
}

function tableColumns(name) {
  return table(
    T.columnHeads,
    db
      .columns(name)
      .map((c) => [
        c.name,
        c.type === "Numeric" ? `Numeric(${c.precision}, ${c.scale})` : c.type,
        String(c.size),
        [c.autoNumber && T.autoNumber, c.calculated && T.calculated]
          .filter(Boolean)
          .join(", "),
      ]),
  );
}

function tableIndexes(name) {
  const indexes = db.indexes(name);
  if (indexes.length === 0)
    return el("p", { class: "note" }, T.noIndexes);
  return table(
    T.indexHeads,
    indexes.map((ix) => [
      ix.name,
      ix.columns
        .map((c) => `${c.name} ${c.descending ? "DESC" : "ASC"}`)
        .join(", "),
      [
        ix.primaryKey && T.primaryKey,
        ix.unique && T.unique,
        ix.ignoreNulls && T.ignoreNulls,
        ix.required && T.required,
      ]
        .filter(Boolean)
        .join(", "),
    ]),
  );
}

// プロパティ: オブジェクト自体のものと、列ごと・追加のまとまり
function propertiesView(name, type) {
  const p = db.properties(name, { type });
  const group = (title, props) =>
    el(
      "section",
      { class: "viewer-props" },
      el("h3", {}, title),
      props.length === 0
        ? el("p", { class: "note" }, T.none)
        : table(
            T.propertyHeads,
            props.map((x) => [x.name, value(x.value)]),
          ),
    );
  return el(
    "div",
    {},
    group(T.objectProperties(T.typeNames[type]), p.object),
    p.columns.map((c) => group(T.columnGroup(c.name), c.properties)),
    p.additional.map((g) => group(g.name, g.properties)),
  );
}

function showTable(name) {
  return tabs([
    [T.tabs.data, () => tableData(name)],
    [T.tabs.columns, () => tableColumns(name)],
    [T.tabs.indexes, () => tableIndexes(name)],
    [T.tabs.properties, () => propertiesView(name, "Table")],
    [T.tabs.dataMacros, () => tableDataMacros(name)],
    [T.tabs.ddl, () => ddlView(name)],
  ]);
}

function tableDataMacros(name) {
  const text = db.dataMacrosText(name);
  if (text === "")
    return el("p", { class: "note" }, T.noDataMacros);
  return textView(text, fileName(`${name}-data-macros`, "xml"));
}

// ---- DDL ----

const DIALECTS = [
  ["sqlite", "SQLite"],
  ["postgres", "PostgreSQL"],
  ["mysql", "MySQL"],
  ["access", "Access SQL"],
];

// 方言を選んで DDL を出す。table が無ければ全部のテーブル
function ddlView(table) {
  const box = el("div");
  const select = el(
    "select",
    { class: "viewer-select", "aria-label": T.dialectLabel },
    DIALECTS.map(([v, label]) => el("option", { value: v }, label)),
  );
  function show() {
    const dialect = select.value;
    let body;
    try {
      const sql = db.ddl(dialect, table ? { table } : {});
      body = textView(
        sql,
        fileName(`${table ?? "database"}-${dialect}`, "sql"),
      );
    } catch (e) {
      body = failure(e);
    }
    box.replaceChildren(
      el("label", { class: "viewer-dialect" }, T.dialect, select),
      body,
    );
  }
  select.addEventListener("change", show);
  show();
  return box;
}

// ---- リレーションシップ ----

function showRelationship(name) {
  const r = db.relationships({ system: true }).find((x) => x.name === name);
  if (!r) throw new Error(`no relationship named ${name}`);
  const tableLink = (t) => el("a", { href: hashOf("table", t) }, t);
  return [
    el(
      "p",
      { class: "viewer-relation" },
      tableLink(r.fromTable),
      " → ",
      tableLink(r.toTable),
    ),
    table(
      [T.relationColumn(r.fromTable), T.relationColumn(r.toTable)],
      r.columns.map((c) => [c.from, c.to]),
    ),
    el(
      "ul",
      { class: "viewer-flags" },
      [
        [T.referentialIntegrity, r.referentialIntegrity],
        [T.cascadeUpdate, r.cascadeUpdate],
        [T.cascadeDelete, r.cascadeDelete],
      ].map(([label, on]) => el("li", {}, `${label}: ${on ? T.yes : T.no}`)),
    ),
  ];
}

// ---- データベース全体 ----

// データベースのプロパティ: MSysDb、SummaryInfo（タイトル・作成者など）、UserDefined
function databaseProperties() {
  const groups = [
    [T.databaseProperties, () => db.properties("MSysDb").object],
    [
      T.summaryInfo,
      () => db.properties("SummaryInfo", { type: "DatabaseProperty" }).object,
    ],
    [
      T.userDefined,
      () => db.properties("UserDefined", { type: "DatabaseProperty" }).object,
    ],
  ];
  return el(
    "div",
    {},
    groups.map(([title, read]) => {
      let content;
      try {
        content = propertyTable(read());
      } catch (e) {
        content =
          e.code === "OBJECT_NOT_FOUND"
            ? el("p", { class: "note" }, T.none)
            : failure(e);
      }
      return el(
        "section",
        { class: "viewer-props" },
        el("h3", {}, title),
        content,
      );
    }),
  );
}

function showDatabase() {
  return tabs([
    [T.tabs.ddl, () => ddlView(null)],
    [T.tabs.properties, () => databaseProperties()],
  ]);
}

// ---- テキスト（SQL・VBA・マクロ）----

// テキストを等幅で出し、コピーとファイルとして保存のボタンを付ける
function textView(text, filename) {
  const copy = el(
    "button",
    {
      type: "button",
      class: "viewer-save",
      onclick: async () => {
        try {
          await navigator.clipboard.writeText(text);
          copy.textContent = T.copied;
        } catch {
          copy.textContent = T.copyFailed;
        }
        setTimeout(() => (copy.textContent = T.copy), 2000);
      },
    },
    T.copy,
  );
  const save = saveButton(T.save, new TextEncoder().encode(text), filename);
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
      el("label", { class: "viewer-wrap" }, wrapBox, T.wrapLines),
      copy,
      save,
    ),
    pre,
  );
}

// ファイル名に使えない文字を _ にする
const fileName = (name, ext) => `${name.replace(/[\\/:*?"<>|]/g, "_")}.${ext}`;

function showQuery(name) {
  return tabs([
    [T.tabs.sql, () => textView(db.querySql(name), fileName(name, "sql"))],
    [T.tabs.properties, () => propertiesView(name, "Query")],
  ]);
}

function showModule(name) {
  const module = db.modules().find((m) => m.name === name);
  const ext = module?.type === "Standard" ? "bas" : "cls";
  return [textView(db.moduleSource(name), fileName(name, ext))];
}

function showMacro(name) {
  const macro = db.macro(name);
  if (macro.text === null)
    return [el("p", { class: "note" }, T.macroNoText)];
  return [textView(macro.text, fileName(name, "txt"))];
}

// ---- フォーム/レポート ----

const propertyTable = (props) =>
  props.length === 0
    ? el("p", { class: "note" }, T.none)
    : table(
        T.propertyHeads,
        props.map((p) => [p.name, value(p.value)]),
      );

function formControls(design) {
  if (design.controls.length === 0)
    return el("p", { class: "note" }, T.noControls);
  return el(
    "div",
    { class: "viewer-controls" },
    design.controls.map((c) =>
      el(
        "details",
        { class: "viewer-control" },
        el("summary", {}, el("strong", {}, c.name), ` (${c.type})`),
        propertyTable(c.properties),
      ),
    ),
  );
}

// 埋め込みマクロ: どのコントロールのどのイベントのものかを見出しにする
function formMacros(name, type) {
  const macros = db.embeddedMacros(name, { type });
  if (macros.length === 0)
    return el("p", { class: "note" }, T.noEmbeddedMacros);
  return el(
    "div",
    {},
    macros.map((m) => {
      const where = [m.source.control ?? name, m.source.event]
        .filter(Boolean)
        .join(".");
      return el(
        "section",
        { class: "viewer-props" },
        el("h3", {}, where),
        m.text === null
          ? el("p", { class: "note" }, T.embeddedNoXml)
          : textView(m.text, fileName(where, "txt")),
      );
    }),
  );
}

// フォームのモジュールがあれば、それを開くリンクを出す
function formModule(name, type) {
  const moduleName = `${type}_${name}`;
  if (!db.modules().some((m) => m.name === moduleName))
    return el("p", { class: "note" }, T.noModule(type));
  return el(
    "p",
    {},
    T.moduleLink(el("a", { href: hashOf("module", moduleName) }, moduleName)),
  );
}

function showForm(name, type) {
  const design = db.formProperties(name, { type });
  return tabs([
    [T.tabs.properties, () => propertyTable(design.properties)],
    [T.tabs.controls, () => formControls(design)],
    [T.tabs.embeddedMacros, () => formMacros(name, type)],
    [T.tabs.module, () => formModule(name, type)],
  ]);
}

// 種類の下に添える一文（クエリとモジュールは、その種類も）
function subtitle(kind, name) {
  if (kind === "database") return T.wholeDatabase;
  if (kind === "query") {
    const q = db.queries({ system: true }).find((x) => x.name === name);
    return q ? `${KIND_LABELS.query} · ${q.type}` : KIND_LABELS.query;
  }
  if (kind === "module") {
    const m = db.modules().find((x) => x.name === name);
    return m ? `${KIND_LABELS.module} · ${m.type}` : KIND_LABELS.module;
  }
  return KIND_LABELS[kind];
}

const RENDERERS = {
  table: showTable,
  query: showQuery,
  module: showModule,
  macro: showMacro,
  form: (name) => showForm(name, "Form"),
  report: (name) => showForm(name, "Report"),
  relationship: showRelationship,
  database: showDatabase,
};

// ---- 右の中身 ----

function showDetail(kind, name) {
  const heading = el(
    "h2",
    { class: "viewer-title" },
    kind === "database" ? T.database : name,
  );
  let type;
  let body;
  try {
    type = el("p", { class: "viewer-type" }, subtitle(kind, name));
    body = RENDERERS[kind](name);
  } catch (e) {
    type ??= el("p", { class: "viewer-type" }, KIND_LABELS[kind] ?? "");
    body = [failure(e)];
  }
  detail.replaceChildren(heading, type, ...body);
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
  status.textContent = T.openingSample;
  const response = await fetch(SAMPLE_URL);
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  open(bytes, T.sample(SAMPLE_NAME), SAMPLE_NAME, null);
} catch (e) {
  status.textContent = T.loadFailed(e.message);
}

input.addEventListener("change", async () => {
  const file = input.files[0];
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
