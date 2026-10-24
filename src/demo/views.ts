// デモのページ: 右に出す中身を、オブジェクトの種類ごとに作る。
// 開いているデータベースと文字の表は、ViewContext として引数でもらう

import type {
  Database,
  DdlDialect,
  FormProperties,
  FormType,
  Property,
} from "/assets/wasm/jetdb_wasm.js";
import {
  type Child,
  codeOf,
  el,
  failure,
  fileName,
  table,
  tabs,
  textView,
  value,
} from "./dom.js";
import type { Messages } from "./messages.js";

/** 左の一覧に出す種類 */
export type Kind =
  | "table"
  | "query"
  | "form"
  | "report"
  | "macro"
  | "module"
  | "relationship";
/** 右に出せるもの: 種類のどれかか、データベース全体 */
export type DetailKind = Kind | "database";

export interface ViewContext {
  db: Database;
  t: Messages;
}

// URL の # の形: #database、#table/<名前> など
export const hashOf = (kind: DetailKind, name: string | null) =>
  kind === "database"
    ? "#database"
    : `#${kind}/${encodeURIComponent(name ?? "")}`;

const note = (text: string) => el("p", { class: "note" }, text);

// ---- テーブル ----

const PAGE_SIZE = 100;

// 100 行ずつ読む。starts[i] は i ページ目を読み始める位置。最後まで読んだら総数がわかる
export function tableData({ db, t }: ViewContext, name: string) {
  const box = el("div");
  const starts: (number | undefined)[] = [undefined];
  let total: number | null = null;
  function show(page: number) {
    const start = starts[page];
    const rows = db.rows(name, {
      ...(start === undefined ? {} : { start }),
      limit: PAGE_SIZE,
    });
    if (rows.next !== null) starts[page + 1] = rows.next;
    else total = page * PAGE_SIZE + rows.rows.length;
    const first = page * PAGE_SIZE + 1;
    const last = page * PAGE_SIZE + rows.rows.length;
    const range =
      rows.rows.length === 0 ? t.noRows : t.rows(first, last, total);
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
        t.previous,
      ),
      el(
        "button",
        {
          type: "button",
          class: "viewer-save",
          onclick: () => show(page + 1),
          ...(rows.next === null ? { disabled: "" } : {}),
        },
        t.next,
      ),
    );
    const skipped = rows.skipped > 0 ? note(t.skipped(rows.skipped)) : null;
    box.replaceChildren(
      pager,
      skipped ?? "",
      table(
        rows.columns,
        rows.rows.map((row) => rows.columns.map((c) => value(t, row[c]))),
      ),
    );
  }
  show(0);
  return box;
}

export function tableColumns({ db, t }: ViewContext, name: string) {
  return table(
    t.columnHeads,
    db
      .columns(name)
      .map((c) => [
        c.name,
        c.type === "Numeric" ? `Numeric(${c.precision}, ${c.scale})` : c.type,
        String(c.size),
        [c.autoNumber && t.autoNumber, c.calculated && t.calculated]
          .filter(Boolean)
          .join(", "),
      ]),
  );
}

export function tableIndexes({ db, t }: ViewContext, name: string) {
  const indexes = db.indexes(name);
  if (indexes.length === 0) return note(t.noIndexes);
  return table(
    t.indexHeads,
    indexes.map((ix) => [
      ix.name,
      ix.columns
        .map((c) => `${c.name} ${c.descending ? "DESC" : "ASC"}`)
        .join(", "),
      [
        ix.primaryKey && t.primaryKey,
        ix.unique && t.unique,
        ix.ignoreNulls && t.ignoreNulls,
        ix.required && t.required,
      ]
        .filter(Boolean)
        .join(", "),
    ]),
  );
}

const propertyTable = (t: Messages, props: Property[]) =>
  props.length === 0
    ? note(t.none)
    : table(
        t.propertyHeads,
        props.map((p) => [p.name, value(t, p.value)]),
      );

// プロパティ: オブジェクト自体のものと、列ごと・追加のまとまり
export function propertiesView(
  { db, t }: ViewContext,
  name: string,
  type: "Table" | "Query",
) {
  const p = db.properties(name, { type });
  const group = (title: string, props: Property[]) =>
    el(
      "section",
      { class: "viewer-props" },
      el("h3", {}, title),
      propertyTable(t, props),
    );
  return el(
    "div",
    {},
    group(t.objectProperties(t.typeNames[type]), p.object),
    p.columns.map((c) => group(t.columnGroup(c.name), c.properties)),
    p.additional.map((g) => group(g.name, g.properties)),
  );
}

