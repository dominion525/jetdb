import type { Database, Row } from "jetdb-wasm";
import { describe, expect, it, vi } from "vitest";
import { MESSAGES } from "../../../src/demo/messages.ts";
import {
  databaseProperties,
  ddlView,
  detailView,
  formControls,
  formMacros,
  formModule,
  formStreams,
  hashOf,
  propertiesView,
  tableColumns,
  tableCsv,
  tableData,
  tableDataMacros,
  tableIndexes,
  type ViewContext,
} from "../../../src/demo/views.ts";

const t = MESSAGES.en;

// jetdb-wasm の Database の代わり。テストで使う関数だけを、テストごとに入れる
function context(fake: Partial<Record<keyof Database, unknown>>): ViewContext {
  return { db: fake as unknown as Database, t };
}

// jetdb のエラーの形（code の付いた Error）
const jetdbError = (code: string, message = code) =>
  Object.assign(new Error(message), { code });

// 中身を body に置いて、文字や要素を見られるようにする
function mount(...nodes: (Node | string | null | undefined)[]): HTMLElement {
  const box = document.createElement("div");
  box.append(
    ...nodes.filter((n): n is Node | string => n !== null && n !== undefined),
  );
  document.body.replaceChildren(box);
  return box;
}

const buttonNamed = (root: HTMLElement, label: string) => {
  const button = [...root.querySelectorAll("button")].find(
    (b) => b.textContent === label,
  );
  if (!button) throw new Error(`no ${label} button`);
  return button;
};

const cells = (root: HTMLElement) =>
  [...root.querySelectorAll("tbody tr")].map((r) =>
    [...r.querySelectorAll("td")].map((d) => d.textContent),
  );

describe("URL の #", () => {
  it("種類と名前から作る。名前は URL の形にする", () => {
    expect(hashOf("database", null)).toBe("#database");
    expect(hashOf("table", "Order Details")).toBe("#table/Order%20Details");
    expect(hashOf("module", "a/b#c")).toBe("#module/a%2Fb%23c");
  });
});

describe("テーブルのデータ", () => {
  // 250 行あるテーブル。start は「行の番号」として扱う（本物は、ファイルの中の位置）
  function pagedRows(total: number, skipped = 0) {
    const calls: unknown[] = [];
    const rows = vi.fn(
      (_name: string, options: { start?: number; limit?: number }) => {
        calls.push(options);
        const start = options.start ?? 0;
        const limit = options.limit ?? total;
        const end = Math.min(start + limit, total);
        const list: Row[] = [];
        for (let i = start; i < end; i++)
          list.push({ ID: i + 1, Name: `n${i + 1}` });
        return {
          columns: ["ID", "Name"],
          rows: list,
          skipped,
          next: end < total ? end : null,
        };
      },
    );
    return { rows, calls };
  }
  const range = (box: HTMLElement) =>
    box.querySelector(".viewer-pager span")?.textContent;

  it("100 行ずつ読み、最後のページまで読んで総数を出す", () => {
    const { rows, calls } = pagedRows(250);
    const box = mount(tableData(context({ rows }), "T"));
    expect(range(box)).toBe("Rows 1–100");
    expect(cells(box)[0]).toEqual(["1", "n1"]);
    expect(buttonNamed(box, "Previous").disabled).toBe(true);

    buttonNamed(box, "Next").click();
    expect(range(box)).toBe("Rows 101–200");
    buttonNamed(box, "Next").click();
    expect(range(box)).toBe("Rows 201–250 of 250");
    expect(buttonNamed(box, "Next").disabled).toBe(true);
    expect(cells(box)).toHaveLength(50);

    // 総数がわかったあとは、前のページにも総数を出す
    buttonNamed(box, "Previous").click();
    expect(range(box)).toBe("Rows 101–200 of 250");
    expect(calls).toEqual([
      { limit: 100 },
      { start: 100, limit: 100 },
      { start: 200, limit: 100 },
      { start: 100, limit: 100 },
    ]);
  });

  it("1 ページで終わるときは、最初から総数を出す", () => {
    const { rows } = pagedRows(5);
    const box = mount(tableData(context({ rows }), "T"));
    expect(range(box)).toBe("Rows 1–5 of 5");
    expect(buttonNamed(box, "Next").disabled).toBe(true);
  });

  it("行が無いとき", () => {
    const { rows } = pagedRows(0);
    const box = mount(tableData(context({ rows }), "T"));
    expect(range(box)).toBe("No rows");
    expect(box.querySelectorAll("th")).toHaveLength(2);
  });

  it("読めなかった行があれば、その数を出す", () => {
    const { rows } = pagedRows(3, 2);
    const box = mount(tableData(context({ rows }), "T"));
    expect(box.querySelector(".note")?.textContent).toBe(
      "2 rows on this page could not be read and were left out.",
    );
  });
});

