# CI ツール

jetdb プロジェクトで使用する CI ツールとその実行方法。

## ツール一覧

### 1. cargo test — テスト実行

プロジェクト全体のユニットテストと統合テストを実行する。

```bash
cargo test
```

個別クレートのテスト:

```bash
cargo test -p jetdb          # ライブラリのみ
cargo test -p jetdb-cli      # CLI のみ
```

一部のテストデータはリポジトリに同梱していない。`scripts/fetch-testdata.sh` を 1 回実行して取得する。未取得の間、該当テストはスキップされる。CI は `cargo test` の前にこのスクリプトを実行する。`testdata/SOURCES.md` を参照。

### 2. cargo clippy — リント

Rust の静的解析ツール。警告をエラーとして扱い、コードの品質を保つ。

```bash
cargo clippy -- -D warnings
```

インストール（rustup に含まれていない場合）:

```bash
rustup component add clippy
```

### 3. Wasm — WASI とブラウザ用のテスト

ライブラリは WebAssembly の 2 つのターゲットでもテストする。2 つは別々のビルドで、依存クレートがそれぞれで異なるコードをコンパイルすることがある（`cfb` → `web-time` はブラウザ用でだけ JavaScript を使う）ため、片方で通っても、もう片方は保証されない。

- WASI（`wasm32-wasip1`）: ライブラリのテストをすべて wasmtime 上で実行する。WASI では `testdata/` 配下のファイルを開ける
- ブラウザ用（`wasm32-unknown-unknown`）: ファイルシステムが無いので、`crates/jetdb/tests/wasm_browser.rs` がデータベースを埋め込み、`PageReader::open_reader` でメモリから開いて、Node.js 上で実行する。このテストのビルドには、ブラウザ用のライブラリのビルドも含まれる

リポジトリのルートで実行する。ブラウザ用テストはリポジトリに同梱していない `testdata/V1997/nwind.mdb` を埋め込むので、先にテストデータを取得する:

```bash
scripts/fetch-testdata.sh
CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime run --dir $(pwd)" cargo test --target wasm32-wasip1 -p jetdb
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --target wasm32-unknown-unknown -p jetdb --test wasm_browser
```

WASI では、テストがテストデータのパスをリポジトリの絶対パスから組み立てるので、wasmtime にも同じディレクトリを見せる。`std::env::temp_dir()` は WASI では使えずパニックするので、ディスク上の一時ファイルが必要なテストは `target/tmp/` に書き込む。

インストール:

```bash
rustup target add wasm32-unknown-unknown wasm32-wasip1
cargo install wasm-bindgen-cli --version <version> --locked
```

`wasm-bindgen-test-runner`（`wasm-bindgen-cli` に含まれる）は、`Cargo.lock` の `wasm-bindgen` と同じ版である必要がある。版は次で確認できる:

```bash
cargo metadata --format-version 1 --filter-platform wasm32-unknown-unknown | jq -r '.packages[] | select(.name == "wasm-bindgen") | .version'
```

ブラウザ用テストには Node.js も必要。wasmtime は https://wasmtime.dev/ を参照（macOS では `brew install wasmtime`）。

### 4. cargo audit — 脆弱性チェック

依存クレートに既知の脆弱性がないか検査する。

```bash
cargo audit
```

インストール:

```bash
cargo install cargo-audit
```

### 5. cargo doc — ドキュメントビルド

ワークスペース全体の API ドキュメントを生成する。リンク切れや doc comment の構文エラーを検出できる。

```bash
cargo doc --workspace
```

生成されたドキュメントは `target/doc/jetdb/index.html` に出力される。

### 6. rust-code-analysis-cli — 複雑度メトリクス

ソースコードの循環的複雑度・認知的複雑度などのメトリクスを計測する。

```bash
rust-code-analysis-cli -m -p crates/ -O json
```

JSON 出力は `complexity-filter` クレート（`crates/complexity-filter/`）を通して、閾値（CC>=10、Cognitive>=10、SLOC>=50）を超える関数のみを表示する。`quality-check.sh` スクリプトが自動的にこの処理を行う。

インストール:

```bash
cargo install rust-code-analysis-cli --locked
```

> **注意**: `--locked` フラグは必須。省略すると tree-sitter のバージョン不一致によりコンパイルが失敗する（[GitHub Issue #1140](https://github.com/nickel-org/rust-code-analysis/issues/1140)）。
>
> このプロジェクトは最終リリースが 2023年1月であり、メンテナンスが停滞している。

### 7. cargo-llvm-cov — テストカバレッジ

LLVM ソースベースのコードカバレッジを使用してテストカバレッジを計測する。

```bash
cargo llvm-cov --workspace
```

HTML レポート:

```bash
cargo llvm-cov --workspace --html
```

HTML レポートは `target/llvm-cov/html/index.html` に出力される。

インストール:

```bash
cargo install cargo-llvm-cov
```

> **注意**: `llvm-tools-preview` コンポーネントが必要。初回実行時に自動でインストールされる。

#### カバレッジの注意点

`relationship.rs`（約76%）と `vba.rs`（約80%）が最も行カバレッジが低い。`crypto.rs`（約88%）も平均以下である。これらのファイルの未カバー行はすべてエラーマッピングクロージャ (`.map_err`)、カラム欠損時の `.ok_or()` エラーパス、XMLパースのエラー分岐、不正データ時の `continue` 分岐など、正常なデータベースでは到達しない異常系パスである。llvm-cov はクロージャを独立した関数としてカウントするため関数カバレッジも低く見えるが、正常系のロジック（全AESキーサイズ、全ハッシュアルゴリズム、ページ復号化を含む）はすべてテスト済み。

## 品質チェックスクリプト

`scripts/quality-check.sh` がすべてのチェックを順番に実行し、合否を報告する。手動で個別に実行するのではなく、常にこのスクリプトを使用すること。

```bash
scripts/quality-check.sh
```

テストまたは clippy が失敗した場合はその場で中断する。その他のチェック（wasm、audit、doc、coverage、complexity）は失敗しても続行する。wasm、audit、coverage、complexity は、必要なツールがインストールされていなければスキップする。

## 実行順序

品質チェックスクリプトは以下の順序でチェックを実行する:

1. `cargo test` — まず既存テストが通ることを確認
2. `cargo clippy -- -D warnings` — コード品質のチェック
3. Wasm — WASI でライブラリのテストを、Node.js 上でブラウザ用テストを実行
4. `cargo audit` — セキュリティ上の問題がないか確認
5. `cargo doc --workspace` — ドキュメントが正しく生成されるか確認
6. `cargo llvm-cov --workspace` — テストカバレッジを計測
7. `rust-code-analysis-cli` — コードの複雑度を計測

テストと clippy は致命的 — いずれかが失敗するとスクリプトは中断する。カバレッジと複雑度は実行時間が長いため最後に実行する。
