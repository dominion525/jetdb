#!/usr/bin/env bash
set -euo pipefail

# Tests the jetdb-wasm web build in Chromium, Firefox and WebKit with the
# Playwright tests in crates/jetdb-wasm/tests/browser/, which drive the
# developer page crates/jetdb-wasm/examples/index.html.
#
# Needs the package built by scripts/build-wasm-package.sh, the test data, and
# the browsers of the pinned Playwright version. CI installs them with
#   npm ci --prefix crates/jetdb-wasm/tests/browser
#   (cd crates/jetdb-wasm/tests/browser && npx playwright install --with-deps)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BROWSER_DIR="$PROJECT_DIR/crates/jetdb-wasm/tests/browser"

npm ci --prefix "$BROWSER_DIR" --silent --no-audit --no-fund
(cd "$BROWSER_DIR" && npx playwright test)