export function tableDataMacros({ db, t }: ViewContext, name: string) {
  const text = db.dataMacrosText(name);
  if (text === "") return note(t.noDataMacros);
  return textView(t, text, fileName(`${name}-data-macros`, "xml"));
}

// CSV: jetdb export と同じ形（既定のオプション）
export function tableCsv({ db, t }: ViewContext, name: string) {
  return textView(t, db.csv(name), fileName(name, "csv"));
}

function showTable(ctx: ViewContext, name: string) {
  const { t } = ctx;
  return tabs(t, [
    [t.tabs.data, () => tableData(ctx, name)],
    [t.tabs.csv, () => tableCsv(ctx, name)],
    [t.tabs.columns, () => tableColumns(ctx, name)],
    [t.tabs.indexes, () => tableIndexes(ctx, name)],
    [t.tabs.properties, () => propertiesView(ctx, name, "Table")],
    [t.tabs.dataMacros, () => tableDataMacros(ctx, name)],
    [t.tabs.ddl, () => ddlView(ctx, name)],
  ]);
}

// ---- DDL ----

const DIALECTS: [DdlDialect, string][] = [
  ["sqlite", "SQLite"],
  ["postgres", "PostgreSQL"],
  ["mysql", "MySQL"],
  ["access", "Access SQL"],
];

// 方言を選んで DDL を出す。table が無ければ全部のテーブル
export function ddlView({ db, t }: ViewContext, tableName: string | null) {
  const box = el("div");
  const select = el(
    "select",
    { class: "viewer-select", "aria-label": t.dialectLabel },
    DIALECTS.map(([v, label]) => el("option", { value: v }, label)),
  );
  function show() {
    const dialect = DIALECTS.find(([v]) => v === select.value)?.[0] ?? "sqlite";
    let body: Node;
    try {
      const sql = db.ddl(dialect, tableName ? { table: tableName } : {});
      body = textView(
        t,
        sql,
        fileName(`${tableName ?? "database"}-${dialect}`, "sql"),
      );
    } catch (e) {
      body = failure(t, e);
    }
    box.replaceChildren(
      el("label", { class: "viewer-dialect" }, t.dialect, select),
      body,
    );
  }
  select.addEventListener("change", show);
  show();
  return box;
}

// ---- リレーションシップ ----

export function showRelationship({ db, t }: ViewContext, name: string) {
  const r = db.relationships({ system: true }).find((x) => x.name === name);
  if (!r) throw new Error(`no relationship named ${name}`);
  const tableLink = (n: string) => el("a", { href: hashOf("table", n) }, n);
  return [
    el(
      "p",
      { class: "viewer-relation" },
      tableLink(r.fromTable),
      " → ",
      tableLink(r.toTable),
    ),
    table(
      [t.relationColumn(r.fromTable), t.relationColumn(r.toTable)],
      r.columns.map((c) => [c.from, c.to]),
    ),
    el(
      "ul",
      { class: "viewer-flags" },
      (
        [
          [t.referentialIntegrity, r.referentialIntegrity],
          [t.cascadeUpdate, r.cascadeUpdate],
          [t.cascadeDelete, r.cascadeDelete],
        ] as const
      ).map(([label, on]) => el("li", {}, `${label}: ${on ? t.yes : t.no}`)),
    ),
  ];
}

// ---- データベース全体 ----

