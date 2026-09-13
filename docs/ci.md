# CI Tools

CI tools and execution methods used in the jetdb project.

## Tool List

### 1. cargo test — Run Tests

Run all unit tests and integration tests across the project.

```bash
cargo test
```

Per-crate tests:

```bash
cargo test -p jetdb          # Library only
cargo test -p jetdb-cli      # CLI only
```

Some test data is not stored in the repository. Run `scripts/fetch-testdata.sh` once to download it; tests that need it are skipped while it is absent. CI runs the script before `cargo test`. See `testdata/SOURCES.md`.

### 2. cargo clippy — Linting

Rust static analysis tool. Treats warnings as errors to maintain code quality.

```bash
cargo clippy -- -D warnings
```

Installation (if not already included with rustup):

```bash
rustup component add clippy
```

### 3. Wasm — Browser Build and WASI Tests

The library is also checked on WebAssembly. The browser target (`wasm32-unknown-unknown`) and WASI (`wasm32-wasip1`) are separate builds, and dependencies can compile different code on each, so each is checked on its own:

- The browser target has no filesystem, so only its build is checked
- The library tests run on WASI under wasmtime, where they can open the files under `testdata/`

Run from the repository root:

```bash
cargo check --target wasm32-unknown-unknown -p jetdb
CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime run --dir $(pwd)" cargo test --target wasm32-wasip1 -p jetdb
```

Tests build test-data paths from the absolute path of the repository, so wasmtime is given that same directory.

`std::env::temp_dir()` is not available on WASI and panics there. A test that needs a scratch file on disk writes it under `target/tmp/` instead.

Installation:

```bash
rustup target add wasm32-unknown-unknown wasm32-wasip1
```

For wasmtime, see https://wasmtime.dev/ (on macOS, `brew install wasmtime`).

### 4. cargo audit — Vulnerability Check

Check dependency crates for known security vulnerabilities.

```bash
cargo audit
```

Installation:

```bash
cargo install cargo-audit
```

### 5. cargo doc — Documentation Build

Generate API documentation for the entire workspace. Detects broken links and doc comment syntax errors.

```bash
cargo doc --workspace
```

Generated documentation is output to `target/doc/jetdb/index.html`.

### 6. rust-code-analysis-cli — Complexity Metrics

Measure cyclomatic complexity, cognitive complexity, and other source code metrics.

```bash
rust-code-analysis-cli -m -p crates/ -O json
```

The raw JSON output is piped through the `complexity-filter` crate (`crates/complexity-filter/`) to display only functions exceeding the threshold (CC>=10, Cognitive>=10, or SLOC>=50). The `quality-check.sh` script runs this automatically.

Installation:

```bash
cargo install rust-code-analysis-cli --locked
```

> **Note**: The `--locked` flag is required. Without it, compilation fails due to a tree-sitter version mismatch ([GitHub Issue #1140](https://github.com/nickel-org/rust-code-analysis/issues/1140)).
>
> This project's last release was January 2023 and maintenance has stalled.

### 7. cargo-llvm-cov — Test Coverage

Measure test coverage using LLVM source-based code coverage.

```bash
cargo llvm-cov --workspace
```

HTML report:

```bash
cargo llvm-cov --workspace --html
```

The HTML report is output to `target/llvm-cov/html/index.html`.

Installation:

```bash
cargo install cargo-llvm-cov
```

> **Note**: The `llvm-tools-preview` component is required. It will be installed automatically on first run.

#### Coverage Notes

`relationship.rs` (~76%) and `vba.rs` (~80%) have the lowest line coverage. `crypto.rs` (~88%) is also below average. Uncovered lines in these files are error-mapping closures (`.map_err`), `.ok_or()` error paths for missing columns, XML parse error branches, and `continue` branches for malformed data — none of which are reachable with valid database files. llvm-cov counts each closure as a separate function, making function coverage appear low, but all normal-path logic (including all AES key sizes, all hash algorithms, and page decryption) is fully tested.

## Quality Check Script

`scripts/quality-check.sh` runs all checks in sequence with pass/fail reporting. Always use this script instead of running checks manually.

```bash
scripts/quality-check.sh
```

The script stops immediately on test or clippy failure. Other checks (wasm, audit, doc, coverage, complexity) report failures but continue to run. The wasm, audit, coverage and complexity checks are skipped when their tools are not installed.

## Execution Order

The quality check script runs checks in the following order:

1. `cargo test` — Verify existing tests pass first
2. `cargo clippy -- -D warnings` — Check code quality
3. Wasm — Verify the browser build compiles and the library tests pass on WASI
4. `cargo audit` — Check for security issues
5. `cargo doc --workspace` — Verify documentation builds correctly
6. `cargo llvm-cov --workspace` — Measure test coverage
7. `rust-code-analysis-cli` — Measure code complexity

Tests and clippy are fatal — the script aborts if either fails. Coverage and complexity run last because they take the longest.
