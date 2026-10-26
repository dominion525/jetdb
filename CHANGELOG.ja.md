# 変更履歴

## 0.4.0

### 追加

- jetdb-wasm：Access のデータベースをブラウザと Node.js で読む npm パッケージ。TypeScript の型定義つき。[docs/wasm.ja.md](docs/wasm.ja.md) を参照。
- メモリ上のバイト列からデータベースを開く（`PageReader::open_reader`）。これにより、ライブラリはブラウザ用を含む WebAssembly で動く。
- マクロ：名前付きマクロ、フォームやレポートに埋め込まれたマクロ、データマクロ。文の並び、XML、SaveAsText 形式のテキストで読める（`jetdb macro`）。
- コンプレックスカラムの値：複数の値、添付ファイル、長いテキストの版の履歴。`jetdb export --attachments` は添付ファイルをファイルに書き出す。
- 集計フィールドの値（結果の型で返す）。
- テーブルの行を 1 行ずつ読む（`TableRows`、`read_row_at`）。`jetdb export` と jetdb-wasm でも使う。
- ライブラリでの CSV の書き出し（`write_csv`）。
- `jetdb prop`、`jetdb form dump`、`jetdb form controls`、`jetdb form props`、`jetdb macro embedded` の `--type`：種類の違うオブジェクトが同じ名前を持つとき、どれかを選ぶ。指定しないと、そのような名前はどれか 1 つを選ばずにエラーになる。

### 変更

- 最小の Rust のバージョンは 1.87。
- 公開の enum と、フィールドがすべて公開の構造体は `#[non_exhaustive]`。`CsvOptions` は `CsvOptions::default()` から作る。
- `ObjectType` は、Access が MSysObjects に持つ値に合わせた。`SystemTable` は無くなり、`Container`、`Database`、`LinkedOdbcTable`、`UserInfo` が増え、値も変わった（たとえば `Form` は -32768）。
- `VbaModuleType::ClassOrDocument` は `Class` と `Document` に分かれた。
- `encoding::decode_text` は、Jet3 かどうかではなく `TextEncoding` を受け取る。
- `DdlDialect` の必須のメソッドが 3 つ増えた：`auto_increment_needs_key`、`supports_comments`、`index_names_per_table`。
- `column_flags::REPLICATION`、`is_replication_column`、`index_type::NORMAL` は非推奨。`column_flags::HIDDEN`、`ColumnDef::is_hidden`、`index_type::PRIMARY` か `index_type::ORDINARY` を使う。
- Access 97 のファイルの文字列を、Latin-1 ではなく、データベースのコードページで読む。
- 列は Access のデザインビューの順に並び、短いテキストの列のサイズは、Access の表示と同じく文字数で表す。
- DDL：Access で値要求の列は `NOT NULL`。参照整合性の無いリレーションシップは外部キーにしない。GUID のオートナンバーの列は GUID の列にする。大きい数値の列は Access SQL で `BIGINT`。SQLite と PostgreSQL では、インデックス名にテーブル名を前に付ける。
- フォームとレポート：デザインの Blob を木として読み、`Application.SaveAsText` の項目名で名前を付けるプロパティを増やし、Access 2000 形式も読む。`jetdb form controls` は Blob のセクションとコントロールを並べ、3 列目はその並びの中の位置。
- Access 97 のフォームとレポートは、見つからないのではなく `FileError::Jet3FormsNotRead`（「the forms and reports of Access 97 are not read」）になる。
- フォームのプロパティの 32 ビットの値は、SaveAsText と同じく符号付きの数で返す（`BlobValue::Color` は無くした）。GUID は大文字で返す。
- Access のデータベースの印の無いファイルは開かない。
- MSysQueries の行を読み切れなかった保存済みクエリの SQL は返さない。
- 一覧のサブコマンド（`queries list`、`vba list`、`form list`、`macro list`）は、`tables` と同じく、既定で 1 行に 1 つの名前を出す。`-1` は受け付けて、何もしない。
- `jetdb prop` は、オブジェクトが無ければ「not found」で失敗する。プロパティの見出しは、いつも `Table Properties:` ではなく、種類を語で書く（`Query Properties:`、`Linked Table Properties:`）。オブジェクト自身のプロパティを、列のプロパティより先に出す。
- `jetdb prop` と `jetdb form props` は、文字列の値を引用符で囲み、改行やほかの制御文字を `\r`、`\n`、`\t`、`\u{..}` と書く。1 つのプロパティは 1 行に収まる。
- `jetdb schema` と `jetdb prop` は、名前を端末での表示の幅でそろえる。
- `jetdb export -d` は 1 文字を取る。空や 2 文字以上の区切りはエラー。
- サブコマンドを付けない `jetdb` は、使い方を stderr に出して終了コード 2 で終わる。`jetdb --version` は `jetdb` と出す。
- I/O エラーは、失敗したパスを示す。

### 修正

- Jet4 の十進型の列の精度と小数点以下桁数が入れ替わっていた。
- 隠しのインデックスを主キーと取り違えることがあった。
- システムテーブルの書き出しが、空の行になっていた。
- 1899-12-30 より前の日時で、時刻が失われていた。
- 可変長のデータが 256 バイトの境目で終わる Jet3 の行。
- 壊れたデータで jetdb が止まる（パニックする）ことがあった：プロパティのまとまりの長さ、Jet3 の NULL のマスク、インデックスの数、十進型の小数点以下桁数、フォームのプロパティの長さ、深く入れ子になったマクロの XML とフォームのデザイン、VBA プロジェクトの圧縮データ、範囲から大きく外れた日付。
- MSysAccessStorage の DirData の項目で、名前の終わりの null が無いものは、名前を 1 バイトずれて読んでいた。壊れた項目として扱う。
- 別の結合の中ですでに結ばれた 2 つのテーブルの結合条件が、クエリの SQL から抜けるか、Access が書く場所でなく外側の結合に書かれていた。
- DDL：SQLite の `INTEGER PRIMARY KEY AUTOINCREMENT` にするのは、オートナンバーの列だけでできた主キーだけ。MySQL で主キーの先頭でない `AUTO_INCREMENT` の列にインデックスを付ける。MySQL の GUID の列は波かっこ付きの値を持つ（`CHAR(38)`）。コンプレックスカラムは、自動採番の列にもインデックスにもしない。
- CSV で、区切り文字を含むフィールドは、文字列だけでなく、どの型でも引用符で囲む。
- `jetdb schema ... | head` のように出力の読み手がパイプを閉じると、CLI がパニックしていた。何も出さずに終わる。
- crate と、CLI のバイナリのアーカイブに、ライセンスのファイルが入る。README のリンクが crates.io でも開く。

0.2.0 から 0.3.3 までは、[GitHub のリリース](https://github.com/dominion525/jetdb/releases) を参照。

## 0.1.1 — 2026-02-16

- VBA 抽出およびデータ行読み取りの堅牢性を改善。
- 全公開モジュールにモジュールレベルのドキュメントを追加。
- crate メタデータの整備（MSRV、exclude パターン）。

## 0.1.0 — 2026-02-15

初回リリース。
