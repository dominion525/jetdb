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
    "saveastext/SportsAdmin/Sports.accdb|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Sports.accdb|30cd8eef5935091a797348a5e6073cd27aab18c4ed5f96a9da33ebac857fbca7"
    "saveastext/SportsAdmin/macros/ApplyFilter.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/ApplyFilter.bas|2ae3d71f8cc906ec8d3c4b15842ee2407d53d06b3ab0c362a292c64ec20fe41c"
    "saveastext/SportsAdmin/macros/AutoExec.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/AutoExec.bas|0e633ddfb2518a3fd13f1369208c06e19933520c10f4f0cc3c966f6d7cb526ae"
    "saveastext/SportsAdmin/macros/AutoKeys.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/AutoKeys.bas|e00d18f91ffb5408abafa25b78f72e0593261121fb8672bc2c23892b36cf066f"
    "saveastext/SportsAdmin/macros/ClosePleaseWait.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/ClosePleaseWait.bas|f5f859b861c6d2996b9398370efc5ac61b0987f8bbd0808d6100b224e52a6a99"
    "saveastext/SportsAdmin/macros/CompactDatabase.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/CompactDatabase.bas|9927b46b0e39132efeb8a4f3eb48c4e1bfe26964e275057cae938a61e0cde8bd"
    "saveastext/SportsAdmin/macros/Menu.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Menu.bas|8b999ed31c24cea2dd0be9feddd5bdda16c9c7cd0ee739c67dad0f71954596f6"
    "saveastext/SportsAdmin/macros/Menu_Carnival.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Menu_Carnival.bas|65ad6fc55934f6cd22ab62045d22c93859c5e4a5fc3fd60ab5dcffbd849470ef"
    "saveastext/SportsAdmin/macros/Menu_Database.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Menu_Database.bas|4bd51a0a54f97037f390aa6937ac77d7dba995b0f222300dc33c0b764390a17a"
    "saveastext/SportsAdmin/macros/Menu_Printing.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Menu_Printing.bas|3aacd905901580657b09987a1aabc5818df7f950317cf32043ace4d64d002816"
    "saveastext/SportsAdmin/macros/Open Help.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Open%20Help.bas|a54da9eb57cbe380c742e118e39a0ac9576b0925190f1ecd08b83ab6bd03bf6f"
    "saveastext/SportsAdmin/macros/ReportPopup-Update.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/ReportPopup-Update.bas|0a02f5fb31978bdfc513052d8ec0b4b763a21c77d97ef860d562798c2b823d74"
    "saveastext/SportsAdmin/macros/Restore.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Restore.bas|1175ef26d11a153e8519e3bb57336ff674aa10dd2362b267793b4b24161a4d24"
    "saveastext/SportsAdmin/macros/ShowPleaseWait.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/ShowPleaseWait.bas|2eed56d9d0960e82d5ad1be62e4f4199b4b370f0420ada2257a0b13259cc783d"
    "saveastext/SportsAdmin/macros/ShowPrintDialog.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/ShowPrintDialog.bas|0377ffe8610365f503fc98d21e1cb4138fea6d18ebc3aa282025a91062bbb327"
    "saveastext/SportsAdmin/macros/Sports Menu.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu.bas|3e8ee2f8bc7c7f39d89c975a2013f525a8cd5e8df794dbbeca9023eb035f6e71"
    "saveastext/SportsAdmin/macros/Sports Menu_Carnival Backup.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu_Carnival%20Backup.bas|1be7e9650efeadc764939321938d7c0ea50522be5fe711e2a233ffd24f360b0b"
    "saveastext/SportsAdmin/macros/Sports Menu_Carnival.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu_Carnival.bas|d8c88b7d74699a8538286f3efeb3a98e9f88f3cb65080df2a1b032c136baf146"
    "saveastext/SportsAdmin/macros/Sports Menu_Competitors.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu_Competitors.bas|8c2ad0cbe682f6ee3612f8257aba6581b1b9cd89a28461ee495eb73dd81c8435"
    "saveastext/SportsAdmin/macros/Sports Menu_Events.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu_Events.bas|c7d161704b9e0484d06fff476e4718d87d988f5e1c5577c1af5a77214712a98d"
    "saveastext/SportsAdmin/macros/Sports Menu_Help.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu_Help.bas|fb30f0dcd41caa1a3a977e5923bc6a8bcf692dd69b10e2574c365da1666a8798"
    "saveastext/SportsAdmin/macros/Sports Menu_Utilities.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu_Utilities.bas|1d245b0ed2a71080f5aae3e15b3cb1b4b547ace19e6ff1c321aaaf48434409cb"
    "saveastext/SportsAdmin/macros/Sports Menu_Utilities_Printing.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Sports%20Menu_Utilities_Printing.bas|1c20204353985b174935f0192d3464f3ee0f8dff0acc1771a844f9b3020ffadc"
    "saveastext/SportsAdmin/macros/Work_AutoEventNumber.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/Work_AutoEventNumber.bas|34cc17c8f65608e7455974e9610c88d6c6c91c38723a91499d607abb0ca393da"
    "saveastext/SportsAdmin/macros/_Run Check inventory.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/macros/_Run%20Check%20inventory.bas|81e8b80a05e408ca7dcc75c75d0ae27e331fe7f7e959fff26fb01713b23b27d7"
    "saveastext/Strings/Strings.mdb|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Strings.mdb|e71e16c4c440db0ca0388ce16d23b9dcfb5c24267c4132ddd089bbe4952d042f"
    "saveastext/Strings/macros/CloseAll.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/CloseAll.accmac|cc2c22b0f9834e9b5ef2a85cee0cf5cc82dcc036f30dbe214f3d12b54c7c5a70"
    "saveastext/Strings/macros/Initialize.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/Initialize.accmac|a7d6b1f1c0673e41f4d7861f1ed1e6709382be9d37e8d92e2355116117d1e7e5"
    "saveastext/Strings/macros/RestoreRefs.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/RestoreRefs.accmac|0fc1cf36816b2c5fbce2b35dcc2c7af32d34321679b01ce913dde13aac6cc3f1"
    "saveastext/Strings/macros/SourcesBackUp.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/SourcesBackUp.accmac|91a966ca66d6767dafb5aae95585dcf177e4880b25d602dd100db48484db623d"
    "saveastext/Strings/macros/SourcesRestore.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/SourcesRestore.accmac|5edfb37dbefad5c14880c2d41385ceb02965264dda53771822ef86320c90c013"
    "saveastext/Strings/macros/UpdateAppInfo.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/UpdateAppInfo.accmac|095316231b10058cd63925b9e7db53252f472aaff3106329b7ec01758e711e97"
    "saveastext/Strings/macros/UpdateAppVersion.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/UpdateAppVersion.accmac|0c0767c7ae968f358a94457e99904ef37834dfdcb9423eb8c560bd91f34c243b"
    "saveastext/Strings/macros/UpdateModuleRevision.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/UpdateModuleRevision.accmac|fe8c9dabcec451a738581b6ad75cba0a3e4baabbdfea094a111d2882b5296ed5"
    "saveastext/Strings/macros/UpdateModuleVersion.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/UpdateModuleVersion.accmac|1999c3d1089c7f26f3442e9cf671753e44eee50ef9c3d67067b043833e657f24"
    "saveastext/Strings/macros/UpdateObjectsFromStorage.accmac|https://raw.githubusercontent.com/iKaRus-VLZ/Strings/9bf2b115e335b64cb57453f179dcfba8498a9a9c/Macro/UpdateObjectsFromStorage.accmac|6b134930977aa0a62b853517fffe47d901786bcb345f8dec1c8907b4671d812a"
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
