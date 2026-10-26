# JavaScript API（jetdb-wasm）

[English](wasm.md)

jetdb-wasm は、jetdb を WebAssembly にしたもので、Microsoft Access のデータベース（.mdb / .accdb）をブラウザと Node.js で読みます。この文書では、その `Database` クラスを説明します。パッケージには TypeScript の型定義が入っていて、メソッドが返すオブジェクトの各フィールドは、型定義の説明文に書かれています。

## モジュールの読み込み

パッケージは 2 つのビルドを持ち、package.json の `exports` で 1 つを選びます。`node` の条件で選ばれる Node.js 向けの CommonJS と、それ以外（ブラウザや、ブラウザ向けにビルドするバンドラーなど）向けの ES モジュールです。

ES モジュールでは、`Database` を使う前に、既定のエクスポートを 1 回呼びます。モジュールの隣にある Wasm のファイルを取得して、コンパイルします。

```js
import init, { Database } from "jetdb-wasm";

await init();
```

CommonJS のビルドでは、読み込んだときに Wasm のファイルが読まれるので、`Database` はそのまま使えます。Node.js と、サーバーサイドレンダリング（SSR）のようにバンドラーがサーバー向けにビルドするものが、このビルドを使います。このビルドには、呼ぶための既定のエクスポートがありません。

```js
import { Database } from "jetdb-wasm";
```

## データベースを開く

`Database.open(bytes, password)` で、ファイルのバイト列からデータベースを開きます。バイト列は `Uint8Array`（Node.js の `Buffer` もこれに当たります）か `ArrayBuffer` で渡します。パスワードは、パスワード保護された .accdb ファイル（Agile Encryption、RC4 CryptoAPI、Standard AES・NonStandard AES）のもので、パスワードの無いファイルでは使いません。

```js
const db = Database.open(bytes, "password");
console.log(db.version()); // "JET3"、"JET4"、"ACE12"、"ACE14"、"ACE15"、"ACE16"、"ACE17" のどれか
```

`Database` は、ファイルのバイト列を Wasm のモジュールのメモリに持ちます。使い終わったら `db.free()` を呼びます。`using` が使える環境では、`using` で開くと、ブロックの終わりで解放されます。

```js
{
  using db = Database.open(bytes);
  // ...
}
```

## テーブル

| メソッド | 返すもの |
|--------|---------|
| `tables(options)` | ユーザーテーブルの名前（並べ替え済み）。`{ system: true }` で、システムテーブルと隠しテーブルも含める |
| `columns(table)` | Access の表示順の列: 名前、型、サイズ、精度と小数点以下桁数、オートナンバー、集計 |
| `indexes(table)` | インデックス: 名前、主キー、列とその順序、重複なし、Null を無視、値要求 |
| `rows(table, options)` | 行。それぞれ、列の名前をキーにしたオブジェクト |
| `csv(table, options)` | 行の CSV。`jetdb export` と同じ形 |

`rows` は、テーブルでの並び順の列名（`columns`）、行（`rows`）、読めずに除いた行の数（`skipped`）を返します。行のキーは列の順を保ちません。JavaScript は、`"2019"` のような整数に見えるキーを、ほかのキーより前に並べます。

大きなテーブルを区切って読むときは、読む行の数の上限を `limit` に、前の呼び出しの `next` を `start` に渡します。最後の行まで読むと、`next` は null になります。

```js
let start;
do {
  const part = db.rows("Orders", { start, limit: 1000 });
  for (const row of part.rows) {
    // ...
  }
  start = part.next;
} while (start !== null);
```

`next` は行の数ではなく、テーブルのページの中での位置です。そのため、同じファイルの同じテーブルでだけ使えます。

`csv` のオプションは、`jetdb export` のものと同じです。

| オプション | 既定 | 意味 |
|--------|---------|---------|
| `header` | `true` | 1 行目に列の名前を書く |
| `delimiter` | `,` | 区切り文字（1 文字） |
| `dateFormat` | `%Y-%m-%d` | 時刻の無い日付の書式（strftime の一部） |
| `datetimeFormat` | `%Y-%m-%d %H:%M:%S` | 時刻のある日付の書式 |
| `bin` | `hex` | バイナリ型と OLE オブジェクト型の値の書き方: `strip`、`raw`、`octal`、`hex` |
| `null` | 空 | NULL の値の文字列 |
| `booleanWords` | `false` | Yes/No型の値を `1` と `0` ではなく `TRUE` と `FALSE` で書く |
| `systemColumns` | `false` | Access が管理して隠している列も含める |

