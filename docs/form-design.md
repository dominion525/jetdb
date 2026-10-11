# Internal Structure of Form/Report Design Definitions

Findings from investigating the storage structure of form/report design definitions in Access database files (.mdb/.accdb).

## Overview

Form/report design definitions are stored in the `MSysAccessStorage` system table (Jet4/ACE format). No public specification exists for the binary format.

## Storage Structure in MSysAccessStorage

MSysAccessStorage is a table with a virtual-filesystem-like tree structure, where each row represents one entry (a folder or a stream).

Column layout:
- `Id` (Long, AUTO) - Entry ID
- `ParentId` (Long) - Parent entry ID
- `Name` (Text) - Entry name
- `Type` (Long) - 1=Storage (folder), 2=Stream (data)
- `Lv` (Ole) - Binary data body

### Tree Structure

```
Root (Id=1)
├── Forms (Type=1)
│   ├── DirData (Type=2)          Mapping of form names to storage numbers
│   ├── PropData (Type=2)         Properties of the Forms folder
│   ├── DirDataCopy (Type=2)      Backup of DirData
│   ├── PropDataCopy (Type=2)     Backup of PropData
│   ├── "0" (Type=1)              Individual form (storage number "0")
│   │   ├── Blob (Type=2)           Design body binary (60-124KB)
│   │   ├── TypeInfo (Type=2)       Control names & type information (~1.6KB)
│   │   ├── PropData (Type=2)       Properties (22 bytes)
│   │   └── BlobDelta (Type=2)      Delta data (usually empty)
│   ├── "1" (Type=1)              Another form
│   │   └── (same 4 streams)
│   ...
├── Reports (Type=1)              Reports (identical structure to Forms)
│   ├── DirData, PropData, ...
│   ├── "1" (Type=1)
│   │   └── Blob, TypeInfo, PropData, BlobDelta
│   ...
├── Modules (Type=1)              VBA modules
├── VBA (Type=1)                  VBA project
│   └── VBAProject (Type=1)
│       ├── VBA/ (streams)
│       ├── PROJECT (Type=2)
│       └── PROJECTwm (Type=2)
├── Scripts, Cmdbars, Databases, CustomGroups, ImExSpecs
└── PropData (Type=2)             Root properties
```

## DirData Format

DirData is a mapping table between form display names and internal storage numbers (strings such as "0", "1", ...).

```
First 4 bytes: Padding (0x00)
Repeated:
  0x04                  Marker byte
  <1 byte>              Payload length (in bytes)
  <UTF-16LE string>     Form display name + storage number
Trailing 4 bytes: 0x00
```

The storage number is stored as the last 2 bytes of the payload (a single UTF-16LE character).

Examples:
- Storage "0" → F_クライアント情報
- Storage "1" → F_クライアント担当一覧
- Storage "5" → F_メニュー

## TypeInfo Format

TypeInfo is a list of names and type codes for all controls within a form.

```
Header (32 bytes):
  4 bytes: Magic number 0xACCDEAF7
  4 bytes: Unknown (observed: 0x96)
  4 bytes: Unknown (observed: 0xFFFFFFFF)
  4 bytes: Entry count
  16 bytes: GUID

Each entry:
  Type code: 2 bytes (Little-Endian)
  Padding: 2 bytes
  Index: 4 bytes (Little-Endian)
  EventProcPrefix: Shift-JIS (cp932), NUL-terminated
    No parentheses → same as control name
    With parentheses → parentheses replaced with underscores (for VBA)
  RealName: Shift-JIS (cp932), NUL-terminated
    No parentheses → empty (NUL only)
    With parentheses → actual control name
```

Example controls included:
- Buttons: `Btn_クライアント一覧`
- Text boxes: `Txt_ログイン中のユーザー名`
- Check boxes: `Check_管理者`
- Sections: `フォームヘッダー`, `フォームフッター`, `詳細`

### Type Code (TypeInfo type_code) Mapping

Based on a comprehensive survey of all controls across 81 forms/reports in the IKP database.

Form controls:

| Code | Name | Count | Description |
|--------|------|--------|------|
| 0x066A | CheckBox | 95 | Check box |
| 0x0A7A | ToggleButton | 2 | Toggle button |
| 0x0B68 | CommandButton | 681 | Command button |
| 0x0C64 | Label | 289 | Label (standalone) |
| 0x0D64 | Label | 662 | Label (attached to control) |
| 0x0E65 | Rectangle | 111 | Rectangle |
| 0x0F67 | Image | 1 | Image |
| 0x126D | TextBox | 735 | Text box |
| 0x136F | ComboBox | 140 | Combo box |
| 0x1470 | SubForm | 9 | Subform |
| 0x1666 | Line | 168 | Line |
| 0x1898 | Detail | 63 | Detail section |
| 0x1899 | FormHeader | 63 | Form header section |
| 0x189A | FormFooter | 63 | Form footer section |
| 0x1EFF | (Internal metadata) | 318 | Not a real control. Stores Caption/ControlSource values or control names from related objects. Excluded by parser |
| 0x247F | EmptyCell | - | Empty cell (for layout) |

