// Checks the TypeScript types of the installed jetdb-wasm package. It is only
// type-checked, never run: scripts/test-wasm-package.sh runs tsc on it once as
// Node.js resolves the package and once as a bundler does, so both builds'
// types are checked. Both use target es2022 rather than esnext, so the types
// must work for projects that do not target the newest JavaScript. A line
// under @ts-expect-error must fail to type-check, and tsc reports it when it
// does not.

import { Database, type Column, type ColumnType } from "jetdb-wasm";

declare const bytes: Uint8Array;

const db = Database.open(bytes);
Database.open(bytes, "password");
Database.open(bytes, undefined);

type Version =
  "JET3" | "JET4" | "ACE12" | "ACE14" | "ACE15" | "ACE16" | "ACE17";
export const version: Version = db.version();
// @ts-expect-error: JET5 is not a version.
if (db.version() === "JET5") {
}

export const tables: string[] = db.tables();
db.tables({});
db.tables({ system: true });
// @ts-expect-error: the option is system.
db.tables({ sytem: true });
// @ts-expect-error: system is a boolean.
db.tables({ system: "yes" });

export const columns: Column[] = db.columns("Table1");
export const column: {
  name: string;
  type: ColumnType;
  size: number;
  precision: number;
  scale: number;
  autoNumber: boolean;
  calculated: boolean;
} = columns[0];
// @ts-expect-error: Integer is not a column type.
if (column.type === "Integer") {
}
// @ts-expect-error: the table name is required.
db.columns();

db.free();