添付ファイルの列は、CSV ではファイルの名前になります。

## リレーションシップと DDL

`relationships(options)` は、テーブルの間のリレーションシップを名前の順で返します。2 つのテーブル、列の組、参照整合性と、連鎖更新・連鎖削除の有無が入ります。`{ system: true }` で、システムテーブルと隠しテーブルのものも含めます。

`ddl(dialect, options)` は、ユーザーテーブルの DDL を、`jetdb schema --ddl` と同じ形で返します。CREATE TABLE、CREATE INDEX、外部キーです。方言は `sqlite`、`postgres`、`mysql`、`access` のどれかです。

| オプション | 既定 | 意味 |
|--------|---------|---------|
| `table` | すべてのユーザーテーブル | このテーブルだけ（システムテーブルも指定できる） |
| `indexes` | `true` | インデックスを含める |
| `relations` | `true` | 外部キーを含める。`false` ではリレーションシップを読まないので、リレーションシップが読めないファイルでもテーブルの DDL が取れる |

## クエリ

`queries(options)` は、保存済みクエリを名前の順で、種類（`Select`、`MakeTable`、`Append`、`Update`、`Delete`、`Crosstab`、`Ddl`、`Passthrough`、`Union`）と一緒に返します。`{ system: true }` で、フォームやレポートのために Access が作るものなど、システムクエリと隠しクエリも含めます。

`querySql(name)` は、保存済みクエリの SQL を、Access の SQL ビューと同じ形で返します。

## VBA

`modules()` は、VBA のモジュールを名前の順で、種類（`Standard`、`Class`、フォームやレポートのモジュールの `Document`）と一緒に返します。`moduleSource(name)` は、モジュールのソースを、Access がその前に持つ `Attribute` の行も含めて、`jetdb vba show` と同じ形で返します。

## フォームとレポート

| メソッド | 返すもの |
|--------|---------|
| `forms()` | フォームとレポート（名前の順）。それぞれ、フォームかレポートか |
| `formControls(name, options)` | セクションとコントロール。デザインの順で、種類と一覧の中の位置を持つ |
| `formProperties(name, options)` | デザイン。フォームやレポート自身のプロパティと、セクションやコントロールごとのプロパティ |
| `formStream(name, options)` | フォームやレポートのストリームのバイト列。`stream` オプションで、`blob`（デザイン、既定）、`typeinfo`、`propdata`、`blobdelta` を選ぶ |

フォームとレポートが同じ名前のときは、`formControls`、`formProperties`、`formStream`、`embeddedMacros` に `{ type: "Form" }` か `{ type: "Report" }` を渡します。渡さないと `INVALID_ARGUMENT` を投げます。

プロパティの名前は `name` に入り、jetdb が知らないものでは番号の `0x....` になります。番号は、どのプロパティでも `id` に入ります。値を持たずに保存された Yes/No のプロパティ（既定でないほうの値に設定されたもの）の値は、SaveAsText と同じく `NotDefault` です。色は、`0x00BBGGRR` の数か、システムの色では負の数です。

## マクロ

| メソッド | 返すもの |
|--------|---------|
| `macros(options)` | 名前付きマクロの名前（並べ替え済み）。`{ system: true }` で、Access が自分用に作り `~` で始まる名前を付けたものも含める |
| `macro(name)` | 名前付きマクロ |
| `embeddedMacros(form, options)` | フォームやレポートのイベントに埋め込まれたマクロ |
| `dataMacros(table)` | テーブルのデータマクロ |
| `dataMacrosText(table)` | テーブルのすべてのデータマクロの SaveAsText 形式のテキスト。`jetdb macro data` と同じ |

マクロは、どこのマクロか（`source`）、文（`statements`）、Access が持つ XML（`xml`。Access 2010 より前に保存されたマクロでは空）、SaveAsText 形式のテキスト（`text`。`jetdb macro show` と `jetdb macro embedded` と同じ）を持ちます。`text` は、データマクロ（テーブルごとのテキストは `dataMacrosText` で取ります）、アクションの表を読めなかった名前付きマクロ、XML なしで保存された埋め込みマクロでは null です。データマクロの `xml` は、そのマクロが保存されていた XML 文書全体で、同じテーブルのほかのデータマクロも含むことがあります。

