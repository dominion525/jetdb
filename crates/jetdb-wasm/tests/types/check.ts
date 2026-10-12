// Checks the TypeScript types of the installed jetdb-wasm package. It is only
// type-checked, never run: scripts/test-wasm-package.sh runs tsc on it once as
// Node.js resolves the package and once as a bundler does, so both builds'
// types are checked. Both use target es2022 rather than esnext, so the types
// must work for projects that do not target the newest JavaScript. A line
// under @ts-expect-error must fail to type-check, and tsc reports it when it
// does not.

import {
  Database,
  type Column,
  type ColumnType,
  type Index,
  type Relationship,
  type DdlDialect,
  type Query,
  type QueryType,
  type Module,
  type ModuleType,
  type Form,
  type FormType,
  type FormProperties,
  type Control,
  type Macro,
  type MacroStatement,
  type ObjectProperties,
  type ObjectType,
  type Property,
  type Row,
  type Value,
  type ErrorCode,
  type JetdbError,
} from "jetdb-wasm";

declare const bytes: Uint8Array;

const db = Database.open(bytes);
Database.open(bytes, "password");
Database.open(bytes, undefined);
Database.open(new ArrayBuffer(8));
// @ts-expect-error: the bytes are not a file name.
Database.open("test.mdb");

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

export const indexes: Index[] = db.indexes("Table1");
export const index: {
  name: string;
  primaryKey: boolean;
  columns: { name: string; descending: boolean }[];
  unique: boolean;
  ignoreNulls: boolean;
  required: boolean;
} = indexes[0];
// @ts-expect-error: an index has no type property.
indexes[0].type;
// @ts-expect-error: the table name is required.
db.indexes();

export const result: { columns: string[]; rows: Row[]; skipped: number } =
  db.rows("Table1");
export const rows: Row[] = result.rows;
// @ts-expect-error: the rows are in the rows property.
export const notRows: Row[] = db.rows("Table1");
export const value: null | boolean | number | bigint | string | Uint8Array =
  rows[0]["A"];
export const values: Value[] = Object.values(rows[0]);
// @ts-expect-error: a value is not always a string.
export const text: string = rows[0]["A"];
// @ts-expect-error: a value is never a Date.
export const date: Date = rows[0]["G"];
// @ts-expect-error: the table name is required.
db.rows();

export const relationships: Relationship[] = db.relationships();
db.relationships({ system: true });
export const relationship: {
  name: string;
  fromTable: string;
  toTable: string;
  columns: { from: string; to: string }[];
  referentialIntegrity: boolean;
  cascadeUpdate: boolean;
  cascadeDelete: boolean;
} = relationships[0];
// @ts-expect-error: system is a boolean.
db.relationships({ system: "yes" });

export const sql: string = db.ddl("postgres");
db.ddl("sqlite", { table: "Table1", indexes: false, relations: false });
db.ddl("mysql", {});
db.ddl("access", null);
export const dialect: DdlDialect = "sqlite";
// @ts-expect-error: oracle is not a dialect.
db.ddl("oracle");
// @ts-expect-error: the dialect is required.
db.ddl();
// @ts-expect-error: table is a name.
db.ddl("postgres", { table: 1 });
// @ts-expect-error: the option is indexes.
db.ddl("postgres", { index: false });

export const queries: Query[] = db.queries();
db.queries({ system: true });
export const query: { name: string; type: QueryType } = queries[0];
// @ts-expect-error: DataDefinition is not a query type; it is Ddl.
if (query.type === "DataDefinition") {
}
// @ts-expect-error: system is a boolean.
db.queries({ system: "yes" });
export const querySql: string = db.querySql("Query1");
// @ts-expect-error: the query name is required.
db.querySql();

export const modules: Module[] = db.modules();
export const vbaModule: { name: string; type: ModuleType } = modules[0];
// @ts-expect-error: Class is not a module type; it is ClassOrDocument.
if (vbaModule.type === "Class") {
}
export const moduleSource: string = db.moduleSource("Module1");
// @ts-expect-error: the module name is required.
db.moduleSource();

export const forms: Form[] = db.forms();
export const formType: FormType = forms[0].type;
// @ts-expect-error: Page is not a form type.
if (formType === "Page") {
}
export const design: FormProperties = db.formProperties("Form1");
export const control: { name: string; type: string; properties: Property[] } = design.controls[0];
export const controls: Control[] = design.controls;
export const formValue: Value = design.properties[0].value;
// @ts-expect-error: the form name is required.
db.formProperties();

export const macroNames: string[] = db.macros();
db.macros({ system: true });
// @ts-expect-error: system is a boolean.
db.macros({ system: "yes" });
export const namedMacro: Macro = db.macro("Macro1");
export const macroText: string | null = namedMacro.text;
export function actionNames(statements: MacroStatement[]): string[] {
  return statements.flatMap((s) => {
    switch (s.kind) {
      case "action":
        return [s.name];
      case "if":
        return s.branches.flatMap((b) => actionNames(b.statements));
      case "group":
      case "submacro":
      case "dataBlock":
        return actionNames(s.statements);
      case "comment":
      case "unknown":
        return [];
    }
  });
}
if (namedMacro.source.kind === "embedded") {
  const control: string | null = namedMacro.source.control;
  void control;
}
export const embedded: Macro[] = db.embeddedMacros("Form1");
export const dataMacros: Macro[] = db.dataMacros("Table1");
export const dataText: string = db.dataMacrosText("Table1");
// @ts-expect-error: the text of a macro may be null.
export const notNull: string = namedMacro.text;
// @ts-expect-error: the macro name is required.
db.macro();

export const properties: ObjectProperties = db.properties("Table1");
db.properties("Customers", { type: "Table" });
export const props: {
  object: { name: string; value: Value }[];
  columns: { name: string; properties: Property[] }[];
  additional: { name: string; properties: Property[] }[];
} = properties;
export const objectType: ObjectType = "Form";
// @ts-expect-error: View is not an object type.
db.properties("Customers", { type: "View" });
// @ts-expect-error: the object name is required.
db.properties();
// @ts-expect-error: a property value is not always a string.
export const propertyText: string = properties.object[0].value;

declare const caught: unknown;
if (caught instanceof Error && caught.name === "JetdbError") {
  const error = caught as JetdbError;
  const code: ErrorCode = error.code;
  if (code === "PASSWORD_REQUIRED" || code === "OBJECT_NOT_FOUND") {
  }
  // @ts-expect-error: NOT_FOUND is not an error code.
  if (code === "NOT_FOUND") {
  }
}

db.free();
