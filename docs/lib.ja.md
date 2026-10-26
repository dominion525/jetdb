Microsoft Access のデータベースファイル (.mdb / .accdb) を読み取るための pure Rust ライブラリです。
ODBC ドライバや C ライブラリは要りません。macOS や Linux など、Rust の動くどの環境でも
Access データベースを直接読み取れます。Access 97 (Jet3) から Access 2019 (ACE17) まで対応しています。

# Quick Start

```toml
[dependencies]
jetdb = "0.3"
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

1. [`PageReader::open`] でデータベースファイルを開きます。ファイルヘッダーからエンジンバージョンを自動判定し、必要に応じて RC4 復号の準備も行います。パスワード保護された .accdb ファイルは、代わりに [`PageReader::open_with_password`] で開きます。
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

## 行を 1 行ずつ読む

[`TableRows`] は、テーブルの行を 1 行ずつ読みます。メモリに載るのは、使っている行だけです。[`read_table_rows`] は、これで全行を読みます。[`PageReader`] は呼ぶたびに渡すので、行の合間に同じ reader でコンプレックスカラムの中身などを読めます。[`TableRows::with_columns`] は、求めた列だけを読み、ほかの列の長い値を読みません。[`TableRows::position`] は続きの位置を返し、[`TableRows::starting_at`] はそこから読み直します。[`read_row_at`] は、1 つの位置の行を読みます。

```rust,no_run
use jetdb::{
    calculated_column_types_in, find_table, read_catalog, read_table_def, PageReader, TableRows,
};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = find_table(&catalog, "Orders")?;
    let table = read_table_def(&mut reader, &entry.name, entry.table_page)?;
    let calculated = calculated_column_types_in(&catalog, &table);
    let mut rows = TableRows::new(&table, &calculated);
    while let Some(row) = rows.next_row(&mut reader)? {
        println!("{:?}", row.values);
    }
    rows.warn_skipped(&table.name);
    Ok(())
}
```

## コンプレックスカラム

コンプレックスカラム（Access 2007 以降の複数値、添付ファイル、追加のみの長いテキストの版の履歴）は、各行に番号を持ちます。[`read_table_rows`] はこの番号を `Value::Long` として返します。[`read_complex_column`] は、そうした列について、番号ごとの中身の場所だけを読みます。[`ComplexColumn::values`] は、1 つの番号の中身を読みます。求めていない行の中身は読みません。添付ファイルは、各ファイルの中身を、圧縮を解いて返します。

```rust,no_run
use jetdb::{
    calculated_column_types_in, find_table, read_catalog, read_complex_column, read_table_def,
    ComplexValues, PageReader, TableRows, Value,
};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.accdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = find_table(&catalog, "Products")?;
    let table = read_table_def(&mut reader, &entry.name, entry.table_page)?;
    let i = table.columns.iter().position(|c| c.name == "Tags").expect("列が見つかりません");
    let Some(tags) = read_complex_column(&mut reader, &catalog, &table, &table.columns[i])? else {
        return Ok(());
    };
    let calculated = calculated_column_types_in(&catalog, &table);
    let mut rows = TableRows::new(&table, &calculated);
    while let Some(row) = rows.next_row(&mut reader)? {
        if let Value::Long(n) = row.values[i] {
            if let ComplexValues::MultiValue(values) = tags.values(&mut reader, n)? {
                println!("{values:?}");
            }
        }
    }
    Ok(())
}
```

## CSV

[`write_csv`] は、テーブルを `jetdb export` と同じ CSV で、1 行ずつ書きます。[`CsvOptions`] は、`jetdb export` の選択肢を持ちます。添付ファイルのフィールドは、ファイル名を出します。`attachments` に関数を渡すと、各ファイルについて、その関数が返すものを出します。

```rust,no_run
use jetdb::{find_table, read_catalog, read_table_def, write_csv, CsvOptions, PageReader};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let catalog = read_catalog(&mut reader)?;
    let entry = find_table(&catalog, "Orders")?;
    let table = read_table_def(&mut reader, &entry.name, entry.table_page)?;
    let mut options = CsvOptions::default();
    options.delimiter = '\t';
    let mut out = std::io::stdout().lock();
    write_csv(&mut reader, &catalog, &table, &options, &mut out, None)?;
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

## フォームとレポート

