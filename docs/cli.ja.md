# jetdb CLI

Microsoft Access データベース (.mdb / .accdb) の読み取り専用コマンドラインツール。

## インストール

```bash
cargo install --path crates/jetdb-cli
```

## グローバルオプション

全てのサブコマンドで使用できるオプション。

- `--password <PASSWORD>` — データベースパスワード (パスワード保護された .accdb ファイル用)

`jetdb --help-skill` は、サブコマンドを付けずに使い、agent-skills リポジトリにある jetdb の Claude Code Skill のインストール方法を表示して終了する。

## サブコマンド

### ver — データベースエンジンバージョンの表示

```
jetdb ver [OPTIONS] <FILE>
```

データベースファイルの Jet/ACE エンジンバージョンを表示する。

#### オプション

- `-l`, `--long` — 詳細なバージョン情報を表示

#### 出力例

```
$ jetdb ver testV2003.mdb
JET4

$ jetdb ver -l testV2003.mdb
Jet4 (Access 2000/2003)
```

#### バージョン一覧

| 短縮名 | 詳細 |
|--------|------|
| JET3 | Jet3 (Access 97) |
| JET4 | Jet4 (Access 2000/2003) |
| ACE12 | ACE12 (Access 2007) |
| ACE14 | ACE14 (Access 2010) |
| ACE15 | ACE15 (Access 2013) |
| ACE16 | ACE16 (Access 2016) |
| ACE17 | ACE17 (Access 2019) |

ACE15 は、Access 2013 にちなんだ版の値 0x04 の名前です。Access 2013 は Access 2010 と同じ形式（ACE14）で書き、0x04 のファイルは知られていません。jetdb は、そのようなファイルを ACE15 として読みます。

### tables — テーブル一覧の表示

```
jetdb tables [OPTIONS] <FILE>
```

データベースに含まれるテーブル名を1行1テーブルで出力する。

#### オプション

- `-s`, `--system` — システムテーブル (SYSTEM/HIDDEN フラグ付き) を含める
- `-t`, `--show-type` — 種別番号をタブ区切りで前置表示
- `-T`, `--show-type-name` — 種別名をタブ区切りで前置表示

`-t` と `-T` は排他 (同時指定不可)。

#### 出力例

```
$ jetdb tables test.mdb
Table1
Table2
Table3
Table4

$ jetdb tables -s test.mdb
MSysACEs
MSysAccessStorage
...
MSysRelationships
Table1
Table2
Table3
Table4

$ jetdb tables -t test.mdb
1	Table1
1	Table2
1	Table3
1	Table4

$ jetdb tables -T test.mdb
table	Table1
table	Table2
table	Table3
table	Table4

$ jetdb tables -s -T test.mdb
systable	MSysACEs
systable	MSysAccessStorage
...
systable	MSysRelationships
table	Table1
table	Table2
table	Table3
table	Table4
```

#### 種別名

-T で表示される種別名:

| 名前 | 条件 |
|------|------|
| table | 通常のユーザーテーブル |
| systable | SYSTEM または HIDDEN フラグ付きテーブル |

### schema — テーブルスキーマの表示

```
jetdb schema [OPTIONS] <FILE>
```

テーブルのカラム定義・インデックス・リレーションシップを表示する。
`--ddl` オプション指定時は SQL の DDL (CREATE TABLE 等) として出力する。

計算列（Access 2010 以降）には `CALC` の印を付け、Access が宣言している型ではなく、値が持つ計算結果の型で表示する。DDL では、その型の普通の列として出力するので、`export` が書き出す値をそのテーブルに入れられる。結果が Numeric の計算列には決まった精度が無いので、表示は `Numeric`、DDL は方言ごとに一番広い Numeric の型（Access は `DECIMAL(28,10)`、PostgreSQL と SQLite は `NUMERIC`、MySQL は `DECIMAL(65,30)`）になる。

#### オプション

- `-T`, `--table <NAME>` — 指定テーブルのみ表示
- `--ddl <DIALECT>` — 指定方言の DDL として出力 (sqlite, postgres, mysql, access)
- `--no-indexes` — インデックス定義を省略
- `--no-relations` — リレーションシップ定義を省略

