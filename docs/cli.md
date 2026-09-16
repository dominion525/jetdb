# jetdb CLI

A read-only command-line tool for Microsoft Access database files (.mdb / .accdb).

## Installation

```bash
cargo install --path crates/jetdb-cli
```

## Global Options

These options can be used with any subcommand.

- `--password <PASSWORD>` — Database password (for password-protected .accdb files)

## Subcommands

### ver — Show database engine version

```
jetdb ver [OPTIONS] <FILE>
```

Display the Jet/ACE engine version of the database file.

#### Options

- `-l`, `--long` — Show detailed version information

#### Output examples

```
$ jetdb ver testV2003.mdb
JET4

$ jetdb ver -l testV2003.mdb
Jet4 (Access 2000/2003)
```

#### Version list

| Short name | Details |
|------------|---------|
| JET3 | Jet3 (Access 97) |
| JET4 | Jet4 (Access 2000/2003) |
| ACE12 | ACE12 (Access 2007) |
| ACE14 | ACE14 (Access 2010) |
| ACE15 | ACE15 (Access 2013) |
| ACE16 | ACE16 (Access 2016) |
| ACE17 | ACE17 (Access 2019) |

### tables — List tables

```
jetdb tables [OPTIONS] <FILE>
```

Output the table names in the database, one per line.

#### Options

- `-s`, `--system` — Include system tables (those with SYSTEM/HIDDEN flags)
- `-t`, `--show-type` — Prefix each line with the type number (tab-separated)
- `-T`, `--show-type-name` — Prefix each line with the type name (tab-separated)

`-t` and `-T` are mutually exclusive (cannot be used together).

#### Output examples

```
$ jetdb tables test.mdb
Table1
Table2

$ jetdb tables -s test.mdb
MSysObjects
MSysACEs
Table1
Table2

$ jetdb tables -t test.mdb
1	Table1
1	Table2

$ jetdb tables -T test.mdb
table	Table1
table	Table2

$ jetdb tables -s -T test.mdb
systable	MSysObjects
systable	MSysACEs
table	Table1
table	Table2
```

#### Type names

Type names displayed with -T:

| Name | Condition |
|------|-----------|
| table | Regular user table |
| systable | Table with SYSTEM or HIDDEN flag |

### schema — Show table schema

```
jetdb schema [OPTIONS] <FILE>
```

Display column definitions, indexes, and relationships of tables.
When the `--ddl` option is specified, output as SQL DDL (CREATE TABLE, etc.).

#### Options

- `-T`, `--table <NAME>` — Show only the specified table
- `--ddl <DIALECT>` — Output as DDL in the specified dialect (sqlite, postgres, mysql, access)
- `--no-indexes` — Omit index definitions
- `--no-relations` — Omit relationship definitions

#### Output examples

```
$ jetdb schema test.mdb -T Table1
Table: Table1

  Columns:
    A  Text(100)
    B  Text(200)
    C  Byte
    D  Int
    E  Long
    F  Double
    G  Timestamp
    H  Money
    I  Boolean

  Indexes:
    B           [B ASC]
    PrimaryKey  [A ASC]  UNIQUE REQUIRED

$ jetdb schema test.mdb --ddl sqlite -T Table1
CREATE TABLE "Table1" (
    "A" TEXT,
    "B" TEXT,
    "C" INTEGER,
    "D" INTEGER,
    "E" INTEGER,
    "F" REAL,
    "G" TEXT,
    "H" NUMERIC,
    "I" INTEGER,
    PRIMARY KEY ("A")
);

CREATE INDEX "B" ON "Table1" ("B");

$ jetdb schema indexTest.mdb --ddl postgres
CREATE TABLE "Table1" (
    "id" INTEGER,
    "otherfk1" INTEGER,
    "otherfk2" INTEGER,
    "data" VARCHAR(100),
    "otherfk3" INTEGER,
    PRIMARY KEY ("id")
);

CREATE TABLE "Table2" (
    "id" INTEGER,
    "data" VARCHAR(100),
    PRIMARY KEY ("id")
);

CREATE TABLE "Table3" (
    "id" INTEGER,
    "data" VARCHAR(100),
    PRIMARY KEY ("id")
);

CREATE INDEX "id" ON "Table1" ("id");

CREATE INDEX "id" ON "Table2" ("id");

CREATE INDEX "id" ON "Table3" ("id");

ALTER TABLE "Table1" ADD CONSTRAINT "Table2Table1"
    FOREIGN KEY ("otherfk1") REFERENCES "Table2" ("id")
    ON DELETE CASCADE;
ALTER TABLE "Table1" ADD CONSTRAINT "Table3Table1"
    FOREIGN KEY ("otherfk2") REFERENCES "Table3" ("id")
    ON UPDATE CASCADE;
```

