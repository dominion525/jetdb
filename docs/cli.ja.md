# jetdb CLI

Microsoft Access データベース (.mdb / .accdb) の読み取り専用コマンドラインツール。

## インストール

```bash
cargo install --path crates/jetdb-cli
```

## グローバルオプション

全てのサブコマンドで使用できるオプション。

- `--password <PASSWORD>` — データベースパスワード (パスワード保護された .accdb ファイル用)

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

$ jetdb tables -s test.mdb
MSysObjects
MSysACEs
Table1
Table2

$ jetdb tables -t test.mdb
1	Table1
1	Table2

$ jetdb tables -T test.mdb
table	Table1
table	Table2

$ jetdb tables -s -T test.mdb
systable	MSysObjects
systable	MSysACEs
table	Table1
table	Table2
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
    A  Text(100)
    B  Text(200)
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

データベースに保存されたクエリの名前一覧を表示する (デフォルトはスペース区切り)。

##### オプション

- `-1`, `--newline` — クエリ名を1行ずつ出力
- `-d`, `--delimiter <STRING>` — クエリ名間のカスタム区切り文字列 (デフォルト: スペース)

##### 出力例

```
$ jetdb queries list queryTest.mdb
AppendQuery CrosstabQuery DataDefinitionQuery DeleteQuery MakeTableQuery PassthroughQuery SelectQuery UnionQuery UpdateQuery

$ jetdb queries list -1 queryTest.mdb
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
DELETE [Table1].[col1], [Table1].[col2], [Table1].[col3]
FROM [Table1]
WHERE (([Table1].[col1]="foo"));
```

### prop — オブジェクトプロパティの表示

```
jetdb prop [OPTIONS] <FILE> <OBJECT_NAME>
```

テーブルやクエリなどのデータベースオブジェクトの LvProp (軽量プロパティ) 値を表示する。
プロパティはテーブル全体、カラムごと、追加プロパティの各マップに分けて出力される。

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

データベースに含まれる VBA モジュールの名前一覧を表示する (デフォルトはスペース区切り、名前順ソート)。
VBA プロジェクトを含まないデータベースでは何も出力しない。

##### オプション

- `-1`, `--newline` — モジュール名を1行ずつ出力
- `-d`, `--delimiter <STRING>` — モジュール名間のカスタム区切り文字列 (デフォルト: スペース)

##### 出力例

```
$ jetdb vba list vbaTest.mdb
Class1 Form_Form1 Module1

$ jetdb vba list -1 vbaTest.mdb
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

#### form list — フォーム/レポート名の一覧表示

```
jetdb form list [OPTIONS] <FILE>
```

データベース内のフォームとレポートの名前を一覧表示する（デフォルトはスペース区切り、アルファベット順）。
フォーム/レポートがない場合は何も出力しない。

##### オプション

- `-1`, `--newline` — 1行に1つの名前を出力
- `-d`, `--delimiter <STRING>` — 名前の間の区切り文字（デフォルト: スペース）
- `--forms-only` — フォームのみ表示
- `--reports-only` — レポートのみ表示

##### 出力例

```
$ jetdb form list database.accdb
F_メニュー F_クライアント一覧 R_月次レポート

$ jetdb form list -1 --forms-only database.accdb
F_メニュー
F_クライアント一覧

$ jetdb form list --reports-only database.accdb
R_月次レポート
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

##### 出力例

```
$ jetdb form dump database.accdb F_メニュー > form_blob.bin
$ xxd form_blob.bin | head
00000000: 1500 1400 0000 0000 9600 1400 ...

$ jetdb form dump -s typeinfo database.accdb F_メニュー > typeinfo.bin
```

#### form controls — TypeInfo からコントロール名一覧を表示

```
jetdb form controls <FILE> <NAME>
```

TypeInfo ストリームをパースして、フォーム/レポート内の全コントロールを一覧表示する。出力はタブ区切り: 名前、型コード（16進数）、インデックス。

