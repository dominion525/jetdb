# フォーム/レポート デザイン定義の内部構造

Access データベース (.mdb/.accdb) におけるフォーム/レポートのデザイン定義の格納構造に関する調査知見。

## 概要

フォーム/レポートのデザイン定義は `MSysAccessStorage` システムテーブル（Jet4/ACE 形式）に格納される。バイナリフォーマットの公開仕様は存在しない。

## MSysAccessStorage 内の格納構造

MSysAccessStorage は仮想ファイルシステム的なツリー構造を持つテーブルで、各行が1つのエントリ（フォルダまたはストリーム）を表す。

カラム構成:
- `Id` (Long, AUTO) - エントリID
- `ParentId` (Long) - 親エントリのID
- `Name` (Text) - エントリ名
- `Type` (Long) - 1=ストレージ（フォルダ）、2=ストリーム（データ）
- `Lv` (Ole) - バイナリデータ本体

### ツリー構造

```
Root (Id=1)
├── Forms (Type=1)
│   ├── DirData (Type=2)          フォーム名→ストレージ番号の対応表
│   ├── PropData (Type=2)         Formsフォルダのプロパティ
│   ├── DirDataCopy (Type=2)      DirData のバックアップ
│   ├── PropDataCopy (Type=2)     PropData のバックアップ
│   ├── "0" (Type=1)              個別フォーム（ストレージ番号 "0"）
│   │   ├── Blob (Type=2)           デザイン本体バイナリ (60-124KB)
│   │   ├── TypeInfo (Type=2)       コントロール名・型情報 (~1.6KB)
│   │   ├── PropData (Type=2)       プロパティ (22 bytes)
│   │   └── BlobDelta (Type=2)      差分データ（通常空）
│   ├── "1" (Type=1)              別のフォーム
│   │   └── (同じ4ストリーム)
│   ...
├── Reports (Type=1)              レポート（Forms と同一構造）
│   ├── DirData, PropData, ...
│   ├── "1" (Type=1)
│   │   └── Blob, TypeInfo, PropData, BlobDelta
│   ...
├── Modules (Type=1)              VBAモジュール
├── VBA (Type=1)                  VBAプロジェクト
│   └── VBAProject (Type=1)
│       ├── VBA/ (ストリーム群)
│       ├── PROJECT (Type=2)
│       └── PROJECTwm (Type=2)
├── Scripts, Cmdbars, Databases, CustomGroups, ImExSpecs
└── PropData (Type=2)             ルートプロパティ
```

## DirData フォーマット

DirData はフォーム表示名と内部ストレージ番号（"0", "1", ... 等の文字列）の対応表。

```
先頭4バイト: パディング（0x00）
繰り返し:
  0x04                  マーカーバイト
  <1バイト>             ペイロード長（バイト数）
  <UTF-16LE 文字列>     フォーム表示名 + ストレージ番号
終端4バイト: 0x00
```

ストレージ番号はペイロードの末尾 2 バイト（UTF-16LE の 1 文字）として格納される。

例:
- ストレージ "0" → F_クライアント情報
- ストレージ "1" → F_クライアント担当一覧
- ストレージ "5" → F_メニュー

## TypeInfo フォーマット

TypeInfo はフォーム内の全コントロールの名前と型コードの一覧。VBA で作ったフォームとレポートのように空のことがあり、名前の古い書き方（Blob と SaveAsText の `H_code` に対して `H_Code` のように、大文字と小文字だけ違う）を持つこともある。jetdb はコントロールの一覧を Blob から作り、TypeInfo の型コードは、大文字と小文字を区別せずに同じ名前のコントロールに付ける。

```
ヘッダ (32バイト):
  4バイト: マジックナンバー 0xACCDEAF7
  4バイト: 不明 (観測値: 0x96)
  4バイト: 不明 (観測値: 0xFFFFFFFF)
  4バイト: エントリ数
  16バイト: GUID

各エントリ:
  型コード: 2バイト (Little-Endian)
  パディング: 2バイト
  インデックス: 4バイト (Little-Endian)
  EventProcPrefix: コードページのバイト列, NUL終端
    括弧なし → コントロール名そのまま
    括弧あり → 括弧をアンダースコアに置換した名前（VBA用）
  RealName: コードページのバイト列, NUL終端
    括弧なし → 空（NULのみ）
    括弧あり → 実際のコントロール名
```

名前は、フォームを保存した Access のコードページで書かれ、ファイルはそのコードページを持っていない。jetdb は、データベースのヘッダーのコードページ、続いて Windows のコードページ（874、932、936、949、950、1250〜1258）の順に試し、ASCII 以外の文字を含む名前が、Blob のコントロールの名前（UTF-16）と大文字と小文字を区別せずにいちばん多く一致するもので読む。一致の数が同じときは、先に試したもの（ヘッダーのコードページが先）で読む。どれも一致しないときは、ヘッダーのコードページで読む。`Strings.mdb` のロシア語の名前は 1251 で、ヘッダーは 1252 である。

