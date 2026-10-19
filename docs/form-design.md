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

Based on a comprehensive survey of all controls across 81 forms/reports in a database in real use.

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
Note: Counts are aggregated from all controls across 81 forms/reports in a database in real use.

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

Property names are not stored as strings; they are identified by numeric IDs (u16). Every entry has the same layout:

```
prop_id(u16) + type(u32) + B(u32) + length(u32) + value[length bytes] + trailer(u32)
```

`length` is the number of bytes of the value. The 4-byte trailer after the value is a number that grows from one entry to the next, except for the last entry of a group, whose trailer is 253, 254 or 255: the properties of the form or report itself end with 255, and those of a control with 253. The type tells how to read the value (see "type Support Status"); B is not used.

type values of text:
- 0x0A -- Name/font types (Name, FontName, Format, RowSourceType, ColumnWidths)
- 0x0C -- Text/data types (RecordSource, ControlSource, RowSource, Filter, Event, Caption)

### Section Boundaries

- The boundary between form-level properties and per-control properties is separated by binary data (layout information, etc.)
- Each control block starts with the Name(0x14) property
- The number of controls in TypeInfo = the number of control blocks in the Blob

## Blob of the Access 2000 Format

A form or report saved in the Access 2000 format keeps its design in the Blob too, but as a tree rather than property entries. Its Blob starts with the byte `0x13`, the `Version =19` of its SaveAsText output; the Blob of Access 2002 and later starts with `0x14` or `0x15`.

```
Blob     12 bytes of header, the properties of the form or report itself, then a list
List     FF + count (u16) + entries, or FE + one entry
Entry    type (u16) + properties + (FD, or a list of the entries in it)
Property ID (1 byte) + value
```

- The entries at the top of the tree are the default controls of the form or report and its sections; the entries in a section are its controls, and the entries in a tab control its pages. jetdb gives the sections and controls, as for later formats, and not the default controls.
- The type of an entry is the `acControlType` of Access, the low byte of the type codes of TypeInfo: 100 a label, 109 a text box, 123 a tab control, 124 a page, 152 the detail section, 155 and 156 the page header and footer, and so on.
- The ID of a property is numbered in each type: `0xDC` is the name of a control and the record source of a form. The ID tells the size of the value: none for `0x00`–`0x2F` (a Yes/No set to the value that is not its default, given as `NotDefault`), 1 byte for `0x30`–`0x5F`, 2 bytes for `0x60`–`0x7F`, 4 bytes for `0x80`–`0xBC`, and a length byte and that many bytes for `0xBD`–`0xFC`, the length byte `0xFF` being followed by a length of 2 bytes. Of `0x80`–`0xBC`, only `0x9C`–`0xA0` have been seen, so where the 4-byte values end in `0xA1`–`0xBC` is not known.