describe("テーブルの列と索引", () => {
  const column = (over: object) => ({
    name: "C",
    type: "Long",
    size: 4,
    precision: 0,
    scale: 0,
    autoNumber: false,
    calculated: false,
    ...over,
  });

  it("Numeric は桁数を添え、オートナンバーと計算の列に印を付ける", () => {
    const columns = () => [
      column({ name: "ID", autoNumber: true }),
      column({
        name: "Price",
        type: "Numeric",
        size: 17,
        precision: 18,
        scale: 2,
      }),
      column({ name: "Total", type: "Double", size: 8, calculated: true }),
    ];
    expect(cells(mount(tableColumns(context({ columns }), "T")))).toEqual([
      ["ID", "Long", "4", "AutoNumber"],
      ["Price", "Numeric(18, 2)", "17", ""],
      ["Total", "Double", "8", "Calculated"],
    ]);
  });

  it("索引の列の向きと、主キーなどの印", () => {
    const indexes = () => [
      {
        name: "PrimaryKey",
        primaryKey: true,
        columns: [{ name: "ID", descending: false }],
        unique: true,
        ignoreNulls: false,
        required: true,
      },
      {
        name: "ix",
        primaryKey: false,
        columns: [
          { name: "A", descending: false },
          { name: "B", descending: true },
        ],
        unique: false,
        ignoreNulls: true,
        required: false,
      },
    ];
    expect(cells(mount(tableIndexes(context({ indexes }), "T")))).toEqual([
      ["PrimaryKey", "ID ASC", "Primary key, Unique, Required"],
      ["ix", "A ASC, B DESC", "Ignore nulls"],
    ]);
  });

  it("索引が無いとき", () => {
    expect(tableIndexes(context({ indexes: () => [] }), "T").textContent).toBe(
      "This table has no indexes.",
    );
  });
});

describe("プロパティ", () => {
  it("オブジェクト自体・列ごと・追加のまとまりを、見出しを付けて出す", () => {
    const properties = vi.fn(() => ({
      object: [{ name: "Description", value: "orders" }],
      columns: [{ name: "Qty", properties: [] }],
      additional: [
        { name: "Extra", properties: [{ name: "Flag", value: true }] },
      ],
    }));
    const box = mount(propertiesView(context({ properties }), "T", "Table"));
    expect(properties).toHaveBeenCalledWith("T", { type: "Table" });
    expect([...box.querySelectorAll("h3")].map((h) => h.textContent)).toEqual([
      "Table properties",
      "Column: Qty",
      "Extra",
    ]);
    expect(box.querySelectorAll(".note")[0]?.textContent).toBe("None.");
    expect(cells(box)).toEqual([
      ["Description", "orders"],
      ["Flag", "true"],
    ]);
  });

  it("日本語のページでは、種類の名前も日本語", () => {
    const properties = () => ({ object: [], columns: [], additional: [] });
    const view = propertiesView(
      { db: { properties } as unknown as Database, t: MESSAGES.ja },
      "Q",
      "Query",
    );
    expect(view.querySelector("h3")?.textContent).toBe("クエリのプロパティ");
  });
});

describe("CSV", () => {
  it("jetdb export と同じ既定のオプションで読み、テキストで出して保存できる", () => {
    const csv = vi.fn(() => "ID,Name\n1,a\n");
    const box = mount(tableCsv(context({ csv }), "Order Details"));
    expect(csv).toHaveBeenCalledWith("Order Details");
    expect(box.querySelector("pre code")?.textContent).toBe("ID,Name\n1,a\n");
    let saved = "";
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:test");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      saved = this.download;
    });
    buttonNamed(box, "Save").click();
    expect(saved).toBe("Order Details.csv");
    vi.restoreAllMocks();
  });

  it("読めなかったら、その場で投げる（タブが読めなかった一文にする）", () => {
    const csv = () => {
      throw jetdbError("TABLE_NOT_FOUND", "no table");
    };
    expect(() => tableCsv(context({ csv }), "T")).toThrow("no table");
  });
});