含まれるコントロール例:
- ボタン: `Btn_クライアント一覧`
- テキストボックス: `Txt_ログイン中のユーザー名`
- チェックボックス: `Check_管理者`
- セクション: `フォームヘッダー`, `フォームフッター`, `詳細`

### 型コード（TypeInfo type_code）マッピング

実際に使われているデータベース（81フォーム/レポート）の全コントロール調査に基づく。

フォーム系コントロール:

| コード | 名称 | 出現数 | 説明 |
|--------|------|--------|------|
| 0x066A | CheckBox | 95 | チェックボックス |
| 0x0A7A | ToggleButton | 2 | トグルボタン |
| 0x0B68 | CommandButton | 681 | コマンドボタン |
| 0x0C64 | Label | 289 | ラベル（独立） |
| 0x0D64 | Label | 662 | ラベル（コントロール付属） |
| 0x0E65 | Rectangle | 111 | 四角形 |
| 0x0F67 | Image | 1 | イメージ |
| 0x126D | TextBox | 735 | テキストボックス |
| 0x136F | ComboBox | 140 | コンボボックス |
| 0x1470 | SubForm | 9 | サブフォーム |
| 0x1666 | Line | 168 | 直線 |
| 0x1898 | Detail | 63 | 詳細セクション |
| 0x1899 | FormHeader | 63 | フォームヘッダーセクション |
| 0x189A | FormFooter | 63 | フォームフッターセクション |
| 0x1EFF | (内部メタデータ) | 318 | 正規コントロールではない。Caption/ControlSource 値や他オブジェクトのコントロール名が格納される。パーサーでは除外対象 |
| 0x247F | EmptyCell | - | 空白セル（レイアウト用） |

レポート系コントロール:

| コード | 名称 | 出現数 | 説明 |
|--------|------|--------|------|
| 0x1B64 | Label | 333 | ラベル |
| 0x1B65 | Rectangle | 14 | 四角形 |
| 0x1B66 | Line | 49 | 直線 |
| 0x1B67 | Image | 1 | イメージ |
| 0x1B68 | CommandButton | 14 | コマンドボタン |
| 0x1B6A | CheckBox | 3 | チェックボックス |
| 0x1B6D | TextBox | 554 | テキストボックス |
| 0x1B6F | ComboBox | 32 | コンボボックス |
| 0x1B70 | SubReport | 2 | サブレポート |
| 0x1998 | Detail | 18 | 詳細セクション |
| 0x1999 | ReportHeader | 18 | レポートヘッダーセクション |
| 0x199A | ReportFooter | 18 | レポートフッターセクション |
| 0x199D | GroupHeader | 29 | グループヘッダーセクション |
| 0x199E | GroupFooter | 15 | グループフッターセクション |
| 0x1F9B | PageHeader | 18 | ページヘッダーセクション |
| 0x1F9C | PageFooter | 18 | ページフッターセクション |

※ フォーム系とレポート系で対になる構造（例: 0x0C64 Label ↔ 0x1B64 Label）
※ 0x1EFF は正規のコントロールではない内部メタデータ。SaveAsText（COM経由）のコントロール一覧には対応するエントリが存在しない。Caption や ControlSource の値、あるいは関連する別オブジェクト（レポート等）のコントロール名が格納されている
※ 出現数は、実際に使われているデータベース（81フォーム/レポート）の全コントロール集計

## Blob（デザイン本体）

フォームデザインの本体バイナリ。ControlSource、イベントプロシージャ、RecordSource 等の構造情報を含む。

- サイズ: 918B（シンプル）〜 52KB+（複雑なフォーム）
- 先頭バイト: `15 00 14 00 00 00 00 00`（全フォーム共通のヘッダ）
- バイナリフォーマットの公開仕様なし。以下はリバースエンジニアリングによる解析結果

### Blob 全体レイアウト