#### 出力例

```
$ jetdb schema test.mdb -T Table1
Table: Table1

  Columns:
    A  Text(50)
    B  Text(100)
    C  Byte
    D  Int
    E  Long
    F  Double
    G  Timestamp
    H  Money
    I  Boolean

  Indexes:
    B           [B ASC]
    PrimaryKey  [A ASC]  UNIQUE REQUIRED

$ jetdb schema test.mdb --ddl sqlite -T Table1
CREATE TABLE "Table1" (
    "A" TEXT,
    "B" TEXT,
    "C" INTEGER,
    "D" INTEGER,
    "E" INTEGER,
    "F" REAL,
    "G" TEXT,
    "H" NUMERIC,
    "I" INTEGER,
    PRIMARY KEY ("A")
);

CREATE INDEX "Table1_B_idx" ON "Table1" ("B");

$ jetdb schema indexTest.mdb --ddl postgres
CREATE TABLE "Table1" (
    "id" INTEGER,
    "otherfk1" INTEGER,
    "otherfk2" INTEGER,
    "data" VARCHAR(50),
    "otherfk3" INTEGER,
    PRIMARY KEY ("id")
);

CREATE TABLE "Table2" (
    "id" INTEGER,
    "data" VARCHAR(50),
    PRIMARY KEY ("id")
);

CREATE TABLE "Table3" (
    "id" INTEGER,
    "data" VARCHAR(50),
    PRIMARY KEY ("id")
);

CREATE INDEX "Table1_id_idx" ON "Table1" ("id");

CREATE INDEX "Table2_id_idx" ON "Table2" ("id");

CREATE INDEX "Table3_id_idx" ON "Table3" ("id");

ALTER TABLE "Table1" ADD CONSTRAINT "Table2Table1"
    FOREIGN KEY ("otherfk1") REFERENCES "Table2" ("id")
    ON DELETE CASCADE;
ALTER TABLE "Table1" ADD CONSTRAINT "Table3Table1"
    FOREIGN KEY ("otherfk2") REFERENCES "Table3" ("id")
    ON UPDATE CASCADE;
```

#### DDL 方言一覧

| 名前 | 説明 |
|------|------|
| sqlite | SQLite |
| postgres | PostgreSQL |
| mysql | MySQL |
| access | Access SQL |

#### 値要求の列

プロパティ Required がはいの列（Access のテーブルのデザインの「値要求」）は、DDL でも、列の一覧の `NOT NULL` でも、`NOT NULL` にする。オートナンバー型の列は、DDL では `NOT NULL` にし、列の一覧では `AUTO` の印を付ける。

#### GUID のオートナンバー型の列

GUID のオートナンバー型（レプリケーション ID）の列には、新しい GUID を作る既定値を付ける。列が文字列のときは、jetdb が GUID を書き出す形（`{7B4C91E0-774D-4889-B11A-0B9D2CF3D167}`）で作る。

| 名前 | 列 |
|------|------|
| sqlite | `TEXT NOT NULL DEFAULT (…)`、乱数の 16 進数 |
| postgres | `UUID NOT NULL DEFAULT gen_random_uuid()`（PostgreSQL 13 以降） |
| mysql | `CHAR(38) NOT NULL DEFAULT (CONCAT('{', UPPER(UUID()), '}'))`（MySQL 8.0.13 以降） |
| access | `UNIQUEIDENTIFIER NOT NULL`、既定値なし |

Access の SQL ビューは、データベースが ANSI-92 の構文（Access のオプションの [オブジェクト デザイナー] にある「SQL Server 互換構文」）を使わない限り、`DEFAULT` を受け付けない。ANSI-92 の構文なら、列に `DEFAULT GenGUID()` を足せる。

#### 参照整合性のないリレーションシップ

Access が参照整合性を強制しないリレーションシップは制約ではないので、DDL は `FOREIGN KEY` ではなくコメント（`-- Relationship ... does not enforce referential integrity.`）として書く。Access の SQL にはコメントがないので、Access の DDL には出さない。

