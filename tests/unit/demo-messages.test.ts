import { describe, expect, it } from "vitest";
import { MESSAGES, type Messages } from "../../src/demo-messages.ts";

// 入れ子の表を「a.b.c」の形のキーの一覧にする
function keys(table: object, prefix = ""): string[] {
  return Object.entries(table).flatMap(([key, value]) =>
    value !== null && typeof value === "object" && !Array.isArray(value)
      ? keys(value, `${prefix}${key}.`)
      : [`${prefix}${key}`],
  );
}

// 文字のままの値（関数でないもの）を、配列の中も含めて全部取り出す
function texts(table: object): unknown[] {
  return Object.values(table).flatMap((value) =>
    Array.isArray(value)
      ? value
      : value !== null && typeof value === "object"
        ? texts(value)
        : typeof value === "function"
          ? []
          : [value],
  );
}

describe("ビューアの文字の表", () => {
  it("日本語と英語でキーが揃っている", () => {
    expect(keys(MESSAGES.ja).sort()).toEqual(keys(MESSAGES.en).sort());
  });

  it("配列の長さが日本語と英語で同じ", () => {
    const lengths = (m: Messages) => [
      ...Object.values(m.kinds).map((k) => k.length),
      ...Object.values(m.complexNouns).map((n) => n.length),
      m.columnHeads.length,
      m.indexHeads.length,
      m.propertyHeads.length,
    ];
    expect(lengths(MESSAGES.ja)).toEqual(lengths(MESSAGES.en));
  });

  for (const lang of ["en", "ja"] as const) {
    it(`${lang}: 空の文字が無い`, () => {
      for (const text of texts(MESSAGES[lang])) {
        expect(typeof text).toBe("string");
        expect((text as string).trim()).not.toBe("");
      }
    });
  }

  it("数と単数・複数", () => {
    expect(MESSAGES.en.count(1, "table", "tables")).toEqual(["1", " table"]);
    expect(MESSAGES.en.count(3, "table", "tables")).toEqual(["3", " tables"]);
    expect(MESSAGES.ja.count(3, "テーブル", "テーブル")).toEqual([
      "3",
      " テーブル",
    ]);
    expect(MESSAGES.en.complexCount(1, "file", "files")).toBe("1 file");
    expect(MESSAGES.ja.complexCount(2, "ファイル", "ファイル")).toBe(
      "ファイル 2 件",
    );
  });

  it("行の範囲は、総数がわかったときだけ総数を出す", () => {
    expect(MESSAGES.en.rows(1, 100, null)).toBe("Rows 1–100");
    expect(MESSAGES.en.rows(101, 105, 105)).toBe("Rows 101–105 of 105");
    expect(MESSAGES.ja.rows(1, 100, null)).toBe("1〜100 行目");
    expect(MESSAGES.ja.rows(101, 105, 105)).toBe("101〜105 行目（全 105 行）");
  });

  it("モジュールへのリンクを文の中に入れる", () => {
    const link = document.createElement("a");
    for (const lang of ["en", "ja"] as const) {
      const parts = MESSAGES[lang].moduleLink(link);
      expect(parts).toContain(link);
      expect(parts.filter((p) => typeof p === "string")).toHaveLength(2);
    }
  });
});