Blob は、Access 2000 形式の Blob と同じく（[Access 2000 形式の Blob](#access-2000-形式の-blob) を参照）デザインを木の形で持ち、要素はプロパティエントリで持つ。

```
Blob      ヘッダー 8 バイト + 6 バイト + フォームやレポート自体のまとまり + 要素の並び
ヘッダー   version(u16) = 0x0015, flags(u16) = 0x0014, reserved(u32) = 0
まとまり   プロパティエントリの列。trailer が 253・254・255 のエントリで終わる
並び      trailer が 253 なら要素なし、254 なら 1 つ、255 なら数（u16）とその数の要素
要素      種類（u16） + 4 バイト + まとまり + その中の要素の並び
```

### プロパティエントリのフォーマット

プロパティ名は文字列として格納されず、数値 ID (u16) で識別される。どのエントリも同じ形をしている。

```
prop_id(u16) + type(u32) + B(u32) + length(u32) + value[length バイト] + trailer(u32)
```

`length` は値のバイト数。値の後ろの 4 バイトの trailer は、エントリごとに増えていく数。ただし、まとまりの最後のエントリの trailer は 253・254・255 のどれかで、まとまりの後ろに続く要素の並びを表す。値の読み方は type で決まる（「type の対応状況」を参照）。B は使わない。

文字列の type の値:
- 0x0A — 名前・フォント系（Name, FontName, Format, RowSourceType, ColumnWidths）
- 0x0C — テキスト・データ系（RecordSource, ControlSource, RowSource, Filter, Event, Caption）

### 要素

- 木のいちばん上の要素は、フォームやレポートの既定のコントロールと、セクション。セクションの中の要素はそのコントロール、タブコントロールの中の要素はそのページ。jetdb は、セクションごとにそのコントロールが続く順で、セクションとコントロールを出し、既定のコントロールは出さない
- 要素の種類は Access の `acControlType` で、TypeInfo の型コードの下位バイトと同じ。テストデータでは、どの要素の種類も、TypeInfo の同じ名前のコントロールの種類と一致した。ただし、TypeInfo の ActiveX コントロールより後ろの項目は、jetdb がずれて読むので除く
- コントロールは、Name（0x14）の前と後ろにプロパティを持つ。たとえば `Left`・`Top`・`Width`・`Height` は Name より前

## Access 2000 形式の Blob

Access 2000 形式で保存したフォームとレポートも、デザインを Blob に持つが、プロパティエントリではなく木の形で持つ。Blob は `0x13`（SaveAsText の `Version =19`）で始まる。Access 2002 以降の Blob は `0x14` か `0x15` で始まる。

```
Blob       ヘッダー 8 バイト + フォームやレポート自体 + 並び
自体        種類（u16、0x96 がフォーム、0x97 がレポート） + プロパティ
並び        FF + 数（u16） + 要素、または FE + 要素 1 つ
要素        種類（u16） + プロパティ + （FD、またはその中の要素の並び）
プロパティ    ID（1 バイト） + 値
```

- 木のいちばん上の要素は、フォームやレポートの既定のコントロールと、セクション。セクションの中の要素はそのコントロール、タブコントロールの中の要素はそのページ。jetdb は、後の形式と同じくセクションとコントロールを出し、既定のコントロールは出さない
- 要素の種類は Access の `acControlType` で、TypeInfo の type_code の下位バイトと同じ。100 がラベル、109 がテキストボックス、123 がタブコントロール、124 がページ、152 が詳細セクション、155・156 がページヘッダー・フッター、など
- プロパティの ID は種類ごとの番号。たとえば `0xDC` は、コントロールでは名前、フォームではレコードソース。値の大きさは ID で決まる。`0x00`〜`0x2F` は値なし（既定と違う値を表すはい/いいえで、`NotDefault` として返す）、`0x30`〜`0x5F` は 1 バイト、`0x60`〜`0x7F` は 2 バイト、`0x80`〜`0xBC` は 4 バイト、`0xBD`〜`0xFC` は長さ 1 バイトとその長さのバイト（長さが `0xFF` なら 2 バイトの長さが続く）。`0x80`〜`0xBC` で実際に見たのは `0x9C`〜`0xA0` だけで、4 バイトの値が `0xA1`〜`0xBC` のどこまでかはわかっていない

jetdb は、種類ごとの表（599 組）で、プロパティに Access 2002 以降の ID を付け、値はその ID の型で返す。そのため、[prop_id マッピング](#prop_id-マッピング) の名前がそのまま付く。そのうち 409 組は、`testdata/V2000/controlPropertyTestV2000.mdb` と `propertyIdTestV2000.mdb` のフォームとレポートを、Access for Microsoft 365 で作った同じオブジェクト（`testdata/V2007/controlPropertyTestV2007.accdb` と `propertyIdTestV2007.accdb`）と突き合わせて決めた。オブジェクトをまたいだ ID の値の並びが、後の形式の ID 1 つとだけ一致するものを組にした。さらに、その 409 組を、2000 形式のファイルそのものの SaveAsText の出力（`testdata/V2000/saveastext`。Microsoft 365 の Access がそのファイルから書き出したもので、`Version =19`）で確かめた。409 組すべてに、その出力に値があり、そのうち 404 組は、後の形式の ID の名前と同じ項目だけがその値を持つ。残る 5 組のうち 2 組（レポートの AllowDesignChanges と DateGrouping）は、その項目のほかに、同じ値を持つ項目もある。残る 3 組（フォームとレポートの `0x0172`、レポートの `0x011B`）は、[prop_id マッピング](#prop_id-マッピング) の名前の無い ID で、いくつもの項目がその値を持つ。名前と値の並びで指す ID が違うときは、名前に従って組にしている。四角形の BorderColor のように、それらのファイルで値がいつも GridlineColor（`0x01CF`）と同じものなど、10 組がそれに当たる。2000 形式に保存するとき、Access は DatasheetGridlinesColor12（`0x0247`）の値を、その形式にある DatasheetGridlinesColor（`0x013F`）として書く。後の形式の ID は Access for Microsoft 365 のもので、`0x01C4` より大きい ID は、それより前の Access のファイルより 1 つ大きい。残る 190 組は、その SaveAsText の出力だけで決めた。種類ごとに、ID のすべての値を持つ項目がそれ 1 つだけで、後の形式の ID は、[prop_id マッピング](#prop_id-マッピング) でその項目の名前が付いているものである。そのうち、フォームのウィンドウの位置の Right と Bottom、PrtDevMode など 18 組は、2 つの形式のファイルで値が違うため、値はその出力と突き合わせて確かめる。組の無いプロパティは、2000 形式の ID に `0x8000` を足した ID で返す。たとえばフォームの `0x8036` は、その出力で Cycle・DatasheetCellsEffect・DatasheetGridlinesBehavior・PictureType のどれもがその値を持つ。

## 既知の制約

### パーサーの制約

- **木**: Blob を先頭から木として読む。未知の type のエントリは長さで読み、値はバイト列として返す。木として最後まで読めないときは、バイトスキャンで Name プロパティのパターン `[14 00 0A 00 00 00]` から各コントロールを探し、Name から後ろのプロパティだけを読む。テストデータの Blob は、すべて木として最後まで読めた
- **レイアウト座標・プリンタ設定**: Blob 内の非プロパティ領域に格納されており、取得対象外
- **Jet3 (Access 97)**: MSysAccessStorage ではなく MSysAccessObjects に格納される異なるフォーマットのため非対応

### prop_id マッピング

判明済みの prop_id は名前で表示し、未知のものは `0xXXXX` 形式で数値表示する。名前は値の種類ごとに付ける。文字列の値には下の 1 つ目の表の名前を、数値とはい/いいえの値（値を持たないはい/いいえ、`NotDefault` を含む）には 2 つ目の表の名前を、バイト列と GUID には 3 つ目の表の名前を付ける。値がそれ以外の種類の ID は、その場所では別のプロパティなので数値で表示する。たとえば 0x0068 は、フォームでは OnKeyDown だが、キー操作のイベントを持たないラベルでは色を持つ。

名前は、`Sports.accdb`（ruddj/SportsAdmin）、`testdata/V2007/controlPropertyTestV2007.accdb` と `propertyIdTestV2007.accdb`、その Access 2000 形式版（`testdata/V2000/`）のフォームとレポートで、その ID のすべての値を持つ SaveAsText の項目がそれ 1 つだけのものである。値を持たないはい/いいえは、それを持つどのブロックでも、SaveAsText が `NotDefault` と書く項目で決める。名前は 0x01C4 までの ID にだけ付ける。Microsoft 365 の Access は、Anchor やレイアウトのものなど、それより大きい ID を、`Sports.accdb` を作った Access より 1 つ大きく書くため。

用途は、Microsoft の Access の Office VBA リファレンスの日本語版の、フォーム・レポート・コントロールのいずれかのプロパティのページから取った。プロパティ ウィンドウでの日本語の名前がページにあるものはその名前を、無いものはページの最初の文を書く。英語版は [MicrosoftDocs/VBA-Docs](https://github.com/MicrosoftDocs/VBA-Docs)（CC BY 4.0）にある。リファレンスに説明の無い項目は、そう書く。

文字列の値の名前:

| ID | プロパティ名 | 用途 |
|--------|--------------|------|
| 0x0007 | Picture | ピクチャ |
| 0x0011 | Caption | 標題 |
| 0x0012 | ColumnWidths | ColumnWidths プロパティを使用して、複数列コンボ ボックス内の各列の幅を指定します。 |
| 0x0014 | Name | フィールド名 |
| 0x0016 | EventProcPrefix | イベント プロシージャ名のプレフィックス部分を取得または設定します。 |
| 0x0017 | DefaultValue | 既定値 |
| 0x001B | ControlSource | コントロールソース |
| 0x0022 | FontName | フォント名 |
| 0x0026 | Format | 書式 |
| 0x0031 | LinkChildFields | リンク子フィールド |
| 0x0032 | LinkMasterFields | リンク親フィールド |
| 0x003D | ValidationText | エラーメッセージ |
| 0x0048 | InputMask | 定型入力 |
| 0x004C | OLEClass | OLEClass プロパティを使用して、グラフ コントロールまたはバインドされていないオブジェクト フレームに含まれる OLE オブジェクトの種類の説明を取得します。 |
| 0x004D | OnOpen | Microsoft の VBA のリファレンスの日本語版に、日本語の説明が無い。 |
| 0x004E | OnClose | フォームまたはレポートの [プロパティ ウィンドウ] にある [閉じるとき] ボックスの値を設定または返します。 |
| 0x004F | OnDelete | レコード削除時 |
| 0x0050 | OnFormat | フォーマット時 |
| 0x0052 | OnPrint | 印刷時 |
| 0x0053 | OnPage | レポートのプロパティ ウィンドウの [ページ内] ボックスの値を設定または返します。 |
| 0x0054 | OnRetreat | 戻り時 |
| 0x0055 | BeforeUpdate | 更新前処理 |
| 0x0056 | AfterUpdate | AfterUpdate イベントは、コントロールまたはレコードで変更されたデータが更新された直後に発生します。 |
| 0x0057 | OnCurrent | レコード移動時 |
| 0x005B | RowSource | 値集合ソース |
| 0x005D | RowSourceType | 値集合タイプ |
| 0x0068 | OnKeyDown | キークリック時 |
| 0x0069 | OnKeyUp | キー解放時 |
| 0x006A | OnKeyPress | キー入力時 |
| 0x006B | OnMouseDown | マウスボタンクリック時 |
| 0x006C | OnMouseUp | Microsoft の VBA のリファレンスの日本語版に、日本語の説明が無い。 |
| 0x006D | OnMouseMove | マウスボタン移動時 |
| 0x006E | OnTimer | タイマー時 |
| 0x0070 | OnActivate | アクティブ時 |
| 0x0071 | OnDeactivate | 非アクティブ時 |
| 0x0072 | OnChange | 変更時 |
| 0x0073 | OnGotFocus | フォーカス取得後 |
| 0x0074 | OnLostFocus | フォーカス喪失時 |
| 0x0075 | OnResize | サイズ変更時 |
| 0x0076 | OnUpdated | Microsoft の VBA のリファレンスの日本語版に、日本語の説明が無い。 |
| 0x0077 | OnError | フォームまたはレポートの [プロパティ ウィンドウ] の [エラー時] ボックスの値を設定または返します。 |
| 0x0078 | OnNotInList | リスト外入力時 |
| 0x0079 | OnNoData | 空データ時 |
| 0x007A | BeforeInsert | BeforeInsert イベントが発生したときに実行するマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列型 ( String ) の値を設定します。 |
| 0x007B | AfterInsert | AfterInsert イベントが発生したときに実行するマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列型 ( String ) の値を設定します。 |
| 0x007C | BeforeDelConfirm | BeforeDelConfirm イベントが発生したときに実行されるマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列を返すか設定します。 |
| 0x007D | AfterDelConfirm | AfterDelConfirm イベントが発生したときに実行されるマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列を返すか設定します。 |
| 0x007E | OnClick | クリック時 |
| 0x007F | OnLoad | 読み込み時 |
| 0x0080 | OnUnload | 読み込み解除時 |
| 0x0084 | SourceObject | ソースオブジェクト |
| 0x0087 | StatusBarText | ステータスバーテキスト |
| 0x0091 | ValidationRule | 入力規則 |
| 0x009C | RecordSource | レコードソース |
| 0x00A0 | DatasheetFontName | DatasheetFontName プロパティを使用して、データシート ビューでフィールド名とデータの表示と印刷に使用するフォントを指定します。 |
| 0x00DA | HelpFile | フォームに関連付けられているヘルプ ファイルの名前。 |
| 0x00DE | OnEnter | フォーカス取得時 |
| 0x00DF | OnExit | フォーカス喪失時 |
| 0x00E0 | OnDblClick | ダブルクリック時 |
| 0x00F0 | MenuBar | フォームに表示するカスタム メニューを指定します。 |
| 0x00F5 | Filter | フィルター |
| 0x010A | Tag | タグ |
| 0x0112 | Class | クラス |
| 0x0130 | ShortcutMenuBar | ショートカットメニューバー |
| 0x013D | ControlTipText | ヒントテキスト |
| 0x014B | OrderBy | 並べ替え |
| 0x014D | OnFilter | フィルター設定時 |
| 0x014E | OnApplyFilter | フィルター実行時 |
| 0x0157 | HyperlinkAddress | ハイパーリンクアドレス |
| 0x015A | ColumnInfo | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x015B | HyperlinkSubAddress | ハイパーリンクサブアドレス |
| 0x0161 | Toolbar | フォームに表示するカスタム ツールバーを指定します。 |
| 0x0167 | FuriganaControl | ふりがな |
| 0x0168 | PostalAddress | PostalAddress プロパティを使用して、指定したフィールドまたはテキスト ボックスに表示される住所情報に対応する郵便番号と顧客バーコード データを指定または決定します。 |
| 0x0177 | OnDirty | Microsoft の VBA のリファレンスの日本語版に、日本語の説明が無い。 |
| 0x0194 | OnUndo | 元に戻すイベントが発生したときに実行されるマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列を返すか設定します。 |
| 0x01A9 | OnConnect | OnConnect イベントが発生したときに実行されるマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列を返すか設定します。 |
| 0x01AA | OnDisconnect | OnDisconnect イベントが発生したときに実行するマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列型 ( String ) の値を指定します。 |
| 0x01AB | OnPivotTableChange | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01AC | OnQuery | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01AD | BeforeQuery | BeforeQuery イベントが発生したときに実行するマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列型 ( String ) の値を取得または設定します。 |
| 0x01AE | OnSelectionChange | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01AF | OnCmdBeforeExecute | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01B0 | OnCmdChecked | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01B1 | OnCmdEnabled | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01B2 | OnCmdExecute | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01B3 | OnDataSetChange | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01B4 | BeforeScreenTip | BeforeScreenTip イベントが発生したときに実行するマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列型 ( String ) の値を取得または設定します。 |
| 0x01B5 | AfterFinalRender | AfterFinalRender イベントが発生したときに実行されるマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列を返すか設定します。 |
| 0x01B6 | AfterRender | AfterRender イベントが発生したときに実行するマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列型 ( String ) の値を設定します。 |
| 0x01B7 | AfterLayout | AfterLayout イベントが発生したときに実行されるマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列型 (String) の値を取得または設定します。 |
| 0x01B8 | BeforeRender | BeforeRender イベントが発生したときに実行されるマクロ、イベント プロシージャ、またはユーザー定義関数を示す文字列を返すか設定します。 |
| 0x01B9 | OnMouseWheel | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01BA | OnViewChange | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01BB | OnDataChange | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |

数値とはい/いいえの値の名前:

| ID | プロパティ名 | 用途 |
|--------|--------------|------|
| 0x0002 | AccessKey | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x0004 | SpecialEffect | 立体表示 |
| 0x0006 | AutoResize | サイズ自動修正 |
| 0x0008 | BorderColor | 境界線色 |
| 0x0009 | BorderStyle | 境界線スタイル |
| 0x000A | BorderWidth | BorderWidth プロパティを使用して、コントロールの境界線の幅を指定します。 |
| 0x000B | BorderLineStyle | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x000C | Bottom | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x000D | BoundColumn | 連結列 |
| 0x000E | CanGrow | 印刷時拡張 |
| 0x000F | RepeatSection | セクション繰り返し |
| 0x0010 | CanShrink | 印刷時縮小 |
| 0x0013 | RecordSelectors | RecordSelectors プロパティを使用して、フォームでフォーム ビューにレコード セレクターを表示するかどうかを指定します。 |
| 0x0019 | Enabled | 使用可能 |
| 0x001C | BackColor | 背景色 |
| 0x001D | BackStyle | BackStyle プロパティを使用して、コントロールを透過的にするかどうかを指定します。 |
| 0x001E | AllowFilters | AllowFilters プロパティを使用して、フォーム内のレコードをフィルター処理できるかどうかを指定します。 |
| 0x0021 | FontItalic | フォント斜体 |
| 0x0023 | FontSize | フォントサイズ |
| 0x0024 | FontUnderline | フォント下線 |
| 0x0025 | FontWeight | フォント太さ |
| 0x0029 | GridX | GridX プロパティを (GridY プロパティと共に) 使用して、フォーム デザイン ビューでの配置グリッドの水平方向と垂直方向の分割を指定します。 |
| 0x002A | GridY | GridY プロパティを (GridX プロパティと共に) 使用して、フォーム デザイン ビューで配置グリッドの水平方向と垂直方向の分割を指定します。 |
| 0x002C | Height | 高さ |
| 0x002D | AllowUpdating | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x002E | Transparent | Transparent プロパティを使用して、コマンド ボタンを単色か透明にするかを指定します。 |
| 0x0036 | Left | 左位置 |
| 0x0037 | LineSlant | 線傾斜 |
| 0x0038 | Locked | 編集ロック |
| 0x003A | OptionValue | オプション値 |
| 0x003E | Modal | 作業ウィンドウ固定 |
| 0x0043 | LimitToList | 入力チェック |
| 0x0044 | AutoExpand | 自動展開プロパティを使うと、コンボ ボックスのテキスト ボックス部分に、入力時に入力した文字と一致するコンボ ボックス リストの値が Microsoft Access によって自動的に入力されるかどうかを指定します。 |
| 0x0045 | HideDuplicates | 重複データ非表示 |
| 0x0046 | ColumnCount | 列数 |
| 0x0047 | DecimalPlaces | 小数点以下表示桁数 |
| 0x004B | KeepTogether | 同一ページ印刷 |
| 0x0059 | SizeMode | SizeMode プロパティを使用して、バインドされたオブジェクト フレーム、バインドされていないオブジェクト フレーム、またはイメージ コントロール内の画像またはその他のオブジェクトのサイズを指定する方法を指定します。 |
| 0x005A | PopUp | ポップアップ |
| 0x005C | RowSourceTypeInt | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x005E | MaxButton | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x005F | MinButton | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x0061 | ControlBox | コントロールボックス |
| 0x0065 | AutoActivate | 自動アクティブ |
| 0x006F | TimerInterval | タイマー間隔 |
| 0x0081 | Right | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x0082 | RunningSum | 集計実行 |
| 0x0083 | ColumnHeads | 列見出し |
| 0x0088 | TextAlign | 文字配置 |
| 0x008D | Top | 上位置 |
| 0x008F | LayoutForPrint | プリンターフォント |
| 0x0090 | FastLaserPrinting | 高速レーザー印刷 |
| 0x0092 | ViewsAllowed | ビュー設定 |
| 0x0093 | DefaultView | 既定のビュー |
| 0x0094 | Visible | 可視 |
| 0x0095 | DisplayWhen | 表示対象 |
| 0x0096 | Width | 幅 |
| 0x0097 | ForceNewPage | 改ページ |
| 0x0098 | ScrollBars | スクロールバー |
| 0x009A | ListWidth | リスト幅 |
| 0x009D | BreakLevel | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x009E | ItemSuffix | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x009F | OverlapFlags | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x00A1 | DatasheetFontHeight | DatasheetFontHeight プロパティを使用して、データシート ビューでフィールド名とデータの表示と印刷に使用するフォント ポイント サイズを指定します。 |
| 0x00A2 | DatasheetFontWeight | DatasheetFontWeight プロパティを使用して、データシート ビューでフィールド名とデータの文字を表示および印刷するために使用するフォントの線幅を指定します。 |
| 0x00A5 | ColumnOrder | データシート ビューで列の順序を指定するには、 ColumnOrder プロパティを使用します。 |
| 0x00A6 | ColumnWidth | データシート ビューで列の幅を指定するには、 ColumnWidth プロパティを使用します。 |
| 0x00A7 | ColumnHidden | ColumnHidden プロパティを使用すると、データシート ビューで指定した列を表示または非表示にできます。 |
| 0x00A8 | RowHeight | RowHeight プロパティを使用して、データシート ビューのすべての行の高さを指定します。 |
| 0x00C2 | TabularCharSet | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x00CC | ForeColor | 前景色 |
| 0x00DB | HelpContextId | HelpContextID プロパティは、HelpFile プロパティ設定によって指定されたカスタム ヘルプ ファイル内のトピックのコンテキスト ID を指定します。 |
| 0x00DC | Default | 既定のボタン |
| 0x00DD | Cancel | キャンセルボタン |
| 0x00F3 | TextFontFamily | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x00F4 | TabularFamily | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x0105 | TabIndex | タブ移動順 |
| 0x0106 | TabStop | タブストップ |
| 0x0116 | AutoCenter | 自動中央寄せ |
| 0x0117 | NavigationButtons | NavigationButtons プロパティを使用して、ナビゲーション ボタンとレコード番号ボックスをフォーム上に表示するかどうかを指定します。 |
| 0x011A | DateGrouping | 日付グループ化 |
| 0x0126 | TripleState | TripleState プロパティを使用して、指定したコントロールで Null 値を表示する方法を指定します。 |
| 0x012F | CloseButton | 閉じるボタン |
| 0x0132 | DatasheetForeColor | Visual Basic の DatasheetForeColor プロパティを使用すると、Access データベース内のデータシート ビューのテーブル、クエリ、またはフォームのすべてのテキストの色を指定または決定できます。 |
| 0x0135 | PictureAlignment | ピクチャ配置 |
| 0x0139 | PictureTiling | ピクチャ全体表示 |
| 0x013A | DividingLines | DividingLines プロパティを使用すると、フォームのセクションを区切るか、連続したフォームに表示されるレコードを区切るかを指定します。 |
| 0x013E | DatasheetGridlinesBehavior | DatasheetGridlinesBehavior プロパティを使用して、データシート ビューに表示するグリッド線を指定します。 |
| 0x013F | DatasheetGridlinesColor | データシート内の目盛線の色を指定するには、 DatasheetGridlinesColor プロパティを使用します。 |
| 0x0146 | AllowAdditions | 追加の許可 |
| 0x0149 | OldBorderStyle | このプロパティを使用して、フォームまたはコントロールの BorderStyle プロパティの未編集の値を設定または返します。 |
| 0x0150 | OrderByOn | 並べ替え適用 |
| 0x0154 | AllowAutoCorrect | AllowAutoCorrect プロパティを使用して、指定したコントロールがユーザーによって行われたエントリを自動的に修正するかどうかを指定します。 |
| 0x0155 | PictureType | ピクチャタイプ |
| 0x015C | MultiRow | 複数行タブ |
| 0x015E | TabFixedHeight | タブ コントロールのタブの高さを指定または決定するには、 TabFixedHeight プロパティを使用します。 |
| 0x015F | TabFixedWidth | タブ コントロールのタブの幅を指定または決定するには、 TabFixedWidth プロパティを使用します。 |
| 0x0163 | Vertical | 縦書き |
| 0x0165 | IMEHold | IMEHold/Hold KanjiConversionMode プロパティを使用して、コントロールがフォーカスを失ったときに漢字変換モードが維持されるかどうかを表示します。 |
| 0x016C | RecSrcDt | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。8 バイトの日時。SaveAsText ではバイナリのブロックに書かれる。 |
| 0x0170 | KeyboardLanguage | Microsoft の VBA のリファレンスの日本語版に、日本語の説明が無い。 |
| 0x0175 | AllowDesignChanges | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x017B | IMESentenceMode | IME 変換モード |
| 0x0180 | LeftMargin | 左余白 |
| 0x0181 | TopMargin | 上余白 |
| 0x0182 | LineSpacing | LineSpacing プロパティを使用して、ラベル コントロールまたはテキスト ボックス コントロール内に表示される情報の場所を指定または決定します。 |
| 0x0183 | SubdatasheetHeight | サブデータシートの高さ |
| 0x0184 | RightMargin | TopMargin、LeftMargin、および BottomMargin プロパティとともに、テキスト ボックス コントロール内に表示される情報の場所を指定します。 |
| 0x0185 | BottomMargin | LeftMargin、RightMargin、および TopMargin プロパティとともに、テキスト ボックス コントロール内に表示される情報の場所を指定します。 |
| 0x018C | IsHyperlink | IsHyperlink プロパティを使用して、テキスト ボックスに含まれるデータがハイパーリンクであるかどうかを指定または決定します。 |
| 0x01A6 | AllowDatasheetView | 指定したフォームをデータシート ビューで表示できるかどうかを示す ブール型 (Boolean) の値を取得または設定します。 |
| 0x01BE | AsianLineBreak | 禁則処理 |
| 0x01C2 | GroupTable | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x01C4 | FilterOnLoad | フォームの読み込み時に Filter プロパティで指定されたフィルターを適用するかどうかを取得または設定します。 |

バイト列と GUID の値の名前:

| ID | プロパティ名 | 用途 |
|--------|--------------|------|
| 0x0063 | ObjectPalette | ObjectPalette プロパティは、Picture プロパティを使用して指定されたコントロールに読み込まれるビットマップまたはその他のグラフィックを作成するために使用されるアプリケーションのパレットを指定します。 |
| 0x0064 | PaintPalette | PaintPalette プロパティを使用して、フォームで使用するパレットを指定します。 |
| 0x00AB | OleData | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |
| 0x00BC | PictureData | PictureData プロパティを使用して、Picture プロパティをサポートする別のオブジェクトにピクチャをコピーします。 |
| 0x00BD | PrtMip | Visual Basic の PrtMip プロパティを使用して、[ 印刷 ] ダイアログ ボックスでフォームまたはレポートに対して指定されたデバイス モード情報を設定または返します。 |
| 0x00BE | PrtDevMode | [印刷] ダイアログ ボックスでフォームまたはレポートに対して指定された印刷デバイス モード情報を設定または返すには、PrtDevMode プロパティを使用します。 |
| 0x00BF | PrtDevNames | PrtDevNames プロパティを使用して、フォームまたはレポートの [印刷] ダイアログ ボックスで選択されたプリンターに関する情報を設定または返します。 |
| 0x0178 | GUID | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。16 バイトの GUID。SaveAsText ではバイナリのブロックに書かれる。 |
| 0x0179 | NameMap | Microsoft の VBA のリファレンスに無い。SaveAsText の項目名。 |

頻出する未知 ID:

| ID | 出現コンテキスト（推定） |
|--------|------|
| 0x0000 | コントロール末尾に頻出 |
| 0x0268 | 色系プロパティ |
| 0x0274 | 色系プロパティ |
| 0x01CA〜0x01D2 | コントロール内の整数（連番で出現） |

### type の対応状況

| type | 値 | 対応状況 |
|------|----|----------|
| 0x01 | はい/いいえ: 既定と違う値のプロパティは値のバイトが無い（SaveAsText は `NotDefault` と書く）、1 バイトは値（0 がいいえ）、2 か 4 バイトは `Overlaps` のような整数 | 対応済み |
| 0x02 | 1 バイトの整数（符号なし） | 対応済み |
| 0x03 | 2 か 4 バイトの整数（符号付き） | 対応済み |
| 0x04 | 4 バイトの 32 ビットの値（多くは色）。SaveAsText と同じ符号付きの数で返す。色は `0x00BBGGRR`、システム色は負の数 | 対応済み |
| 0x06 | 4 バイトの単精度の小数（`AlternateBackShade` など） | 対応済み |
| 0x08 | 8 バイトの倍精度の小数 | 対応済み |
| 0x09 | 16 バイトの GUID | 対応済み |
| 0x0A | Text（UTF-16LE） | 対応済み |
| 0x0B | Binary | 対応済み |
| 0x0C | Memo（UTF-16LE） | 対応済み |
| その他 | (不明。0x11 など) | 長さで読み、バイト列として返す |

type に無い長さの値は、そのバイト列として返す。Sports.accdb の 116 のフォームとレポートのうち、プロジェクトが SaveAsText の出力を公開している 115 の値を、その出力と突き合わせた。SaveAsText が書く整数とはい/いいえの値は、すべて一致した。

## 参考リンク

- MDB Tools HACKING.md: https://github.com/mdbtools/mdbtools/blob/main/HACKING.md
- jabakobob.net Unofficial MDB Guide: http://jabakobob.net/mdb/
- isladogs.co.uk Long Value Binary fields: https://www.isladogs.co.uk/lv-fields/index.html
