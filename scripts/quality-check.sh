#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

# --- Colors (disabled if not a terminal) ---
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    RED='' GREEN='' YELLOW='' BOLD='' RESET=''
fi

pass()  { printf "${GREEN}PASS${RESET}"; }
fail()  { printf "${RED}FAIL${RESET}"; }
skip()  { printf "${YELLOW}SKIP${RESET}"; }

# --- Prerequisite checks ---
has_cmd() { command -v "$1" &>/dev/null; }

if ! has_cmd cargo || ! has_cmd rustc; then
    echo "Error: cargo and rustc are required" >&2
    exit 1
fi

HAS_LLVM_COV=false
HAS_AUDIT=false
HAS_RCA=false
has_cmd cargo-llvm-cov && HAS_LLVM_COV=true
has_cmd cargo-audit    && HAS_AUDIT=true
# cargo-audit is invoked as `cargo audit`, but the binary is cargo-audit
# cargo-llvm-cov is invoked as `cargo llvm-cov`, but the binary is cargo-llvm-cov
has_cmd rust-code-analysis-cli && HAS_RCA=true

# Wasm checks need wasmtime plus both Wasm targets installed through rustup.
HAS_WASM=false
if has_cmd wasmtime && has_cmd rustup; then
    installed_targets=$(rustup target list --installed 2>/dev/null)
    if echo "$installed_targets" | grep -qx wasm32-unknown-unknown \
        && echo "$installed_targets" | grep -qx wasm32-wasip1; then
        HAS_WASM=true
    fi
fi

TOTAL=7
STEP=0
ERRORS=0

coverage_summary=""
coverage_pct=""
complexity_summary=""
complexity_count=""

header() {
    STEP=$((STEP + 1))
    printf "${BOLD}[%d/%d] %-16s${RESET}" "$STEP" "$TOTAL" "$1"
}

echo ""
echo "${BOLD}=== jetdb quality check ===${RESET}"
echo ""

# --- 1. Test ---
header "Test"
test_output=$(cargo test 2>&1) || {
    printf " $(fail)\n"
    echo "$test_output" | tail -20
    echo ""
    echo "Tests failed. Aborting."
    exit 1
}
test_passed=$(echo "$test_output" | grep -oE '[0-9]+ passed' | tail -1 | grep -oE '[0-9]+')
printf " $(pass) (%s passed)\n" "${test_passed:-?}"

# --- 2. Clippy ---
header "Clippy"
clippy_output=$(cargo clippy --all-targets -- -D warnings 2>&1) || {
    printf " $(fail)\n"
    echo "$clippy_output" | tail -20
    echo ""
    echo "Clippy failed. Aborting."
    exit 1
}
printf " $(pass)\n"

# --- 3. Wasm ---
# The browser target and WASI are separate builds, so both are checked: the
# browser build must compile, and the library tests must pass on WASI, where
# they can open the files under testdata/. On failure the whole output is shown.
header "Wasm"
if [ "$HAS_WASM" = true ]; then
    wasm_output=$( {
        cargo check --target wasm32-unknown-unknown -p jetdb \
            && CARGO_TARGET_WASM32_WASIP1_RUNNER="wasmtime run --dir $PROJECT_DIR" \
                cargo test --target wasm32-wasip1 -p jetdb
    } 2>&1 ) && {
        wasm_passed=$(echo "$wasm_output" | grep -oE '[0-9]+ passed' | grep -m1 -oE '[0-9]+')
        printf " $(pass) (browser build ok, %s passed on WASI)\n" "${wasm_passed:-?}"
    } || {
        printf " $(fail)\n"
        echo "$wasm_output"
        ERRORS=$((ERRORS + 1))
    }
else
    printf " $(skip) (needs wasmtime and the wasm32-unknown-unknown / wasm32-wasip1 targets)\n"
fi

# --- 4. Audit ---
header "Audit"
if [ "$HAS_AUDIT" = true ]; then
    audit_output=$(cargo audit 2>&1) && {
        printf " $(pass)\n"
    } || {
        printf " $(fail)\n"
        echo "$audit_output" | tail -20
        ERRORS=$((ERRORS + 1))
    }
else
    printf " $(skip) (cargo-audit not installed)\n"
fi

# --- 5. Doc ---
header "Doc"
doc_output=$(cargo doc --workspace 2>&1) && {
    printf " $(pass)\n"
} || {
    printf " $(fail)\n"
    echo "$doc_output" | tail -20
    ERRORS=$((ERRORS + 1))
}

# --- 6. Coverage ---
header "Coverage"
if [ "$HAS_LLVM_COV" = true ]; then
    # Run with test output suppressed; capture only the summary table
    cov_output=$(cargo llvm-cov --workspace 2>&1) && {
        # Extract the summary table (lines starting with "Filename" through the blank line after TOTAL)
        coverage_summary=$(echo "$cov_output" | sed -n '/^Filename/,/^$/p')
        coverage_pct=$(echo "$cov_output" | grep '^TOTAL' | awk '{
            # Find the "Lines" percentage — it is the column that contains the line coverage
            # cargo llvm-cov text format: TOTAL  regions  miss  cover%  funcs  miss  cover%  lines  miss  cover%
            for (i=1; i<=NF; i++) {
                if ($i ~ /[0-9]+\.[0-9]+%/) { last=$i }
            }
            print last
        }')
        printf " $(pass) (%s lines)\n" "${coverage_pct:-?}"
    } || {
        printf " $(fail)\n"
        ERRORS=$((ERRORS + 1))
    }
else
    printf " $(skip) (cargo-llvm-cov not installed)\n"
fi

# --- 7. Complexity ---
header "Complexity"
if [ "$HAS_RCA" = true ]; then
    rca_output=$(rust-code-analysis-cli -m -p crates/ -O json 2>/dev/null) || true
    complexity_summary=$(echo "$rca_output" | cargo run --manifest-path "$PROJECT_DIR/crates/complexity-filter/Cargo.toml" --quiet 2>&1) && {
        complexity_count=$(echo "$complexity_summary" | head -1 | grep -oE '[0-9]+')
        printf " $(pass) (%s functions above threshold)\n" "${complexity_count:-0}"
    } || {
        printf " $(fail)\n"
        ERRORS=$((ERRORS + 1))
    }
else
    printf " $(skip) (rust-code-analysis-cli not installed)\n"
fi

# --- Detail sections ---
if [ -n "$coverage_summary" ]; then
    echo ""
    echo "${BOLD}--- Coverage ---${RESET}"
    echo "$coverage_summary"
fi

if [ -n "$complexity_summary" ]; then
    echo ""
    echo "${BOLD}--- Complexity (CC>=10 | Cog>=10 | SLOC>=50) ---${RESET}"
    echo "$complexity_summary"
fi

echo ""
if [ "$ERRORS" -gt 0 ]; then
    echo "${RED}${BOLD}Quality check completed with $ERRORS issue(s).${RESET}"
    exit 1
else
    echo "${GREEN}${BOLD}Quality check passed.${RESET}"
    exit 0
fi
