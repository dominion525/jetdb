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
//! in macro XML (`Macro Name` is `MacroName`).
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
