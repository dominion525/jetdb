// Serves crates/jetdb-wasm/ over HTTP so that crates/jetdb-wasm/examples/index.html
// can load the package built by scripts/build-wasm-package.sh.
//
//   node scripts/serve-wasm-example.mjs [port]
//
// Then open http://localhost:8080/examples/ (or the port given). It listens on
// 127.0.0.1 only. Wasm is served as application/wasm, which the browser needs
// to compile it while it downloads.

import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("../crates/jetdb-wasm/", import.meta.url)));
const port = Number(process.argv[2] ?? 8080);

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
};

if (!existsSync(join(root, "pkg", "web"))) {
  console.error("crates/jetdb-wasm/pkg/ is missing: run scripts/build-wasm-package.sh first");
  process.exit(1);
}

createServer(async (request, response) => {
  const path = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
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
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`http://localhost:${port}/examples/`);
});
