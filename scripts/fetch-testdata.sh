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
    "saveastext/SportsAdmin/forms/Statistical Reports.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Statistical%20Reports.bas|be8163208f2b86689431c3532935a4a4cb2459e8b509840582dd46c8b3fde0d0"
    "saveastext/SportsAdmin/reports/Field (High Jump).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Field%20%28High%20Jump%29.bas|dd141206267811454c831b7900b4c53ccc48a878a2c7af55b236886bbdefa173"
    "saveastext/SportsAdmin/forms/About.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/About.bas|bc20dde725eb3a90ab90a78342705aad0759e7a5b1ff8dbb5c86f435c432ea35"
    "saveastext/SportsAdmin/forms/Carnival Copy.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Carnival%20Copy.bas|48a1fc698651602b762579e0f364669b7cc7ee6b666e8bb1b26838e07dc3f992"
    "saveastext/SportsAdmin/forms/Carnivals Maintain.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Carnivals%20Maintain.bas|c24108c34ee22d005f37f8d475132dd84d8d444201c76802acc67085d93db4e4"
    "saveastext/SportsAdmin/forms/CompEventsSummary.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/CompEventsSummary.bas|98effd25da2028751d4dd797443685cb7c0329ddfddeec1cd6e1051146ba8c21"
    "saveastext/SportsAdmin/forms/Competitor-Check.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Competitor-Check.bas|ce1102da430ee5bf6baafbf3ede48959e8672f57fe5de56f30c9bb0d789bafe9"
    "saveastext/SportsAdmin/forms/Competitor-QuickAdd.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Competitor-QuickAdd.bas|90b2222f0dca6cbf2c6c4b07471cb7fce6ca2544820c0494cd218dc9d3d84b10"
    "saveastext/SportsAdmin/forms/Competitors Bulk Maintain SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Competitors%20Bulk%20Maintain%20SF.bas|9d1b50c73c817f05fedfc1c6d78ea3d7f263d1c51e529e1e8d11ce8b1b0e1ad2"
    "saveastext/SportsAdmin/forms/Competitors Bulk Maintain.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Competitors%20Bulk%20Maintain.bas|f6b08ee95d9562e6f0560ae7f1e8efbb09834bc43ba0194172c57037d7dddedc"
    "saveastext/SportsAdmin/forms/Competitors Subform.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Competitors%20Subform.bas|12a164c40a0d1488c3d1ee430f99ed69882b71cd121cf95042a2e672d0619c4b"
    "saveastext/SportsAdmin/forms/Competitors.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Competitors.bas|393d83f304f48896d3c396f586f04a757fac1819289a9985eeb88008fa7707c0"
    "saveastext/SportsAdmin/forms/CompetitorsSummary.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/CompetitorsSummary.bas|7f81f4af93b6438c07e04fb4c3223cf6658dead5cdca28d57c899835728a18d6"
    "saveastext/SportsAdmin/forms/CopyCompetitorsBetweenEvents.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/CopyCompetitorsBetweenEvents.bas|474478f9d410ba1c0dbffbfe933b662d95e640ec1e9cd4c33fd8730ef0524a2b"
    "saveastext/SportsAdmin/forms/Enter Competitors Subform1.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Enter%20Competitors%20Subform1.bas|8274fe65eca4e875c4b50cf57d3408672ea8f3706aa6d4425d790531f60854bc"
    "saveastext/SportsAdmin/forms/EnterCompetitor Memo.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EnterCompetitor%20Memo.bas|1d1f2700b07878cbe491b491c176312126580a65f9e29c754d28b9fdf4e77a91"
    "saveastext/SportsAdmin/forms/EnterCompetitors.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EnterCompetitors.bas|268f2029317c76bf2456ffd7fb6ee73dacef53ee9fff491efc11d8618deaaca6"
    "saveastext/SportsAdmin/forms/EnterCompetitorsAuto.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EnterCompetitorsAuto.bas|99714275ccf6142290a8809381a35568381a6c1b7472151a434716eb2639e19d"
    "saveastext/SportsAdmin/forms/EventOrder.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventOrder.bas|8fb3c7c6d83c3389d4f946fd634a2d3435b76dc3bcf0e2c145e10b35b1a5cba5"
    "saveastext/SportsAdmin/forms/EventOrderSub-New.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventOrderSub-New.bas|62e5d6425398605af174e3b70fce73151617adab9b236e7a130d5e1df50214c0"
    "saveastext/SportsAdmin/forms/EventOrderSub-Orig.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventOrderSub-Orig.bas|35d620714f24724d5e432c92bd565e1d5defc59f4d5040bfd82870693a104cbf"
    "saveastext/SportsAdmin/forms/EventOrderSub.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventOrderSub.bas|1c7c0ea8e457a68f2dc03fe0eaffec7d0982880fb3cf77f1aba273f4d1e8acfd"
    "saveastext/SportsAdmin/forms/EventRecord.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventRecord.bas|db9b454e80d8f4dcad40a8eb414b6f1928127cd170dee8fd096f6603e4d5b893"
    "saveastext/SportsAdmin/forms/EventRecordHistory.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventRecordHistory.bas|c9e8c8b5250b7c435c791e45511d797918f455fb950de0315f66fc42c5b36b9a"
    "saveastext/SportsAdmin/forms/EventRecordsSF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventRecordsSF.bas|181d1f1374bf70da9a758b58c94c329ed6d6b469ec167c87d1b17cbc3656038d"
    "saveastext/SportsAdmin/forms/EventType-Wizard.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventType-Wizard.bas|073ee2d16470ac55eddb4ff6d6993d199fca9933f4ebe8490fb64afb19bea60c"
    "saveastext/SportsAdmin/forms/EventType.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventType.bas|4868eb0077a0fc373c733e7dcdc17f8a6a17851e0bd2078313e9eefc3d97c320"
    "saveastext/SportsAdmin/forms/EventTypeCopy.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventTypeCopy.bas|7cca142ddc8ae4ac9c97c4c6bfc61c09eec344e901857f364fa32dfa7b4517f8"
    "saveastext/SportsAdmin/forms/EventTypeSub1.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventTypeSub1.bas|cb8398be7454c63a4cb2f63c7e0469d66886c70295afa8e01d54c77994998233"
    "saveastext/SportsAdmin/forms/EventTypeSub2.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventTypeSub2.bas|e42e0ecdd3856318817d309d5337eb829e12204fa5710a6ec120eb1561b5fab7"
    "saveastext/SportsAdmin/forms/EventTypeSub3.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventTypeSub3.bas|450c14248bd61ca2ec872b4721b46783a77a0516271bb082eb6010092eae852e"
    "saveastext/SportsAdmin/forms/EventTypeSummary.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/EventTypeSummary.bas|cdbeaa695f4a2ca929419990e918a7aa1d03b5b27f96c9888ff894e969bf0e79"
    "saveastext/SportsAdmin/forms/ExportData.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/ExportData.bas|5703bda87328be095b809bafe142e865cb341232e293cc5ad50b9561c67a8120"
    "saveastext/SportsAdmin/forms/ExportTextSF1.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/ExportTextSF1.bas|27f7ba8f534e276cd171a880875c7858ee913b132f0951d9fd59813eb7cefc10"
    "saveastext/SportsAdmin/forms/Final Level SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Final%20Level%20SF.bas|f05a3bd8ad2bb28192e5a54fbb3fb9b64710a52a6ec6c3520c7b70eedf9276f2"
    "saveastext/SportsAdmin/forms/Final_Lev_Sub.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Final_Lev_Sub.bas|cce1903f7bc911498ae169172f7e7e76e2044b1dabbe4c4f8a69605410fb250d"
    "saveastext/SportsAdmin/forms/Final_lev.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Final_lev.bas|60f43f8987c57954bfc1009f8d9f2b198e6b054df5d9a37394e07eaa968717bb"
    "saveastext/SportsAdmin/forms/House Points-Extra SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/House%20Points-Extra%20SF.bas|444faa3fecd70b55a2811e698bc054cb065006f05a364b3335c60ecc8f690aa9"
    "saveastext/SportsAdmin/forms/House Points-Extra.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/House%20Points-Extra.bas|7259c9d76e00ebdaf9816122276690fd3ffb90d883b7dd798d758fe81b55292d"
    "saveastext/SportsAdmin/forms/House Summary SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/House%20Summary%20SF.bas|134f2c4b119989f9c2012347515ddb6ecaf8296ccf07c1c9ad5f59b0a9c72992"
    "saveastext/SportsAdmin/forms/House Summary.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/House%20Summary.bas|760ca61d057e96a03e236a65a76a47f45261cf51bb0653105f6377aa11d5031f"
    "saveastext/SportsAdmin/forms/Houses.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Houses.bas|de23afc9b41e13311d560e101b0103a4f52a9b694e011d73110becde5983fa0d"
    "saveastext/SportsAdmin/forms/Import Competitors SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Import%20Competitors%20SF.bas|de0163c165ee4e88fb191251365a1624b97cb40789f31daf634176226ec82107"
    "saveastext/SportsAdmin/forms/Import Competitors.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Import%20Competitors.bas|b5dff0d3a4407925fcb3e36a42a5517f47060a15e09adf78fbceb73dc53de1ef"
    "saveastext/SportsAdmin/forms/Import Data SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Import%20Data%20SF.bas|89aa61e18f8ccb3d3f7e197a237fd796a94a54a1cc7f9ee67ee4a068d2779302"
    "saveastext/SportsAdmin/forms/Import Data.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Import%20Data.bas|010bc57de048b85d93731c2ca2e62d5e3748ed6d17f629fb742031ff6b5cde6a"
    "saveastext/SportsAdmin/forms/Lane Promotion Allocation SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Lane%20Promotion%20Allocation%20SF.bas|a735740f9eb515e92d5107beaf4874cd0dbb56df185529c7ad7a07047dc04815"
    "saveastext/SportsAdmin/forms/Lane Promotion Allocation.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Lane%20Promotion%20Allocation.bas|a61b12612d163d0453dbed58c7a56a190b9c2fa547222a1c27d867ebd94fae5d"
    "saveastext/SportsAdmin/forms/Lane Subtitution SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Lane%20Subtitution%20SF.bas|5e7ab7b6b72faa21720c901cdc9d3e71dee9cbc430a1d340d7526490afae2eb2"
    "saveastext/SportsAdmin/forms/Lanes SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Lanes%20SF.bas|848d7b83082a4aa6c3802351c446eea2c9a9713c017b6cb458aeba4f0fc8060e"
    "saveastext/SportsAdmin/forms/Lanes.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Lanes.bas|a109e67dcdaa2b4faf52f83b06cf2982e06c8b6075fdfd14117f1c4150580bf2"
    "saveastext/SportsAdmin/forms/Main Menu.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Main%20Menu.bas|97af4460c025605206296fbb5061fb8d0158947b2963ab69380245fd9dedba73"
    "saveastext/SportsAdmin/forms/MeetManagerDivisions.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/MeetManagerDivisions.bas|6bf3fb5858c706e52ecf366cc0a8489aad26521e675b760e4fbb5573c1d3097c"
    "saveastext/SportsAdmin/forms/PleaseWait.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/PleaseWait.bas|ec9845adbffd27ecaf531a0a5a01928202d6a0b3b44d8581c4ad8fec623c13a0"
    "saveastext/SportsAdmin/forms/PointScale Add.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/PointScale%20Add.bas|90667b9ce8ff212721c8126a879cd75f58b40912d50f71e0cc0293a447076ad1"
    "saveastext/SportsAdmin/forms/PointScale.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/PointScale.bas|252d50ad2ee7afdc005c899a74c34454b5e23328ec102ca1fdd7c08e1330f513"
    "saveastext/SportsAdmin/forms/PointScaleSubform.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/PointScaleSubform.bas|7a3d3fc1d38a2119d02fc9fc2c8a75786d65710f23bcd9ad2a1f0ef0c33c6c65"
    "saveastext/SportsAdmin/forms/PromoteEvents.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/PromoteEvents.bas|60a00e24fc1f17ea2bcaf1716e7eabdf9418639b5dbdf19cf39a3000d2149c22"
    "saveastext/SportsAdmin/forms/Report SF2.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Report%20SF2.bas|054c1773a40452b6a89776886299e919f5124f43d2943bf6706f88cb813dfa37"
    "saveastext/SportsAdmin/forms/ReportsPopUp-SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/ReportsPopUp-SF.bas|7835180f59973586b54751ddc9cc79151a889897e304f7eccb3abc55a3dbab44"
    "saveastext/SportsAdmin/forms/ReportsPopUp.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/ReportsPopUp.bas|5f56f5cd129213b5943be7e7cf7f3ee6aa7af56cd72720c7c69a31c470b303b9"
    "saveastext/SportsAdmin/forms/Reports_Event.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Reports_Event.bas|5ca613cbb5f1b1f98a7d5cfcfc670dfda3d57aa3c556f8ee97faaebc62cc22f1"
    "saveastext/SportsAdmin/forms/Setup Carnival Competitors.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Setup%20Carnival%20Competitors.bas|cfff33b472091f940bbdd1830765c4670a6ac010028be13b9a6c5af8eb61913c"
    "saveastext/SportsAdmin/forms/Setup Carnival Disks.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Setup%20Carnival%20Disks.bas|5f4f91bfcbfa6895f17ba615df526750bc0e2ac20d841bd57b502704fbf2eabd"
    "saveastext/SportsAdmin/forms/Setup Carnival.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Setup%20Carnival.bas|a5583e2a70068b01383deae729c22970eae180462000a5bb1145d861109b9911"
    "saveastext/SportsAdmin/forms/SexSub.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/SexSub.bas|b63a34d16277533efb055fa21036c7c2e5f4227c938ead082496ea31553fd718"
    "saveastext/SportsAdmin/forms/Statistical Reports - Team SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Statistical%20Reports%20-%20Team%20SF.bas|00e0d38d912b63b6e332b1b78c8a061b9cdb9f3b36360664d2f036c49c05c1b9"
    "saveastext/SportsAdmin/forms/Temporary Results-Place Order.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Temporary%20Results-Place%20Order.bas|9f01ddeec8fd9ed830d1037c614400d40b83979cd09c5cf76489c343c2d9fa57"
    "saveastext/SportsAdmin/forms/TemporaryResultsSF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/TemporaryResultsSF.bas|7bd7ccd14b88d9b3961ef3b0a41ad3ecd16949bcc95e39afdb77d37b558bf4a3"
    "saveastext/SportsAdmin/forms/Utilities 2.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Utilities%202.bas|0e8555722497d0343d10eeb716a19aa9802bbbbd5e3029740cc324bc8a77d59d"
    "saveastext/SportsAdmin/forms/Utilities 3.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Utilities%203.bas|47b9a8cbbfe7b9cdeebf76cd9b780ac05a6b1379ae2437c7538ec5151c5185af"
    "saveastext/SportsAdmin/forms/Utilities-Export.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Utilities-Export.bas|0d779c8c2e07888c15653900b5bda23bac63948fd8479fb17f7c67a8ac154ac3"
    "saveastext/SportsAdmin/forms/Utilities-HTML.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Utilities-HTML.bas|a376cc4ae44b17d528dbfa829bb965b9a94d55eb73330dfc5217458f36868579"
    "saveastext/SportsAdmin/forms/Utilities.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/Utilities.bas|88742f76ff147f8482385a144aeb00608d228c434cfaf2ed8b1a6db41338b97c"
    "saveastext/SportsAdmin/forms/WhiteScreen.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/forms/WhiteScreen.bas|83919875067dfa3c5153a30c900b49dcd172cd6df79e197765547fc8ad00f4ff"
    "saveastext/SportsAdmin/reports/Competitor List.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Competitor%20List.bas|fdfcb4104349eeca2b7d240e1e9f9e504ef93e9f584d5aa8df3293e0c49250f9"
    "saveastext/SportsAdmin/reports/CompetitorEvents.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/CompetitorEvents.bas|97e9042403437def54b1c14e37b242f9a94fc861401a750c127f1da0fd02388b"
    "saveastext/SportsAdmin/reports/CompetitorList-ByTeamAge.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/CompetitorList-ByTeamAge.bas|1ebb66a99f82194e9f62ecdb2436175e778117b890129e6f745572fd6ad7b811"
    "saveastext/SportsAdmin/reports/EventResultsEntrySheet.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/EventResultsEntrySheet.bas|fc3cfb397c294364e59bdffa6e774c414af5a3578b21988b79ad80732d704e7c"
    "saveastext/SportsAdmin/reports/Field (3 attempts).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Field%20%283%20attempts%29.bas|56d1e720bdf152baeba5e125e6e902b29392040d6308b9a69db82c72fcffa750"
    "saveastext/SportsAdmin/reports/Misc-Non Participators.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Misc-Non%20Participators.bas|a36d59f2a71bc6fc5a67ee3d9e6a7b9f7b8c3377f661813ce354bca5c07ae2a0"
    "saveastext/SportsAdmin/reports/Name Tags SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Name%20Tags%20SF.bas|db0497587bffd21ebe104babcdbf6726c85cafc7b1a0b9a0fa5224bd40cbb73f"
    "saveastext/SportsAdmin/reports/Name Tags.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Name%20Tags.bas|2d77cfeec1bff5d920a3f5ac869991f1e4fa70ac25015296b0b67772de7098c5"
    "saveastext/SportsAdmin/reports/Program of Events SF-3 Col.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Program%20of%20Events%20SF-3%20Col.bas|4ad42c8dd5dfc3ae6c7000b165110908c58ab72dc352e0be634ad0519b447144"
    "saveastext/SportsAdmin/reports/Program of Events SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Program%20of%20Events%20SF.bas|a16b983bc79fb7eb04f5b53ac424adf3ed7ba1b79811d75abdf8e84070752cd3"
    "saveastext/SportsAdmin/reports/Program of Events-3 Col.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Program%20of%20Events-3%20Col.bas|659f590e441ce3cb8492f155bb5ad88fbc934196e8331a3e6447edad5535785d"
    "saveastext/SportsAdmin/reports/Program of Events-Summary.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Program%20of%20Events-Summary.bas|a0c67ff409f28e908b3aef36bc7e1749a95c66c7c92db467361c22f92584002d"
    "saveastext/SportsAdmin/reports/Program of Events.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Program%20of%20Events.bas|726e1b1b55655683220244f73772121252fd284501cae3356a9caf65e941ad72"
    "saveastext/SportsAdmin/reports/RecordDetails-ByDay.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/RecordDetails-ByDay.bas|58190fdbc77731e7c5ceaa657349b7b0287d5d4b0354dcbcf6878a2fdea7f6a4"
    "saveastext/SportsAdmin/reports/RecordDetails-Current.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/RecordDetails-Current.bas|c7ed6520062056c0c933a926d81df0cc5bdcab0fe3f4cdc2af8c0e894f2b2e1c"
    "saveastext/SportsAdmin/reports/RecordDetailsMini-Current.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/RecordDetailsMini-Current.bas|bcc7566acf1fdda6058035d5b6442e7a90869ed7ef0f6c4e64c6f8626a21b340"
    "saveastext/SportsAdmin/reports/RecordDetailsMini-SF.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/RecordDetailsMini-SF.bas|04033e5f082b88f21e7a8d85441f3b5677822a043f50aa5f49bc2c3f66a960a7"
    "saveastext/SportsAdmin/reports/Relay (Limited Lanes).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Relay%20%28Limited%20Lanes%29.bas|4e560166d29b8325ebf00a553a120f3eeda608e6bc55173b2aed8297b5ba31f8"
    "saveastext/SportsAdmin/reports/Relay Small (Limited Lanes).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Relay%20Small%20%28Limited%20Lanes%29.bas|431d2b817a539c9f5d53a8e7ccf035dfaf3e443966d435ae0f8ae99f75f48f2b"
    "saveastext/SportsAdmin/reports/Statistics-Age.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-Age.bas|0b3ff0999083a41292e8f9cdfc39d8b06b2446f7ea74251ab30f8e41aa0207aa"
    "saveastext/SportsAdmin/reports/Statistics-AgeChampions-AcrossAllDivisions.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-AgeChampions-AcrossAllDivisions.bas|08127ccb6cad0e1cc9d2d67dde0862266da80b5bd327e6231e6c4218327fcc3a"
    "saveastext/SportsAdmin/reports/Statistics-AgeChampions.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-AgeChampions.bas|7afa39482c757bba086783c65fdda507053a3c99663e805b212de3d2ebb3cdd2"
    "saveastext/SportsAdmin/reports/Statistics-AgeGender.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-AgeGender.bas|02d5caac36098a840146681633ce54f655270c271265a67605334bb97469b355"
    "saveastext/SportsAdmin/reports/Statistics-ByCarnival.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-ByCarnival.bas|1c281a69fb85d5b218143198c974a846aaebd32f4a81415cd1a9686a7caa1ce5"
    "saveastext/SportsAdmin/reports/Statistics-ByCarnivalType.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-ByCarnivalType.bas|ba73e59a11863d44e82804ec2da14cdf677106ad5e8f7edbb94cc56a1fcd5b4d"
    "saveastext/SportsAdmin/reports/Statistics-Competitor Results by House-Event.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-Competitor%20Results%20by%20House-Event.bas|e8df962a03fdf2bc03619dfef104b30a7a1bee2a698b6d0f5a665fef63f19ded"
    "saveastext/SportsAdmin/reports/Statistics-CompetitorEvents.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-CompetitorEvents.bas|6caf6f4ae29247f3e8d9c86cbdde2ce3e5f8b3d8a58c8e29fa8f6196f3513fbf"
    "saveastext/SportsAdmin/reports/Statistics-CompetitorResultsbyEventTeam.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-CompetitorResultsbyEventTeam.bas|c8340e030344af11ce885a86ee560dd0708341b90de3945c782c411594d435b4"
    "saveastext/SportsAdmin/reports/Statistics-Event.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-Event.bas|1d5f76e975d97654a012a0ca1c4823702003d0084a3940d22ddc030fad86f8bb"
    "saveastext/SportsAdmin/reports/Statistics-EventPlaces.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-EventPlaces.bas|92d69ef31e5f02c2407492980e876ff429e4895fd36ac497e85a5404ebdf1145"
    "saveastext/SportsAdmin/reports/Statistics-EventTimesOverallAsc.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-EventTimesOverallAsc.bas|12afcbaabafe80a70eeffb4d2fab4c6f48339fe3e1144f7cb0e73d6fe3e58deb"
    "saveastext/SportsAdmin/reports/Statistics-Overall%.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-Overall%25.bas|abe0f9de374211811a9ef8f8491c2f285f05b854821dc60e74502ae3e88e70e2"
    "saveastext/SportsAdmin/reports/Statistics-Overall.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-Overall.bas|6ec30f0fa54c3317980c45e93b430b533e996c5b0962966c4d947e5972532a18"
    "saveastext/SportsAdmin/reports/Statistics-PointByEventNumber.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-PointByEventNumber.bas|0f91f06c56dd4946cb6f087cc63fed0dbf0be9d2de8bd898dd0b1d1547295e5c"
    "saveastext/SportsAdmin/reports/Statistics-Sex.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-Sex.bas|1c8dff6ab8c4848a6ca7ce9d544cd7355fde4f422fdc5de82d5641aa1a65b0dd"
    "saveastext/SportsAdmin/reports/Statistics-byPlace.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Statistics-byPlace.bas|6f3b67ee2f1e9c4a2a9419ea73568b6233b966b939e86d46d855deffb66c4110"
    "saveastext/SportsAdmin/reports/Summary-Points.bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Summary-Points.bas|c0af208b0cc2cd9f2da4f9d308046182f0aef03979b85cac13359b0817066054"
    "saveastext/SportsAdmin/reports/Track (Limited Lanes).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Track%20%28Limited%20Lanes%29.bas|845ef48dfbc87be7b958062220652162b5d6cc759a91b9dd7af1aa2e3d592930"
    "saveastext/SportsAdmin/reports/Track (unlimited Lanes).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Track%20%28unlimited%20Lanes%29.bas|b159e55910c1782801453c9516896c198831d26bac39a95034489cfbd3ccca3e"
    "saveastext/SportsAdmin/reports/Track Small (Limited Lanes).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Track%20Small%20%28Limited%20Lanes%29.bas|7b8e29aba5ede2a18bdff1154f7fde05f7de3815d72d42508ba7cfc9ab901d00"
    "saveastext/SportsAdmin/reports/Track Small (Unlimited Lanes).bas|https://raw.githubusercontent.com/ruddj/SportsAdmin/a5b908665035125f92e6f44a0c78f7710e82cb95/Source/reports/Track%20Small%20%28Unlimited%20Lanes%29.bas|89833a2279d680ae12def3f788284a5efd988a59e010779bed52b458462764ec"
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
    # A failure, such as a 503 of the host, is tried again with a growing
    # wait in between. Every error is, as some curl builds give a 503 over
    # HTTP/2 the exit code of a receive failure, which --retry alone does not
    # take for a transient one.
    curl -sSLf --retry 5 --retry-all-errors -o "$tmp" "$url"

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
