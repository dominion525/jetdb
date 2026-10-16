# jetdb のサイト

jetdb の紹介ページ・全項目の比較表・ブラウザで試せるデモ（日英）の置き場です。HTML と CSS は手で書き、JavaScript は `src/` の TypeScript から作ります。

## 置き場所

- `index.html`・`compare/`・`demo/`・`ja/`: 各ページの HTML
- `assets/style.css`: 全部のページの CSS
- `src/main.ts`: 全部のページが読む小さな動き（コピーボタン、インストールのタブ）。モジュールではないので `import` / `export` は書かない
- `src/demo/`: デモのページの動き。`main.ts`（起動と画面の流れ）、`dom.ts`（DOM の部品）、`views.ts`（オブジェクトの種類ごとの表示）、`messages.ts`（日英の文字）
- `assets/main.js`・`assets/demo/`: `src/` から作った JavaScript。ページはこれを読む
- `tests/unit/`: `src/` の関数のテスト（Vitest のブラウザモード、Chromium）
- `tests/e2e/`: ページを開いて確かめるテスト（Playwright、Chromium・Firefox・WebKit）
- `scripts/serve.ts`: サイトを手元で配るスクリプト

## 前提

- Node.js 24
- デモが読む jetdb-wasm（コミットしない）: jetdb 本体で `scripts/build-wasm-package.sh` を実行し、`crates/jetdb-wasm/pkg/web/` の `jetdb_wasm.js`・`jetdb_wasm_bg.wasm`・`jetdb_wasm.d.ts` を `assets/wasm/` に写す。`.d.ts` は型の検査に使う
- ページを開くテストで使う jetdb 本体の `testdata/`: 環境変数 `JETDB_TESTDATA` で指す。指さないときは、このディレクトリの隣の `../jetdb/testdata` を読む

`assets/wasm/` が無いとき、デモのページを開くテストは飛ばす。testdata のファイルが無いときも、そのファイルを使うテストは飛ばす。

## 初めの準備

```bash
npm ci
npx playwright install chromium firefox webkit
```

## コマンド

```bash
npm run build      # src/ の TypeScript から assets/ の JavaScript を作る
npm run check      # 型の検査（src/ とテスト）と Biome の検査
npm run lint       # Biome の検査だけ
npm test           # tests/unit/ のテスト
npm run test:e2e   # tests/e2e/ のテスト
node scripts/serve.ts 8080   # http://localhost:8080/ でサイトを配る（キャッシュさせない）
```

## 決まり

- `src/` を書き換えたら `npm run build` を実行し、作った `assets/` の JavaScript も同じコミットに入れる。ページは作ったものを読むので、作り忘れるとページに出ない
- `assets/main.js` と `assets/demo/` は直接書き換えない
- Biome が検査・整形するのは `src/`・`tests/`・`scripts/` と、直下の設定のファイルだけ。HTML・CSS・Markdown は対象にしない
