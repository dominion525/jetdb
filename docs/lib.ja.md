Microsoft Access のデータベースファイル (.mdb / .accdb) を読み取るための pure Rust ライブラリです。
ODBC ドライバや C ライブラリへの依存がないため、Windows がなくても macOS や Linux 上で
Access データベースの中身をそのまま読み取れます。Access 97 (Jet3) から Access 2019 (ACE17) まで対応しています。

# Quick Start

```toml
[dependencies]
jetdb = { git = "https://github.com/dominion525/jetdb" }
```

```rust,no_run
use jetdb::{PageReader, read_catalog};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    for entry in &catalog {
        println!("{}", entry.name);
    }
    Ok(())
}
```

# データの流れ

jetdb の API は、Access データベースのページベースの内部構造に沿って設計されています。
データを読み取るには、以下の順序で関数を呼び出します。

```text
PageReader::open → read_catalog → read_table_def → read_table_rows → Value
```

1. [`PageReader::open`] でデータベースファイルを開きます。ファイルヘッダーからエンジンバージョンを自動判定し、必要に応じて RC4 復号の準備も行います。
2. [`read_catalog`] でシステムカタログ (MSysObjects) を読み取り、データベース内のオブジェクト一覧を [`CatalogEntry`] のベクタとして返します。
3. [`read_table_def`] でテーブル定義ページ (TDEF) を解析し、カラムやインデックスの情報を含む [`TableDef`] を返します。
4. [`read_table_rows`] でデータページを走査し、各行の値を [`Value`] enum として返します。

# テーブルのデータを読む

最も基本的なユースケースです。ファイルを開いてから行データを取得するまでの完全な例を示します。

```rust,no_run
use jetdb::{PageReader, read_catalog, read_table_def, read_table_rows, Value};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;

    // カタログからテーブルを探す
    let catalog = read_catalog(&mut reader)?;
    let entry = catalog.iter()
        .find(|e| e.name == "Customers")
        .expect("テーブルが見つかりません");

    // テーブル定義を読み取る
    let table_def = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    // カラム名を表示する
    for col in &table_def.columns {
        print!("{}\t", col.name);
    }
    println!();

    // 行データを読み取る
    let result = read_table_rows(&mut reader, &table_def)?;
    for row in &result.rows {
        for value in row {
            match value {
                Value::Text(s) => print!("{s}\t"),
                Value::Long(n) => print!("{n}\t"),
                Value::Double(f) => print!("{f}\t"),
                Value::Null => print!("(null)\t"),
                other => print!("{other:?}\t"),
            }
        }
        println!();
    }

    Ok(())
}
```

[`ReadResult`] の `skipped_rows` フィールドには、読み取り中にエラーが発生してスキップされた行の数が入ります。
`warn_skipped(table)` メソッドを呼ぶと、スキップされた行がある場合に `log::warn!` で警告を出力します。ライブラリ内部のメタデータ読み取り（カタログ、クエリ、リレーションシップ、プロパティ、VBA）では自動的にこの警告が出力されます。

# Value 型

[`Value`] enum は Access の各データ型に対応しています。

| Value バリアント | Access の型 | 備考 |
|-----------------|------------|------|
| `Null` | (すべての型の NULL 値) | |
| `Bool(bool)` | Yes/No | |
| `Byte(u8)` | Byte | |
| `Int(i16)` | Integer | |
| `Long(i32)` | Long Integer | |
| `BigInt(i64)` | Large Number | ACE16 以降 |
| `Float(f32)` | Single | |
| `Double(f64)` | Double | |
| `Text(String)` | Text / Memo | Memo は長いテキスト |
| `Binary(Vec<u8>)` | Binary / OLE Object | |
| `Money(String)` | Currency | 固定小数点 (小数4桁) の文字列 |
| `Numeric(String)` | Decimal | スケール可変の文字列 |
| `Timestamp(f64)` | Date/Time | 1899-12-30 からの経過日数 |
| `Guid(String)` | Replication ID | `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` 形式 |

`Money` と `Numeric` は精度を保つために文字列で返します。
`Timestamp` は Access 内部の OLE Date 形式（1899-12-30 を基準とした浮動小数点の日数）をそのまま返します。

# その他の機能

## テーブル名の一覧

ユーザーが作成したテーブルの名前だけが必要な場合は [`table_names`] が便利です。
システムテーブルと隠しテーブルは自動的に除外されます。

```rust,no_run
use jetdb::{PageReader, table_names};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let names = table_names(&mut reader)?;
    for name in &names {
        println!("{name}");
    }
    Ok(())
}
```

## スキーマ情報

[`TableDef`] にはカラム定義 ([`ColumnDef`]) とインデックス定義 ([`IndexDef`]) が含まれています。

