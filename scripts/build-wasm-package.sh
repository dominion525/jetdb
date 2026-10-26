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

# Panic messages hold the paths of the sources they come from. Those of the
# dependencies and of the standard library are under the home directory of
# the machine that builds, so they are remapped: the cargo home to /cargo, and
# the sources of the standard library to /rustc/<commit>, where the standard
# library of a release has them. The flags are passed one per field, so a path
# may hold spaces.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sysroot="$(rustc --print sysroot)"
commit="$(rustc -vV | awk '/^commit-hash:/ {print $2}')"
remap_registry="--remap-path-prefix=$cargo_home=/cargo"
remap_std="--remap-path-prefix=$sysroot/lib/rustlib/src/rust=/rustc/$commit"
sep=$'\x1f'

CARGO_ENCODED_RUSTFLAGS="$remap_registry$sep$remap_std" \
    cargo build --profile wasm-release --target wasm32-unknown-unknown -p jetdb-wasm \
    --manifest-path "$PROJECT_DIR/Cargo.toml"
wasm="$target_dir/wasm32-unknown-unknown/wasm-release/jetdb_wasm.wasm"

rm -rf "$OUT_DIR"
wasm-bindgen --target web --out-dir "$OUT_DIR/web" "$wasm"
wasm-bindgen --target nodejs --out-dir "$OUT_DIR/node" "$wasm"
# The web build is an ES module; node/ stays CommonJS.
echo '{ "type": "module" }' >"$OUT_DIR/web/package.json"

# The web build is initialized with its default export, init, or with
# initSync. The Node.js build loads its Wasm when it is required, so it gets
# an init and an initSync that do nothing: the two builds then have the same
# shape, and code that calls init runs on both. __esModule tells TypeScript
# and bundlers that the default export is exports.default. Node.js gives
# module.exports as the default export of CommonJS to an ES module, so ES
# modules import the Node.js build through jetdb_wasm.mjs, which gives init
# as its default export. Its types do not use those of the browser, which a
# project for Node.js may not have.
cat >>"$OUT_DIR/node/jetdb_wasm.js" <<'EOF'

exports.default = async function init() {};
exports.initSync = function initSync() {};
Object.defineProperty(exports, "__esModule", { value: true });
EOF
cat >"$OUT_DIR/node/jetdb_wasm.mjs" <<'EOF'
import jetdb from "./jetdb_wasm.js";

export * from "./jetdb_wasm.js";
export default jetdb.default;
EOF
cat >>"$OUT_DIR/node/jetdb_wasm.d.ts" <<'EOF'

/**
 * Does nothing: the Node.js build loads its Wasm when it is imported. The web
 * build is initialized with it, so code that calls it runs on both.
 */
export function initSync(module?: unknown): void;

/**
 * Does nothing: the Node.js build loads its Wasm when it is imported. The web
 * build is initialized with it, so code that calls it runs on both.
 */
export default function init(module_or_path?: unknown): Promise<void>;
EOF

# The generated types declare [Symbol.dispose](), which the TypeScript
# libraries before esnext lack. Referencing that library from the types lets
# projects with an older target use them without changing their settings
# (TypeScript 5.2 or later).
for build in web node; do
    types="$OUT_DIR/$build/jetdb_wasm.d.ts"
    { echo '/// <reference lib="esnext.disposable" />'; cat "$types"; } >"$types.tmp"
    mv "$types.tmp" "$types"
done
cp "$OUT_DIR/node/jetdb_wasm.d.ts" "$OUT_DIR/node/jetdb_wasm.d.mts"

jq --arg version "$crate_version" '.version = $version' "$CRATE_DIR/package.json" \
    >"$OUT_DIR/package.json"
cp "$PROJECT_DIR/LICENSE-MIT" "$PROJECT_DIR/LICENSE-APACHE" "$CRATE_DIR/README.md" "$OUT_DIR/"

echo "built: $OUT_DIR (jetdb-wasm $crate_version)"
