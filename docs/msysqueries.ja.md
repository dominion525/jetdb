# MSysQueries の内部構造

Access データベース (.mdb/.accdb) における保存済みクエリの格納構造に関する調査知見。

## 概要

保存済みクエリの定義は `MSysQueries` システムテーブルに格納される。各クエリは複数行で構成され、`ObjectId` で紐付けられる。クエリ名は `MSysQueries` には格納されず、カタログ（`MSysObjects`）の `ObjectType=5`（Query）エントリから取得する。

## カラム構成

| カラム名 | 型 | 説明 |
|----------|------|------|
| ObjectId | Long | クエリID（MSysObjects.Id との紐付け） |
| Attribute | Byte | 行の種別（後述） |
| Order | Binary | 同一 Attribute 内のソート順 |
| Name1 | Text | テーブル名、パラメータ名等（Attribute 依存） |
| Name2 | Text | エイリアス等（Attribute 依存） |
| Expression | Memo | SQL 式（カラム式、JOIN 条件、WHERE 句等） |
| Flag | Int | フラグ値（Attribute 依存） |
| LvExtra | Long | jetdb は Text 型パラメータのサイズ（Attribute=2, Flag=10）として読む。Jet4 以降のみで、Jet3（Access 97）にはこのカラムがない |

## Attribute 定数

| 値 | 定数名 | 説明 | Flag の意味 |
|----|--------|------|-------------|
| 0 | (ヘッダー) | クエリヘッダー行 | クエリフラグ（0 が多い） |
| 1 | ATTR_TYPE | クエリ種別 | QueryType 値（下表参照） |
| 2 | ATTR_PARAMETER | パラメータ定義 | パラメータのデータ型 |
| 3 | ATTR_FLAG | SELECT フラグ（DISTINCT 等） | ビットフラグ |
| 5 | ATTR_TABLE | FROM 句テーブル | - |
| 6 | ATTR_COLUMN | SELECT 句カラム | カラムフラグ |
| 7 | ATTR_JOIN | JOIN 条件 | JOIN 種別 |
| 8 | ATTR_WHERE | WHERE 句 | - |
| 9 | ATTR_GROUPBY | GROUP BY 句 | - |
| 10 | ATTR_HAVING | HAVING 句 | - |
| 11 | ATTR_ORDERBY | ORDER BY 句 | - |
| 255 | (終端) | クエリ終端マーカー | - |

## QueryType 値（ATTR_TYPE の Flag）

| 値 | 定数名 | Access のクエリ種別 |
|----|--------|---------------------|
| 1 | Select | 選択クエリ |
| 2 | MakeTable | テーブル作成クエリ |
| 3 | Append | 追加クエリ |
| 4 | Update | 更新クエリ |
| 5 | Delete | 削除クエリ |
| 6 | Crosstab | クロス集計クエリ |
| 7 | Ddl | データ定義クエリ |
| 8 | Passthrough | パススルークエリ |
| 9 | Union | ユニオンクエリ |

## 埋め込みクエリ（~sq_ プレフィックス）

フォームやレポートのコントロールに設定された RowSource や RecordSource の SQL は、Access が内部的に `~sq_` プレフィックス付きの名前で MSysQueries に保存する。これらはユーザーが明示的に作成したクエリではなく、Access のフォーム/レポートデザイナーが自動生成するものである。

### 命名規則

```
~sq_<種別><フォーム/レポート名>[~sq_<種別><コントロール名>]
```

種別コード:

| コード | 意味 | 例 |
|--------|------|-----|
| c | コントロールの RowSource | ~sq_cF_請求書一覧~sq_ccmb_クライアント名検索 |
| d | レポートコントロールの DataSource | ~sq_dR_請求書~sq_dクライアントID |
| f | フォーム/サブフォームの RecordSource | ~sq_fF_クライアント情報 |
| r | レポートの RecordSource | ~sq_rR_請求書 |

### ATTR_TYPE の欠落

埋め込みクエリは ATTR_TYPE（Attribute=1）行を持たないことが多い。典型的な構成は Attribute=0（ヘッダー）、3（FLAG）、5（TABLE）、255（終端）のみ。

実際に使われているデータベースでの実測値:

  全クエリ数（MSysObjects の Query エントリ）: 227
  ATTR_TYPE を持つクエリ: 43（全てユーザー定義クエリ）
  ATTR_TYPE を持たないクエリ: 184（全て ~sq_ 埋め込みクエリ）

### jetdb での扱い

`queries list` / `queries show` ではユーザー定義クエリのみを対象とする。ATTR_TYPE 行がないクエリは以下のように処理する:

- 名前が `~sq_` で始まる → 埋め込みクエリとしてスキップ
- 名前が `~sq_` で始まらない → QueryType::Select としてフォールバック

後者のケースは、formPropTest.accdb のように ATTR_TYPE 行が欠落したユーザー定義クエリに対応するためのもの。

## formPropTest.accdb の特殊ケース

formPropTest.accdb では全3クエリが ATTR_TYPE 行を持たない（Attribute=0, 3, 5, 255 のみ）。原因は不明だが、同様のファイルが実環境にも存在しうるため、Select フォールバックで対応している。queryTestV2007.accdb や queryTestV2010.accdb では正常に ATTR_TYPE が存在する。

## 参考

- Access SQL リファレンス: https://learn.microsoft.com/en-us/office/client-developer/access/desktop-database-reference/microsoft-access-sql-reference