```rust,no_run
use jetdb::{PageReader, read_catalog, read_table_def};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = catalog.iter().find(|e| e.name == "Customers").unwrap();
    let table_def = read_table_def(&mut reader, &entry.name, entry.table_page)?;

    for col in &table_def.columns {
        println!("{}: {:?}", col.name, col.col_type);
    }
    for idx in &table_def.indexes {
        println!("INDEX {}: {:?}", idx.name, idx.columns);
    }
    Ok(())
}
```

## 保存済みクエリの SQL 復元

[`read_queries`] で保存済みクエリの定義を読み取り、[`query_to_sql`] で SQL 文字列に復元できます。

```rust,no_run
use jetdb::{PageReader, read_queries, query_to_sql};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let queries = read_queries(&mut reader)?;
    for qdef in &queries {
        println!("-- {} ({:?})", qdef.name, qdef.query_type);
        println!("{}", query_to_sql(qdef));
    }
    Ok(())
}
```

## リレーションシップ

[`read_relationships`] でテーブル間の外部キー定義を取得できます。

```rust,no_run
use jetdb::{PageReader, read_relationships};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let rels = read_relationships(&mut reader)?;
    for rel in &rels {
        println!("{}: {} -> {}", rel.name, rel.from_table, rel.to_table);
    }
    Ok(())
}
```

## VBA ソースコード

[`read_vba_project`] で VBA モジュールのソースコードを抽出できます。
VBA を含まないデータベースの場合は、空の [`VbaProject`] が返ります。

```rust,no_run
use jetdb::{PageReader, read_vba_project};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let project = read_vba_project(&mut reader)?;
    for module in &project.modules {
        println!("--- {} ({:?}) ---", module.name, module.module_type);
        println!("{}", module.source);
    }
    Ok(())
}
```

## DDL 生成

[`ddl`] モジュールで、テーブル定義から SQLite / PostgreSQL / MySQL / Access SQL の DDL を生成できます。

```rust,no_run
use jetdb::{PageReader, read_catalog, read_table_def, read_relationships};
use jetdb::ddl::{generate_ddl, Sqlite};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;

    let mut tables = Vec::new();
    for entry in &catalog {
        if entry.object_type == jetdb::ObjectType::Table
            && !entry.name.starts_with("MSys")
        {
            tables.push(read_table_def(&mut reader, &entry.name, entry.table_page)?);
        }
    }
    let rels = read_relationships(&mut reader)?;
    let sql = generate_ddl(&Sqlite, &tables, &rels, true, true);
    println!("{sql}");
    Ok(())
}
```

方言は [`ddl::Sqlite`]、[`ddl::Postgres`]、[`ddl::Mysql`]、[`ddl::Access`] から選べます。

## メモリから開く（WebAssembly）

[`PageReader::open_reader`] は、ファイルパスの代わりに `std::io::Cursor<Vec<u8>>` のようなメモリ上の入力からデータベースを開きます。WebAssembly で使うときはこの方法で開きます。ファイルシステムの無いブラウザ用ターゲット `wasm32-unknown-unknown` でも動作します。パスワード保護されたファイルは [`PageReader::open_reader_with_password`] を使います。

```rust,no_run
use std::io::Cursor;
use jetdb::{PageReader, table_names};

fn list_tables(bytes: Vec<u8>) -> Result<Vec<String>, jetdb::FileError> {
    let mut reader = PageReader::open_reader(Cursor::new(bytes))?;
    table_names(&mut reader)
}
```

入力は [`file::DataSource`]（`Read + Seek + Send + Sync + UnwindSafe + RefUnwindSafe`）を満たす必要があるため、`Rc` や `RefCell` を内部に持つ読み取り元は使えません。

# エラーハンドリング

すべての公開関数は `Result<T,` [`FileError`]`>` を返します。
[`FileError`] は I/O エラー、フォーマット不正、オブジェクト未検出などのバリアントを持つ enum で、`?` 演算子でそのまま伝播できます。

# 対応バージョン

| エンジン | Access バージョン | ファイル形式 |
|---------|-----------------|-------------|
| Jet3 | Access 97 | .mdb |
| Jet4 | Access 2000/2003 | .mdb |
| ACE12 | Access 2007 | .accdb |
| ACE14 | Access 2010 | .accdb |
| ACE15 | Access 2013 | .accdb |
| ACE16 | Access 2016 | .accdb |
| ACE17 | Access 2019 | .accdb |

# 制限事項

- 読み取り専用です（書き込みには対応していません）
- インデックスを使った検索は非対応です（順次スキャンのみ）
- [`read_table_rows`] はテーブルの全行をメモリに読み込みます。行数が非常に多いテーブルではメモリ使用量に注意してください
- パスワード保護されたデータベースは非対応です