describe("データマクロ", () => {
  it("無ければそう書き、あればテキストで出す", () => {
    expect(
      tableDataMacros(context({ dataMacrosText: () => "" }), "T").textContent,
    ).toBe("This table has no data macros.");
    const box = mount(
      tableDataMacros(context({ dataMacrosText: () => "<DataMacros/>" }), "T"),
    );
    expect(box.querySelector("pre code")?.textContent).toBe("<DataMacros/>");
  });
});

describe("DDL", () => {
  it("方言を選び直すと、その方言で作り直す", () => {
    const ddl = vi.fn((dialect: string) => `-- ${dialect}`);
    const box = mount(ddlView(context({ ddl }), "T"));
    expect(ddl).toHaveBeenLastCalledWith("sqlite", { table: "T" });
    expect(box.querySelector("pre code")?.textContent).toBe("-- sqlite");
    const select = box.querySelector("select");
    if (!select) throw new Error("no select");
    expect([...select.options].map((o) => o.textContent)).toEqual([
      "SQLite",
      "PostgreSQL",
      "MySQL",
      "Access SQL",
    ]);
    select.value = "postgres";
    select.dispatchEvent(new Event("change"));
    expect(ddl).toHaveBeenLastCalledWith("postgres", { table: "T" });
    expect(box.querySelector("pre code")?.textContent).toBe("-- postgres");
  });

  it("データベース全体のときは、テーブルを指定しない", () => {
    const ddl = vi.fn(() => "");
    mount(ddlView(context({ ddl }), null));
    expect(ddl).toHaveBeenLastCalledWith("sqlite", {});
  });

  it("作れなかったら、方言の欄は残して読めなかった一文を出す", () => {
    const ddl = () => {
      throw jetdbError("INVALID_FILE", "bad relationships");
    };
    const box = mount(ddlView(context({ ddl }), "T"));
    expect(box.querySelector("select")).not.toBeNull();
    expect(box.querySelector(".viewer-error")?.textContent).toBe(
      "Could not read: bad relationships",
    );
  });
});

describe("データベースのプロパティ", () => {
  it("まとまりが無いときは None、ほかのエラーは読めなかった一文", () => {
    const properties = vi.fn((name: string) => {
      if (name === "MSysDb")
        return {
          object: [{ name: "AccessVersion", value: "09.50" }],
          columns: [],
          additional: [],
        };
      if (name === "SummaryInfo") throw jetdbError("OBJECT_NOT_FOUND");
      throw jetdbError("INVALID_FILE", "broken");
    });
    const box = mount(databaseProperties(context({ properties })));
    expect(properties).toHaveBeenCalledWith("SummaryInfo", {
      type: "DatabaseProperty",
    });
    const sections = [...box.querySelectorAll("section")].map((s) => [
      s.querySelector("h3")?.textContent,
      s.querySelector("td, .note, .viewer-error")?.textContent,
    ]);
    expect(sections).toEqual([
      ["Database properties", "AccessVersion"],
      ["Summary info", "None."],
      ["User-defined", "Could not read: broken"],
    ]);
  });
});

