# AGENTS.md（日本語版）

## プロジェクト概要

jetdb は Microsoft Access データベース (.mdb/.accdb) の読み取り専用ライブラリおよび CLI ツールです（純 Rust 実装）。
Jet3 (Access 97) から ACE17 (Access 2019) まで対応しています。

## ビルド・テストコマンド

```bash
scripts/fetch-testdata.sh            # リポジトリ非同梱のテストデータを取得
cargo build                          # 全 crate ビルド
cargo test -p jetdb                  # ライブラリテストのみ
cargo test -p jetdb-cli              # CLI テストのみ（ユニット＋統合）
cargo test -p jetdb-cli --test cli_vba   # 統合テストファイル単位
cargo test -p jetdb -- vba::tests::vba_v2003  # テスト名指定
```

## アーキテクチャ

2 crate のワークスペース構成：

- **jetdb** (`crates/jetdb/`) — コアライブラリ。RC4 復号を含む低レベルページ読み取り (`file.rs` → `PageReader`)、フォーマット定数 (`format.rs`)、その上に `&mut PageReader` を受け取る高レベルモジュール群：カタログ、テーブル定義、データ行、クエリ、リレーションシップ、プロパティ、VBA 抽出、DDL 生成。
- **jetdb-cli** (`crates/jetdb-cli/`) — CLI バイナリ。各サブコマンドは個別モジュール (`query.rs`, `vba.rs`, `export.rs`, `prop.rs`) または `main.rs` 内 (ver, tables, schema) に実装。公開 API は全て `lib.rs` から re-export。

データフロー: `PageReader` → `read_catalog` → `read_table_def` → `read_table_rows` → `Value` enum。

## CLI サブコマンドパターン

全サブコマンドは同一構造に従う（正規例: `query.rs`）：

1. Clap `Args`/`Subcommand` 構造体で CLI 定義
2. `cmd_xxx(args) -> ExitCode` — エントリーポイント、stderr に `jetdb:` プレフィクス付きでエラー出力
3. `run_xxx(args) -> Result<(), FileError>` — 実際のロジック

一覧コマンドは名前順にソートして出力。データが存在しない場合（テーブルなし、クエリなし、VBA なし）は空出力で成功を返し、エラーにはしない。show コマンドで名前が見つからない場合は固有のエラーを返す（例: `ModuleNotFound`, `QueryNotFound`）。

## テスト

- テストデータ: `testdata/` 配下に実 .mdb/.accdb ファイルをバージョン別に格納 (V1997, V2000, V2003, V2007, V2010, V2019)
- 一部のファイルはリポジトリに同梱せず `scripts/fetch-testdata.sh` で取得する（`testdata/SOURCES.md` 参照）。CI は `cargo test` の前に実行する
- 全テストファイルで `skip_if_missing!` マクロを使い、テストデータ欠落時はスキップ
- CLI 統合テストは `Command::new(env!("CARGO_BIN_EXE_jetdb"))` でバイナリを起動
- ライブラリテストはモジュール内の `#[cfg(test)] mod tests`

## ドキュメント

CLI ドキュメントは英語版 (`docs/cli.md`) と日本語版 (`docs/cli.ja.md`) の両方が存在する。サブコマンドの追加・変更時は両方を更新すること。

## 規約

- コミットメッセージは英語
