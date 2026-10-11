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

TypeInfo はフォーム内の全コントロールの名前と型コードの一覧。

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
  EventProcPrefix: Shift-JIS (cp932), NUL終端
    括弧なし → コントロール名そのまま
    括弧あり → 括弧をアンダースコアに置換した名前（VBA用）
  RealName: Shift-JIS (cp932), NUL終端
    括弧なし → 空（NULのみ）
    括弧あり → 実際のコントロール名
```

含まれるコントロール例:
- ボタン: `Btn_クライアント一覧`
- テキストボックス: `Txt_ログイン中のユーザー名`
- チェックボックス: `Check_管理者`
- セクション: `フォームヘッダー`, `フォームフッター`, `詳細`

### 型コード（TypeInfo type_code）マッピング

IKP データベース（81フォーム/レポート）の全コントロール調査に基づく。

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
※ 出現数は IKP データベース（81フォーム/レポート）の全コントロール集計

## Blob（デザイン本体）

フォームデザインの本体バイナリ。ControlSource、イベントプロシージャ、RecordSource 等の構造情報を含む。

- サイズ: 918B（シンプル）〜 52KB+（複雑なフォーム）
- 先頭バイト: `15 00 14 00 00 00 00 00`（全フォーム共通のヘッダ）
- バイナリフォーマットの公開仕様なし。以下はリバースエンジニアリングによる解析結果

### Blob 全体レイアウト

```
[ヘッダ 8B]
  version(u16) = 0x0015, flags(u16) = 0x0014, reserved(u32) = 0
[フォームレベル固定長プロパティ群]
  RecordSource, Filter, FontName 等のフォーム全体に関わるプロパティ
[フォームレベル可変長プロパティ群]
[バイナリデータ（レイアウト情報、プリンタ設定、フォント定義等）]
[コントロール別プロパティブロック ×N]
  各コントロールが独立したプロパティブロックを持つ
  Name(0x14) プロパティで始まる
```

### プロパティエントリのフォーマット

プロパティ名は文字列として格納されず、数値 ID (u16) で識別される。

**可変長プロパティ（文字列系）:**
```
prop_id(u16) + unknown(u16) + type(u32) + flags(u32) + byte_length(u32) + UTF-16LE data
```

type の値:
- 0x0A — 名前・フォント系（Name, FontName, Format, RowSourceType, ColumnWidths）
- 0x0C — テキスト・データ系（RecordSource, ControlSource, RowSource, Filter, Event, Caption）

**固定長プロパティ（数値系）:**
```
prop_id(u16) + unknown(u16) + type(u32) + flags(u32) + data[type依存]
```

type の値:
- 0x01 — Bool (4B)
- 0x02 — Short (4B)
- 0x03 — Long (6B)
- 0x04 — Color (8B)
- 0x08 — Double (8B)
- 0x09 — GUID (16B)
- 0x0B — Binary (可変長)

### セクション境界

- フォームレベルプロパティとコントロール別プロパティの境界は、バイナリデータ（レイアウト情報等）を挟んで分かれている
- 各コントロールブロックは Name(0x14) プロパティで始まる
- TypeInfo のコントロール数 = Blob 内のコントロールブロック数

## 既知の制約

### パーサーの制約

- **フォームレベル**: Blob 先頭から逐次パースするが、未知の type に遭遇するとそこでパースを中断する。中断以降のフォームレベルプロパティは取得できない。現状確認しているデータでは主要プロパティ（RecordSource, Filter, Caption, FontName 等）は全て中断前に出現しており実用上の問題は未確認
- **コントロールレベル**: フォームレベルパースとは別にバイトスキャンで Name プロパティのパターン `[14 00 0A 00 00 00]` を検出し、各コントロールセクションを個別にパースする。コントロール内で未知の type に遭遇した場合、そのコントロールの残りのプロパティは取得できない
- **レイアウト座標・プリンタ設定**: Blob 内の非プロパティ領域に格納されており、取得対象外
- **Jet3 (Access 97)**: MSysAccessStorage ではなく MSysAccessObjects に格納される異なるフォーマットのため非対応

### prop_id マッピング

判明済みの prop_id は名前で表示し、未知のものは `0xXXXX` 形式で数値表示する。名前の付くプロパティの値はすべて文字列で、値が文字列でない ID は、その場所では別のプロパティなので数値で表示する。たとえば 0x0068 は、フォームでは OnKeyDown だが、キー操作のイベントを持たないラベルでは色を持つ。名前は、データにあるものについて、`Sports.accdb`（ruddj/SportsAdmin）のフォームとレポート 116 個の、Access の SaveAsText の出力と突き合わせて確かめた。どの値も、その名前のプロパティと一致した。ただし、12 個のフォームとレポートでは、フォーム・レポート自体の FontName が、SaveAsText の出力に無かった。

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
| 0x00A0 | FontName | フォームレベルフォント名 |
| 0x00DA | HelpFile | ヘルプファイル |
| 0x00DE | OnEnter | フォーカス取得時 |
| 0x00DF | OnExit | フォーカス喪失時 |
| 0x00E0 | OnDblClick | ダブルクリック時 |
| 0x00F5 | Filter | フィルタ条件 |
| 0x010A | LabelType | ラベル種別 |
| 0x0112 | Class | オブジェクトフレームのクラス |
| 0x0130 | ShortcutMenuBar | ショートカットメニュー |
| 0x013D | ControlTipText | コントロールヒントテキスト |
| 0x015A | InputMask | 入力マスク |
| 0x0161 | Toolbar | ツールバー |

頻出する未知 ID:

| ID | 出現コンテキスト（推定） |
|--------|------|
| 0x0000 | コントロール末尾に頻出 |
| 0x0013 | フォームレベル Bool |
| 0x001D | コントロール内 |
| 0x0178 | GUID（全コントロールに出現） |
| 0x024A | コントロール内 Short |
| 0x024C | コントロール内 Short |
| 0x0268 | 色系プロパティ |
| 0x0274 | 色系プロパティ |
| 0x01CA〜0x01D2 | コントロール内 Short（連番で出現） |

### type の対応状況

| type | 名前 | データサイズ | 対応状況 |
|------|------|-------------|----------|
| 0x01 | Bool | 4B | 対応済み |
| 0x02 | Short | 5B | 対応済み |
| 0x03 | Long | 6B | 対応済み |
| 0x04 | Color | 8B | 対応済み |
| 0x06 | (不明) | 6B | 対応済み（Long として解釈） |
| 0x08 | Double | 8B+4B trailer | 対応済み |
| 0x09 | GUID | 16B+4B trailer | 対応済み |
| 0x0A | Text | C bytes+4B trailer | 対応済み（UTF-16LE） |
| 0x0B | Binary | C bytes+4B trailer | 対応済み |
| 0x0C | Memo | C bytes+4B trailer | 対応済み（UTF-16LE） |
| 0x05 | (不明) | 不明 | 未対応（遭遇時パース中断） |
| 0x07 | (不明) | 不明 | 未対応（遭遇時パース中断） |

## 参考リンク

- MDB Tools HACKING.md: https://github.com/mdbtools/mdbtools/blob/main/HACKING.md
- jabakobob.net Unofficial MDB Guide: http://jabakobob.net/mdb/
- isladogs.co.uk Long Value Binary fields: https://www.isladogs.co.uk/lv-fields/index.html