### queries — 保存済みクエリの管理

#### queries list — クエリ名の一覧表示

```
jetdb queries list [OPTIONS] <FILE>
```

データベースに保存されたクエリの名前一覧を表示する (1 行に 1 つ)。

##### オプション

- `-1`, `--newline` — クエリ名を1行ずつ出力 (デフォルトの動作。互換のために受け付けるだけで、何もしない)
- `-d`, `--delimiter <STRING>` — クエリ名を、この文字列で区切って 1 行に出力

##### 出力例

```
$ jetdb queries list queryTest.mdb
AppendQuery
CrosstabQuery
DataDefinitionQuery
DeleteQuery
MakeTableQuery
PassthroughQuery
SelectQuery
UnionQuery
UpdateQuery

$ jetdb queries list test.mdb
(出力なし — データベースにクエリなし)
```

#### queries show — クエリ SQL の表示

```
jetdb queries show <FILE> <QUERY_NAME>
```

指定したクエリの復元 SQL 定義を表示する。

MSysQueries に読めない行があるときは、一部が欠けたかもしれない SQL を出さずにエラーになる。
読めない行がどのクエリのものかはわからないため。その場合も、`queries list` はクエリを一覧する。

##### 出力例

```
$ jetdb queries show queryTest.mdb DeleteQuery
DELETE Table1.col1, Table1.col2, Table1.col3
FROM Table1
WHERE (((Table1.col1)>"blah"));
```

### prop — オブジェクトプロパティの表示

```
jetdb prop [OPTIONS] <FILE> <OBJECT_NAME>
```

テーブルやクエリなどのデータベースオブジェクトの LvProp (軽量プロパティ) 値を表示する。
プロパティはオブジェクト全体、カラムごと、追加プロパティの各マップに分けて出力される。
オブジェクト全体のプロパティの見出しは、`Table Properties:`、`Query Properties:`、`Linked Table Properties:`、`Database Property Properties:` のように、オブジェクトの種類を英語の語で示す。jetdb の知らない種類のオブジェクトでは `Object Properties:` になる。
オブジェクト全体のプロパティを先に出し、ほかのマップはデータベースが格納している順に続ける。
テキストの値は二重引用符で囲み、`\` と `"` を `\\` と `\"`、復帰・改行・タブを `\r`・`\n`・`\t`、そのほかの制御文字を `\u{..}` と書くので、1 つのプロパティは 1 行に収まる。

テーブルとフォームがどちらも `Customers` という名前のように、種類の違うオブジェクトが同じ名前を持つことがある。
その場合はエラーになり、種類を一覧で示すので、`--type` で 1 つを指定する。

#### オプション

- `-t`, `--type <TYPE>` — オブジェクトの種類: `table`、`query`、`form`、`report`、`macro`、`module`、`linkedtable`、`linkedodbctable`、`relationship`、`container`、`database`、`dbproperty`、`userinfo`

#### 出力例

```
$ jetdb prop test.mdb Table1
Object: Table1

  Table Properties:
    Orientation  0
    OrderByOn    no
    NameMap      (496 bytes)
    GUID         {5A29A676-1145-4D1A-AE47-9F5415CDF2F1}
    DefaultView  2

  Column: A
    ColumnWidth         -1
    ColumnOrder         0
    ColumnHidden        no
    Required            no
    AllowZeroLength     no
    DisplayControl      109
    UnicodeCompression  yes
    GUID                {E9EDD90C-CE55-4151-ABE1-A1ACE1007515}
    IMEMode             0
    IMESentenceMode     3
  ...

$ jetdb prop nwind.mdb Customers
jetdb: objects of several types are named Customers: form, macro, table; choose one with --type

$ jetdb prop --type table nwind.mdb Customers
Object: Customers
  ...
```

### vba — VBA モジュールの管理

#### vba list — VBA モジュール名の一覧表示

```
jetdb vba list [OPTIONS] <FILE>
```

