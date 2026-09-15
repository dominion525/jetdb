Option Compare Database
Option Explicit

' Run ProbeMacroActions once in the database used by ResaveMacroTestObjects.
' Loads macros written in the older text format (Action ="..." rows). No macro is run.
'   x_<name>:   the action under its current (Access 2010 and later) name, without arguments.
'   n<k>_<name>: the action with k empty arguments, for k = 1, 2, ... until Access rejects it.
'                The largest k that loads is logged as the argument count.
' Results go to tblGenLog. A rejected load leaves an import error file, as before.

Public Sub ProbeMacroActions()
    Dim current As Variant, older As Variant, a As Variant, seen As Object

    current = Array("AddMenu", "ApplyFilter", "Beep", "BrowseTo", "CancelEvent", "CancelRecordChange", _
        "ClearMacroError", "CloseDatabase", "CloseWindow", "CopyDatabaseFile", "CopyObject", "DeleteObject", _
        "DeleteRecord", "DisplayHourglassPointer", "Echo", "EMailDatabaseObject", "ExitForEachRecord", _
        "ExportWithFormatting", "FindNextRecord", "FindRecord", "GoToControl", "GoToPage", "GoToRecord", _
        "ImportExportData", "ImportExportSpreadsheet", "ImportExportText", "ImportSharePointList", _
        "LockNavigationPane", "LogEvent", "MaximizeWindow", "MessageBox", "MinimizeWindow", "MoveAndSizeWindow", _
        "NavigateTo", "OnError", "OpenDiagram", "OpenForm", "OpenFunction", "OpenQuery", "OpenReport", _
        "OpenStoredProcedure", "OpenTable", "OpenView", "OpenVisualBasicModule", "PrintOut", "QuitAccess", _
        "RaiseError", "RefreshRecord", "RemoveAllTempVars", "RemoveTempVar", "RenameObject", "RepaintObject", _
        "Requery", "RestoreWindow", "RunApplication", "RunCode", "RunDataMacro", "RunMacro", "RunMenuCommand", _
        "RunSavedImportExport", "RunSQL", "SaveObject", "SearchForRecord", "SelectObject", "SendEmail", _
        "SendKeys", "SetDisplayedCategories", "SetField", "SetFilter", "SetLocalVar", "SetMenuItem", _
        "SetOrderBy", "SetProperty", "SetReturnVar", "SetTempVar", "SetValue", "SetWarnings", "ShowAllRecords", _
        "ShowToolbar", "SingleStep", "StartNewWorkflow", "StopAllMacros", "StopMacro", "TransferSQLDatabase", _
        "WorkflowTasks")
    older = Array("AddMenu", "ApplyFilter", "Beep", "CancelEvent", "Close", "CopyDatabaseFile", _
        "CopyObject", "DeleteObject", "Echo", "FindNext", "FindRecord", "GoToControl", "GoToPage", _
        "GoToRecord", "Hourglass", "Maximize", "Minimize", "MoveSize", "MsgBox", "OpenDataAccessPage", _
        "OpenDiagram", "OpenForm", "OpenFunction", "OpenModule", "OpenQuery", "OpenReport", _
        "OpenStoredProcedure", "OpenTable", "OpenView", "OutputTo", "PrintOut", "Quit", "Rename", _
        "RepaintObject", "Requery", "Restore", "RunApp", "RunCode", "RunCommand", "RunMacro", "RunSQL", _
        "Save", "SelectObject", "SendKeys", "SendObject", "SetMenuItem", "SetValue", "SetWarnings", _
        "ShowAllRecords", "ShowToolbar", "StopAllMacros", "StopMacro", "TransferDatabase", _
        "TransferSpreadsheet", "TransferSQLDatabase", "TransferText")

    For Each a In current
        If TryLoad("x_" & a, a, 0) Then LogResult "probe-action", CStr(a), "ok" Else LogResult "probe-action", CStr(a), "rejected"
    Next

    Set seen = CreateObject("Scripting.Dictionary")
    For Each a In current
        If Not seen.Exists(a) Then seen.Add a, True
    Next
    For Each a In older
        If Not seen.Exists(a) Then seen.Add a, True
    Next
    For Each a In seen.Keys
        ProbeArgumentCount CStr(a)
    Next

    Debug.Print "done"
End Sub

Private Sub ProbeArgumentCount(ByVal action As String)
    Dim k As Long
    If Not TryLoad("n0_" & action, action, 0) Then
        LogResult "probe-args", action, "rejected"
        Exit Sub
    End If
    For k = 1 To 10
        If Not TryLoad("n" & k & "_" & action, action, k) Then Exit For
    Next
    LogResult "probe-args", action, CStr(k - 1)
End Sub

' Loads a one-row macro with argumentCount empty arguments; returns whether Access accepted it.
Private Function TryLoad(ByVal name As String, ByVal action As String, ByVal argumentCount As Long) As Boolean
    Dim f As String, s As String, i As Long
    s = "Version =196611" & vbCrLf & "ColumnsShown =0" & vbCrLf & "Begin" & vbCrLf & "    Action =""" & action & """" & vbCrLf
    For i = 1 To argumentCount
        s = s & "    Argument =""""" & vbCrLf
    Next
    s = s & "End" & vbCrLf
    f = Environ("TEMP") & "\jetdb_" & name & ".txt"
    WriteUnicode f, s
    On Error Resume Next
    Application.LoadFromText acMacro, name, f
    TryLoad = (Err.Number = 0)
    Err.Clear
    On Error GoTo 0
    Kill f
End Function

Private Sub WriteUnicode(ByVal path As String, ByVal text As String)
    Dim st As Object
    Set st = CreateObject("ADODB.Stream")
    st.Type = 2
    st.Charset = "Unicode"
    st.Open
    st.WriteText text
    st.SaveToFile path, 2
    st.Close
End Sub

Private Sub LogResult(ByVal kind As String, ByVal name As String, ByVal result As String)
    Dim rs As DAO.Recordset
    Set rs = CurrentDb.OpenRecordset("tblGenLog", dbOpenDynaset)
    rs.AddNew
    rs!Kind = kind
    rs!ObjName = name
    rs!Result = Left(result, 255)
    rs.Update
    rs.Close
    Debug.Print kind, name, result
End Sub
