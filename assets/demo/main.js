// デモのページ（/demo/ と /ja/demo/）の入り口: jetdb（WebAssembly 版）でデータベースを開き、中身を見せる。
// 最初はサイトに置いたサンプルを開く。利用者が選んだファイルはブラウザの外に送らない。
// 紹介ページの動きは src/main.ts にある
import { codeOf, el, messageOf, reasonOf } from "./dom.js";
import { MESSAGES } from "./messages.js";
import { detailView, hashOf } from "./views.js";
// ページの言語の文字
const T = MESSAGES[document.documentElement.lang === "ja" ? "ja" : "en"];
const SAMPLE_URL = "/assets/jetdb-sample.accdb";
const SAMPLE_NAME = "jetdb-sample.accdb";
function byId(id) {
    const element = document.getElementById(id);
    if (!element)
        throw new Error(`no #${id} on the page`);
    return element;
}
const input = byId("demo-file");
const status = byId("demo-status");
const result = byId("demo-result");
const passwordForm = byId("demo-password");
const passwordInput = byId("demo-password-input");
const systemToggle = byId("demo-system");
const nav = byId("demo-nav");
const detail = byId("demo-detail");
// jetdb-wasm。サイトの /assets/wasm/ から、起動の中で読む。
// ファイルの頭の import で読むと、読めなかったときにモジュールごと止まり、エラーの一文も出せないため
let jetdb = null;
// 今開いているデータベースと、パスワードを待っているファイル
let db = null;
let pending = null;
// 左の一覧に出す種類。read は名前の一覧を返す（system はシステムや隠しのものも含めるか）
const KIND_READERS = [
    { kind: "table", read: (db, system) => db.tables({ system }) },
    {
        kind: "query",
        read: (db, system) => db.queries({ system }).map((q) => q.name),
    },
    {
        kind: "form",
        read: (db) => db
            .forms()
            .filter((f) => f.type === "Form")
            .map((f) => f.name),
    },
    {
        kind: "report",
        read: (db) => db
            .forms()
            .filter((f) => f.type === "Report")
            .map((f) => f.name),
    },
    { kind: "macro", read: (db, system) => db.macros({ system }) },
    { kind: "module", read: (db) => db.modules().map((m) => m.name) },
    {
        kind: "relationship",
        read: (db, system) => db.relationships({ system }).map((r) => r.name),
    },
];
// 種類ごとに名前の一覧を読む。読めなかった種類は、名前の代わりにエラーを持つ
function listObjects(db, system) {
    return KIND_READERS.map(({ kind, read }) => {
        const [one, many, title] = T.kinds[kind];
        try {
            return { kind, one, many, title, names: read(db, system) };
        }
        catch (e) {
            return { kind, one, many, title, error: reasonOf(T, e) };
        }
    });
}
// 開けたら、前のデータベースを解放して差し替える。パスワードが要るときは欄を出す
function open(bytes, label, name, password) {
    if (!jetdb)
        return;
    let next;
    try {
        next = jetdb.Database.open(bytes, password);
    }
    catch (e) {
        const code = codeOf(e);
        if (code === "PASSWORD_REQUIRED" || code === "INVALID_PASSWORD") {
            pending = { bytes, label, name };
            passwordForm.hidden = false;
            passwordInput.value = "";
            passwordInput.focus();
            status.textContent =
                code === "PASSWORD_REQUIRED"
                    ? T.passwordRequired(name)
                    : T.passwordWrong;
            return;
        }
        status.textContent =
            code === "INVALID_FILE"
                ? T.invalidFile(name, messageOf(e))
                : T.openFailed(name, reasonOf(T, e));
        return;
    }
    pending = null;
    passwordForm.hidden = true;
    try {
        db?.free();
    }
    catch {
        // 途中で止まった（パニックした）Database は解放できない。残したまま差し替える
    }
    db = next;
    byId("demo-current").textContent = label;
    showSummary(next, name);
    showNav(next);
    selectFromHash();
    status.textContent = "";
}
function showSummary(db, name) {
    byId("demo-name").textContent = name;
    byId("demo-version").textContent = db.version();
    byId("demo-counts").replaceChildren(...listObjects(db, false).map((k) => {
        const li = document.createElement("li");
        const value = document.createElement("strong");
        if (!k.names) {
            value.textContent = "—";
            li.append(value, ` ${k.many}`);
        }
        else {
            const [count, noun] = T.count(k.names.length, k.one, k.many);
            value.textContent = count;
            li.append(value, noun);
        }
        return li;
    }));
    result.hidden = false;
}
function navButton(kind, name, label) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "viewer-item";
    button.textContent = label;
    // 長い名前は 1 行で「…」と切るので、全部の名前を title に持たせる
    button.title = label;
    const hash = hashOf(kind, name);
    button.dataset.hash = hash;
    button.addEventListener("click", () => {
        if (location.hash !== hash)
            location.hash = hash;
    });
    return button;
}
// この数を超えるまとまりは、最初は閉じておく（選んでいるものがあれば開く）
const OPEN_LIMIT = 50;
function showNav(db) {
    const kinds = listObjects(db, systemToggle.checked);
    const groups = kinds.map((k) => {
        const group = document.createElement("details");
        group.className = "viewer-group";
        const current = (k.names ?? []).some((n) => hashOf(k.kind, n) === location.hash);
        group.open = !k.names || k.names.length <= OPEN_LIMIT || current;
        const summary = document.createElement("summary");
        summary.textContent = `${k.title} (${k.names ? k.names.length : "—"})`;
        group.append(summary);
        if (k.error !== undefined || !k.names) {
            const p = document.createElement("p");
            p.className = "viewer-error";
            p.textContent = T.couldNotRead(k.error ?? "");
            group.append(p);
        }
        else {
            const ul = document.createElement("ul");
            for (const name of k.names) {
                const li = document.createElement("li");
                li.append(navButton(k.kind, name, name));
                ul.append(li);
            }
            group.append(ul);
        }
        return group;
    });
    nav.replaceChildren(navSelect(kinds), navButton("database", null, T.database), ...groups);
    markCurrent();
}
// 狭い幅で一覧の代わりに出す選択の欄。種類ごとにまとめる
function navSelect(kinds) {
    const select = el("select", {
        class: "viewer-select viewer-nav-select",
        "aria-label": T.objectToShow,
    }, el("option", { value: "#database" }, T.database), kinds.flatMap(({ kind, title, names }) => names && names.length > 0
        ? [
            el("optgroup", { label: `${title} (${names.length})` }, names.map((n) => el("option", { value: hashOf(kind, n) }, n))),
        ]
        : []));
    select.addEventListener("change", () => {
        location.hash = select.value;
    });
    return select;
}
// 今選んでいるものに印を付ける
function markCurrent() {
    for (const button of nav.querySelectorAll(".viewer-item")) {
        if (button.dataset.hash === location.hash) {
            button.setAttribute("aria-current", "true");
            // 閉じているまとまりの中のものなら、まとまりを開く
            const group = button.closest("details");
            if (group)
                group.open = true;
        }
        else
            button.removeAttribute("aria-current");
    }
    const select = nav.querySelector(".viewer-nav-select");
    if (select)
        select.value = location.hash;
}
// # のものを右に出す。開いたファイルに無いもの、# が無いときは Database を出す。
// # の置き換えは replaceState で行う。location.hash に代入すると履歴に足され、hashchange も起きて
// 中身を 2 回作り直す（戻るボタンで前のファイルにしか無い # に戻るたびに #database が足され、抜け出せなくなる）
function selectFromHash() {
    if (!db)
        return;
    const known = [...nav.querySelectorAll(".viewer-item")].some((b) => b.dataset.hash === location.hash);
    if (!known)
        history.replaceState(null, "", "#database");
    const [kind, ...rest] = location.hash.slice(1).split("/");
    const name = rest.length ? decodeURIComponent(rest.join("/")) : "";
    markCurrent();
    detail.replaceChildren(...detailView({ db, t: T }, kind, name).filter((c) => c !== null && c !== undefined));
}
window.addEventListener("hashchange", () => selectFromHash());
systemToggle.addEventListener("change", () => {
    if (!db)
        return;
    showNav(db);
    selectFromHash();
});
// ファイルを選んだ回数。読み取りを待っている間に別のファイルを選んだら、前のものは開かない
let chosen = 0;
// ファイルを選んだときとパスワードを入れたときの処理は、起動より先に付ける。
// サンプルを取得している間（入力欄はもう有効）に選んだファイルも受け取るため
input.addEventListener("change", async () => {
    const file = input.files?.[0];
    if (!file)
        return;
    const turn = ++chosen;
    status.textContent = T.opening(file.name);
    try {
        const bytes = new Uint8Array(await file.arrayBuffer());
        // 開いている間は画面が止まるので、先に「開いています」を描かせる
        await new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve)));
        if (turn !== chosen)
            return;
        open(bytes, file.name, file.name, null);
    }
    catch (e) {
        // ファイルを読めなかった（選んだあとに消された、など）。開く処理のエラーは open() が出す
        if (turn !== chosen)
            return;
        status.textContent = T.openFailed(file.name, messageOf(e));
    }
});
passwordForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!pending)
        return;
    open(pending.bytes, pending.label, pending.name, passwordInput.value);
});
// 起動: jetdb-wasm を読む。読めなければデモは動かない（入力欄は無効のまま）
try {
    jetdb = await import("/assets/wasm/jetdb_wasm.js");
    await jetdb.default();
}
catch (e) {
    status.textContent = T.loadFailed(messageOf(e));
}
// サンプルを取得して開く。取得している間に自分のファイルを選んだ（読み取り中・パスワード待ちを含む）ときは、
// 届いたサンプルで上書きしない。サンプルだけ読めなくても、自分のファイルは開ける
if (jetdb) {
    input.disabled = false;
    status.textContent = T.openingSample;
    try {
        const response = await fetch(SAMPLE_URL);
        if (!response.ok)
            throw new Error(`HTTP ${response.status}`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (chosen === 0)
            open(bytes, T.sample(SAMPLE_NAME), SAMPLE_NAME, null);
    }
    catch (e) {
        if (chosen === 0)
            status.textContent = T.sampleFailed(messageOf(e));
    }
}
