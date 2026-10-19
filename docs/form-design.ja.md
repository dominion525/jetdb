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

名前は、フォームを保存した Access のコードページで書かれ、ファイルはそのコードページを持っていない。jetdb はデータベースのヘッダーのコードページで読む。`Strings.mdb` のロシア語の名前は 1251 で、ヘッダーは 1252 なので、文字化けする。

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

jetdb は、種類ごとの表（409 組）で、プロパティに Access 2002 以降の ID を付け、値はその ID の型で返す。そのため、[prop_id マッピング](#prop_id-マッピング) の名前がそのまま付く。組は、`testdata/V2000/controlPropertyTestV2000.mdb` と `propertyIdTestV2000.mdb` のフォームとレポートを、Access for Microsoft 365 で作った同じオブジェクト（`testdata/V2007/controlPropertyTestV2007.accdb` と `propertyIdTestV2007.accdb`）と突き合わせて決めた。オブジェクトをまたいだ ID の値の並びが、後の形式の ID 1 つとだけ一致するものを組にした。両方の形式で名前がわかる組 307 組は、すべて名前が一致した。さらに、2000 形式のファイルそのものの SaveAsText の出力（`testdata/V2000/saveastext`。Microsoft 365 の Access がそのファイルから書き出したもので、`Version =19`）で、組を確かめた。409 組のうちデータにある 386 組のうち、381 組は両方の形式で同じ名前の項目にある。残る 4 組は、両方の形式でいつも同じ値を持つ項目どうしにあり、データからは区別できない。名前と値の並びで指す ID が違うときは、名前に従って組にしている。四角形の BorderColor のように、それらのファイルで値がいつも GridlineColor（`0x01CF`）と同じものなど、10 組がそれに当たる。2000 形式に保存するとき、Access は DatasheetGridlinesColor12（`0x0247`）の値を、その形式にある DatasheetGridlinesColor（`0x013F`）として書く。後の形式の ID は Access for Microsoft 365 のもので、`0x01C4` より大きい ID は、それより前の Access のファイルより 1 つ大きい。組の無いプロパティは、2000 形式の ID に `0x8000` を足した ID（`0x80DD` など）で返す。その多くは、後の形式でコントロールが名前より前に持つプロパティで、組を決めたときには突き合わせていない

## 既知の制約

### パーサーの制約

- **木**: Blob を先頭から木として読む。未知の type のエントリは長さで読み、値はバイト列として返す。木として最後まで読めないときは、バイトスキャンで Name プロパティのパターン `[14 00 0A 00 00 00]` から各コントロールを探し、Name から後ろのプロパティだけを読む。テストデータの Blob は、すべて木として最後まで読めた
- **レイアウト座標・プリンタ設定**: Blob 内の非プロパティ領域に格納されており、取得対象外
- **Jet3 (Access 97)**: MSysAccessStorage ではなく MSysAccessObjects に格納される異なるフォーマットのため非対応

### prop_id マッピング

判明済みの prop_id は名前で表示し、未知のものは `0xXXXX` 形式で数値表示する。名前は値の種類ごとに付ける。文字列の値には下の 1 つ目の表の名前を、整数とはい/いいえの値には 2 つ目の表の名前を付ける。値がそれ以外の種類の ID は、その場所では別のプロパティなので数値で表示する。たとえば 0x0068 は、フォームでは OnKeyDown だが、キー操作のイベントを持たないラベルでは色を持つ。名前は、データにあるものについて、Access の SaveAsText の出力と突き合わせて確かめた。突き合わせたのは、`Sports.accdb`（ruddj/SportsAdmin）のフォームとレポート 116 個と、`testdata/V2007/controlPropertyTestV2007.accdb`・`propertyIdTestV2007.accdb` とその Access 2000 形式版（`testdata/V2000/`）のフォームとレポートで、後者は SaveAsText の出力を表 `tblGenExport` に持っている。どの値も、その名前のプロパティと一致した。

| ID | プロパティ名 | 用途 |
|--------|--------------|------|
| 0x0011 | Caption | コントロール表示テキスト |
| 0x0012 | ColumnWidths | コンボボックス列幅 |
| 0x0014 | Name | コントロール名 |
| 0x0016 | EventProcPrefix | イベントプロシージャ名の接頭辞 |
| 0x0017 | DefaultValue | 既定値 |
| 0x001B | ControlSource | フィールドバインド先 |
| 0x0022 | FontName | フォント名（コントロールレベル） |
| 0x0026 | Format | 表示書式 |
| 0x004C | OLEClass | オブジェクトフレームの OLE クラス |
| 0x004D | OnOpen | 開く時 |
| 0x004E | OnClose | 閉じる時 |
| 0x0056 | AfterUpdate | 更新後処理 |
| 0x005B | RowSource | コンボボックスデータソース (SQL) |
| 0x005D | RowSourceType | データソース種別 (Table/Query) |
| 0x0068 | OnKeyDown | キークリック時 |
| 0x0069 | OnKeyUp | キー解放時 |
| 0x006A | OnKeyPress | キー入力時 |
| 0x006B | OnMouseDown | マウスボタンクリック時 |
| 0x006C | OnMouseUp | マウスボタン解放時 |
| 0x006D | OnMouseMove | マウスボタン移動時 |
| 0x0073 | OnGotFocus | フォーカス取得後 |
| 0x0074 | OnLostFocus | フォーカス喪失後 |
| 0x0079 | OnNoData | レポートのデータなし時 |
| 0x007E | OnClick | クリック時 |
| 0x007F | OnLoad | 読み込み時 |
| 0x0084 | SourceObject | サブフォームに表示するフォーム・レポート |
| 0x0087 | StatusBarText | ステータスバーテキスト |
| 0x009C | RecordSource | フォーム/レポートのデータソース |
| 0x00A0 | DatasheetFontName | データシートビューのフォント名 |
| 0x00DA | HelpFile | ヘルプファイル |
| 0x00DE | OnEnter | フォーカス取得時 |
| 0x00DF | OnExit | フォーカス喪失時 |
| 0x00E0 | OnDblClick | ダブルクリック時 |
| 0x00F5 | Filter | フィルタ条件 |
| 0x010A | Tag | タグ |
| 0x0112 | Class | オブジェクトフレームのクラス |
| 0x0130 | ShortcutMenuBar | ショートカットメニュー |
| 0x013D | ControlTipText | コントロールヒントテキスト |
| 0x015A | ColumnInfo | コンボボックスやリストボックスの列の情報 |
| 0x0161 | Toolbar | ツールバー |

整数とはい/いいえの値の名前は、`Sports.accdb` と、Access でプロパティを 1 つずつ設定して作った `testdata/V2007/propertyIdTestV2007.accdb` の、その ID が出てくるすべてのフォームとレポートで、値が一致する SaveAsText のキーである。名前を付けるのは、1 つのキーがすべての値に合うよう値がいろいろに変わる ID か、`propertyIdTestV2007.accdb` でそのプロパティを設定すると値が変わる ID である。名前は 0x01C4 までの ID にだけ付ける。Microsoft 365 の Access は、Anchor やレイアウトのものなど、それより大きい ID を、`Sports.accdb` を作った Access より 1 つ大きく書くため。

| ID | プロパティ名 | 用途 |
|--------|--------------|------|
| 0x0009 | BorderStyle | 境界線のスタイル |
| 0x000C | Bottom | 保存されたフォームやレポートのウィンドウの下端 |
| 0x0029 | GridX | 横方向のグリッドの間隔 |
| 0x002A | GridY | 縦方向のグリッドの間隔 |
| 0x0036 | Left | 保存されたフォームやレポートのウィンドウの左端 |
| 0x006F | TimerInterval | Timer イベントの間隔（ミリ秒） |
| 0x0081 | Right | 保存されたフォームやレポートのウィンドウの右端 |
| 0x008D | Top | 保存されたフォームやレポートのウィンドウの上端 |
| 0x0092 | ViewsAllowed | 許可するビュー |
| 0x0093 | DefaultView | 既定のビュー |
| 0x0096 | Width | 幅 |
| 0x0098 | ScrollBars | 表示するスクロールバー |
| 0x009E | ItemSuffix | 次のコントロール名に使う番号 |
| 0x00A1 | DatasheetFontHeight | データシートのフォントサイズ |
| 0x00DB | HelpContextId | ヘルプのコンテキスト ID |
| 0x0105 | TabIndex | タブの順番 |
| 0x0135 | PictureAlignment | ピクチャの配置 |
| 0x013E | DatasheetGridlinesBehavior | データシートの枠線の表示 |
| 0x013F | DatasheetGridlinesColor | データシートの枠線の色 |
| 0x01A6 | AllowDatasheetView | データシート ビューを許可する（はい/いいえ） |
| 0x01C4 | FilterOnLoad | 読み込み時にフィルターを適用する（はい/いいえ） |

頻出する未知 ID:

| ID | 出現コンテキスト（推定） |
|--------|------|
| 0x0000 | コントロール末尾に頻出 |
| 0x0013 | フォームレベルのはい/いいえ。値を持たない（`NotDefault`） |
| 0x001D | コントロール内 |
| 0x0178 | GUID（全コントロールに出現） |
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
