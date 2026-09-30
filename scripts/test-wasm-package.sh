#!/usr/bin/env bash
set -euo pipefail

# Tests the jetdb-wasm npm package built by scripts/build-wasm-package.sh.
#
# Packs crates/jetdb-wasm/pkg/ with npm pack, installs the tarball into a
# temporary directory, and runs crates/jetdb-wasm/tests/smoke.mjs there, so
# the test sees what npm would install. Then type-checks
# crates/jetdb-wasm/tests/types/check.ts against the same install, with the
# TypeScript version fixed by its package-lock.json. Requires Node.js and npm.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$PROJECT_DIR/crates/jetdb-wasm"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

tarball="$(cd "$work" && npm pack --silent "$CRATE_DIR/pkg")"
(cd "$work" && npm init -y >/dev/null && npm install --silent --no-audit --no-fund "./$tarball")
cp "$CRATE_DIR/tests/smoke.mjs" "$work/"
node "$work/smoke.mjs" "$PROJECT_DIR/testdata"

# tsc finds jetdb-wasm in $work/node_modules, one directory up. The node config
# resolves the node condition of the exports, the bundler config the default.
cp -R "$CRATE_DIR/tests/types" "$work/types"
(cd "$work/types" && npm ci --silent --no-audit --no-fund)
for config in tsconfig.node.json tsconfig.bundler.json; do
    "$work/types/node_modules/.bin/tsc" -p "$work/types/$config"
    echo "types ok: $config"
done