データベースに含まれる VBA モジュールの名前一覧を表示する (1 行に 1 つ、名前順ソート)。
VBA プロジェクトを含まないデータベースでは何も出力しない。

##### オプション

- `-1`, `--newline` — モジュール名を1行ずつ出力 (デフォルトの動作。互換のために受け付けるだけで、何もしない)
- `-d`, `--delimiter <STRING>` — モジュール名を、この文字列で区切って 1 行に出力

##### 出力例

```
$ jetdb vba list vbaTest.mdb
Class1
Form_Form1
Module1

$ jetdb vba list -d "|" vbaTest.mdb
Class1|Form_Form1|Module1

$ jetdb vba list test.mdb
(出力なし — データベースに VBA プロジェクトなし)
```

#### vba show — VBA モジュールのソースコード表示

```
jetdb vba show <FILE> <MODULE_NAME>
```

指定した VBA モジュールのソースコードを表示する。

##### 出力例

```
$ jetdb vba show vbaTest.mdb Module1
Attribute VB_Name = "Module1"
Option Compare Database
Option Explicit

Public Function Hello() As String
    Hello = "Hello, World!"
End Function
...
```

### form — フォーム/レポートの管理

Access 97 (Jet3) のデータベースのフォームとレポートは別の形式で保存されており、jetdb は読まない。`form list` は何も出力せず、`form dump`・`form controls`・`form props` は読まないことを伝えるエラーで失敗する。

#### form list — フォーム/レポート名の一覧表示

```
jetdb form list [OPTIONS] <FILE>
```

データベース内のフォームとレポートの名前を一覧表示する（1 行に 1 つ、アルファベット順）。
フォーム/レポートがない場合は何も出力しない。

##### オプション

- `-1`, `--newline` — 1行に1つの名前を出力（デフォルトの動作。互換のために受け付けるだけで、何もしない）
- `-d`, `--delimiter <STRING>` — 名前を、この文字列で区切って 1 行に出力
- `--forms-only` — フォームのみ表示
- `--reports-only` — レポートのみ表示

##### 出力例

```
$ jetdb form list formPropTest.accdb
F_Buttons
F_Table0
F_Table1
jp_フォーム_2

$ jetdb form list --forms-only formPropTest.accdb
F_Buttons
F_Table0
F_Table1
jp_フォーム_2

$ jetdb form list --reports-only formPropTest.accdb
(出力なし — データベースにレポートなし)
```

#### form dump — バイナリストリームのダンプ

```
jetdb form dump [OPTIONS] <FILE> <NAME>
```

フォーム/レポートの生バイナリストリームを標準出力にダンプする。デフォルトは `blob`（デザイン定義本体）。分析用にファイルにリダイレクトして使用する。

##### オプション

- `-s`, `--stream <STREAM>` — ダンプするストリーム（デフォルト: `blob`）
  - `blob` — デザイン定義本体（レイアウト、コントロール、プロパティ、イベント）
  - `typeinfo` — コントロール名と型の一覧
  - `propdata` — プロパティメタデータ（小）
  - `blobdelta` — 差分データ（通常は空）
- `-t`, `--type <TYPE>` — `form` か `report`。フォームとレポートは同じ名前を持てる。その場合は、このオプションが無いとエラーになる。無いときは、その名前の唯一のフォームかレポートを読む

##### 出力例

```
$ jetdb form dump formPropTest.accdb F_Table1 > form_blob.bin
$ xxd form_blob.bin | head
00000000: 1500 1400 0000 0000 9600 1400 0000 3a01  ..............:.
...

$ jetdb form dump -s typeinfo formPropTest.accdb F_Table1 > typeinfo.bin
```

#### form controls — セクションとコントロールの一覧を表示

```
jetdb form controls [OPTIONS] <FILE> <NAME>
```

フォーム/レポートのセクションとコントロールを、Blob から一覧表示する。並びは `form props` と同じく、セクションごとにそのコントロールが続く。出力はタブ区切り: 名前、種類（jetdb の知らない種類は 16 進数の型コード）、一覧の中の順番（0 から）。

