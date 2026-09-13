# jetdb

[![CI](https://github.com/dominion525/jetdb/actions/workflows/ci.yml/badge.svg)](https://github.com/dominion525/jetdb/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![MSRV: 1.86](https://img.shields.io/badge/MSRV-1.86-orange)](https://blog.rust-lang.org/2025/04/03/Rust-1.86.0.html)
[![Crates.io](https://img.shields.io/crates/v/jetdb)](https://crates.io/crates/jetdb)
[![docs.rs](https://img.shields.io/docsrs/jetdb)](https://docs.rs/jetdb)

[English](README.md)

Microsoft Access のデータベースファイル (.mdb / .accdb) を Rust から直接読み取れるライブラリと CLI ツールです。

pure Rust で書かれているので C/C++ のインストールや FFI は不要で、macOS・Windows・Linux のどれでもそのまま動きます。Access 97 (Jet3) から Access 2019 (ACE17) まで幅広く対応しています。

## インストール

### CLI ツール

```bash
cargo install jetdb-cli
```

### ライブラリとして使う

```toml
[dependencies]
jetdb = "0.2"
```

## CLI の使い方

```bash
# パスワード保護された .accdb ファイルを開く
jetdb --password secret tables protected.accdb

# データベースのエンジンバージョンを表示する
jetdb ver database.mdb

# テーブルの一覧を表示する
jetdb tables database.mdb

# テーブルのスキーマ（カラム、インデックス、リレーションシップ）を表示する
jetdb schema database.mdb
jetdb schema database.mdb -T Table1

# DDL を生成する（SQLite / PostgreSQL / MySQL / Access）
jetdb schema database.mdb --ddl sqlite

# テーブルのデータを CSV で出力する
jetdb export database.mdb Table1

# 保存済みクエリの一覧を表示する・SQL を確認する
jetdb queries list database.mdb
jetdb queries show database.mdb SelectQuery

# VBA モジュールの一覧を表示する・ソースコードを確認する
jetdb vba list database.mdb
jetdb vba show database.mdb Module1

# オブジェクトプロパティを表示する
jetdb prop database.mdb Table1
```

詳しいオプションや出力例は [docs/cli.ja.md](docs/cli.ja.md) にまとめてあります。

## ライブラリの使い方

```rust
use jetdb::{PageReader, read_catalog, read_table_def, read_table_rows};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;

    // テーブル一覧を取得する
    let catalog = read_catalog(&mut reader)?;
    for entry in &catalog {
        println!("{}", entry.name);
    }

    // テーブル定義を読み取る
    let entry = &catalog[0];
    let table_def = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    // 行データを読み取る
    let result = read_table_rows(&mut reader, &table_def)?;
    for row in &result.rows {
        println!("{:?}", row);
    }

    Ok(())
}
```

詳しい API ドキュメントやコード例は `cargo doc --open` で確認できます。[docs.rs/jetdb](https://docs.rs/jetdb) でも閲覧できます。

## 対応バージョン

| エンジン | Access バージョン | ファイル形式 |
|---------|-----------------|-------------|
| Jet3    | Access 97       | .mdb        |
| Jet4    | Access 2000/2003 | .mdb       |
| ACE12   | Access 2007     | .accdb      |
| ACE14   | Access 2010     | .accdb      |
| ACE15   | Access 2013     | .accdb      |
| ACE16   | Access 2016     | .accdb      |
| ACE17   | Access 2019     | .accdb      |

## できること

- テーブルの一覧や定義（カラム、インデックス、リレーションシップ）の読み取り
- 行データの読み取りと Rust の型への変換（Text, Long, Double, Timestamp, Money, Memo, OLE, GUID など）
- SQLite・PostgreSQL・MySQL・Access SQL 向けの DDL 生成
- 保存済みクエリからの SQL 復元
- VBA モジュールのソースコード抽出
- オブジェクトプロパティ（LvProp）の読み取り
- RC4 暗号化データベースの復号、およびパスワード保護された .accdb ファイルの復号 (Agile, RC4 CryptoAPI, Standard/NonStandard AES)
- Jet3 (Latin-1) / Jet4 以降 (UTF-16LE、圧縮テキスト) のエンコーディング処理
- WebAssembly 上での動作（ブラウザ用の `wasm32-unknown-unknown` を含む。`PageReader::open_reader` でメモリ上のバイト列から開く）

## 制限事項

- 読み取り専用（書き込みには対応していない）
- インデックスを使った検索は非対応（順次スキャンのみ）
- テーブルの全行をメモリに読み込むため、行数が非常に多いテーブルではメモリ使用量に注意
- パスワード保護された .accdb ファイルは `PageReader::open_with_password` が必要 (Agile, RC4 CryptoAPI, Standard/NonStandard AES)
- レプリケーションデータベース (.mda) は未テスト

## AI エージェント連携

[agent-skills](https://github.com/dominion525/agent-skills) リポジトリ経由で Claude Code Skill として利用できます。

Plugin Marketplace 経由:

```
/plugin marketplace add dominion525/agent-skills
/plugin install jetdb-cli@dominion525-skills
```

Vercel skills CLI 経由:

```bash
npx skills add dominion525/agent-skills --skill jetdb-cli -a claude-code
```

## 謝辞

- MDB/ACCDB ファイルフォーマットの理解にあたって [mdbtools](https://github.com/mdbtools/mdbtools) の [HACKING.md](https://github.com/mdbtools/mdbtools/blob/dev/HACKING.md) を大いに参考にしました
- テスト用の .mdb / .accdb ファイルは大部分を [Jackcess](https://github.com/spannm/jackcess)（Apache License 2.0）から利用しています（一部は本プロジェクトで独自に作成）。詳細は [testdata/SOURCES.md](testdata/SOURCES.md) を参照
- 暗号化関連のテストファイルは [jackcessencrypt](https://github.com/jahlborn/jackcessencrypt)（Apache License 2.0、Jackcess の暗号化プラグイン）から利用しています
- 暗号化の実装にあたり [MS-OFFCRYPTO](https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-offcrypto/)（Microsoft の Office ドキュメント暗号化仕様）を参考にしました

## ライセンス

MIT OR Apache-2.0
