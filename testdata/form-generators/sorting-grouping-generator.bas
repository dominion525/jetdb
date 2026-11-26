Option Compare Database
Option Explicit

' Creates reports for jetdb whose Sorting and Grouping covers each property of a
' group level, and a form and a report with embedded macros on more events than
' macro-generator.bas has, then stores Access's own SaveAsText output of each
' object in tblGenExport. Run CreateSortingGroupingTestObjects once in a new,
' empty .accdb.
'
' Each property of a group level is also set alone in one level, so that the
' SaveAsText output tells which ID is which: GroupHeader, GroupFooter and
' SortOrder are all written as NotDefault.
'
' Access writes the property IDs above 0x01C4 one higher in Access for
' Microsoft 365 than in older Access, which includes those of embedded macros;
' the version that runs this module gives the IDs of its files only.

Private Const NS As String = " xmlns='http://schemas.microsoft.com/office/accessservices/2009/11/application'"

' AcGroupOn values.
Private Const GROUP_EACH_VALUE As Integer = 0
Private Const GROUP_PREFIX As Integer = 1
Private Const GROUP_YEAR As Integer = 2
Private Const GROUP_QUARTER As Integer = 3
Private Const GROUP_MONTH As Integer = 4
Private Const GROUP_WEEK As Integer = 5
Private Const GROUP_DAY As Integer = 6
Private Const GROUP_INTERVAL As Integer = 9

Public Sub CreateSortingGroupingTestObjects()
    CurrentDb.Execute "CREATE TABLE tblGenLog ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Result] TEXT(255))"
    CurrentDb.Execute "CREATE TABLE tblGenExport ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Content] MEMO)"
    CurrentDb.Execute "CREATE TABLE tblSales ([ID] COUNTER PRIMARY KEY, [Region] TEXT(20), [Customer] TEXT(50), [OrderDate] DATETIME, [Amount] CURRENCY, [Qty] LONG)"

    ' Each level is Array(expression, header, footer, sort descending,
    ' group on, group interval, keep together); -1 leaves a value as Access
    ' sets it.
    CreateGroupedReport "rptGroupHeaderOnly", Array( _
        Array("Region", True, False, False, -1, -1, -1))
    CreateGroupedReport "rptGroupFooterOnly", Array( _
        Array("Region", False, True, False, -1, -1, -1))
    CreateGroupedReport "rptSortOrderOnly", Array( _
        Array("Amount", False, False, True, -1, -1, -1))
    CreateGroupedReport "rptKeepTogether", Array( _
        Array("Region", True, False, False, -1, -1, 1), _
        Array("Customer", True, False, False, -1, -1, 2), _
        Array("ID", False, False, False, -1, -1, 0))
    CreateGroupedReport "rptGroupOnText", Array( _
        Array("Customer", True, False, False, GROUP_PREFIX, -1, -1), _
        Array("Region", True, True, False, GROUP_PREFIX, 3, -1), _
        Array("ID", False, False, False, -1, -1, -1))
    CreateGroupedReport "rptGroupOnDate", Array( _
        Array("OrderDate", True, False, False, GROUP_YEAR, -1, -1), _
        Array("OrderDate", True, False, False, GROUP_QUARTER, -1, -1), _
        Array("OrderDate", True, True, False, GROUP_MONTH, 2, -1), _
        Array("OrderDate", False, False, False, GROUP_WEEK, -1, -1), _
        Array("OrderDate", False, False, True, GROUP_DAY, -1, -1))
    CreateGroupedReport "rptGroupOnInterval", Array( _
        Array("Amount", True, True, False, GROUP_INTERVAL, 1000, -1), _
        Array("Qty", False, False, False, GROUP_INTERVAL, 10, -1), _
        Array("=Year([OrderDate])", True, False, True, GROUP_EACH_VALUE, -1, -1))

    CreateMoreEmbeddedMacros

    Debug.Print "done"
End Sub

' Creates the report `name` on tblSales with the group levels `levels`.
Private Sub CreateGroupedReport(ByVal name As String, ByVal levels As Variant)
    Dim rpt As Report, tmpName As String, lv As Variant, i As Long

    Set rpt = CreateReport()
    rpt.RecordSource = "tblSales"
    tmpName = rpt.Name
    On Error GoTo Failed
    For Each lv In levels
        i = CreateGroupLevel(tmpName, lv(0), lv(1), lv(2))
        rpt.GroupLevel(i).SortOrder = lv(3)
        If lv(4) <> -1 Then rpt.GroupLevel(i).GroupOn = lv(4)
        If lv(5) <> -1 Then rpt.GroupLevel(i).GroupInterval = lv(5)
        If lv(6) <> -1 Then rpt.GroupLevel(i).KeepTogether = lv(6)
    Next
    On Error GoTo 0
    DoCmd.Close acReport, tmpName, acSaveYes
    DoCmd.Rename name, acReport, tmpName
    LogResult "report", name, "ok"
    ExportObject acReport, "report", name
    Exit Sub