文は、アクションとその引数、コメント、`If` のブロック、グループ、サブマクロ、データマクロのレコードのブロック、jetdb が解釈しない要素（そのまま残す）のどれかです。

## オブジェクトのプロパティ

`properties(name, options)` は、説明や書式など、Access がオブジェクトについて持つプロパティを返します。オブジェクト自身のもの（`object`）、テーブルの列ごとのもの（`columns`）、追加のまとまり（`additional`）です。`Customers` という名前のテーブルとフォームのように、種類の違うオブジェクトが同じ名前のときは、`{ type: "Table" }` のように種類を渡します。渡さないと `INVALID_ARGUMENT` を投げます。

## 値

行やプロパティの値は、列の型に応じた JavaScript の値になります。

| Access の型 | JavaScript の値 |
|-------------|------------------|
| バイト型、整数型、長整数型、単精度浮動小数点型、倍精度浮動小数点型 | `number`。単精度浮動小数点型は、Access の表示と同じく、保存された値に戻るいちばん短い 10 進数。保存された値そのものは `Math.fround(value)` で得られます |
| 大きい数値（BigInt） | `bigint` |
| Yes/No型 | `boolean` |
| 短いテキスト、長いテキスト、レプリケーション ID型（GUID）、通貨型、十進型 | `string`（桁を失わないため） |
| 日付/時刻型 | `string`。`2021-06-14`、時刻が 0 時でなければ `2021-06-14 22:45:12`。Access の表示と同じく秒に丸める |
| 拡張した日付/時刻 | すべての桁を持つ `string`。`2021-06-14 22:45:12.3456789` など。日時として読めないときは、保存されたバイト列の `Uint8Array` |
| バイナリ型、OLE オブジェクト型、jetdb が知らない型 | `Uint8Array` |
| 複数の値を持つ列 | 値の配列（値が無ければ空） |
| 添付ファイル | ファイルの配列。それぞれ、名前、種類、中身を持つ |
| 履歴を持つ長いテキスト | 版の配列（古い順） |
| jetdb が知らない種類のコンプレックスカラム | `number`（隠しテーブルにある値の ID） |
| NULL | `null` |

## エラー

失敗したメソッドは、`name` が `JetdbError` の `Error` を投げます。何が起きたかは `code` でわかります。`JetdbError` はこのエラーの TypeScript の型で、パッケージがエクスポートするクラスではありません。そのため、`instanceof` ではなく、`name` と `code` で見分けます。

| コード | 意味 |
|------|---------|
| `PASSWORD_REQUIRED` | ファイルがパスワード保護されていて、パスワードが渡されていない |
| `INVALID_PASSWORD` | パスワードが違う |
| `UNSUPPORTED_ENCRYPTION` | jetdb が読めない方式で暗号化されている |
| `TABLE_NOT_FOUND`、`QUERY_NOT_FOUND`、`MODULE_NOT_FOUND`、`FORM_NOT_FOUND`、`MACRO_NOT_FOUND`、`OBJECT_NOT_FOUND` | その名前のオブジェクトが無い |
| `INVALID_FILE` | Access のデータベースでないか、壊れている。何が悪いかはメッセージにある |
| `NOT_SUPPORTED` | Access 97 のフォームとレポートのように、jetdb が読まないものをデータベースが持っている |
| `INVALID_ARGUMENT` | メソッドに正しくない引数やオプションが渡された（型が違う、決まった値のどれでもない、など）。または、種類の違うオブジェクトが同じ名前で、どれかを選ぶ `type` のオプションが無い |
| `IO` | バイト列の読み取りに失敗した |

```js
try {
  db.rows("Customers");
} catch (e) {
  if (e.name === "JetdbError" && e.code === "TABLE_NOT_FOUND") {
    // ...
  }
}
```

## 制限事項

- 読み取り専用です（Access のファイルには書き込みません）。
- ファイル全体をメモリに持ちます。
- Access 97（Jet3）のファイルでは、フォーム・レポートと VBA を読みません。`forms()` と `modules()` は何も返さず、名前を指定してフォームやレポートを読むと `NOT_SUPPORTED` を投げます。