describe("フォーム/レポート", () => {
  it("コントロールの一覧の表（jetdb form controls と同じ）と、コントロールごとのプロパティ", () => {
    const design = {
      properties: [],
      controls: [
        {
          name: "btnOk",
          type: "CommandButton",
          properties: [{ name: "Caption", value: "OK" }],
        },
      ],
    };
    const formControlsOf = vi.fn(() => [
      { name: "Detail", type: "Detail", typeCode: 0, index: 0 },
      { name: "btnOk", type: "CommandButton", typeCode: 104, index: 1 },
    ]);
    const box = mount(
      formControls(
        context({ formControls: formControlsOf }),
        "F",
        "Report",
        design as never,
      ),
    );
    expect(formControlsOf).toHaveBeenCalledWith("F", { type: "Report" });
    expect(
      [...(box.querySelectorAll("table")[0]?.querySelectorAll("th") ?? [])].map(
        (h) => h.textContent,
      ),
    ).toEqual(["Name", "Type", "Index"]);
    expect(cells(box).slice(0, 2)).toEqual([
      ["Detail", "Detail", "0"],
      ["btnOk", "CommandButton", "1"],
    ]);
    expect(box.querySelector("h3")?.textContent).toBe(
      "Properties of each control",
    );
    expect(box.querySelector("summary")?.textContent).toBe(
      "btnOk (CommandButton)",
    );
    expect(cells(box).slice(2)).toEqual([["Caption", "OK"]]);
  });

  it("コントロールが無いとき", () => {
    expect(
      formControls(context({ formControls: () => [] }), "F", "Form", {
        properties: [],
        controls: [],
      } as never).textContent,
    ).toBe("This design has no controls.");
  });

  it("4 つのストリームを大きさと保存のボタンで出し、無いものと読めないものはそう書く", () => {
    const formStream = vi.fn(
      (_name: string, options: { type: string; stream: string }) => {
        if (options.stream === "blob") return new Uint8Array(2048);
        if (options.stream === "typeinfo") return new Uint8Array(10);
        if (options.stream === "propdata")
          throw jetdbError("FORM_NOT_FOUND", "no propdata");
        throw jetdbError("INVALID_FILE", "broken stream");
      },
    );
    const box = mount(formStreams(context({ formStream }), "F", "Form"));
    expect(formStream).toHaveBeenCalledWith("F", {
      type: "Form",
      stream: "blob",
    });
    expect(box.querySelector(".note")?.textContent).toContain(
      "jetdb form dump",
    );
    expect(cells(box)).toEqual([
      [
        "blob",
        "The design: layout, controls, properties and events",
        "2.0 KB Save",
      ],
      ["typeinfo", "The names and types of the controls", "10 bytes Save"],
      ["propdata", "A little property data", "None"],
      ["blobdelta", "Changes, usually none", "Could not read: broken stream"],
    ]);
    let saved = "";
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:test");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      saved = this.download;
    });
    buttonNamed(box, "Save").click();
    expect(saved).toBe("F.blob.bin");
    vi.restoreAllMocks();
  });

  it("埋め込みマクロの見出しは、コントロールとイベント。テキストの無いものはそう書く", () => {
    const embeddedMacros = vi.fn(() => [
      {
        source: {
          kind: "embedded",
          objectType: "Form",
          objectName: "F",
          control: "btn",
          event: "OnClick",
        },
        text: "Macro text",
      },
      {
        source: {
          kind: "embedded",
          objectType: "Form",
          objectName: "F",
          control: null,
          event: "OnLoad",
        },
        text: "Load text",
      },
      {
        source: {
          kind: "embedded",
          objectType: "Form",
          objectName: "F",
          control: null,
          event: "",
        },
        text: null,
      },
    ]);
    const box = mount(formMacros(context({ embeddedMacros }), "F", "Form"));
    expect(embeddedMacros).toHaveBeenCalledWith("F", { type: "Form" });
    expect([...box.querySelectorAll("h3")].map((h) => h.textContent)).toEqual([
      "btn.OnClick",
      "F.OnLoad",
      "F",
    ]);
    expect(box.querySelectorAll("pre")).toHaveLength(2);
    expect(box.querySelector(".note")?.textContent).toBe(
      "This macro was stored without XML, so jetdb has no text to show.",
    );
  });

  it("埋め込みマクロが無いとき", () => {
    expect(
      formMacros(context({ embeddedMacros: () => [] }), "F", "Report")
        .textContent,
    ).toBe("There are no embedded macros.");
  });

  it("フォームのモジュールがあれば、それを開くリンク", () => {
    const modules = () => [{ name: "Form_F", type: "Document" }];
    const link = formModule(context({ modules }), "F", "Form").querySelector(
      "a",
    );
    expect(link?.getAttribute("href")).toBe("#module/Form_F");
    expect(link?.textContent).toBe("Form_F");
    expect(formModule(context({ modules }), "R", "Report").textContent).toBe(
      "This report has no module.",
    );
  });
});