##### オプション

- `-t`, `--type <TYPE>` — `form` か `report`。フォームとレポートは同じ名前を持てる。その場合は、このオプションが無いとエラーになる。無いときは、その名前の唯一のフォームかレポートを読む

##### 出力例

```
$ jetdb form controls formPropTest.accdb F_Table1
フォームヘッダー	FormHeader	0
Auto_Logo0	Image	1
Auto_Header0	Label	2
詳細	Detail	3
ID	TextBox	4
ラベル0	Label	5
ProductName	TextBox	6
ラベル3	Label	7
Price	TextBox	8
ラベル6	Label	9
Qty	TextBox	10
ラベル9	Label	11
Text_01_SubTotal	TextBox	12
ラベル15	Label	13
空白セル20	EmptyCell	14
空白セル21	EmptyCell	15
空白セル25	EmptyCell	16
btn_msg	CommandButton	17
空白セル34	EmptyCell	18
空白セル39	EmptyCell	19
フォームフッター	FormFooter	20
```

#### form props — フォーム/レポートとコントロールのプロパティ表示

```
jetdb form props [OPTIONS] <FILE> <NAME>
```

Blob バイナリストリームをパースして、フォーム/レポート自体と各コントロールのプロパティを表示する。RecordSource、ControlSource、Filter、Caption、FontName、イベントハンドラ等が含まれる。

既知のプロパティ ID は名前で表示、未知の ID は `0xXXXX` で表示。Access 2000 形式のフォームとレポートで、jetdb が Access 2002 以降の ID を付けないプロパティは、その形式の ID に `0x8000` を足した ID（`0x8036` など）で表示。Binary 値は `(N bytes)`、GUID は `{...}`、Boolean は `yes`/`no`、色を含む数値は SaveAsText と同じ符号付きの数、値を持たずに既定と違う値を表すはい/いいえのプロパティは、SaveAsText と同じく `NotDefault` で表示。テキストの値は `prop` と同じく二重引用符で囲んでエスケープするので、1 つのプロパティは 1 行に収まる。

##### オプション

- `-t`, `--type <TYPE>` — `form` か `report`。フォームとレポートは同じ名前を持てる。その場合は、このオプションが無いとエラーになる。無いときは、その名前の唯一のフォームかレポートを読む

##### 出力例

```
$ jetdb form props formPropTest.accdb F_Table1
Form: F_Table1

  Form Properties:
    DividingLines               NotDefault
    AllowDesignChanges          NotDefault
    ...
    Filter                      "[ID] > 0"
    ...
    PrtDevMode                  (5364 bytes)
    ...

  Control: フォームヘッダー (FormHeader)
    Height     546
    BackColor  15783096
    Name       "フォームヘッダー"
    ...

  ...

  Control: ID (TextBox)
    ...
    Name           "ID"
    ControlSource  "ID"
    GUID           {AD992C6B-598C-4EFE-ACA1-6C2EAE08A5F3}
    ...

  ...

  Control: btn_msg (CommandButton)
    ...
    Name          "btn_msg"
    Caption       "btn_msg"
    OnClick       "[Event Procedure]"
    ...

  ...
```

### macro — マクロの管理

#### macro list — 名前付きマクロ名の一覧表示

```
jetdb macro list [OPTIONS] <FILE>
```

データベースに含まれる名前付きマクロの名前一覧を表示する (1 行に 1 つ、名前順ソート)。
名前付きマクロを含まないデータベースでは何も出力しない。

##### オプション

- `-1`, `--newline` — マクロ名を1行ずつ出力 (デフォルトの動作。互換のために受け付けるだけで、何もしない)
- `-d`, `--delimiter <STRING>` — マクロ名を、この文字列で区切って 1 行に出力

##### 出力例

```
$ jetdb macro list database.accdb
AutoExec
mcrConditions
mcrLongText
mcrSimple
~TMPCLPMacro

$ jetdb macro list nwind.mdb
Customer Labels Dialog
Customer Phone List
Customers
Employees (page break)
Sales Totals by Amount
Sample Autokeys
Suppliers
```