[`list_forms`] でフォームとレポートの一覧を取り、[`read_form_properties_of_type`] でそのデザインを読みます。フォームやレポート自身のプロパティと、セクションやコントロールごとのプロパティが取れます。
プロパティの名前は [`BlobProperty::display_name`] で取れます。名前のわからないプロパティは `0xXXXX` になります。値（[`BlobValue`]）は、そのまま文字列として出力できます。

```rust,no_run
use jetdb::{PageReader, control_type_label, list_forms, read_form_properties_of_type};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.accdb")?;
    for entry in list_forms(&mut reader)? {
        let form = read_form_properties_of_type(&mut reader, &entry.name, entry.object_type)?;
        println!("{:?} {}", form.object_type, form.form_name);
        for p in &form.properties {
            println!("  {} = {}", p.display_name(), p.value);
        }
        for control in &form.controls {
            println!("  {} ({})", control.name, control_type_label(control.type_code));
        }
    }
    Ok(())
}
```

[`read_form_type_info`] は TypeInfo ストリームからコントロールの名前と種類を、[`read_form_stream`] はストリーム（[`StreamKind`]）の生のバイト列を読みます。

## マクロ

3 種類のマクロを、どれも [`MacroDef`] として読みます。名前付きマクロは [`list_macros`] と [`read_macro`] で、フォームやレポートのイベントに埋め込まれたマクロは [`read_embedded_macros`] で、テーブルのデータマクロは [`read_data_macros`] で読みます。
[`MacroDef`] は、マクロの文の並び（[`MacroStatement`]）、読み取り元の XML、名前付きマクロと埋め込みマクロのマクロの表（grid）を持ちます。

```rust,no_run
use jetdb::{MacroStatement, PageReader, list_macros, read_macro};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.accdb")?;
    for entry in list_macros(&mut reader)? {
        let def = read_macro(&mut reader, &entry.name)?;
        println!("{}", entry.name);
        for statement in &def.statements {
            if let MacroStatement::Action { name, .. } = statement {
                println!("  {name}");
            }
        }
    }
    Ok(())
}
```

[`data_macros_to_text`] と [`embedded_macro_to_text`] は、`Application.SaveAsText` がデータマクロと埋め込みマクロについて書き出すテキストを作ります。

## オブジェクトのプロパティ

[`read_object_properties`] で、テーブルとその列の説明や書式など、オブジェクトのプロパティを読みます。プロパティは、オブジェクト自身のものと列ごとのもので、まとまり（[`PropertyMap`]）に分かれています。

```rust,no_run
use jetdb::{PageReader, read_object_properties};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open("database.mdb")?;
    let props = read_object_properties(&mut reader, "Customers")?;
    for map in &props.maps {
        println!("{:?} {}", map.map_type, map.name);
        for p in &map.properties {
            println!("  {} = {:?}", p.name, p.value);
        }
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

## パスワード保護されたデータベース

パスワード保護された .accdb ファイル（Agile Encryption、RC4 CryptoAPI、Standard AES・NonStandard AES で暗号化されたもの）は、[`PageReader::open_with_password`] で開きます。

```rust,no_run
use jetdb::{PageReader, table_names};

fn main() -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password("protected.accdb", Some("secret"))?;
    let names = table_names(&mut reader)?;
    for name in &names {
        println!("{name}");
    }
    Ok(())
}
```

暗号化されていないファイルでは、`open_with_password` は `open` と同じに動きます（パスワードは使いません）。

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

ACE15 は、Access 2013 にちなんだ版の値 0x04 の名前です。Access 2013 は Access 2010 と同じ形式（ACE14）で書き、0x04 のファイルは知られていません。jetdb は、そのようなファイルを ACE15 として読みます。

Jet3 (Access 97) のファイルは、文字列をデータベースのコードページで持ちます。西欧の言語なら 1252、日本語なら 932 などです。jetdb は、ファイルのヘッダーにあるコードページで読みます ([`PageReader::text_encoding`])。知らないコードページのときは、Latin-1 として読みます。

# 制限事項

- 読み取り専用です（書き込みには対応していません）
- インデックスを使った検索は非対応です（順次スキャンのみ）
- パスワード保護された .accdb ファイルは [`PageReader::open_with_password`] で開く必要があります（Agile Encryption、RC4 CryptoAPI、Standard AES・NonStandard AES）
