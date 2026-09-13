# AGENTS.md

## Project Overview

jetdb is a pure Rust, read-only library and CLI for Microsoft Access databases (.mdb/.accdb).
Supports Jet3 (Access 97) through ACE17 (Access 2019).

## Build & Test Commands

```bash
scripts/fetch-testdata.sh            # Download test data not stored in the repo
cargo build                          # Build all crates
cargo test -p jetdb                  # Library tests only
cargo test -p jetdb-cli              # CLI tests only (unit + integration)
cargo test -p jetdb-cli --test cli_vba   # Single integration test file
cargo test -p jetdb -- vba::tests::vba_v2003  # Single test by name
```

## Architecture

Two-crate workspace:

- **jetdb** (`crates/jetdb/`) — Core library. Low-level page reading with RC4 decryption (`file.rs` → `PageReader`), format constants (`format.rs`), then higher-level modules that all take `&mut PageReader`: catalog, table definitions, data rows, queries, relationships, properties, VBA extraction, DDL generation.
- **jetdb-cli** (`crates/jetdb-cli/`) — CLI binary. Each subcommand lives in its own module (`query.rs`, `vba.rs`, `export.rs`, `prop.rs`) or inline in `main.rs` (ver, tables, schema). All public APIs are re-exported from `lib.rs`.

Data flow: `PageReader` → `read_catalog` → `read_table_def` → `read_table_rows` → `Value` enum.

## CLI Subcommand Pattern

All subcommands follow the same structure (see `query.rs` as the canonical example):

1. Clap `Args`/`Subcommand` structs for CLI definition
2. `cmd_xxx(args) -> ExitCode` — entry point, prints errors to stderr with `jetdb:` prefix
3. `run_xxx(args) -> Result<(), FileError>` — actual logic

List commands output sorted alphabetically. When data is absent (no tables, no queries, no VBA), return empty output (success), not an error. Name-not-found on show commands returns a specific error (e.g., `ModuleNotFound`, `QueryNotFound`).

## Testing

- Test data: `testdata/` with real .mdb/.accdb files organized by version (V1997, V2000, V2003, V2007, V2010, V2019)
- Some files are not stored in the repository and are downloaded by `scripts/fetch-testdata.sh` (see `testdata/SOURCES.md`); CI runs it before `cargo test`
- All test files use `skip_if_missing!` macro to gracefully skip when test data is absent
- CLI integration tests use `Command::new(env!("CARGO_BIN_EXE_jetdb"))` to invoke the binary
- Library tests are in-module `#[cfg(test)] mod tests`

## Documentation

CLI docs exist in both English (`docs/cli.md`) and Japanese (`docs/cli.ja.md`). Both must be updated when adding or changing subcommands.

## Conventions

- Commit messages in English

## Quality Check

Run the code quality check before submitting changes:

```bash
scripts/quality-check.sh
```

This script runs all checks defined in `docs/ci.md` in sequence (test → clippy → wasm → audit → doc → coverage → complexity). Always use this script instead of running checks manually or writing inline scripts.

See `docs/ci.md` for details on individual tools and installation instructions.
