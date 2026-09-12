#!/usr/bin/env bash
set -euo pipefail

# Downloads the test data files that are not stored in this repository.
#
# Tests that use these files are guarded by `skip_if_missing!`, so they are
# skipped until this script has been run. CI runs it before `cargo test`;
# locally, run it once and the files stay in place.
#
# Each entry pins a URL to an immutable commit hash and is verified against a
# SHA-256 digest. See `testdata/SOURCES.md` for where the files come from and
# why they are fetched rather than committed.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TESTDATA_DIR="$PROJECT_DIR/testdata"

# path relative to testdata/ | URL | SHA-256
FILES=(
    "V1997/nwind.mdb|https://raw.githubusercontent.com/mdbtools/mdbtestdata/156fc65bb35ac50a8d3428ca8861af3c775a6b6b/data/nwind.mdb|4682dfc91be526e6508948cc53adf5c63d70fdf7f2cc1f1403ee76b66ac914b2"
)

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        echo "Error: neither sha256sum nor shasum is available" >&2
        exit 1
    fi
}

for entry in "${FILES[@]}"; do
    IFS='|' read -r rel_path url expected <<<"$entry"
    dest="$TESTDATA_DIR/$rel_path"

    if [ -f "$dest" ] && [ "$(sha256_of "$dest")" = "$expected" ]; then
        echo "up to date: testdata/$rel_path"
        continue
    fi

    echo "fetching:   testdata/$rel_path"
    mkdir -p "$(dirname "$dest")"
    tmp="$dest.download"
    curl -sSLf -o "$tmp" "$url"

    actual="$(sha256_of "$tmp")"
    if [ "$actual" != "$expected" ]; then
        rm -f "$tmp"
        echo "Error: checksum mismatch for testdata/$rel_path" >&2
        echo "  expected $expected" >&2
        echo "  actual   $actual" >&2
        exit 1
    fi
    mv "$tmp" "$dest"
done
