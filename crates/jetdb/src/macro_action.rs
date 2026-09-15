//! Macro action numbers.
//!
//! A macro grid row stores its action as a number and its arguments by
//! position (see [`MacroGridRow`](crate::MacroGridRow)). [`macro_action`]
//! gives the action's name and argument names for a number.
//!
//! The numbers were read from macros that Access for Microsoft 365 created
//! from `Action ="..."` rows of the text format `Application.LoadFromText`
//! accepts, one macro per action name. Actions renamed in Access 2010 are
//! accepted only under their earlier name, which is also the name
//! `Application.SaveAsText` writes, and is kept here as `text_name`.
//! The number of arguments of each action is the largest number of
//! `Argument` rows Access accepted for it. Argument names follow the
//! argument tables of the macro action reference, written without spaces as
//! in macro XML (`Macro Name` is `MacroName`). `DoMenuItem`, which Access for
//! Microsoft 365 no longer accepts, is from macros converted from Access 97.
//!
//! Actions that only data macros use (for example `SetField`) have no number,
//! since data macros are stored only as XML.

/// A macro action that has a number in the macro grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroAction {
    /// The number stored in a macro grid row.
    pub code: u16,
    /// The action name used in macro XML and the macro designer.
    pub name: &'static str,
    /// The action name `Application.SaveAsText` writes in `Action ="..."`.
    pub text_name: &'static str,
    /// The argument names in grid slot order.
    pub arguments: &'static [&'static str],
    /// The grid slots `Application.SaveAsText` writes as `Argument` lines, in
    /// order, for an action whose text arguments are not the grid slots in
    /// order; `None` for every other action.
    pub text_slots: Option<&'static [usize]>,
}

/// Returns the action with the given macro grid number.
pub fn macro_action(code: u16) -> Option<&'static MacroAction> {
    MACRO_ACTIONS.iter().find(|a| a.code == code)
}

const fn action(
    code: u16,
    name: &'static str,
    text_name: &'static str,
    arguments: &'static [&'static str],
) -> MacroAction {
    MacroAction {
        code,
        name,
        text_name,
        arguments,
        text_slots: None,
    }
}