describe("右に出す中身", () => {
  const show = (
    ctx: ViewContext,
    kind: Parameters<typeof detailView>[1],
    name: string,
  ) => mount(...detailView(ctx, kind, name));

  it("クエリとモジュールは、種類の一文にその種類も添える", () => {
    const ctx = context({
      queries: () => [{ name: "Q", type: "Select" }],
      querySql: () => "SELECT 1;",
      properties: () => ({ object: [], columns: [], additional: [] }),
      modules: () => [{ name: "M", type: "Standard" }],
      moduleSource: () => "Sub A()\nEnd Sub",
    });
    const query = show(ctx, "query", "Q");
    expect(query.querySelector("h2")?.textContent).toBe("Q");
    expect(query.querySelector(".viewer-type")?.textContent).toBe(
      "query · Select",
    );
    expect(query.querySelector("pre code")?.textContent).toBe("SELECT 1;");

    const module = show(ctx, "module", "M");
    expect(module.querySelector(".viewer-type")?.textContent).toBe(
      "module · Standard",
    );
    expect(module.querySelector("pre code")?.textContent).toBe(
      "Sub A()\nEnd Sub",
    );
  });

  it("マクロのアクションを読めなかったときは、そう書く", () => {
    const box = show(context({ macro: () => ({ text: null }) }), "macro", "m");
    expect(box.querySelector(".note")?.textContent).toBe(
      "jetdb could not read the actions of this macro, so there is no text to show.",
    );
  });

  it("リレーションシップは、テーブルへのリンク・列の対応・連鎖の印", () => {
    const relationships = () => [
      {
        name: "fk",
        fromTable: "Orders",
        toTable: "Customers",
        columns: [{ from: "CustomerID", to: "ID" }],
        referentialIntegrity: true,
        cascadeUpdate: false,
        cascadeDelete: true,
      },
    ];
    const box = show(context({ relationships }), "relationship", "fk");
    expect(
      [...box.querySelectorAll(".viewer-relation a")].map((a) =>
        a.getAttribute("href"),
      ),
    ).toEqual(["#table/Orders", "#table/Customers"]);
    expect([...box.querySelectorAll("th")].map((h) => h.textContent)).toEqual([
      "Orders column",
      "Customers column",
    ]);
    expect(
      [...box.querySelectorAll(".viewer-flags li")].map((li) => li.textContent),
    ).toEqual([
      "Referential integrity: yes",
      "Cascade update: no",
      "Cascade delete: yes",
    ]);
  });

  it("読めなかったら、見出しと種類の一文は残して、中身の代わりにエラーの一文", () => {
    const box = show(
      context({ relationships: () => [] }),
      "relationship",
      "gone",
    );
    expect(box.querySelector("h2")?.textContent).toBe("gone");
    expect(box.querySelector(".viewer-type")?.textContent).toBe("relationship");
    expect(box.querySelector(".viewer-error")?.textContent).toBe(
      "Could not read: no relationship named gone",
    );
  });

  it("種類の一文を作れなかったときも、種類の名前は出す", () => {
    const queries = () => {
      throw jetdbError("INVALID_FILE", "broken catalog");
    };
    const box = show(context({ queries }), "query", "Q");
    expect(box.querySelector(".viewer-type")?.textContent).toBe("query");
    expect(box.querySelector(".viewer-error")?.textContent).toBe(
      "Could not read: broken catalog",
    );
  });

  it("データベース全体は、DDL とプロパティのタブ", () => {
    const box = show(context({ ddl: () => "CREATE TABLE x;" }), "database", "");
    expect(box.querySelector("h2")?.textContent).toBe("Database");
    expect(box.querySelector(".viewer-type")?.textContent).toBe(
      "The whole database",
    );
    expect([...box.querySelectorAll(".tab")].map((b) => b.textContent)).toEqual(
      ["DDL", "Properties"],
    );
    expect(box.querySelector("pre code")?.textContent).toBe("CREATE TABLE x;");
  });

  it("テーブルは 7 つのタブで、最初はデータ", () => {
    const rows = () => ({
      columns: ["A"],
      rows: [{ A: 1 }],
      skipped: 0,
      next: null,
    });
    const box = show(context({ rows }), "table", "T");
    expect([...box.querySelectorAll(".tab")].map((b) => b.textContent)).toEqual(
      ["Data", "CSV", "Columns", "Indexes", "Properties", "Data macros", "DDL"],
    );
    expect(box.querySelector(".viewer-type")?.textContent).toBe("table");
    expect(cells(box)).toEqual([["1"]]);
  });

  it("フォームとレポートは、種類を付けてデザインを読む", () => {
    const formProperties = vi.fn(() => ({ properties: [], controls: [] }));
    const box = show(context({ formProperties }), "report", "R");
    expect(formProperties).toHaveBeenCalledWith("R", { type: "Report" });
    expect([...box.querySelectorAll(".tab")].map((b) => b.textContent)).toEqual(
      ["Properties", "Controls", "Embedded macros", "Module", "Streams"],
    );
  });
});