#### macro show — 名前付きマクロの表示

```
jetdb macro show [OPTIONS] <FILE> <MACRO_NAME>
```

名前付きマクロを `Application.SaveAsText` が書き出すテキスト (改行は CRLF) で表示する。Access 97 のマクロを含め、XML を持たずに保存されたマクロも同じ形で表示する。

##### オプション

- `--xml` — テキストの代わりにマクロの XML を表示。XML を持たずに保存されたマクロでは何も出力しない。

##### 出力例

```
$ jetdb macro show nwind.mdb Customers
Version =131074
PublishOption =1
ColumnsShown =3
Begin
    Comment ="Attached to the Customers form."
End
...
Begin
    MacroName ="Update Country List"
    Action ="Requery"
    Comment ="Requery the Country control."
    Argument ="Country"
End

$ jetdb macro show --xml database.accdb mcrSimple
<?xml version="1.0" encoding="UTF-16" standalone="no"?>
<UserInterfaceMacro ...><Statements><Action Name="OpenForm">...</Statements></UserInterfaceMacro>
```

#### macro embedded — フォーム/レポートの埋め込みマクロの表示

```
jetdb macro embedded [OPTIONS] <FILE> <NAME>
```

フォームまたはレポートの埋め込みマクロを、`Application.SaveAsText` がフォーム/レポートのテキストの中に書き出す `<イベント名>EmMacro = Begin` ... `End` のかたまりとして表示する (かたまり自体の字下げは付けず、改行は CRLF)。埋め込みマクロを持たないフォーム/レポートでは何も出力しない。

##### オプション

- `--xml` — テキストの代わりに各埋め込みマクロの XML を 1 つずつ、それぞれの後に改行を付けて表示。`For` 属性がコントロール名、`Event` 属性がイベント名を表す。
- `-t`, `--type <TYPE>` — `form` か `report`。フォームとレポートは同じ名前を持てる。その場合は、このオプションが無いとエラーになる。無いときは、その名前の唯一のフォームかレポートを読む

##### 出力例

```
$ jetdb macro embedded database.accdb frmEmbedded
OnLoadEmMacro = Begin
    Version =196611
    ColumnsShown =0
    Begin
        Comment ="_AXL:<?xml version='1.0' encoding='UTF-16' standalone='no'?><UserInterfaceMacro "
            "Event='OnLoad' xmlns='http://schemas.microsoft.com/office/accessservices/2009/11"
            "/application'><Statements><Action Name='SetTe"
    End
    ...
End
OnClickEmMacro = Begin
    ...
End

$ jetdb macro embedded --xml database.accdb frmEmbedded
<?xml version='1.0' encoding='UTF-16' standalone='no'?><UserInterfaceMacro Event='OnLoad' ...>...</UserInterfaceMacro>
<?xml version='1.0' encoding='UTF-16' standalone='no'?><UserInterfaceMacro For='btnHello' Event='OnClick' ...>...</UserInterfaceMacro>
```

#### macro data — テーブルのデータマクロの表示

```
jetdb macro data [OPTIONS] <FILE> <TABLE>
```

テーブルのデータマクロを `Application.SaveAsText` が書き出すテキストで表示する。1 つの `DataMacros` の XML 文書で、テーブルイベントを AfterInsert、AfterUpdate、AfterDelete、BeforeChange、BeforeDelete の順に並べ、その後に名前付きデータマクロを並べる。データマクロを持たないテーブルでは何も出力しない。

##### オプション

- `--xml` — テキストの代わりに、保存されている各データマクロの XML を 1 つずつ、それぞれの後に改行を付けて表示

##### 出力例

```
$ jetdb macro data database.accdb tblItems
<?xml version="1.0" encoding="UTF-16" standalone="no"?>
<DataMacros xmlns="..."><DataMacro Event="AfterInsert">...</DataMacro><DataMacro Event="BeforeChange">...</DataMacro></DataMacros>

$ jetdb macro data --xml database.accdb tblNamed
<?xml version="1.0" encoding="UTF-16" standalone="no"?>
<DataMacro Name="dmLog" ...><Parameters><Parameter Name="msg"/></Parameters><Statements>...</Statements></DataMacro>
```