/// Every numbered action, ordered by number.
pub static MACRO_ACTIONS: &[MacroAction] = &[
    action(
        1,
        "AddMenu",
        "AddMenu",
        &["MenuName", "MenuMacroName", "StatusBarText"],
    ),
    action(
        3,
        "ApplyFilter",
        "ApplyFilter",
        &["FilterName", "WhereCondition", "ControlName"],
    ),
    action(4, "Beep", "Beep", &[]),
    action(5, "CancelEvent", "CancelEvent", &[]),
    action(
        6,
        "CloseWindow",
        "Close",
        &["ObjectType", "ObjectName", "Save"],
    ),
    action(
        7,
        "CopyObject",
        "CopyObject",
        &[
            "DestinationDatabase",
            "NewName",
            "SourceObjectType",
            "SourceObjectName",
        ],
    ),
    // An Access 97 action that later versions no longer accept. Its number and
    // text are from macros converted from Access 97 and their SaveAsText output:
    // the grid holds seven slots, and the text writes slot 6 and then slots 0
    // to 3. The argument names are not known.
    MacroAction {
        code: 8,
        name: "DoMenuItem",
        text_name: "DoMenuItem",
        arguments: &[],
        text_slots: Some(&[6, 0, 1, 2, 3]),
    },
    action(9, "Echo", "Echo", &["EchoOn", "StatusBarText"]),
    action(11, "FindNextRecord", "FindNext", &[]),
    action(
        12,
        "FindRecord",
        "FindRecord",
        &[
            "FindWhat",
            "Match",
            "MatchCase",
            "Search",
            "SearchAsFormatted",
            "OnlyCurrentField",
            "FindFirst",
        ],
    ),
    action(13, "GoToControl", "GoToControl", &["ControlName"]),
    action(14, "GoToPage", "GoToPage", &["PageNumber", "Right", "Down"]),
    action(
        15,
        "GoToRecord",
        "GoToRecord",
        &["ObjectType", "ObjectName", "Record", "Offset"],
    ),
    action(17, "DisplayHourglassPointer", "Hourglass", &["HourglassOn"]),
    action(19, "MaximizeWindow", "Maximize", &[]),
    action(20, "MinimizeWindow", "Minimize", &[]),
    action(
        21,
        "MoveAndSizeWindow",
        "MoveSize",
        &["Right", "Down", "Width", "Height"],
    ),
    action(
        22,
        "MessageBox",
        "MsgBox",
        &["Message", "Beep", "Type", "Title"],
    ),
    action(
        23,
        "OpenForm",
        "OpenForm",
        &[
            "FormName",
            "View",
            "FilterName",
            "WhereCondition",
            "DataMode",
            "WindowMode",
        ],
    ),
    action(
        24,
        "OpenQuery",
        "OpenQuery",
        &["QueryName", "View", "DataMode"],
    ),
    action(
        25,
        "OpenTable",
        "OpenTable",
        &["TableName", "View", "DataMode"],
    ),
    action(
        26,
        "PrintOut",
        "PrintOut",
        &[
            "PrintRange",
            "PageFrom",
            "PageTo",
            "PrintQuality",
            "Copies",
            "CollateCopies",
        ],
    ),
    action(27, "QuitAccess", "Quit", &["Options"]),
    action(28, "Requery", "Requery", &["ControlName"]),
    action(
        29,
        "RepaintObject",
        "RepaintObject",
        &["ObjectType", "ObjectName"],
    ),
    action(
        30,
        "RenameObject",
        "Rename",
        &["NewName", "ObjectType", "OldName"],
    ),
    action(31, "RestoreWindow", "Restore", &[]),
    action(32, "RunApplication", "RunApp", &["CommandLine"]),
    action(33, "RunCode", "RunCode", &["FunctionName"]),
    action(
        34,
        "RunMacro",
        "RunMacro",
        &["MacroName", "RepeatCount", "RepeatExpression"],
    ),
    action(35, "RunSQL", "RunSQL", &["SQLStatement", "UseTransaction"]),
    action(
        37,
        "SelectObject",
        "SelectObject",
        &["ObjectType", "ObjectName", "InNavigationPane"],
    ),
    action(38, "SendKeys", "SendKeys", &["Keystrokes", "Wait"]),
    action(40, "SetValue", "SetValue", &["Item", "Expression"]),
    action(41, "SetWarnings", "SetWarnings", &["WarningsOn"]),
    action(43, "ShowAllRecords", "ShowAllRecords", &[]),
    action(44, "StopAllMacros", "StopAllMacros", &[]),
    action(45, "StopMacro", "StopMacro", &[]),
    action(
        46,
        "OpenReport",
        "OpenReport",
        &[
            "ReportName",
            "View",
            "FilterName",
            "WhereCondition",
            "WindowMode",
        ],
    ),
    action(
        47,
        "ImportExportData",
        "TransferDatabase",
        &[
            "TransferType",
            "DatabaseType",
            "DatabaseName",
            "ObjectType",
            "Source",
            "Destination",
            "StructureOnly",
        ],
    ),
    action(
        48,
        "ImportExportSpreadsheet",
        "TransferSpreadsheet",
        &[
            "TransferType",
            "SpreadsheetType",
            "TableName",
            "FileName",
            "HasFieldNames",
            "Range",
        ],
    ),
    action(
        49,
        "ImportExportText",
        "TransferText",
        &[
            "TransferType",
            "SpecificationName",
            "TableName",
            "FileName",
            "HasFieldNames",
            "HTMLTableName",
            "CodePage",
        ],
    ),
    action(
        51,
        "ExportWithFormatting",
        "OutputTo",
        &[
            "ObjectType",
            "ObjectName",
            "OutputFormat",
            "OutputFile",
            "AutoStart",
            "TemplateFile",
            "Encoding",
            "OutputQuality",
        ],
    ),
    action(
        52,
        "DeleteObject",
        "DeleteObject",
        &["ObjectType", "ObjectName"],
    ),
    action(
        53,
        "OpenVisualBasicModule",
        "OpenModule",
        &["ModuleName", "ProcedureName"],
    ),
    action(
        54,
        "EMailDatabaseObject",
        "SendObject",
        &[
            "ObjectType",
            "ObjectName",
            "OutputFormat",
            "To",
            "Cc",
            "Bcc",
            "Subject",
            "MessageText",
            "EditMessage",
            "TemplateFile",
        ],
    ),
    action(55, "ShowToolbar", "ShowToolbar", &["ToolbarName", "Show"]),
    action(56, "SaveObject", "Save", &["ObjectType", "ObjectName"]),
    action(
        57,
        "SetMenuItem",
        "SetMenuItem",
        &["MenuIndex", "CommandIndex", "SubcommandIndex", "Flag"],
    ),
    action(58, "RunMenuCommand", "RunCommand", &["Command"]),
    action(
        59,
        "OpenDataAccessPage",
        "OpenDataAccessPage",
        &["DataAccessPageName", "View"],
    ),
    action(
        60,
        "OpenView",
        "OpenView",
        &["ViewName", "View", "DataMode"],
    ),
    action(61, "OpenDiagram", "OpenDiagram", &["DiagramName"]),
    action(
        62,
        "OpenStoredProcedure",
        "OpenStoredProcedure",
        &["ProcedureName", "View", "DataMode"],
    ),
    action(
        63,
        "TransferSQLDatabase",
        "TransferSQLDatabase",
        &[
            "Server",
            "Database",
            "UseTrustedConnection",
            "Login",
            "Password",
            "TransferCopyData",
        ],
    ),
    action(
        64,
        "CopyDatabaseFile",
        "CopyDatabaseFile",
        &[
            "DatabaseFileName",
            "OverwriteExistingFile",
            "DisconnectAllUsers",
        ],
    ),
    action(
        65,
        "OpenFunction",
        "OpenFunction",
        &["FunctionName", "View", "DataMode"],
    ),
    action(67, "CloseDatabase", "CloseDatabase", &[]),
    action(68, "NavigateTo", "NavigateTo", &["Category", "Group"]),
    action(
        69,
        "SearchForRecord",
        "SearchForRecord",
        &["ObjectType", "ObjectName", "Record", "WhereCondition"],
    ),
    action(
        70,
        "SetProperty",
        "SetProperty",
        &["ControlName", "Property", "Value"],
    ),
    action(71, "SingleStep", "SingleStep", &[]),
    action(72, "ClearMacroError", "ClearMacroError", &[]),
    action(73, "OnError", "OnError", &["Goto", "MacroName"]),
    action(76, "SetTempVar", "SetTempVar", &["Name", "Expression"]),
    action(77, "RemoveAllTempVars", "RemoveAllTempVars", &[]),
    action(78, "RemoveTempVar", "RemoveTempVar", &["Name"]),
    action(
        79,
        "SetDisplayedCategories",
        "SetDisplayedCategories",
        &["Show", "Category"],
    ),
    action(80, "LockNavigationPane", "LockNavigationPane", &["Lock"]),
    action(
        81,
        "RunSavedImportExport",
        "RunSavedImportExport",
        &["SavedImportExportName"],
    ),
    action(82, "SetLocalVar", "SetLocalVar", &["Name", "Expression"]),
    action(
        83,
        "BrowseTo",
        "BrowseTo",
        &[
            "ObjectType",
            "ObjectName",
            "PathToSubformControl",
            "WhereCondition",
            "Page",
            "DataMode",
        ],
    ),
    action(85, "RunDataMacro", "RunDataMacro", &["Name"]),
    action(86, "SetOrderBy", "SetOrderBy", &["OrderBy", "ControlName"]),
    action(
        87,
        "SetFilter",
        "SetFilter",
        &["FilterName", "WhereCondition", "ControlName"],
    ),
    action(88, "RefreshRecord", "RefreshRecord", &[]),
    action(95, "DeleteRecord", "DeleteRecord", &[]),
];

