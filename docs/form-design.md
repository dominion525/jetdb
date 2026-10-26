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
  <UTF-16LE string>     Form display name
  <u16 LE>              Storage number
  0x0000                Terminator
Trailing 4 bytes: 0x00
```

The storage number is the u16 (Little-Endian) before the closing 0x0000. jetdb turns it into a decimal string, such as `"5"`, and takes that as the name of the storage of the form.

Examples:
- Storage "0" → F_クライアント情報
- Storage "1" → F_クライアント担当一覧
- Storage "5" → F_メニュー

## TypeInfo Format

TypeInfo is a list of names and type codes for all controls within a form. It can be empty, as in forms and reports made by VBA, and can keep an old spelling of a name that differs in case from that of the Blob and of SaveAsText, such as `H_Code` for `H_code`. jetdb lists the controls from the Blob, and gives a control the type code of TypeInfo for the control of the same name, ignoring case.

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
  EventProcPrefix: bytes in a code page, NUL-terminated
    No parentheses → same as control name
    With parentheses → parentheses replaced with underscores (for VBA)
  RealName: bytes in a code page, NUL-terminated
    No parentheses → empty (NUL only)
    With parentheses → actual control name
```

The names are in the code page of the Access that saved the form, which the file does not keep. jetdb reads them in the code page, of that in the header of the database and then those of Windows (874, 932, 936, 949, 950 and 1250 to 1258), in which the most names that are not ASCII are among the names of the controls in the Blob, which are UTF-16, regardless of case; in the one tried first, that of the header being first, when more than one have as many; in that of the header when none is. The Russian names of `Strings.mdb` are in 1251, while its header has 1252.

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
- Leading bytes: `14 00 14 00 00 00 00 00` or `15 00 14 00 00 00 00 00`, the first byte being the `Version` of its SaveAsText output (`Version =20` or `Version =21`)
- No public specification for the binary format. The following is based on reverse engineering analysis.

### Blob Overall Layout