// データベースのプロパティ: MSysDb、SummaryInfo（タイトル・作成者など）、UserDefined
export function databaseProperties({ db, t }: ViewContext) {
  const groups: [string, () => Property[]][] = [
    [t.databaseProperties, () => db.properties("MSysDb").object],
    [
      t.summaryInfo,
      () => db.properties("SummaryInfo", { type: "DatabaseProperty" }).object,
    ],
    [
      t.userDefined,
      () => db.properties("UserDefined", { type: "DatabaseProperty" }).object,
    ],
  ];
  return el(
    "div",
    {},
    groups.map(([title, read]) => {
      let content: Node;
      try {
        content = propertyTable(t, read());
      } catch (e) {
        content =
          codeOf(e) === "OBJECT_NOT_FOUND" ? note(t.none) : failure(t, e);
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

function showDatabase(ctx: ViewContext) {
  const { t } = ctx;
  return tabs(t, [
    [t.tabs.ddl, () => ddlView(ctx, null)],
    [t.tabs.properties, () => databaseProperties(ctx)],
  ]);
}

// ---- テキスト（SQL・VBA・マクロ）----

function showQuery(ctx: ViewContext, name: string) {
  const { db, t } = ctx;
  return tabs(t, [
    [t.tabs.sql, () => textView(t, db.querySql(name), fileName(name, "sql"))],
    [t.tabs.properties, () => propertiesView(ctx, name, "Query")],
  ]);
}

function showModule({ db, t }: ViewContext, name: string) {
  const module = db.modules().find((m) => m.name === name);
  const ext = module?.type === "Standard" ? "bas" : "cls";
  return [textView(t, db.moduleSource(name), fileName(name, ext))];
}

function showMacro({ db, t }: ViewContext, name: string) {
  const macro = db.macro(name);
  if (macro.text === null) return [note(t.macroNoText)];
  return [textView(t, macro.text, fileName(name, "txt"))];
}

// ---- フォーム/レポート ----

export function formControls(t: Messages, design: FormProperties) {
  if (design.controls.length === 0) return note(t.noControls);
  return el(
    "div",
    { class: "viewer-controls" },
    design.controls.map((c) =>
      el(
        "details",
        { class: "viewer-control" },
        el("summary", {}, el("strong", {}, c.name), ` (${c.type})`),
        propertyTable(t, c.properties),
      ),
    ),
  );
}

// 埋め込みマクロ: どのコントロールのどのイベントのものかを見出しにする
export function formMacros(
  { db, t }: ViewContext,
  name: string,
  type: FormType,
) {
  const macros = db.embeddedMacros(name, { type });
  if (macros.length === 0) return note(t.noEmbeddedMacros);
  return el(
    "div",
    {},
    macros.map((m) => {
      const where = (
        m.source.kind === "embedded"
          ? [m.source.control ?? name, m.source.event]
          : [name]
      )
        .filter(Boolean)
        .join(".");
      return el(
        "section",
        { class: "viewer-props" },
        el("h3", {}, where),
        m.text === null
          ? note(t.embeddedNoXml)
          : textView(t, m.text, fileName(where, "txt")),
      );
    }),
  );
}

// フォームのモジュールがあれば、それを開くリンクを出す
export function formModule(
  { db, t }: ViewContext,
  name: string,
  type: FormType,
) {
  const moduleName = `${type}_${name}`;
  if (!db.modules().some((m) => m.name === moduleName))
    return note(t.noModule(type));
  return el(
    "p",
    {},
    t.moduleLink(el("a", { href: hashOf("module", moduleName) }, moduleName)),
  );
}

function showForm(ctx: ViewContext, name: string, type: FormType) {
  const { db, t } = ctx;
  const design = db.formProperties(name, { type });
  return tabs(t, [
    [t.tabs.properties, () => propertyTable(t, design.properties)],
    [t.tabs.controls, () => formControls(t, design)],
    [t.tabs.embeddedMacros, () => formMacros(ctx, name, type)],
    [t.tabs.module, () => formModule(ctx, name, type)],
  ]);
}

// ---- 右の中身 ----

const kindLabel = (t: Messages, kind: DetailKind) =>
  kind === "database" ? t.wholeDatabase : t.kinds[kind][0];

// 種類の下に添える一文（クエリとモジュールは、その種類も）
function subtitle({ db, t }: ViewContext, kind: DetailKind, name: string) {
  if (kind === "query") {
    const q = db.queries({ system: true }).find((x) => x.name === name);
    return q ? `${kindLabel(t, kind)} · ${q.type}` : kindLabel(t, kind);
  }
  if (kind === "module") {
    const m = db.modules().find((x) => x.name === name);
    return m ? `${kindLabel(t, kind)} · ${m.type}` : kindLabel(t, kind);
  }
  return kindLabel(t, kind);
}

const RENDERERS: Record<
  DetailKind,
  (ctx: ViewContext, name: string) => Child[]
> = {
  table: showTable,
  query: showQuery,
  module: showModule,
  macro: showMacro,
  form: (ctx, name) => showForm(ctx, name, "Form"),
  report: (ctx, name) => showForm(ctx, name, "Report"),
  relationship: showRelationship,
  database: (ctx) => showDatabase(ctx),
};

/** 右に出す中身: 見出し、種類の一文、中身。読めなかったら、中身の代わりにエラーの一文 */
export function detailView(
  ctx: ViewContext,
  kind: DetailKind,
  name: string,
): Child[] {
  const heading = el(
    "h2",
    { class: "viewer-title" },
    kind === "database" ? ctx.t.database : name,
  );
  let type: HTMLElement | undefined;
  let body: Child[];
  try {
    type = el("p", { class: "viewer-type" }, subtitle(ctx, kind, name));
    body = RENDERERS[kind](ctx, name);
  } catch (e) {
    // 前の作りと同じく、データベース全体のときは種類の一文を空にする
    type ??= el(
      "p",
      { class: "viewer-type" },
      kind === "database" ? "" : kindLabel(ctx.t, kind),
    );
    body = [failure(ctx.t, e)];
  }
  return [heading, type, ...body];
}