/// Returns the name of an argument value stored as a number in a macro grid
/// row: `value` in the argument `argument` (a name from
/// [`MacroAction::arguments`]) of the action numbered `code`, or `None` if the
/// argument is not a choice of named values or the number is not one of them.
///
/// The numbers are those of the VBA enumerations the corresponding `DoCmd`
/// methods take (for example `AcFormView` for the `View` argument of
/// `OpenForm`), and `-1` and `0` for arguments set to Yes or No. The names are
/// the settings the macro action reference lists for each argument, and are
/// not checked against Access: Access accepts any value in macro XML.
/// `MessageBox`'s `Type` takes its numbers from the order of its settings.
/// Real macros confirm `No` = 0 for `Beep` and `Information` = 4 for `Type`
/// of `MessageBox`.
pub fn macro_argument_value_name(code: u16, argument: &str, value: &str) -> Option<&'static str> {
    let action = macro_action(code)?.name;
    let number: i32 = value.parse().ok()?;
    ARGUMENT_VALUES
        .iter()
        .find(|(a, arg, _)| *a == action && *arg == argument)?
        .2
        .iter()
        .find(|(n, _)| *n == number)
        .map(|(_, name)| *name)
}

const YES_NO: &[(i32, &str)] = &[(-1, "Yes"), (0, "No")];
/// `AcObjectType`.
const OBJECT_TYPE: &[(i32, &str)] = &[
    (0, "Table"),
    (1, "Query"),
    (2, "Form"),
    (3, "Report"),
    (4, "Macro"),
    (5, "Module"),
    (7, "Server View"),
    (8, "Diagram"),
    (9, "Stored Procedure"),
    (10, "Function"),
];
/// `AcView` for tables, queries, views, stored procedures, and functions.
const DATA_VIEW: &[(i32, &str)] = &[
    (0, "Datasheet"),
    (1, "Design"),
    (2, "Print Preview"),
    (3, "PivotTable"),
    (4, "PivotChart"),
];
/// `AcOpenDataMode`.
const DATA_MODE: &[(i32, &str)] = &[(0, "Add"), (1, "Edit"), (2, "Read Only")];
/// `AcWindowMode`.
const WINDOW_MODE: &[(i32, &str)] = &[(0, "Normal"), (1, "Hidden"), (2, "Icon"), (3, "Dialog")];
/// `AcRecord`.
const RECORD: &[(i32, &str)] = &[
    (0, "Previous"),
    (1, "Next"),
    (2, "First"),
    (3, "Last"),
    (4, "Go To"),
    (5, "New"),
];
/// `AcDataTransferType`.
const TRANSFER_TYPE: &[(i32, &str)] = &[(0, "Import"), (1, "Export"), (2, "Link")];

