// サイト（このリポジトリの直下）を HTTP で配る。ページを通すテスト（Playwright）が使う
//
//   node scripts/serve.ts [port] [--local-wasm]
//
// --local-wasm: デモの import map を、CDN でなく手元の node_modules/jetdb-wasm/web/ に向け直す。
// ページを通すテストが、ネットワークに出ずに同じ版の jetdb-wasm で動くようにするため。
//
// 127.0.0.1 だけで待つ。Wasm は、ブラウザが読みながらコンパイルできるように
// application/wasm で返す。書き換えた JS をすぐ読めるように、キャッシュさせない。
// Node.js 24 は型を取り除いてそのまま動かすので、型は消せる書き方だけを使う

import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("../", import.meta.url)));
const args = process.argv.slice(2);
const localWasm = args.includes("--local-wasm");
const port = Number(args.find((a) => !a.startsWith("--")) ?? 8080);

// CDN の jetdb-wasm の場所（版は何でも）。--local-wasm のときに HTML の中のこれを書き換える
const CDN_WASM = /https:\/\/cdn\.jsdelivr\.net\/npm\/jetdb-wasm@[^/"]+\/web\//g;
const LOCAL_WASM = "/node_modules/jetdb-wasm/web/";

const types: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".xml": "application/xml",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".wasm": "application/wasm",
};

createServer(async (request, response) => {
  let path: string;
  try {
    path = decodeURIComponent(
      new URL(request.url ?? "/", "http://localhost").pathname,
    );
  } catch {
    // /%E0 のような壊れた % の書き方で、この関数の外へ例外が出てサーバーが止まらないようにする
    response.writeHead(400).end();
    return;
  }
  let file = normalize(join(root, path));
  if (file !== root && !file.startsWith(root + sep)) {
    response.writeHead(403).end();
    return;
  }
  try {
    if ((await stat(file)).isDirectory()) file = join(file, "index.html");
    let body: Buffer | string = await readFile(file);
    if (localWasm && extname(file) === ".html")
      body = body.toString().replace(CDN_WASM, LOCAL_WASM);
    response.writeHead(200, {
      "Content-Type": types[extname(file)] ?? "application/octet-stream",
      "Cache-Control": "no-store",
    });
    response.end(body);
  } catch {
    // GitHub Pages と同じく、無いパスにはルートの 404.html を返す
    try {
      const body = await readFile(join(root, "404.html"));
      response.writeHead(404, {
        "Content-Type": types[".html"],
        "Cache-Control": "no-store",
      });
      response.end(body);
    } catch {
      response.writeHead(404).end();
    }
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`http://localhost:${port}/`);
});