#### DDL dialect list

| Name | Description |
|------|-------------|
| sqlite | SQLite |
| postgres | PostgreSQL |
| mysql | MySQL |
| access | Access SQL |

### queries — Manage saved queries

#### queries list — List saved query names

```
jetdb queries list [OPTIONS] <FILE>
```

List the names of saved queries in the database (space-separated by default).

##### Options

- `-1`, `--newline` — Print one query name per line
- `-d`, `--delimiter <STRING>` — Custom delimiter between query names (default: space)

##### Output examples

```
$ jetdb queries list queryTest.mdb
AppendQuery CrosstabQuery DataDefinitionQuery DeleteQuery MakeTableQuery PassthroughQuery SelectQuery UnionQuery UpdateQuery

$ jetdb queries list -1 queryTest.mdb
AppendQuery
CrosstabQuery
DataDefinitionQuery
DeleteQuery
MakeTableQuery
PassthroughQuery
SelectQuery
UnionQuery
UpdateQuery

$ jetdb queries list test.mdb
(no output — database has no saved queries)
```

#### queries show — Show query SQL

```
jetdb queries show <FILE> <QUERY_NAME>
```

Show the restored SQL definition of the specified saved query.

##### Output examples

```
$ jetdb queries show queryTest.mdb DeleteQuery
DELETE [Table1].[col1], [Table1].[col2], [Table1].[col3]
FROM [Table1]
WHERE (([Table1].[col1]="foo"));
```

### prop — Show object properties

```
jetdb prop <FILE> <OBJECT_NAME>
```

Display the LvProp (Lightweight Property) values for a database object
such as a table or query. Properties are grouped into table-level,
per-column, and additional property maps.

#### Output examples

```
$ jetdb prop test.mdb Table1
Object: Table1

  Table Properties:
    Orientation  0
    OrderByOn    no
    NameMap      (496 bytes)
    GUID         {5A29A676-1145-4D1A-AE47-9F5415CDF2F1}
    DefaultView  2

  Column: A
    ColumnWidth         -1
    ColumnOrder         0
    ColumnHidden        no
    Required            no
    AllowZeroLength     no
    DisplayControl      109
    UnicodeCompression  yes
    GUID                {E9EDD90C-CE55-4151-ABE1-A1ACE1007515}
    IMEMode             0
    IMESentenceMode     3
  ...
```

### vba — Manage VBA modules

#### vba list — List VBA module names

```
jetdb vba list [OPTIONS] <FILE>
```

List the names of VBA modules in the database (space-separated by default, sorted alphabetically).
If the database has no VBA project, produces no output.

##### Options

- `-1`, `--newline` — Print one module name per line
- `-d`, `--delimiter <STRING>` — Custom delimiter between module names (default: space)

##### Output examples

```
$ jetdb vba list vbaTest.mdb
Class1 Form_Form1 Module1

$ jetdb vba list -1 vbaTest.mdb
Class1
Form_Form1
Module1

$ jetdb vba list -d "|" vbaTest.mdb
Class1|Form_Form1|Module1

$ jetdb vba list test.mdb
(no output — database has no VBA project)
```

#### vba show — Show VBA module source code

```
jetdb vba show <FILE> <MODULE_NAME>
```

Show the source code of the specified VBA module.

##### Output examples

```
$ jetdb vba show vbaTest.mdb Module1
Attribute VB_Name = "Module1"
Option Compare Database
Option Explicit

Public Function Hello() As String
    Hello = "Hello, World!"
End Function
...
```

### form — Manage forms and reports

#### form list — List form and report names

```
jetdb form list [OPTIONS] <FILE>
```

List the names of forms and reports in the database (space-separated by default, sorted alphabetically).
If the database has no forms or reports, produces no output.

##### Options