Report controls:

| Code | Name | Count | Description |
|--------|------|--------|------|
| 0x1B64 | Label | 333 | Label |
| 0x1B65 | Rectangle | 14 | Rectangle |
| 0x1B66 | Line | 49 | Line |
| 0x1B67 | Image | 1 | Image |
| 0x1B68 | CommandButton | 14 | Command button |
| 0x1B6A | CheckBox | 3 | Check box |
| 0x1B6D | TextBox | 554 | Text box |
| 0x1B6F | ComboBox | 32 | Combo box |
| 0x1B70 | SubReport | 2 | Subreport |
| 0x1998 | Detail | 18 | Detail section |
| 0x1999 | ReportHeader | 18 | Report header section |
| 0x199A | ReportFooter | 18 | Report footer section |
| 0x199D | GroupHeader | 29 | Group header section |
| 0x199E | GroupFooter | 15 | Group footer section |
| 0x1F9B | PageHeader | 18 | Page header section |
| 0x1F9C | PageFooter | 18 | Page footer section |

Note: Form and report controls have a paired structure (e.g., 0x0C64 Label <-> 0x1B64 Label).
Note: 0x1EFF is not a real control but internal metadata. No corresponding entry exists in the SaveAsText (COM-based) control listing. These entries store Caption or ControlSource values, or control names from related objects (e.g., reports).
Note: Counts are aggregated from all controls across 81 forms/reports in the IKP database.

## Blob (Design Body)

The main binary body of the form design. Contains structural information including ControlSource, event procedures, RecordSource, etc.

- Size: 918B (simple) to 52KB+ (complex forms)
- Leading bytes: `15 00 14 00 00 00 00 00` (common header across all forms)
- No public specification for the binary format. The following is based on reverse engineering analysis.

### Blob Overall Layout

```
[Header 8B]
  version(u16) = 0x0015, flags(u16) = 0x0014, reserved(u32) = 0
[Form-level fixed-length properties]
  Properties related to the form as a whole: RecordSource, Filter, FontName, etc.
[Form-level variable-length properties]
[Binary data (layout information, printer settings, font definitions, etc.)]
[Per-control property blocks x N]
  Each control has its own independent property block
  Starts with the Name(0x14) property
```

### Property Entry Format

Property names are not stored as strings; they are identified by numeric IDs (u16).

**Variable-length properties (string types):**
```
prop_id(u16) + unknown(u16) + type(u32) + flags(u32) + byte_length(u32) + UTF-16LE data
```

type values:
- 0x0A -- Name/font types (Name, FontName, Format, RowSourceType, ColumnWidths)
- 0x0C -- Text/data types (RecordSource, ControlSource, RowSource, Filter, Event, Caption)

**Fixed-length properties (numeric types):**
```
prop_id(u16) + unknown(u16) + type(u32) + flags(u32) + data[type-dependent]
```

type values:
- 0x01 -- Bool (4B)
- 0x02 -- Short (4B)
- 0x03 -- Long (6B)
- 0x04 -- Color (8B)
- 0x08 -- Double (8B)
- 0x09 -- GUID (16B)
- 0x0B -- Binary (variable length)

### Section Boundaries

- The boundary between form-level properties and per-control properties is separated by binary data (layout information, etc.)
- Each control block starts with the Name(0x14) property
- The number of controls in TypeInfo = the number of control blocks in the Blob

## Known Limitations

### Parser Limitations

- **Form level**: Sequential parsing from the beginning of the Blob, but parsing halts when an unknown type is encountered. Form-level properties after the halt point cannot be retrieved. In currently observed data, all major properties (RecordSource, Filter, Caption, FontName, etc.) appear before the halt point, and no practical issues have been identified.
- **Control level**: Separate from form-level parsing, a byte scan detects the Name property pattern `[14 00 0A 00 00 00]` to locate each control section, which is then parsed individually. When an unknown type is encountered within a control, the remaining properties of that control cannot be retrieved.
- **Layout coordinates & printer settings**: Stored in non-property regions within the Blob and are not extraction targets.
- **Jet3 (Access 97)**: Not supported because it uses a different format stored in MSysAccessObjects rather than MSysAccessStorage.

