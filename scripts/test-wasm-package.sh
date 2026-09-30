#!/usr/bin/env bash
set -euo pipefail

# Tests the jetdb-wasm npm package built by scripts/build-wasm-package.sh.
#
# Packs crates/jetdb-wasm/pkg/ with npm pack, installs the tarball into a
# temporary directory, and runs crates/jetdb-wasm/tests/smoke.mjs there, so
# the test sees what npm would install. Requires Node.js and npm.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$PROJECT_DIR/crates/jetdb-wasm"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

tarball="$(cd "$work" && npm pack --silent "$CRATE_DIR/pkg")"
(cd "$work" && npm init -y >/dev/null && npm install --silent --no-audit --no-fund "./$tarball")
cp "$CRATE_DIR/tests/smoke.mjs" "$work/"
node "$work/smoke.mjs" "$PROJECT_DIR/testdata"
