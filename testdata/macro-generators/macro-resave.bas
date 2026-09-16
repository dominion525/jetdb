Option Compare Database
Option Explicit

' Run ResaveMacroTestObjects once in the database made by CreateMacroTestObjects.
' Macros loaded from XML text hold no action numbers. This makes Access write them in two ways:
'   1. Open each macro in Design view and save it.
'   2. Load macros written in the older text format (Action ="MsgBox" rows), one per older action name.
' No macro is run. Results go to tblGenLog and Access's SaveAsText output to tblGenExport.

Public Sub ResaveMacroTestObjects()
    Dim i As Long, names() As String, n As Long, a As Variant

    ' 1. Resave every existing macro in Design view.
    n = 0
    For i = 0 To CurrentProject.AllMacros.Count - 1
        ReDim Preserve names(n)
        names(n) = CurrentProject.AllMacros(i).Name
        n = n + 1
    Next
    For i = 0 To n - 1
        ResaveInDesign names(i)
    Next

    ' 2. Older action names (Access 2003 and earlier). Each is loaded without arguments
    '    (old_*) and with numbered arguments (oldarg_*) so that argument positions can be matched.
    For Each a In Array("AddMenu", "ApplyFilter", "Beep", "CancelEvent", "Close", "CopyDatabaseFile", _
        "CopyObject", "DeleteObject", "Echo", "FindNext", "FindRecord", "GoToControl", "GoToPage", _
        "GoToRecord", "Hourglass", "Maximize", "Minimize", "MoveSize", "MsgBox", "OpenDataAccessPage", _
        "OpenDiagram", "OpenForm", "OpenFunction", "OpenModule", "OpenQuery", "OpenReport", _
        "OpenStoredProcedure", "OpenTable", "OpenView", "OutputTo", "PrintOut", "Quit", "Rename", _
        "RepaintObject", "Requery", "Restore", "RunApp", "RunCode", "RunCommand", "RunMacro", "RunSQL", _
        "Save", "SelectObject", "SendKeys", "SendObject", "SetMenuItem", "SetValue", "SetWarnings", _
        "ShowAllRecords", "ShowToolbar", "StopAllMacros", "StopMacro", "TransferDatabase", _
        "TransferSpreadsheet", "TransferSQLDatabase", "TransferText")
        LoadOldMacro "old_" & a, OldMacroText(a, 0)
        LoadOldMacro "oldarg_" & a, OldMacroText(a, 10)
    Next

    Debug.Print "done"
End Sub

Private Sub ResaveInDesign(ByVal name As String)
    On Error Resume Next
    DoCmd.SelectObject acMacro, name, True
    DoCmd.RunCommand acCmdDesignView
    If Err.Number <> 0 Then
        LogResult "resave", name, "open error " & Err.Number & ": " & Err.Description
        Err.Clear
        DoCmd.Close acMacro, name, acSaveNo
        Exit Sub
    End If
    DoCmd.Save acMacro, name
    If Err.Number <> 0 Then
        LogResult "resave", name, "save error " & Err.Number & ": " & Err.Description
        Err.Clear
    End If
    DoCmd.Close acMacro, name, acSaveYes
    If Err.Number <> 0 Then
        LogResult "resave", name, "close error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    LogResult "resave", name, "ok"
    ExportObject acMacro, "macro-resaved", name
End Sub

' One row with the action and argumentCount arguments whose values are 1, 2, ...
Private Function OldMacroText(ByVal action As String, ByVal argumentCount As Long) As String
    Dim s As String, i As Long
    s = "Version =196611" & vbCrLf & "ColumnsShown =0" & vbCrLf & "Begin" & vbCrLf & "    Action =""" & action & """" & vbCrLf
    For i = 1 To argumentCount
        s = s & "    Argument =""" & i & """" & vbCrLf
    Next
    OldMacroText = s & "End" & vbCrLf
End Function

Private Sub LoadOldMacro(ByVal name As String, ByVal text As String)
    Dim f As String
    f = TempFile(name)
    WriteUnicode f, text
    On Error Resume Next
    Application.LoadFromText acMacro, name, f
    If Err.Number <> 0 Then
        LogResult "old-macro", name, "error " & Err.Number & ": " & Err.Description
        Err.Clear
    Else
        LogResult "old-macro", name, "ok"
        ExportObject acMacro, "old-macro", name
    End If
    On Error GoTo 0
    Kill f
End Sub

' Stores Access's SaveAsText output of an object in tblGenExport.
Private Sub ExportObject(ByVal objectType As AcObjectType, ByVal kind As String, ByVal name As String)
    Dim f As String, rs As DAO.Recordset
    f = TempFile("export_" & name)
    On Error Resume Next
    Application.SaveAsText objectType, name, f
    If Err.Number <> 0 Then
        LogResult kind & "-export", name, "error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    Set rs = CurrentDb.OpenRecordset("tblGenExport", dbOpenDynaset)
    rs.AddNew
    rs!Kind = kind
    rs!ObjName = name
    rs!Content = ReadText(f)
    rs.Update
    rs.Close
    Kill f
End Sub

Private Function TempFile(ByVal name As String) As String
    TempFile = Environ("TEMP") & "\jetdb_" & name & ".txt"
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

' Reads a text file written by SaveAsText (UTF-16 with a byte order mark, or UTF-8).
Private Function ReadText(ByVal path As String) As String
    Dim st As Object, head As Variant
    Set st = CreateObject("ADODB.Stream")
    st.Type = 1
    st.Open
    st.LoadFromFile path
    head = st.Read(2)
    st.Close
    Set st = CreateObject("ADODB.Stream")
    st.Type = 2
    If head(0) = &HFF And head(1) = &HFE Then st.Charset = "Unicode" Else st.Charset = "utf-8"
    st.Open
    st.LoadFromFile path
    ReadText = st.ReadText
    st.Close
End Function

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