Failed:
    LogResult "report", name, "error " & Err.Number & ": " & Err.Description
    DoCmd.Close acReport, tmpName, acSaveNo
End Sub

' Creates frmEvents and rptEvents with embedded macros on the events
' macro-generator.bas leaves out, each with its own message.
Private Sub CreateMoreEmbeddedMacros()
    Dim frm As Form, rpt As Report, ctl As Control, tmpName As String

    Set frm = CreateForm()
    frm.RecordSource = "tblSales"
    Set ctl = CreateControl(frm.Name, acTextBox, acDetail, , "Customer", 500, 500, 2000, 300)
    ctl.Name = "txtCustomer"
    Set ctl = CreateControl(frm.Name, acComboBox, acDetail, , "Region", 500, 1000, 2000, 300)
    ctl.Name = "cboRegion"
    ctl.RowSource = "SELECT DISTINCT Region FROM tblSales"
    ctl.LimitToList = True
    Set ctl = CreateControl(frm.Name, acCheckBox, acDetail, , , 500, 1500)
    ctl.Name = "chkFlag"
    tmpName = frm.Name
    DoCmd.Close acForm, tmpName, acSaveYes
    DoCmd.Rename "frmEvents", acForm, tmpName

    Set rpt = CreateReport()
    rpt.RecordSource = "tblSales"
    tmpName = rpt.Name
    DoCmd.Close acReport, tmpName, acSaveYes
    DoCmd.Rename "rptEvents", acReport, tmpName

    AddEmbeddedMacros acForm, "form", "frmEvents", Array( _
        ControlMacro("txtCustomer", "OnDblClick"), _
        ControlMacro("txtCustomer", "OnGotFocus"), _
        ControlMacro("txtCustomer", "OnLostFocus"), _
        ControlMacro("txtCustomer", "OnEnter"), _
        ControlMacro("txtCustomer", "OnExit"), _
        ControlMacro("txtCustomer", "OnChange"), _
        ControlMacro("txtCustomer", "BeforeUpdate"), _
        ControlMacro("txtCustomer", "AfterUpdate"), _
        ControlMacro("cboRegion", "OnNotInList"), _
        ControlMacro("cboRegion", "AfterUpdate"), _
        ControlMacro("chkFlag", "OnClick"), _
        ObjectMacro("RecordSource =""tblSales""", "OnOpen"), _
        ObjectMacro("RecordSource =""tblSales""", "OnCurrent"), _
        ObjectMacro("RecordSource =""tblSales""", "OnClose"), _
        ObjectMacro("RecordSource =""tblSales""", "OnActivate"), _
        ObjectMacro("RecordSource =""tblSales""", "BeforeInsert"), _
        ObjectMacro("RecordSource =""tblSales""", "AfterUpdate"), _
        ObjectMacro("RecordSource =""tblSales""", "OnDirty"))
    AddEmbeddedMacros acReport, "report", "rptEvents", Array( _
        ObjectMacro("RecordSource =""tblSales""", "OnNoData"), _
        ObjectMacro("RecordSource =""tblSales""", "OnClose"), _
        ObjectMacro("RecordSource =""tblSales""", "OnActivate"))
End Sub

' The macro of the event `eventName` of the control `controlName`, inserted
' after the control's Name line.
Private Function ControlMacro(ByVal controlName As String, ByVal eventName As String) As Variant
    ControlMacro = Array("Name =""" & controlName & """", eventName & "EmMacro", _
        "<UserInterfaceMacro For='" & controlName & "' Event='" & eventName & "'" & NS & "><Statements>" & _
        "<Action Name='MessageBox'><Argument Name='Message'>" & controlName & " " & eventName & "</Argument></Action>" & _
        "</Statements></UserInterfaceMacro>")
End Function

' The macro of the event `eventName` of the form or report, inserted after the
' line `anchor`.
Private Function ObjectMacro(ByVal anchor As String, ByVal eventName As String) As Variant
    ObjectMacro = Array(anchor, eventName & "EmMacro", _
        "<UserInterfaceMacro Event='" & eventName & "'" & NS & "><Statements>" & _
        "<Action Name='MessageBox'><Argument Name='Message'>" & eventName & "</Argument></Action>" & _
        "</Statements></UserInterfaceMacro>")
End Function

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