##### 出力例

```
$ jetdb form controls database.accdb F_クライアント一覧
フォームヘッダー    FormHeader      0
詳細                Detail          1
フォームフッター    FormFooter      2
Btn_検索            CommandButton   3
Txt_名前            TextBox         4
Cmb_ステータス      ComboBox        5
```

#### form props — フォーム/レポートとコントロールのプロパティ表示

```
jetdb form props <FILE> <NAME>
```

Blob バイナリストリームをパースして、フォーム/レポート自体と各コントロールのプロパティを表示する。RecordSource、ControlSource、Filter、Caption、FontName、イベントハンドラ等が含まれる。

既知のプロパティ ID は名前で表示、未知の ID は `0xXXXX` で表示。Binary 値は `(N bytes)`、GUID は `{...}`、Boolean は `yes`/`no` で表示。

##### 出力例

```
$ jetdb form props database.accdb F_クライアント一覧
Form: F_クライアント一覧

  Form Properties:
    RecordSource  SELECT * FROM T_クライアント ORDER BY ID;
    Filter        ([ID] > 0)
    Caption       クライアント一覧
    FontName      ＭＳ Ｐゴシック

  Control: フォームヘッダー (FormHeader)
    Name    フォームヘッダー

  Control: Txt_名前 (TextBox)
    Name           Txt_名前
    ControlSource  名前
    FontName       Meiryo UI

  Control: Cmb_ステータス (ComboBox)
    Name           Cmb_ステータス
    RowSourceType  Table/Query
    RowSource      SELECT ステータス FROM T_ステータス一覧;
    FontName       Meiryo UI
```

### macro — マクロの管理

#### macro list — 名前付きマクロ名の一覧表示

```
jetdb macro list [OPTIONS] <FILE>
```

データベースに含まれる名前付きマクロの名前一覧を表示する (デフォルトはスペース区切り、名前順ソート)。
名前付きマクロを含まないデータベースでは何も出力しない。

##### オプション

- `-1`, `--newline` — マクロ名を1行ずつ出力
- `-d`, `--delimiter <STRING>` — マクロ名間のカスタム区切り文字列 (デフォルト: スペース)

##### 出力例

```
$ jetdb macro list database.accdb
AutoExec mcrConditions mcrSimple

$ jetdb macro list -1 nwind.mdb
Customer Labels Dialog
Customers
Sample Autokeys
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
- `-d`, `--delimiter <CHAR>` — カラム区切り文字 (デフォルト: `,`)
- `-D`, `--date-format <FMT>` — 日付フォーマット、strftime サブセット (デフォルト: `%Y-%m-%d`)
- `-T`, `--datetime-format <FMT>` — 日時フォーマット、strftime サブセット (デフォルト: `%Y-%m-%d %H:%M:%S`)
- `-b`, `--bin <MODE>` — バイナリ出力モード (デフォルト: `hex`)
- `-0`, `--null <STRING>` — NULL 値の表現文字列 (デフォルト: 空文字列)
- `-B`, `--boolean-words` — 真偽値を 1/0 ではなく TRUE/FALSE で出力
- `-s`, `--system-columns` — Access が管理して隠しているカラムを含める

#### 出力例

```
$ jetdb export test.mdb Table1
A,B,C,D,E,F,G,H,I
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",1

$ jetdb export test.mdb Table1 -H
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",1

$ jetdb export test.mdb Table1 -d "\t"
A	B	C	D	E	F	G	H	I

$ jetdb export test.mdb Table1 -B
A,B,C,D,E,F,G,H,I
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",TRUE
```

#### バイナリ出力モード一覧

| 名前 | 説明 |
|------|------|
| strip | バイナリデータを省略 (空文字列) |
| raw | 生バイトとして出力 (UTF-8 lossy) |
| octal | 各バイトを \NNN 形式の8進エスケープ |
| hex | 各バイトを小文字16進数で連結 (デフォルト) |