- `-1`, `--newline` — Print one name per line
- `-d`, `--delimiter <STRING>` — Custom delimiter between names (default: space)
- `--forms-only` — Show only forms
- `--reports-only` — Show only reports

##### Output examples

```
$ jetdb form list database.accdb
F_Menu F_ClientList R_MonthlyReport

$ jetdb form list -1 --forms-only database.accdb
F_Menu
F_ClientList

$ jetdb form list --reports-only database.accdb
R_MonthlyReport
```

#### form dump — Dump a binary stream

```
jetdb form dump [OPTIONS] <FILE> <NAME>
```

Dump the raw binary stream from a form or report to standard output. The default stream is the `blob` (main design definition). Redirect to a file for further analysis.

##### Options

- `-s`, `--stream <STREAM>` — Which stream to dump (default: `blob`)
  - `blob` — Main design binary (layout, controls, properties, events)
  - `typeinfo` — Control name and type list
  - `propdata` — Small property metadata
  - `blobdelta` — Delta data (usually empty)

##### Output examples

```
$ jetdb form dump database.accdb F_Menu > form_blob.bin
$ xxd form_blob.bin | head
00000000: 1500 1400 0000 0000 9600 1400 ...

$ jetdb form dump -s typeinfo database.accdb F_Menu > typeinfo.bin
```

#### form controls — List control names from TypeInfo

```
jetdb form controls <FILE> <NAME>
```

Parse the TypeInfo stream and list all controls in the form or report. Output is tab-separated: name, type code (hex), index.

##### Output examples

```
$ jetdb form controls database.accdb F_ClientList
FormHeader      FormHeader      0
Detail          Detail          1
FormFooter      FormFooter      2
Btn_Search      CommandButton   3
Txt_Name        TextBox         4
Cmb_Status      ComboBox        5
```

#### form props — Show form/report and control properties

```
jetdb form props <FILE> <NAME>
```

Parse the Blob binary stream and display properties for the form/report itself and each control. Properties include RecordSource, ControlSource, Filter, Caption, FontName, event handlers, and more.

Known property IDs are displayed by name; unknown IDs are shown as `0xXXXX`. Binary values are shown as `(N bytes)`, GUIDs as `{...}`, and booleans as `yes`/`no`.

##### Output examples

```
$ jetdb form props database.accdb F_ClientList
Form: F_ClientList

  Form Properties:
    RecordSource  SELECT * FROM T_Clients ORDER BY ID;
    Filter        ([ID] > 0)
    Caption       Client List
    FontName      MS PGothic

  Control: FormHeader (FormHeader)
    Name    FormHeader

  Control: Txt_Name (TextBox)
    Name           Txt_Name
    ControlSource  Name
    FontName       Meiryo UI

  Control: Cmb_Status (ComboBox)
    Name           Cmb_Status
    RowSourceType  Table/Query
    RowSource      SELECT Status FROM T_StatusList;
    FontName       Meiryo UI
```

### macro — Manage macros

#### macro list — List named macro names

```
jetdb macro list [OPTIONS] <FILE>
```

List the names of named macros in the database (space-separated by default, sorted alphabetically).
If the database has no named macros, produces no output.

##### Options

- `-1`, `--newline` — Print one macro name per line
- `-d`, `--delimiter <STRING>` — Custom delimiter between macro names (default: space)

##### Output examples

```
$ jetdb macro list database.accdb
AutoExec mcrConditions mcrSimple

$ jetdb macro list -1 nwind.mdb
Customer Labels Dialog
Customers
Sample Autokeys
```

#### macro show — Show a named macro

```
jetdb macro show [OPTIONS] <FILE> <MACRO_NAME>
```

Show the named macro as the text `Application.SaveAsText` writes, with CRLF line endings. Macros saved without XML, including Access 97 macros, are shown the same way.

##### Options

- `--xml` — Show the macro's XML instead. A macro saved without XML produces no output.

##### Output examples

```
$ jetdb macro show nwind.mdb Customers
Version =131074
PublishOption =1
ColumnsShown =3
Begin
    Comment ="Attached to the Customers form."
End
...
Begin
    MacroName ="Update Country List"
    Action ="Requery"
    Comment ="Requery the Country control."
    Argument ="Country"
End

$ jetdb macro show --xml database.accdb mcrSimple
<?xml version="1.0" encoding="UTF-16" standalone="no"?>
<UserInterfaceMacro ...><Statements><Action Name="OpenForm">...</Statements></UserInterfaceMacro>
```