### export — テーブルデータの CSV エクスポート

```
jetdb export [OPTIONS] <FILE> <TABLE>
```

テーブルの全行を RFC 4180 準拠の CSV として標準出力に出力する。
デフォルトでは Access が管理して隠しているカラムは除外される。システムテーブルは、全カラムがそのようなカラムなので、全カラムを出力する。

#### オプション

- `-H`, `--no-header` — ヘッダー行を省略
- `-d`, `--delimiter <CHAR>` — カラム区切り文字、1 文字 (デフォルト: `,`)。空や 2 文字以上はエラーになる。bash や zsh では `-d $'\t'` でタブを渡せる。`-d "\t"` はバックスラッシュと `t` を渡すので、エラーになる
- `-D`, `--date-format <FMT>` — 日付フォーマット、strftime サブセット (デフォルト: `%Y-%m-%d`)
- `-T`, `--datetime-format <FMT>` — 日時フォーマット、strftime サブセット (デフォルト: `%Y-%m-%d %H:%M:%S`)
- `-b`, `--bin <MODE>` — バイナリ出力モード (デフォルト: `hex`)
- `-0`, `--null <STRING>` — NULL 値の表現文字列 (デフォルト: 空文字列)
- `-B`, `--boolean-words` — 真偽値を 1/0 ではなく TRUE/FALSE で出力
- `-s`, `--system-columns` — Access が管理して隠しているカラムを含める
- `--attachments <DIR>` — 添付ファイル列のファイルを `<DIR>/<番号>/<ファイル名>` に書き出し、フィールドには `<DIR>` からのパスを出す

#### コンプレックスカラム

Access 2007 以降のコンプレックスカラムは、種類ごとに次のフィールドになる。

- 複数値: 値を `;` でつなぎ、ダブルクォートで囲む。Access の書き出しと同じ (`"value1;value4"`)。
- 添付ファイル: ファイル名を `;` でつなぎ、ダブルクォートで囲む。`--attachments` を付けると、ファイルをそのディレクトリの下に書き出し、フィールドにはそこからのパスを出す (`"2/test_data.txt;2/test_data2.txt"`)。番号は、その行で列が持つ番号。ファイル名の `/` と `\` は `_` にする。
- 追加のみの長いテキストの版の履歴: 隠しテーブルの中身の番号で、件数ではない。最新のテキストは、長いテキストの列そのものが持つ。

中身が無い行は、空のフィールドになる。

#### 出力例

```
$ jetdb export test.mdb Table1
A,B,C,D,E,F,G,H,I
"abcdefg","hijklmnop",2,222,333333333,444.555,1974-09-21,3.5000,1
"a","b",0,0,0,0,1981-12-12,0.0000,0

$ jetdb export test.mdb Table1 -H
"abcdefg","hijklmnop",2,222,333333333,444.555,1974-09-21,3.5000,1
"a","b",0,0,0,0,1981-12-12,0.0000,0

$ jetdb export test.mdb Table1 -d $'\t'
A	B	C	D	E	F	G	H	I
"abcdefg"	"hijklmnop"	2	222	333333333	444.555	1974-09-21	3.5000	1
"a"	"b"	0	0	0	0	1981-12-12	0.0000	0

$ jetdb export test.mdb Table1 -B
A,B,C,D,E,F,G,H,I
"abcdefg","hijklmnop",2,222,333333333,444.555,1974-09-21,3.5000,TRUE
"a","b",0,0,0,0,1981-12-12,0.0000,FALSE
```

#### バイナリ出力モード一覧

| 名前 | 説明 |
|------|------|
| strip | バイナリデータを省略 (空文字列) |
| raw | 生バイトとして出力 (UTF-8 lossy) |
| octal | 各バイトを \NNN 形式の8進エスケープ |
| hex | 各バイトを小文字16進数で連結 (デフォルト) |
