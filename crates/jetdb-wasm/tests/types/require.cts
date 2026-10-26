// Checks the types of the Node.js build as a CommonJS module imports it, with
// require rather than import. Only type-checked, with tsconfig.node.json; see
// check.ts.

import init, { Database, initSync } from "jetdb-wasm";

declare const bytes: Uint8Array;

export async function open(): Promise<Database> {
  await init();
  initSync({ module: bytes });
  return Database.open(bytes);
}