#### macro embedded — Show the embedded macros of a form or report

```
jetdb macro embedded [OPTIONS] <FILE> <NAME>
```

Show each embedded macro of the form or report as the `<event>EmMacro = Begin` ... `End` block `Application.SaveAsText` writes in the form's or report's text, without the block's indentation and with CRLF line endings. If the form or report has no embedded macros, produces no output.

##### Options

- `--xml` — Show each embedded macro's XML instead, each followed by a newline. The `For` attribute names the control and `Event` the event.

##### Output examples

```
$ jetdb macro embedded database.accdb frmEmbedded
OnLoadEmMacro = Begin
    Version =196611
    ColumnsShown =0
    Begin
        Comment ="_AXL:<?xml version='1.0' encoding='UTF-16' standalone='no'?><UserInterfaceMacro "
            "Event='OnLoad' xmlns='http://schemas.microsoft.com/office/accessservices/2009/11"
            "/application'><Statements><Action Name='SetTe"
    End
    ...
End
OnClickEmMacro = Begin
    ...
End

$ jetdb macro embedded --xml database.accdb frmEmbedded
<?xml version='1.0' encoding='UTF-16' standalone='no'?><UserInterfaceMacro Event='OnLoad' ...>...</UserInterfaceMacro>
<?xml version='1.0' encoding='UTF-16' standalone='no'?><UserInterfaceMacro For='btnHello' Event='OnClick' ...>...</UserInterfaceMacro>
```

#### macro data — Show the data macros of a table

```
jetdb macro data [OPTIONS] <FILE> <TABLE>
```

Show the data macros of the table as the text `Application.SaveAsText` writes: one `DataMacros` XML document, with the table events in the order AfterInsert, AfterUpdate, AfterDelete, BeforeChange, BeforeDelete, followed by the named data macros. If the table has no data macros, produces no output.

##### Options

- `--xml` — Show each data macro's XML as stored instead, each followed by a newline

##### Output examples

```
$ jetdb macro data database.accdb tblItems
<?xml version="1.0" encoding="UTF-16" standalone="no"?>
<DataMacros xmlns="..."><DataMacro Event="AfterInsert">...</DataMacro><DataMacro Event="BeforeChange">...</DataMacro></DataMacros>

$ jetdb macro data --xml database.accdb tblNamed
<?xml version="1.0" encoding="UTF-16" standalone="no"?>
<DataMacro Name="dmLog" ...><Parameters><Parameter Name="msg"/></Parameters><Statements>...</Statements></DataMacro>
```

### export — Export table data as CSV

```
jetdb export [OPTIONS] <FILE> <TABLE>
```

Export all rows from a table as RFC 4180 compliant CSV to standard output.
By default, replication system columns are excluded.

#### Options

- `-H`, `--no-header` — Suppress the header row
- `-d`, `--delimiter <CHAR>` — Column delimiter (default: `,`)
- `-D`, `--date-format <FMT>` — Date format, strftime subset (default: `%Y-%m-%d`)
- `-T`, `--datetime-format <FMT>` — Date-time format, strftime subset (default: `%Y-%m-%d %H:%M:%S`)
- `-b`, `--bin <MODE>` — Binary output mode (default: `hex`)
- `-0`, `--null <STRING>` — String to represent NULL values (default: empty string)
- `-B`, `--boolean-words` — Output booleans as TRUE/FALSE instead of 1/0
- `-s`, `--system-columns` — Include replication system columns

#### Output examples

```
$ jetdb export test.mdb Table1
A,B,C,D,E,F,G,H,I
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",1

$ jetdb export test.mdb Table1 -H
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",1

$ jetdb export test.mdb Table1 -d "\t"
A	B	C	D	E	F	G	H	I

$ jetdb export test.mdb Table1 -B
A,B,C,D,E,F,G,H,I
"foo","bar",1,2,3,1.5,2003-01-02,"12.3400",TRUE
```

#### Binary output modes

| Name | Description |
|------|-------------|
| strip | Omit binary data (empty string) |
| raw | Output as raw bytes (lossy UTF-8) |
| octal | Each byte as \NNN octal escape |
| hex | Each byte as lowercase hex (default) |
