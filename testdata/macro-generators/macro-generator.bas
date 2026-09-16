Option Compare Database
Option Explicit

' Creates macro test objects for jetdb in the current database, records whether
' each one was created, and stores Access's own SaveAsText output of each object.
' Run CreateMacroTestObjects once in a new, empty .accdb.

Private Const NS As String = " xmlns='http://schemas.microsoft.com/office/accessservices/2009/11/application'"

Public Sub CreateMacroTestObjects()
    ' Column names are bracketed: NOTE is a Jet SQL type name (a synonym of MEMO).
    CurrentDb.Execute "CREATE TABLE tblGenLog ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Result] TEXT(255))"
    CurrentDb.Execute "CREATE TABLE tblGenExport ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Content] MEMO)"
    CurrentDb.Execute "CREATE TABLE tblItems ([ID] COUNTER PRIMARY KEY, [ItemName] TEXT(50), [Qty] LONG)"
    CurrentDb.Execute "CREATE TABLE tblAudit ([ID] COUNTER PRIMARY KEY, [Note] TEXT(255))"
    CurrentDb.Execute "CREATE TABLE tblNamed ([ID] COUNTER PRIMARY KEY, [Note] TEXT(255))"

    ' One named macro per action (names from the Access macro action reference).
    Dim actions As Variant, a As Variant
    actions = Array("AddMenu", "ApplyFilter", "Beep", "BrowseTo", "CancelEvent", "CancelRecordChange", _
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
    For Each a In actions
        LoadMacro "act_" & a, "<UserInterfaceMacro" & NS & "><Statements><Action Name='" & a & "'/></Statements></UserInterfaceMacro>"
    Next

    ' Groups and submacros.
    LoadMacro "mcrStructure", "<UserInterfaceMacro" & NS & "><Statements>" & _
        "<StatementGroup Description='Group A'><Statements><Action Name='SetTempVar'><Argument Name='Name'>x</Argument><Argument Name='Expression'>1</Argument></Action></Statements></StatementGroup>" & _
        "<SubMacro Name='SubOne'><Statements><Action Name='MessageBox'><Argument Name='Message'>sub one</Argument></Action></Statements></SubMacro>" & _
        "<SubMacro Name='SubTwo'><Statements><Action Name='RunMacro'><Argument Name='MacroName'>mcrStructure.SubOne</Argument></Action></Statements></SubMacro>" & _
        "</Statements></UserInterfaceMacro>"

    ' Error handling.
    LoadMacro "mcrErrors", "<UserInterfaceMacro" & NS & "><Statements>" & _
        "<Action Name='OnError'><Argument Name='Goto'>Macro Name</Argument><Argument Name='MacroName'>Handler</Argument></Action>" & _
        "<Action Name='RunCode'><Argument Name='FunctionName'>NoSuchFunction()</Argument></Action>" & _
        "<SubMacro Name='Handler'><Statements><Action Name='MessageBox'><Argument Name='Message'>=[MacroError].[Description]</Argument></Action><Action Name='ClearMacroError'/></Statements></SubMacro>" & _
        "</Statements></UserInterfaceMacro>"

    ' Data macros: several events on one table.
    LoadDataMacros "tblItems", "<DataMacros" & NS & ">" & _
        "<DataMacro Event='BeforeChange'><Statements><ConditionalBlock><If><Condition>[Qty]&lt;0</Condition><Statements><Action Name='RaiseError'><Argument Name='Number'>1</Argument><Argument Name='Description'>negative qty</Argument></Action></Statements></If></ConditionalBlock></Statements></DataMacro>" & _
        "<DataMacro Event='AfterInsert'><Statements><CreateRecord><Data><Reference>tblAudit</Reference></Data><Statements><Action Name='SetField'><Argument Name='Field'>Note</Argument><Argument Name='Value'>'inserted'</Argument></Action></Statements></CreateRecord></Statements></DataMacro>" & _
        "</DataMacros>"

    ' A named data macro with a parameter, on its own table.
    LoadDataMacros "tblNamed", "<DataMacros" & NS & ">" & _
        "<DataMacro Name='dmLog'><Parameters><Parameter Name='msg'/></Parameters><Statements><CreateRecord><Data><Reference>tblAudit</Reference></Data><Statements><Action Name='SetField'><Argument Name='Field'>Note</Argument><Argument Name='Value'>[msg]</Argument></Action></Statements></CreateRecord></Statements></DataMacro>" & _
        "</DataMacros>"

    ' Embedded macros on a form (a button's click and the form's load) and a report (open).
    CreateEmbeddedMacroObjects

    Debug.Print "done"
End Sub

' Creates frmEmbedded and rptEmbedded, then adds embedded macros by exporting each with
' SaveAsText, inserting <Event>EmMacro blocks, and loading the text back.
Private Sub CreateEmbeddedMacroObjects()
    Dim frm As Form, rpt As Report, ctl As Control, tmpName As String

    Set frm = CreateForm()
    frm.RecordSource = "tblItems"
    Set ctl = CreateControl(frm.Name, acCommandButton, acDetail, , , 500, 500, 1500, 400)
    ctl.Name = "btnHello"
    tmpName = frm.Name
    DoCmd.Close acForm, tmpName, acSaveYes
    DoCmd.Rename "frmEmbedded", acForm, tmpName

    Set rpt = CreateReport()
    rpt.RecordSource = "tblItems"
    tmpName = rpt.Name
    DoCmd.Close acReport, tmpName, acSaveYes
    DoCmd.Rename "rptEmbedded", acReport, tmpName

    AddEmbeddedMacros acForm, "form", "frmEmbedded", Array( _
        Array("Name =""btnHello""", "OnClickEmMacro", "<UserInterfaceMacro For='btnHello' Event='OnClick'" & NS & "><Statements><Action Name='MessageBox'><Argument Name='Message'>button clicked</Argument></Action></Statements></UserInterfaceMacro>"), _
        Array("RecordSource =""tblItems""", "OnLoadEmMacro", "<UserInterfaceMacro Event='OnLoad'" & NS & "><Statements><Action Name='SetTempVar'><Argument Name='Name'>x</Argument><Argument Name='Expression'>2</Argument></Action></Statements></UserInterfaceMacro>"))
    AddEmbeddedMacros acReport, "report", "rptEmbedded", Array( _
        Array("RecordSource =""tblItems""", "OnOpenEmMacro", "<UserInterfaceMacro Event='OnOpen'" & NS & "><Statements><Action Name='MessageBox'><Argument Name='Message'>report opened</Argument></Action></Statements></UserInterfaceMacro>"))
End Sub

' Each item of macros is Array(line to insert after, property name, XML).
Private Sub AddEmbeddedMacros(ByVal objectType As AcObjectType, ByVal kind As String, ByVal name As String, ByVal macros As Variant)
    Dim f As String, text As String, m As Variant, pos As Long, lineStart As Long, lineEnd As Long, indent As String
    f = TempFile(kind & "_" & name)
    On Error Resume Next
    Application.SaveAsText objectType, name, f
    If Err.Number <> 0 Then
        LogResult kind, name, "export error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    text = ReadText(f)

    For Each m In macros
        pos = InStr(1, text, m(0))
        If pos = 0 Then
            LogResult kind, name & "." & m(1), "anchor not found: " & m(0)
        Else
            lineStart = InStrRev(text, vbCrLf, pos) + 2
            lineEnd = InStr(pos, text, vbCrLf)
            indent = Left(Mid(text, lineStart), Len(Mid(text, lineStart)) - Len(LTrim(Mid(text, lineStart))))
            text = Left(text, lineEnd + 1) & EmbeddedMacroBlock(indent, m(1), m(2)) & Mid(text, lineEnd + 2)
        End If
    Next

    WriteUnicode f, text
    On Error Resume Next
    Application.LoadFromText objectType, name, f
    If Err.Number <> 0 Then
        LogResult kind, name, "load error " & Err.Number & ": " & Err.Description
        Err.Clear
    Else
        LogResult kind, name, "ok"
        ExportObject objectType, kind, name
    End If
    On Error GoTo 0
    Kill f
End Sub

Private Function EmbeddedMacroBlock(ByVal indent As String, ByVal propertyName As String, ByVal xml As String) As String
    Const CHUNK As Long = 200
    Dim s As String, i As Long
    xml = "<?xml version='1.0' encoding='UTF-16' standalone='no'?>" & xml
    s = indent & propertyName & " = Begin" & vbCrLf & _
        indent & "    Version =196611" & vbCrLf & _
        indent & "    ColumnsShown =0" & vbCrLf
    For i = 1 To Len(xml) Step CHUNK
        s = s & indent & "    Begin" & vbCrLf & _
            indent & "        Comment =""_AXL:" & Mid(xml, i, CHUNK) & """" & vbCrLf & _
            indent & "    End" & vbCrLf
    Next
    EmbeddedMacroBlock = s & indent & "End" & vbCrLf
End Function

' Builds a SaveAsText-style macro file whose content is the XML as _AXL: comments.
' The XML uses single quotes so that no escaping is needed inside the text file.
Private Function MacroText(ByVal xml As String) As String
    Const CHUNK As Long = 200
    Dim s As String, i As Long
    xml = "<?xml version='1.0' encoding='UTF-16' standalone='no'?>" & xml
    s = "Version =196611" & vbCrLf & "ColumnsShown =0" & vbCrLf
    For i = 1 To Len(xml) Step CHUNK
        s = s & "Begin" & vbCrLf & "    Comment =""_AXL:" & Mid(xml, i, CHUNK) & """" & vbCrLf & "End" & vbCrLf
    Next
    MacroText = s
End Function

Private Sub LoadMacro(ByVal name As String, ByVal xml As String)
    Dim f As String
    f = TempFile(name)
    WriteUnicode f, MacroText(xml)
    On Error Resume Next
    Application.LoadFromText acMacro, name, f
    If Err.Number <> 0 Then
        LogResult "macro", name, "error " & Err.Number & ": " & Err.Description
        Err.Clear
    Else
        LogResult "macro", name, "ok"
        ExportObject acMacro, "macro", name
    End If
    On Error GoTo 0
    Kill f
End Sub

Private Sub LoadDataMacros(ByVal table As String, ByVal xml As String)
    Dim f As String
    f = TempFile("dm_" & table)
    WriteUnicode f, xml
    On Error Resume Next
    Application.LoadFromText acTableDataMacro, table, f
    If Err.Number <> 0 Then
        LogResult "datamacro", table, "error " & Err.Number & ": " & Err.Description
        Err.Clear
    Else
        LogResult "datamacro", table, "ok"
        ExportObject acTableDataMacro, "datamacro", table
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