### prop_id Mapping

Known prop_id values are displayed by name; unknown ones are shown in `0xXXXX` numeric format. Every named property holds text: an ID whose value is not text belongs to another property in that place and is shown by number. For example, 0x0068 is OnKeyDown on a form, but holds a color on a label, which has no key events. Where the data has them, the names were checked against the SaveAsText output of Access for the 116 forms and reports of `Sports.accdb` (ruddj/SportsAdmin): each value matched the property of that name, except that 12 forms and reports have a FontName of their own that their SaveAsText output does not have.

| ID | Property Name | Purpose |
|--------|--------------|------|
| 0x0011 | Caption | Control display text |
| 0x0012 | ColumnWidths | Combo box column widths |
| 0x0014 | Name | Control name |
| 0x0016 | EventProcPrefix | Prefix of the event procedure names |
| 0x0017 | DefaultValue | Default value |
| 0x001B | ControlSource | Field binding target |
| 0x0022 | FontName | Font name (control level) |
| 0x0026 | Format | Display format |
| 0x004C | OLEClass | OLE class of an object frame |
| 0x004D | OnOpen | Open event |
| 0x004E | OnClose | Close event |
| 0x0056 | AfterUpdate | After update event |
| 0x005B | RowSource | Combo box data source (SQL) |
| 0x005D | RowSourceType | Data source type (Table/Query) |
| 0x0068 | OnKeyDown | Key down event |
| 0x0069 | OnKeyUp | Key up event |
| 0x006A | OnKeyPress | Key press event |
| 0x006B | OnMouseDown | Mouse button down event |
| 0x006C | OnMouseUp | Mouse button up event |
| 0x006D | OnMouseMove | Mouse move event |
| 0x0073 | OnGotFocus | After receiving focus |
| 0x0074 | OnLostFocus | After losing focus |
| 0x0079 | OnNoData | Report no data event |
| 0x007E | OnClick | Click event |
| 0x007F | OnLoad | Load event |
| 0x0084 | SourceObject | Form or report shown in a subform |
| 0x0087 | StatusBarText | Status bar text |
| 0x009C | RecordSource | Form/report data source |
| 0x00A0 | FontName | Form-level font name |
| 0x00DA | HelpFile | Help file |
| 0x00DE | OnEnter | Enter (focus received) event |
| 0x00DF | OnExit | Exit (focus lost) event |
| 0x00E0 | OnDblClick | Double-click event |
| 0x00F5 | Filter | Filter condition |
| 0x010A | LabelType | Label type |
| 0x0112 | Class | Class of an object frame |
| 0x0130 | ShortcutMenuBar | Shortcut menu |
| 0x013D | ControlTipText | Control tip text |
| 0x015A | InputMask | Input mask |
| 0x0161 | Toolbar | Toolbar |

Frequently occurring unknown IDs:

| ID | Occurrence Context (Estimated) |
|--------|------|
| 0x0000 | Frequently appears at the end of controls |
| 0x0013 | Form-level Bool |
| 0x001D | Within controls |
| 0x0178 | GUID (appears in all controls) |
| 0x024A | Short within controls |
| 0x024C | Short within controls |
| 0x0268 | Color-related property |
| 0x0274 | Color-related property |
| 0x01CA-0x01D2 | Short within controls (appears in consecutive sequence) |

### type Support Status

| type | Name | Data Size | Status |
|------|------|-------------|----------|
| 0x01 | Bool | 4B | Supported |
| 0x02 | Short | 5B | Supported |
| 0x03 | Long | 6B | Supported |
| 0x04 | Color | 8B | Supported |
| 0x06 | (Unknown) | 6B | Supported (interpreted as Long) |
| 0x08 | Double | 8B+4B trailer | Supported |
| 0x09 | GUID | 16B+4B trailer | Supported |
| 0x0A | Text | C bytes+4B trailer | Supported (UTF-16LE) |
| 0x0B | Binary | C bytes+4B trailer | Supported |
| 0x0C | Memo | C bytes+4B trailer | Supported (UTF-16LE) |
| 0x05 | (Unknown) | Unknown | Not supported (parsing halts on encounter) |
| 0x07 | (Unknown) | Unknown | Not supported (parsing halts on encounter) |

## References

- MDB Tools HACKING.md: https://github.com/mdbtools/mdbtools/blob/main/HACKING.md
- jabakobob.net Unofficial MDB Guide: http://jabakobob.net/mdb/
- isladogs.co.uk Long Value Binary fields: https://www.isladogs.co.uk/lv-fields/index.html
