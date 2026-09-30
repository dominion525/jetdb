#!/usr/bin/env bash
set -euo pipefail

# Builds the jetdb-wasm npm package into crates/jetdb-wasm/pkg/.
#
# The package holds two builds of the same Wasm: web/ (an ES module for
# browsers and bundlers) and node/ (CommonJS for Node.js). package.json picks
# one for the importer through its "exports" field.
#
# Requires the wasm32-unknown-unknown target, jq, and the wasm-bindgen CLI of
# the same version as the wasm-bindgen crate in Cargo.lock
# (cargo install wasm-bindgen-cli --version <that version>).

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CRATE_DIR="$PROJECT_DIR/crates/jetdb-wasm"
OUT_DIR="$CRATE_DIR/pkg"

metadata="$(cargo metadata --format-version 1 --manifest-path "$PROJECT_DIR/Cargo.toml" \
    --filter-platform wasm32-unknown-unknown)"
bindgen_version="$(jq -r '.packages[] | select(.name == "wasm-bindgen") | .version' <<<"$metadata")"
crate_version="$(jq -r '.packages[] | select(.name == "jetdb-wasm") | .version' <<<"$metadata")"
target_dir="$(jq -r '.target_directory' <<<"$metadata")"

cli_version="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$cli_version" != "$bindgen_version" ]; then
    echo "Error: wasm-bindgen CLI $cli_version does not match wasm-bindgen $bindgen_version in Cargo.lock" >&2
    echo "  cargo install wasm-bindgen-cli --version $bindgen_version" >&2
    exit 1
fi

cargo build --release --target wasm32-unknown-unknown -p jetdb-wasm \
    --manifest-path "$PROJECT_DIR/Cargo.toml"
wasm="$target_dir/wasm32-unknown-unknown/release/jetdb_wasm.wasm"

rm -rf "$OUT_DIR"
wasm-bindgen --target web --out-dir "$OUT_DIR/web" "$wasm"
wasm-bindgen --target nodejs --out-dir "$OUT_DIR/node" "$wasm"
# The web build is an ES module; node/ stays CommonJS.
echo '{ "type": "module" }' >"$OUT_DIR/web/package.json"

# The generated types declare [Symbol.dispose](), which the TypeScript
# libraries before esnext lack. Referencing that library from the types lets
# projects with an older target use them without changing their settings
# (TypeScript 5.2 or later).
for build in web node; do
    types="$OUT_DIR/$build/jetdb_wasm.d.ts"
    { echo '/// <reference lib="esnext.disposable" />'; cat "$types"; } >"$types.tmp"
    mv "$types.tmp" "$types"
done

jq --arg version "$crate_version" '.version = $version' "$CRATE_DIR/package.json" \
    >"$OUT_DIR/package.json"
cp "$PROJECT_DIR/LICENSE-MIT" "$PROJECT_DIR/LICENSE-APACHE" "$OUT_DIR/"

echo "built: $OUT_DIR (jetdb-wasm $crate_version)"
