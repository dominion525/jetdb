// サイト（このリポジトリの直下）を HTTP で配る。ページを通すテスト（Playwright）が使う
//
//   node scripts/serve.ts [port]
//
// 127.0.0.1 だけで待つ。Wasm は、ブラウザが読みながらコンパイルできるように
// application/wasm で返す。書き換えた JS をすぐ読めるように、キャッシュさせない。
// Node.js 24 は型を取り除いてそのまま動かすので、型は消せる書き方だけを使う

import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("../", import.meta.url)));
const port = Number(process.argv[2] ?? 8080);

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
    const body = await readFile(file);
    response.writeHead(200, {
      "Content-Type": types[extname(file)] ?? "application/octet-stream",
      "Cache-Control": "no-store",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`http://localhost:${port}/`);
});