The Blob keeps the design as a tree, as the Blob of the Access 2000 format does (see [Blob of the Access 2000 Format](#blob-of-the-access-2000-format)), with property entries:

```
Blob     8 bytes of header + 6 bytes + the group of the form or report itself + nodes
Header   version(u16) = 0x0014 or 0x0015, flags(u16) = 0x0014, reserved(u32) = 0
Group    property entries, through the one whose trailer is 253, 254 or 255
Nodes    none after a trailer of 253, one after 254, and a count (u16) and that many after 255
Node     type (u16) + 4 bytes + a group + the nodes in it
```

### Property Entry Format

Property names are not stored as strings; they are identified by numeric IDs (u16). Every entry has the same layout:

```
prop_id(u16) + type(u32) + B(u32) + length(u32) + value[length bytes] + trailer(u32)
```

`length` is the number of bytes of the value. The 4-byte trailer after the value is a number that grows from one entry to the next, except for the last entry of a group, whose trailer is 253, 254 or 255 and tells the nodes after the group. The type tells how to read the value (see "type Support Status"); B is not used.

type values of text:
- 0x0A -- Name/font types (Name, FontName, Format, RowSourceType, ColumnWidths)
- 0x0C -- Text/data types (RecordSource, ControlSource, RowSource, Filter, Event, Caption)

### Nodes

- The nodes at the top of the tree are the default controls of the form or report and its sections; the nodes in a section are its controls, and the nodes in a tab control its pages. jetdb gives the sections and controls, each section followed by its controls, and not the default controls.
- The type of a node is the `acControlType` of Access, the low byte of the type codes of TypeInfo. In the test data, the type of each node agrees with that of TypeInfo for the control of the same name, except for the entries TypeInfo has after an ActiveX control, which jetdb does not read in step.
- A control keeps its properties before and after its Name (0x14), such as `Left`, `Top`, `Width` and `Height` before it.

## Blob of the Access 2000 Format

A form or report saved in the Access 2000 format keeps its design in the Blob too, but as a tree rather than property entries. Its Blob starts with the byte `0x13`, the `Version =19` of its SaveAsText output; the Blob of Access 2002 and later starts with `0x14` or `0x15`.

```
Blob     8 bytes of header + the form or report itself + a list
Object   type (u16, 0x96 a form, 0x97 a report) + properties
List     FF + count (u16) + entries, or FE + one entry
Entry    type (u16) + properties + (FD, or a list of the entries in it)
Property ID (1 byte) + value
```

- The entries at the top of the tree are the default controls of the form or report and its sections; the entries in a section are its controls, and the entries in a tab control its pages. jetdb gives the sections and controls, as for later formats, and not the default controls.
- The type of an entry is the `acControlType` of Access, the low byte of the type codes of TypeInfo: 100 a label, 109 a text box, 123 a tab control, 124 a page, 152 the detail section, 155 and 156 the page header and footer, and so on.
- The ID of a property is numbered in each type: `0xDC` is the name of a control and the record source of a form. The ID tells the size of the value: none for `0x00`–`0x2F` (a Yes/No set to the value that is not its default, given as `NotDefault`), 1 byte for `0x30`–`0x5F`, 2 bytes for `0x60`–`0x7F`, 4 bytes for `0x80`–`0xBC`, and a length byte and that many bytes for `0xBD`–`0xFC`, the length byte `0xFF` being followed by a length of 2 bytes. Of `0x80`–`0xBC`, only `0x9C`–`0xA0` have been seen, so where the 4-byte values end in `0xA1`–`0xBC` is not known.

jetdb gives a property the ID of Access 2002 and later from a table of each type (599 pairs), and the value as that ID holds it, so that the names of [prop_id Mapping](#prop_id-mapping) apply. Of them, 409 pairs were tied by the forms and reports of `testdata/V2000/controlPropertyTestV2000.mdb` and `propertyIdTestV2000.mdb` against the same objects in `testdata/V2007/controlPropertyTestV2007.accdb` and `propertyIdTestV2007.accdb`, made in Access for Microsoft 365: the values of an ID across the objects are those of one ID of the later format and of no other. Each of them was then checked against the SaveAsText output of the files of the 2000 format themselves (`testdata/V2000/saveastext`, written by Access for Microsoft 365 from those files with `Version =19`). All 409 pairs have values in that output, and in 404 of them the values are held only by the key named as the ID of the later format is. Of the other five, two are held by that key and also by other keys of the same values: AllowDesignChanges and DateGrouping of a report. The other three, `0x0172` of a form and a report and `0x011B` of a report, are IDs the names of [prop_id Mapping](#prop_id-mapping) do not cover, whose values several keys hold. Where the names and the values point to different IDs, the pair follows the names: ten pairs, such as the BorderColor of a rectangle, whose values in those files are always those of GridlineColor (`0x01CF`). Saving in the 2000 format, Access writes the value of DatasheetGridlinesColor12 (`0x0247`) as DatasheetGridlinesColor (`0x013F`), the one that format has. The IDs of the later format are those of Access for Microsoft 365, one higher above `0x01C4` than in files of earlier Access. The other 190 pairs were tied by that SaveAsText output alone: the values of an ID of a type are all held by one key, and by no other, and the ID of the later format is the one [prop_id Mapping](#prop_id-mapping) names after that key. In 18 of them, such as Right and Bottom, the window position of a form, and PrtDevMode, the values differ between the files of the two formats; their values are checked against that output. A property without a pair keeps the ID of the 2000 format plus `0x8000`, such as `0x8036` of a form, whose values Cycle, DatasheetCellsEffect, DatasheetGridlinesBehavior and PictureType all hold in that output.

## Known Limitations

### Parser Limitations

- **Tree**: The Blob is read as a tree from its beginning. An entry of an unknown type is read by its length, and its value is given as bytes. When the bytes do not read to the end as a tree, a byte scan finds each control by the Name property pattern `[14 00 0A 00 00 00]`, and only the properties from its Name on are read. All the Blobs of the test data read to the end as a tree.
- **Layout and printer settings**: The position and size of a section or control (`Left`, `Top`, `Width`, `Height`) are properties of the Blob and are read. The printer settings (`PrtMip`, `PrtDevMode`, `PrtDevNames`) are given as their bytes, such as `(5364 bytes)`, without reading what they hold.
- **Jet3 (Access 97)**: Not supported because it uses a different format stored in MSysAccessObjects rather than MSysAccessStorage.

### prop_id Mapping

Known prop_id values are displayed by name; unknown ones are shown in `0xXXXX` numeric format. A name is given to values of one kind: text values by the first table below, number and Yes/No values, with a Yes/No stored without a value (`NotDefault`), by the second, and bytes and GUIDs by the third. An ID whose value is of another kind belongs to another property in that place and is shown by number. For example, 0x0068 is OnKeyDown on a form, but holds a color on a label, which has no key events.

Each name is the SaveAsText key, and the only one, that holds every value of the ID in the forms and reports of `Sports.accdb` (ruddj/SportsAdmin), of `testdata/V2007/controlPropertyTestV2007.accdb` and `propertyIdTestV2007.accdb`, and of their Access 2000 versions in `testdata/V2000/`; a Yes/No without a value is held by a key that SaveAsText writes as `NotDefault` in each block that has it. Only IDs up to 0x01C4 are named: Access for Microsoft 365 writes the IDs above, such as those of the anchors and the layout, one higher than the Access that made `Sports.accdb`.

The purposes are the first sentence of the page of each property in Microsoft's Office VBA reference for Access ([MicrosoftDocs/VBA-Docs](https://github.com/MicrosoftDocs/VBA-Docs), CC BY 4.0), from the page of a form, a report or a control. A key that the reference does not describe is marked so.

Names of text values:

| ID | Property Name | Purpose |
|--------|--------------|------|
| 0x0007 | Picture | Use the Picture property to specify a bitmap or other type of graphic to be used as a background picture on a form. |
| 0x0011 | Caption | Gets or sets the text that appears in the title bar in Form view. |
| 0x0012 | ColumnWidths | Use the ColumnWidths property to specify the width of each column in a multiple-column combo box. |
| 0x0014 | Name | Use the Name property to specify or determine the string expression that identifies the name of an object. |
| 0x0016 | EventProcPrefix | Gets or sets the prefix portion of an event procedure name. |
| 0x0017 | DefaultValue | Specifies a value that is automatically entered in a field when a new record is created. For example, in an Addresses table, you can set the default value for the City field to New York. When users add a record to the table, they can either accept this value or enter the name of a different city. |
| 0x001B | ControlSource | Use the ControlSource property to specify what data appears in a control. You can display and edit data bound to a field in a table, query, or SQL statement. You can also display the result of an expression. |
| 0x0022 | FontName | Use the FontName property to specify the font for text in the following situations: |
| 0x0026 | Format | Use the Format property to customize the way numbers, dates, times, and text are displayed and printed. |
| 0x0031 | LinkChildFields | Use the LinkChildFields property (along with the LinkMasterFields property) to specify how Microsoft Access links records in a form or report to records in a subform, subreport, or embedded object, such as a chart. If these properties are set, Access automatically updates the related record in the subform when you change to a new record in a main form. |
| 0x0032 | LinkMasterFields | Use the LinkMasterFields property (along with the LinkChildFields property) to specify how Microsoft Access links records in a form or report to records in a subform, subreport, or embedded object, such as a chart. If these properties are set, Access automatically updates the related record in the subform when you change to a new record in a main form. |
| 0x003D | ValidationText | Use the ValidationText property to specify a message to be displayed to the user when data is entered that violates a ValidationRule setting for a record, field, or control. |
| 0x0048 | InputMask | Use the InputMask property to make data entry easier and to control the values that users can enter in a text box control. |
| 0x004C | OLEClass | Use the OLEClass property to obtain a description of the kind of OLE object contained in a chart control or an unbound object frame. |
| 0x004D | OnOpen | Sets or returns the value of the On Open box in the Properties window of a form or report. |
| 0x004E | OnClose | Sets or returns the value of the On Close box in the Properties window of a form or report. |
| 0x004F | OnDelete | Sets or returns the value of the On Delete box in the Properties window of a form. |
| 0x0050 | OnFormat | Sets or returns the value of the On Format box in the Properties window of a report section. |
| 0x0052 | OnPrint | Sets or returns the value of the On Print box in the Properties window of a report section. |
| 0x0053 | OnPage | Sets or returns the value of the On Page box in the Properties window of a report. |
| 0x0054 | OnRetreat | Sets or returns the value of the On Retreat box in the Properties window of a report section. |
| 0x0055 | BeforeUpdate | The BeforeUpdate event occurs before changed data in a control or record is updated. |
| 0x0056 | AfterUpdate | The AfterUpdate event occurs after changed data in a control or record is updated. |
| 0x0057 | OnCurrent | Sets or returns the value of the On Current box in the Properties window of a form. |
| 0x005B | RowSource | Use the RowSource property (along with the RowSourceType property) to tell Microsoft Access how to provide data to the specified object. |
| 0x005D | RowSourceType | Use the RowSourceType property (along with the RowSource property) to tell Microsoft Access how to provide data to the specified object. |
| 0x0068 | OnKeyDown | Sets or returns the value of the On Key Down box in the Properties window. |
| 0x0069 | OnKeyUp | Sets or returns the value of the On Key Up box in the Properties window. |
| 0x006A | OnKeyPress | Sets or returns the value of the On Key Press box in the Properties window. |
| 0x006B | OnMouseDown | Sets or returns the value of the On Mouse Down box in the Properties window. |
| 0x006C | OnMouseUp | Sets or returns the value of the On Mouse Up box in the Properties window. |
| 0x006D | OnMouseMove | Sets or returns the value of the On Mouse Move box in the Properties window. |
| 0x006E | OnTimer | Sets or returns the value of the On Timer box in the Properties window of a form. |
| 0x0070 | OnActivate | Sets or returns the value of the On Activate box in the Properties window of a form or report. |
| 0x0071 | OnDeactivate | Sets or returns the value of the On Deactivate box in the Properties window of a form or report. |
| 0x0072 | OnChange | Sets or returns the value of the On Change box in the Properties window of one of the objects in the Applies To list. |
| 0x0073 | OnGotFocus | Sets or returns the value of the On Got Focus box in the Properties window of the specified object. |
| 0x0074 | OnLostFocus | Sets or returns the value of the On Lost Focus box in the Properties window of the specified object. |
| 0x0075 | OnResize | Sets or returns the value of the On Resize box in the Properties window of a form. |
| 0x0076 | OnUpdated | Sets or returns the value of the On Updated box in the Properties window of a form or report. |
| 0x0077 | OnError | Sets or returns the value of the On Error box in the Properties window of a form or report. |
| 0x0078 | OnNotInList | Sets or returns the value of the On Not in List box in the Properties window of a combo box. |
| 0x0079 | OnNoData | Sets or returns the value of the On No Data box in the Properties window of a report. |
| 0x007A | BeforeInsert | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the BeforeInsert event occurs. |
| 0x007B | AfterInsert | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the AfterInsert event occurs. |
| 0x007C | BeforeDelConfirm | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the BeforeDelConfirm event occurs. |
| 0x007D | AfterDelConfirm | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the AfterDelConfirm event occurs. |
| 0x007E | OnClick | Sets or returns the value of the On Click box in the Properties window. |
| 0x007F | OnLoad | Sets or returns the value of the On Load box in the Properties window of a form. |
| 0x0080 | OnUnload | Sets or returns the value of the On Unload box in the Properties window of a form. |
| 0x0084 | SourceObject | Use the SourceObject property to identify the form or report that is the source of the subform or subreport on a form or report. |
| 0x0087 | StatusBarText | Use the StatusBarText property to specify the text that is displayed in the status bar when a control is selected. |
| 0x0091 | ValidationRule | Use the ValidationRule property to specify requirements for data entered into a record, field, or control. When data is entered that violates the ValidationRule setting, you can use the ValidationText property to specify the message to be displayed to the user. |
| 0x009C | RecordSource | Use the RecordSource property to specify the source of the data for a form. |
| 0x00A0 | DatasheetFontName | Use the DatasheetFontName property to specify the font used to display and print field names and data in Datasheet view. |
| 0x00DA | HelpFile | The name of a help file associated with a form. |
| 0x00DE | OnEnter | Sets or returns the value of the On Enter box in the Properties window of specified object. |
| 0x00DF | OnExit | Sets or returns the value of the On Exit box in the Properties window of specified object. |
| 0x00E0 | OnDblClick | Sets or returns the value of the On Dbl Click box in the Properties window. |
| 0x00F0 | MenuBar | Specifies a custom menu to display for a form. |
| 0x00F5 | Filter | Use the Filter property to specify a subset of records to be displayed when a filter is applied to a form, report, query, or table. |
| 0x010A | Tag | Stores extra information about a form, report, section, or control needed by a Microsoft Access application. |
| 0x0112 | Class | Use the Class property to specify or determine the class name of an embedded OLE object. |
| 0x0130 | ShortcutMenuBar | Use the ShortcutMenuBar property to specify the shortcut menu that appears when you right-click the specified object. |
| 0x013D | ControlTipText | Use the ControlTipText property to specify the text that appears in a ScreenTip when you hold the mouse pointer over a control. |
| 0x014B | OrderBy | Use the OrderBy property to specify how you want to sort records in a form. |
| 0x014D | OnFilter | Sets or returns the value of the On Filter box in the Properties window of a form. |
| 0x014E | OnApplyFilter | Sets or returns the value of the On Apply Filter box in the Properties window of a form. |
| 0x0157 | HyperlinkAddress | Use the HyperlinkAddress property to specify or determine the path to an object, document, webpage, or other destination for a hyperlink associated with the specified control. |
| 0x015A | ColumnInfo | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x015B | HyperlinkSubAddress | Use the HyperlinkSubAddress property to specify or determine a location within the target document specified by the HyperlinkAddress property. |
| 0x0161 | Toolbar | Specifies a custom toolbar to display for a form. |
| 0x0167 | FuriganaControl | Microsoft's VBA reference has no summary of it. The setting value is the name of the control for entering furigana. |
| 0x0168 | PostalAddress | Use the PostalAddress property to specify or determine the postal code and the Customer Barcode data corresponding to the address information displayed in a specified field or text box. The PostalAddress Property Wizard enables the setting of these properties. |
| 0x0177 | OnDirty | Sets or returns the value of the On Dirty box in the Properties window of a form or report. |
| 0x0194 | OnUndo | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the Undo event occurs. |
| 0x01A9 | OnConnect | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the OnConnect event occurs. |
| 0x01AA | OnDisconnect | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the OnDisconnect event occurs. |
| 0x01AB | OnPivotTableChange | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01AC | OnQuery | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01AD | BeforeQuery | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the BeforeQuery event occurs. |
| 0x01AE | OnSelectionChange | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01AF | OnCmdBeforeExecute | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01B0 | OnCmdChecked | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01B1 | OnCmdEnabled | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01B2 | OnCmdExecute | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01B3 | OnDataSetChange | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01B4 | BeforeScreenTip | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the BeforeScreenTip event occurs. |
| 0x01B5 | AfterFinalRender | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the AfterFinalRender event occurs. |
| 0x01B6 | AfterRender | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the AfterRender event occurs. |
| 0x01B7 | AfterLayout | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the AfterLayout event occurs. |
| 0x01B8 | BeforeRender | Returns or sets a String indicating which macro, event procedure, or user-defined function runs when the BeforeRender event occurs. |
| 0x01B9 | OnMouseWheel | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01BA | OnViewChange | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01BB | OnDataChange | Not in Microsoft's VBA reference; a key of SaveAsText. |

Names of number and Yes/No values:

| ID | Property Name | Purpose |
|--------|--------------|------|
| 0x0002 | AccessKey | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x0004 | SpecialEffect | Use the SpecialEffect property to specify whether special formatting will apply to the specified object. |
| 0x0006 | AutoResize | Returns or sets a Boolean indicating whether a Form window opens automatically sized to display complete records. |
| 0x0008 | BorderColor | Use the BorderColor property to specify the color of a control's border. |
| 0x0009 | BorderStyle | Specifies the type of border and border elements (title bar, Control menu, Minimize and Maximize buttons, or Close button) to use for the form. You typically use different border styles for normal forms, pop-up forms, and custom dialog boxes. |
| 0x000A | BorderWidth | Use the BorderWidth property to specify the width of a control's border. |
| 0x000B | BorderLineStyle | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x000C | Bottom | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x000D | BoundColumn | When you make a selection from a combo box, the BoundColumn property tells Microsoft Access which column's values to use as the value of the control. If the control is bound to a field, the value in the column specified by the BoundColumn property is stored in the field named in the ControlSource property. |
| 0x000E | CanGrow | Gets or sets whether the specified control automatically adjusts vertically to print or preview all the data that the control contains. |
| 0x000F | RepeatSection | Use the RepeatSection property to specify whether a group header is repeated on the next page or column when a group spans more than one page or column. |
| 0x0010 | CanShrink | Gets or sets whether the specified control automatically adjusts vertically to print or preview all the data that the section or control contains. |
| 0x0013 | RecordSelectors | Use the RecordSelectors property to specify whether a form displays record selectors in Form view. |
| 0x0019 | Enabled | Microsoft's VBA reference has no summary of it. The first sentence of its pages describes the FormatCondition object instead. |
| 0x001C | BackColor | Gets or sets the interior color of the specified object. |
| 0x001D | BackStyle | Use the BackStyle property to specify whether a control will be transparent. |
| 0x001E | AllowFilters | Use the AllowFilters property to specify whether records in a form can be filtered. |
| 0x0021 | FontItalic | Use the FontItalic property to specify whether text is italic in the following situations: |
| 0x0023 | FontSize | Use the FontSize property to specify the point size for text in the following situations: |
| 0x0024 | FontUnderline | Use the FontUnderline property to specify whether text is underlined in the following situations: |
| 0x0025 | FontWeight | Use the FontWeight property to specify the line width that Windows uses to display and print characters in a control. |
| 0x0029 | GridX | Use the GridX property (along with the GridY property) to specify the horizontal and vertical divisions of the alignment grid in form Design view. |
| 0x002A | GridY | Use the GridY property (along with the GridX property) to specify the horizontal and vertical divisions of the alignment grid in form Design view. |
| 0x002C | Height | Gets or sets the height of the specified object in twips. |
| 0x002D | AllowUpdating | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x002E | Transparent | Use the Transparent property to specify whether a command button is solid or transparent. |
| 0x0036 | Left | Use the Left property to specify an object's location on a form or report. |
| 0x0037 | LineSlant | You use the LineSlant property to specify whether a line control slants from upper-left to lower-right or from upper-right to lower-left. |
| 0x0038 | Locked | The Locked property specifies whether you can edit data in a control in Form view. |
| 0x003A | OptionValue | Each control in an option group has a numeric value that you can set with the OptionValue property. |
| 0x003E | Modal | Use the Modal property to specify whether a form opens as a modal window. When a form opens as a modal window, you must close the window before you can move the focus to another object. |
| 0x0043 | LimitToList | Use the LimitToList property to limit a combo box's values to the listed items. |
| 0x0044 | AutoExpand | Use the AutoExpand property to specify whether Microsoft Access automatically fills the text box portion of a combo box with a value from the combo box list that matches the characters that you enter as you type in the combo box. This lets you quickly enter an existing value in a combo box without displaying the list box portion of the combo box. |
| 0x0045 | HideDuplicates | Use the HideDuplicates property to hide a control on a report when its value is the same as in the preceding record. |
| 0x0046 | ColumnCount | Use the ColumnCount property to specify the number of columns displayed in a list box or in the list box portion of a combo box, or sent to OLE objects in a chart control or unbound object frame. |
| 0x0047 | DecimalPlaces | Use the DecimalPlaces property to specify the number of decimal places that Microsoft Access uses to display numbers. |
| 0x004B | KeepTogether | Use the KeepTogether property for a section to print a form or report section all on one page. For example, you might have a group of related information that you don't want printed across two pages. The KeepTogether property applies only to form and report sections (except page headers and page footers). |
| 0x0059 | SizeMode | Use the SizeMode property to specify how to size a picture or other object in a bound object frame, an unbound object frame, or an image control. |
| 0x005A | PopUp | Specifies whether a form opens as a pop-up window. |
| 0x005C | RowSourceTypeInt | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x005E | MaxButton | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x005F | MinButton | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x0061 | ControlBox | Specifies whether a form has a Control menu in Form view and Datasheet view. |
| 0x0065 | AutoActivate | Use the AutoActivate property to specify how the user can activate an OLE object. |
| 0x006F | TimerInterval | Use the TimerInterval property to specify the interval, in milliseconds, between Timer events on a form. |
| 0x0081 | Right | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x0082 | RunningSum | Use the RunningSum property to calculate record-by-record or group-by-group totals in a report. |
| 0x0083 | ColumnHeads | Use the ColumnHeads property to display a single row of column headings for list boxes, combo boxes, and OLE objects that accept column headings. You can also use this property to create a label for each entry in a chart control. What is actually displayed as the first-row column heading depends on the object's RowSourceType property setting. |
| 0x0088 | TextAlign | The TextAlign property specifies the text alignment in new controls. |
| 0x008D | Top | Use the Top property to specify an object's location on a form or report. |
| 0x008F | LayoutForPrint | Use the LayoutForPrint property to specify whether the form uses printer or screen fonts. |
| 0x0090 | FastLaserPrinting | Use the FastLaserPrinting property to specify whether lines and rectangles are replaced by text character lines, similar to the underscore ( _ ) and vertical bar ( \| ) characters, when you print a form by using most laser printers. Replacing lines and rectangles with text character lines can make printing much faster. |
| 0x0092 | ViewsAllowed | Use the ViewsAllowed property to specify whether users can switch between Datasheet view and Form view by choosing the Form view or Datasheet view command on the View menu, or by choosing the arrow next to the View button and choosing Form view or Datasheet view. |
| 0x0093 | DefaultView | Use the DefaultView property to specify the opening view of a form. |
| 0x0094 | Visible | Returns or sets whether the object is visible. |
| 0x0095 | DisplayWhen | Use the DisplayWhen property to specify which of a form's controls you want displayed on screen and in print. |
| 0x0096 | Width | Gets or sets the width of the specified object in twips. |
| 0x0097 | ForceNewPage | Use the ForceNewPage property to specify whether form sections (detail, footer) or report sections (header, detail, footer) print on a separate page rather than on the current page. |
| 0x0098 | ScrollBars | Use the ScrollBars property to specify whether scroll bars appear on a form. |
| 0x009A | ListWidth | Use the ListWidth property to set the width of the list box portion of a combo box. |
| 0x009D | BreakLevel | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x009E | ItemSuffix | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x009F | OverlapFlags | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x00A1 | DatasheetFontHeight | Use the DatasheetFontHeight property to specify the font point size used to display and print field names and data in Datasheet view. |
| 0x00A2 | DatasheetFontWeight | Use the DatasheetFontWeight property to specify the line width of the font used to display and print characters for field names and data in Datasheet view. |
| 0x00A5 | ColumnOrder | Use the ColumnOrder property to specify the order of the columns in Datasheet view. |
| 0x00A6 | ColumnWidth | Use the ColumnWidth property to specify the width of a column in Datasheet view. |
| 0x00A7 | ColumnHidden | Use the ColumnHidden property to show or hide a specified column in Datasheet view. |
| 0x00A8 | RowHeight | Use the RowHeight property to specify the height of all rows in Datasheet view. |
| 0x00C2 | TabularCharSet | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x00CC | ForeColor | Use the ForeColor property to specify the color for text in a control. |
| 0x00DB | HelpContextId | The HelpContextID property specifies the context ID of a topic in the custom Help file specified by the HelpFile property setting. |
| 0x00DC | Default | Use the Default property to specify whether a command button is the default button on a form. |
| 0x00DD | Cancel | Use the Cancel property to specify whether a command button is also the Cancel button on a form. |
| 0x00F3 | TextFontFamily | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x00F4 | TabularFamily | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x0105 | TabIndex | Use the TabIndex property to specify a control's place in the tab order on a form or report. |
| 0x0106 | TabStop | Use the TabStop property to specify whether you can use the Tab key to move the focus to a control. |
| 0x0116 | AutoCenter | Returns or sets a Boolean indicating whether a form will be centered automatically in the application window when the form is opened. |
| 0x0117 | NavigationButtons | Use the NavigationButtons property to specify whether navigation buttons and a record number box are displayed on a form. |
| 0x011A | DateGrouping | Use the DateGrouping property to specify how you want to group dates in a report. |
| 0x0126 | TripleState | Use the TripleState property to specify how the specified control displays Null values. |
| 0x012F | CloseButton | Specifies whether the Close button on a form is enabled. |
| 0x0132 | DatasheetForeColor | Use the DatasheetForeColor property in Visual Basic to specify or determine the color of all text in a table, query, or form in Datasheet view within an Access database. |
| 0x0135 | PictureAlignment | Use the PictureAlignment property to specify where a background picture will appear in an image control or on a form or report. Read/write Byte. |
| 0x0139 | PictureTiling | Use the PictureTiling property to specify whether a background picture is tiled across the entire image control, Form window, form, or page of a report. |
| 0x013A | DividingLines | Use the DividingLines property to specify whether dividing lines will separate sections on a form or records displayed on a continuous form. |
| 0x013E | DatasheetGridlinesBehavior | Use the DatasheetGridlinesBehavior property to specify which gridlines will appear in Datasheet view. |
| 0x013F | DatasheetGridlinesColor | Use the DatasheetGridlinesColor property to specify the color of gridlines in a datasheet. |
| 0x0146 | AllowAdditions | Use the AllowAdditions property to specify whether a user can add a record when using a form. |
| 0x0149 | OldBorderStyle | Use this property to set or return the unedited value of the BorderStyle property for a form or control. This property is useful if you need to revert to an unedited or preferred border style. |
| 0x0150 | OrderByOn | Use the OrderByOn property to specify whether an object's OrderBy property setting is applied. |
| 0x0154 | AllowAutoCorrect | Use the AllowAutoCorrect property to specify whether the specified control will automatically correct entries made by the user. |
| 0x0155 | PictureType | Use the PictureType property to specify whether Microsoft Access stores an object's picture as a linked or an embedded object. |
| 0x015C | MultiRow | Use the MultiRow property to specify or determine whether a tab control can display more than one row of tabs. |
| 0x015E | TabFixedHeight | Use the TabFixedHeight property to specify or determine the height of the tabs on a tab control. |
| 0x015F | TabFixedWidth | Use the TabFixedWidth property to specify or determine the width of the tabs on a tab control. |
| 0x0163 | Vertical | Use the Vertical property to set a form control for vertical display and editing, or to set a report control for vertical display and printing. |
| 0x0165 | IMEHold | Use the IMEHold/Hold KanjiConversionMode property to show whether the Kanji Conversion Mode is maintained when the control loses the focus. |
| 0x016C | RecSrcDt | Not in Microsoft's VBA reference; a key of SaveAsText. An 8-byte date and time, in a binary block of SaveAsText. |
| 0x0170 | KeyboardLanguage | Microsoft's VBA reference has no summary of it. |
| 0x0175 | AllowDesignChanges | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x017B | IMESentenceMode | Microsoft's VBA reference has no summary of it. |
| 0x0180 | LeftMargin | Along with the TopMargin, RightMargin, and BottomMargin properties, specifies the location of information displayed within a text box control. |
| 0x0181 | TopMargin | Along with the LeftMargin, RightMargin, and BottomMargin properties, specifies the location of information displayed within a text box control. |
| 0x0182 | LineSpacing | Use the LineSpacing property to specify or determine the location of information displayed within a label or text box control. |
| 0x0183 | SubdatasheetHeight | Use the SubdatasheetHeight property to specify or determine the default display height of a subdatasheet when expanded. |
| 0x0184 | RightMargin | Along with the TopMargin, LeftMargin, and BottomMargin properties, specifies the location of information displayed within a text box control. |
| 0x0185 | BottomMargin | Along with the LeftMargin, RightMargin, and TopMargin properties, specifies the location of information displayed within a text box control. |
| 0x018C | IsHyperlink | Use the IsHyperlink property to specify or determine if the data contained in a text box is a hyperlink. |
| 0x01A6 | AllowDatasheetView | Returns or sets a Boolean indicating whether the specified form may be viewed in Datasheet view. True if Datasheet view is allowed. |
| 0x01BE | AsianLineBreak | Returns or sets a Boolean indicating whether line breaks in text boxes follow rules governing East Asian languages. True to control line breaks based on East Asian language rules. |
| 0x01C2 | GroupTable | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x01C4 | FilterOnLoad | Gets or sets whether the filter specified by the Filter property is applied when the form is loaded. |

Names of bytes and GUIDs:

| ID | Property Name | Purpose |
|--------|--------------|------|
| 0x0063 | ObjectPalette | The ObjectPalette property specifies the palette in the application used to create a bitmap or other graphic that is loaded into the specified control by using the Picture property. |
| 0x0064 | PaintPalette | Use the PaintPalette property to specify a palette to be used by a form. |
| 0x00AB | OleData | Not in Microsoft's VBA reference; a key of SaveAsText. |
| 0x00BC | PictureData | Use the PictureData property to copy the picture to another object that supports the Picture property. |
| 0x00BD | PrtMip | Use the PrtMip property in Visual Basic to set or return the device mode information specified for a form or report in the Print dialog box. |
| 0x00BE | PrtDevMode | Use the PrtDevMode property to set or return printing device mode information specified for a form or report in the Print dialog box. |
| 0x00BF | PrtDevNames | Use the PrtDevNames property to set or return information about the printer selected in the Print dialog box for a form or report. |
| 0x0178 | GUID | Not in Microsoft's VBA reference; a key of SaveAsText. A GUID of 16 bytes, in a binary block of SaveAsText. |
| 0x0179 | NameMap | Not in Microsoft's VBA reference; a key of SaveAsText. |

Frequently occurring unknown IDs:

| ID | Occurrence Context (Estimated) |
|--------|------|
| 0x0000 | Frequently appears at the end of controls |
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