jetdb gives a property the ID of Access 2002 and later from a table of each type (409 pairs), and the value as that ID holds it, so that the names of [prop_id Mapping](#prop_id-mapping) apply. The pairs were tied by the forms and reports of `testdata/V2000/controlPropertyTestV2000.mdb` and `propertyIdTestV2000.mdb` against the same objects in `testdata/V2007/controlPropertyTestV2007.accdb` and `propertyIdTestV2007.accdb`, made in Access for Microsoft 365: the values of an ID across the objects are those of one ID of the later format and of no other. Of the pairs whose names are known in both formats, all 307 agree. The IDs of the later format are those of Access for Microsoft 365, one higher above `0x01C4` than in files of earlier Access. A property without a pair keeps the ID of the 2000 format plus `0x8000`, such as `0x80DD`; most of them are properties a control keeps before its name in later formats, which jetdb does not read.

## Known Limitations

### Parser Limitations

- **Form level**: Sequential parsing from the beginning of the Blob, through the entry whose trailer ends the group. An entry of an unknown type is read by its length, and its value is given as bytes.
- **Control level**: Separate from form-level parsing, a byte scan detects the Name property pattern `[14 00 0A 00 00 00]` to locate each control section, which is then parsed individually. Each section is read as a group, as at the form level. The properties a control keeps before its Name (in the forms and reports compared with SaveAsText, such as `Left`, `Top`, `Width`, `Height` and most `NotDefault` values) are not read.
- **Layout coordinates & printer settings**: Stored in non-property regions within the Blob and are not extraction targets.
- **Jet3 (Access 97)**: Not supported because it uses a different format stored in MSysAccessObjects rather than MSysAccessStorage.

### prop_id Mapping

Known prop_id values are displayed by name; unknown ones are shown in `0xXXXX` numeric format. A name is given to values of one kind: text values by the first table below, and integer and Yes/No values by the second. An ID whose value is of another kind belongs to another property in that place and is shown by number. For example, 0x0068 is OnKeyDown on a form, but holds a color on a label, which has no key events. Where the data has them, the names were checked against the SaveAsText output of Access for the 116 forms and reports of `Sports.accdb` (ruddj/SportsAdmin): each value matched the property of that name, except that 12 forms and reports have a FontName of their own that their SaveAsText output does not have.

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
| 0x0161 | Toolbar | Toolbar |

Names of integer and Yes/No values: each is the SaveAsText key that the values of the ID agree with in every form and report where it appears, in `Sports.accdb` and in `testdata/V2007/propertyIdTestV2007.accdb`, which was made in Access with one property set at a time. An ID is named where its values vary, so that one key fits them all, or where setting the property in `propertyIdTestV2007.accdb` changes the value of the ID. Only IDs up to 0x01C4 are named: Access for Microsoft 365 writes the IDs above, such as those of the anchors and the layout, one higher than the Access that made `Sports.accdb`.

| ID | Property Name | Purpose |
|--------|--------------|------|
| 0x0009 | BorderStyle | Border style |
| 0x000C | Bottom | Bottom of the saved window of the form or report |
| 0x0029 | GridX | Horizontal grid spacing |
| 0x002A | GridY | Vertical grid spacing |
| 0x0036 | Left | Left of the saved window of the form or report |
| 0x006F | TimerInterval | Interval of the Timer event (milliseconds) |
| 0x0081 | Right | Right of the saved window of the form or report |
| 0x008D | Top | Top of the saved window of the form or report |
| 0x0092 | ViewsAllowed | Views allowed |
| 0x0093 | DefaultView | Default view |
| 0x0096 | Width | Width |
| 0x0098 | ScrollBars | Scroll bars shown |
| 0x009E | ItemSuffix | Number used for the next control name |
| 0x00A1 | DatasheetFontHeight | Datasheet font size |
| 0x00DB | HelpContextId | Help context ID |
| 0x0105 | TabIndex | Tab order |
| 0x0135 | PictureAlignment | Alignment of the picture |
| 0x013E | DatasheetGridlinesBehavior | Datasheet gridlines shown |
| 0x013F | DatasheetGridlinesColor | Datasheet gridline color |
| 0x01A6 | AllowDatasheetView | Allow Datasheet view (Yes/No) |
| 0x01C4 | FilterOnLoad | Apply the filter on load (Yes/No) |

Frequently occurring unknown IDs:

| ID | Occurrence Context (Estimated) |
|--------|------|
| 0x0000 | Frequently appears at the end of controls |
| 0x0013 | Form-level Yes/No, stored without a value (`NotDefault`) |
| 0x001D | Within controls |
| 0x0178 | GUID (appears in all controls) |
| 0x0268 | Color-related property |
| 0x0274 | Color-related property |
| 0x01CA-0x01D2 | Integer within controls (appears in consecutive sequence) |

### type Support Status

| type | Value | Status |
|------|-------|--------|
| 0x01 | Yes/No: no bytes for a property set to the value that is not its default (SaveAsText writes `NotDefault`), 1 byte for a value (0 is No), 2 or 4 bytes for an integer such as `Overlaps` | Supported |
| 0x02 | Integer of 1 byte, unsigned | Supported |
| 0x03 | Integer of 2 or 4 bytes, signed | Supported |
| 0x04 | 32-bit value of 4 bytes, often a color, given as the signed number SaveAsText writes: a color is `0x00BBGGRR`, a system color negative | Supported |
| 0x06 | Single of 4 bytes, such as `AlternateBackShade` | Supported |
| 0x08 | Double of 8 bytes | Supported |
| 0x09 | GUID of 16 bytes | Supported |
| 0x0A | Text (UTF-16LE) | Supported |
| 0x0B | Binary | Supported |
| 0x0C | Memo (UTF-16LE) | Supported |
| Others | (Unknown, such as 0x11) | Read by length, given as bytes |

A value of a length its type does not have is given as its bytes. The values of the 115 of the 116 forms and reports of Sports.accdb whose SaveAsText output its project publishes were compared with that output: of the integer and Yes/No values SaveAsText writes, all agree.

## References

- MDB Tools HACKING.md: https://github.com/mdbtools/mdbtools/blob/main/HACKING.md
- jabakobob.net Unofficial MDB Guide: http://jabakobob.net/mdb/
- isladogs.co.uk Long Value Binary fields: https://www.isladogs.co.uk/lv-fields/index.html