/// The named values of each argument that takes a choice, by action and
/// argument name.
static ARGUMENT_VALUES: &[(&str, &str, &[(i32, &str)])] = &[
    ("CloseWindow", "ObjectType", OBJECT_TYPE),
    // AcCloseSave.
    (
        "CloseWindow",
        "Save",
        &[(0, "Prompt"), (1, "Yes"), (2, "No")],
    ),
    ("CopyDatabaseFile", "OverwriteExistingFile", YES_NO),
    ("CopyDatabaseFile", "DisconnectAllUsers", YES_NO),
    ("CopyObject", "SourceObjectType", OBJECT_TYPE),
    ("DeleteObject", "ObjectType", OBJECT_TYPE),
    ("DisplayHourglassPointer", "HourglassOn", YES_NO),
    ("Echo", "EchoOn", YES_NO),
    ("EMailDatabaseObject", "ObjectType", OBJECT_TYPE),
    ("EMailDatabaseObject", "EditMessage", YES_NO),
    ("ExportWithFormatting", "ObjectType", OBJECT_TYPE),
    ("ExportWithFormatting", "AutoStart", YES_NO),
    // AcExportQuality.
    (
        "ExportWithFormatting",
        "OutputQuality",
        &[(0, "Print"), (1, "Screen")],
    ),
    // AcFindMatch.
    (
        "FindRecord",
        "Match",
        &[
            (0, "Any Part of Field"),
            (1, "Whole Field"),
            (2, "Start of Field"),
        ],
    ),
    ("FindRecord", "MatchCase", YES_NO),
    // AcSearchDirection.
    (
        "FindRecord",
        "Search",
        &[(0, "Up"), (1, "Down"), (2, "All")],
    ),
    ("FindRecord", "SearchAsFormatted", YES_NO),
    ("FindRecord", "OnlyCurrentField", YES_NO),
    ("FindRecord", "FindFirst", YES_NO),
    ("GoToRecord", "ObjectType", OBJECT_TYPE),
    ("GoToRecord", "Record", RECORD),
    ("ImportExportData", "TransferType", TRANSFER_TYPE),
    ("ImportExportData", "ObjectType", OBJECT_TYPE),
    ("ImportExportData", "StructureOnly", YES_NO),
    ("ImportExportSpreadsheet", "TransferType", TRANSFER_TYPE),
    ("ImportExportSpreadsheet", "HasFieldNames", YES_NO),
    // AcTextTransferType.
    (
        "ImportExportText",
        "TransferType",
        &[
            (0, "Import Delimited"),
            (1, "Import Fixed Width"),
            (2, "Export Delimited"),
            (3, "Export Fixed Width"),
            (4, "Export Word for Windows Merge"),
            (5, "Link Delimited"),
            (6, "Link Fixed Width"),
            (7, "Import HTML"),
            (8, "Export HTML"),
            (9, "Link HTML"),
        ],
    ),
    ("ImportExportText", "HasFieldNames", YES_NO),
    ("LockNavigationPane", "Lock", YES_NO),
    ("MessageBox", "Beep", YES_NO),
    (
        "MessageBox",
        "Type",
        &[
            (0, "None"),
            (1, "Critical"),
            (2, "Warning?"),
            (3, "Warning!"),
            (4, "Information"),
        ],
    ),
    // AcFormView.
    (
        "OpenForm",
        "View",
        &[
            (0, "Form"),
            (1, "Design"),
            (2, "Print Preview"),
            (3, "Datasheet"),
            (4, "PivotTable"),
            (5, "PivotChart"),
            (6, "Layout"),
        ],
    ),
    // AcFormOpenDataMode; -1 (property settings) is the empty default.
    ("OpenForm", "DataMode", DATA_MODE),
    ("OpenForm", "WindowMode", WINDOW_MODE),
    ("OpenFunction", "View", DATA_VIEW),
    ("OpenFunction", "DataMode", DATA_MODE),
    ("OpenQuery", "View", DATA_VIEW),
    ("OpenQuery", "DataMode", DATA_MODE),
    // AcView.
    (
        "OpenReport",
        "View",
        &[
            (0, "Print"),
            (1, "Design"),
            (2, "Print Preview"),
            (5, "Report"),
            (6, "Layout"),
        ],
    ),
    ("OpenReport", "WindowMode", WINDOW_MODE),
    ("OpenStoredProcedure", "View", DATA_VIEW),
    ("OpenStoredProcedure", "DataMode", DATA_MODE),
    ("OpenTable", "View", DATA_VIEW),
    ("OpenTable", "DataMode", DATA_MODE),
    ("OpenView", "View", DATA_VIEW),
    ("OpenView", "DataMode", DATA_MODE),
    // AcPrintRange.
    (
        "PrintOut",
        "PrintRange",
        &[(0, "All"), (1, "Selection"), (2, "Pages")],
    ),
    // AcPrintQuality.
    (
        "PrintOut",
        "PrintQuality",
        &[(0, "High"), (1, "Medium"), (2, "Low"), (3, "Draft")],
    ),
    ("PrintOut", "CollateCopies", YES_NO),
    // AcQuitOption.
    (
        "QuitAccess",
        "Options",
        &[(0, "Prompt"), (1, "Save All"), (2, "Exit")],
    ),
    ("RenameObject", "ObjectType", OBJECT_TYPE),
    ("RepaintObject", "ObjectType", OBJECT_TYPE),
    ("RunSQL", "UseTransaction", YES_NO),
    ("SaveObject", "ObjectType", OBJECT_TYPE),
    ("SearchForRecord", "ObjectType", OBJECT_TYPE),
    ("SearchForRecord", "Record", RECORD),
    ("SelectObject", "ObjectType", OBJECT_TYPE),
    ("SelectObject", "InNavigationPane", YES_NO),
    ("SendKeys", "Wait", YES_NO),
    ("SetDisplayedCategories", "Show", YES_NO),
    ("SetWarnings", "WarningsOn", YES_NO),
    // AcShowToolbar.
    (
        "ShowToolbar",
        "Show",
        &[(0, "Yes"), (1, "Where Appropriate"), (2, "No")],
    ),
    ("TransferSQLDatabase", "UseTrustedConnection", YES_NO),
    ("TransferSQLDatabase", "TransferCopyData", YES_NO),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::read_catalog;
    use crate::data::{read_table_rows, Value};
    use crate::file::PageReader;
    use crate::macro_def::{list_macros, read_macro};
    use crate::table::read_table_def;

    #[test]
    fn codes_are_unique_and_ordered() {
        assert!(MACRO_ACTIONS.windows(2).all(|w| w[0].code < w[1].code));
    }

    #[test]
    fn argument_values_name_existing_arguments() {
        for (action, argument, values) in ARGUMENT_VALUES {
            let entry = MACRO_ACTIONS.iter().find(|a| a.name == *action);
            assert!(
                entry.is_some_and(|a| a.arguments.contains(argument)),
                "{action}.{argument}"
            );
            let mut numbers: Vec<i32> = values.iter().map(|(n, _)| *n).collect();
            numbers.sort_unstable();
            numbers.dedup();
            assert_eq!(numbers.len(), values.len(), "{action}.{argument}");
        }
    }

    #[test]
    fn argument_value_names() {
        assert_eq!(
            macro_argument_value_name(23, "View", "2"),
            Some("Print Preview")
        );
        assert_eq!(
            macro_argument_value_name(46, "View", "2"),
            Some("Print Preview")
        );
        assert_eq!(macro_argument_value_name(6, "Save", "0"), Some("Prompt"));
        // Not a named choice, not one of the choices, or not a number.
        assert_eq!(macro_argument_value_name(23, "FormName", "0"), None);
        assert_eq!(macro_argument_value_name(23, "DataMode", "-1"), None);
        assert_eq!(macro_argument_value_name(23, "View", "Form"), None);
    }

    #[test]
    fn argument_value_names_match_macro_xml() {
        // mcrSimple's second action is a MessageBox whose XML sets Beep to No
        // and Type to Information, stored in the grid as 0 and 4.
        use crate::macro_def::MacroStatement;

        let Some(path) = test_data_path("V2010/macroTestV2010.accdb") else {
            eprintln!("SKIP: test data not found: V2010/macroTestV2010.accdb");
            return;
        };
        let mut reader = PageReader::open(&path).unwrap();
        let def = read_macro(&mut reader, "mcrSimple").unwrap();
        let MacroStatement::Action { arguments, .. } = &def.statements[1] else {
            panic!("{:?}", def.statements[1]);
        };
        let row = &def.grid.unwrap().rows[1];
        for (slot, name) in [(1, "Beep"), (2, "Type")] {
            let xml_value = &arguments.iter().find(|a| a.name == name).unwrap().value;
            let grid_value = row.arguments[slot].as_deref().unwrap();
            assert_eq!(
                macro_argument_value_name(row.action_code, name, grid_value),
                Some(xml_value.as_str()),
                "{name}"
            );
        }
    }

    #[test]
    fn lookup() {
        let action = macro_action(22).unwrap();
        assert_eq!(action.name, "MessageBox");
        assert_eq!(action.text_name, "MsgBox");
        assert_eq!(action.arguments, ["Message", "Beep", "Type", "Title"]);
        assert_eq!(macro_action(2), None);
    }

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(relative);
        path.exists().then_some(path)
    }

    // macroGeneratedTestV2010.accdb holds, for each action name, a macro loaded
    // from one `Action ="<name>"` row: `x_<name>` for the names of the macro
    // action reference and `old_<name>` for the earlier names. Its tblGenLog
    // records which reference names Access rejected (`probe-action`) and the
    // largest number of arguments Access accepted for each name (`probe-args`).
    const GENERATED: &str = "V2010/macroGeneratedTestV2010.accdb";

    fn find_by_name(name: &str) -> Option<&'static MacroAction> {
        MACRO_ACTIONS
            .iter()
            .find(|a| a.name == name || a.text_name == name)
    }

    #[test]
    fn codes_match_the_generated_macros() {
        let Some(path) = test_data_path(GENERATED) else {
            eprintln!("SKIP: test data not found: {GENERATED}");
            return;
        };
        let mut reader = PageReader::open(&path).unwrap();
        let names: Vec<String> = list_macros(&mut reader)
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        let mut checked = 0;
        for macro_name in &names {
            let (action_name, by_text_name) = if let Some(n) = macro_name.strip_prefix("x_") {
                (n, false)
            } else if let Some(n) = macro_name.strip_prefix("old_") {
                (n, true)
            } else {
                continue;
            };
            let grid = read_macro(&mut reader, macro_name).unwrap().grid.unwrap();
            let codes: Vec<u16> = grid
                .rows
                .iter()
                .filter(|r| r.action_code != 0)
                .map(|r| r.action_code)
                .collect();
            assert_eq!(codes.len(), 1, "{macro_name}");
            let action = macro_action(codes[0])
                .unwrap_or_else(|| panic!("{macro_name}: no action {}", codes[0]));
            let expected = if by_text_name {
                action.text_name
            } else {
                action.name
            };
            assert_eq!(expected, action_name, "{macro_name}");
            checked += 1;
        }
        assert_eq!(checked, 56 + 56);
    }

    #[test]
    fn argument_counts_match_the_generated_log() {
        let Some(path) = test_data_path(GENERATED) else {
            eprintln!("SKIP: test data not found: {GENERATED}");
            return;
        };
        let mut reader = PageReader::open(&path).unwrap();
        let page = read_catalog(&mut reader)
            .unwrap()
            .into_iter()
            .find(|e| e.name == "tblGenLog")
            .unwrap()
            .table_page;
        let tdef = read_table_def(&mut reader, "tblGenLog", page).unwrap();
        let text = |v: &Value| match v {
            Value::Text(s) => s.clone(),
            other => panic!("unexpected {other:?}"),
        };
        let mut checked = 0;
        for row in read_table_rows(&mut reader, &tdef).unwrap().rows {
            let (kind, name, result) = (text(&row[1]), text(&row[2]), text(&row[3]));
            match (kind.as_str(), result.as_str()) {
                // A reference name Access rejects is either renamed (its
                // earlier name is the text name) or used only by data macros.
                ("probe-action", "rejected") => {
                    assert!(MACRO_ACTIONS.iter().all(|a| a.text_name != name), "{name}");
                }
                ("probe-args", "rejected") => {
                    assert!(MACRO_ACTIONS.iter().all(|a| a.text_name != name), "{name}");
                }
                ("probe-args", count) => {
                    let action = find_by_name(&name).unwrap_or_else(|| panic!("{name}"));
                    assert_eq!(action.arguments.len().to_string(), count, "{name}");
                    checked += 1;
                }
                _ => {}
            }
        }
        assert_eq!(checked, 56 + 20);
    }
}
