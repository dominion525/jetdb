Attribute VB_Name = "SameNameGenerator"
' Creates a form and a report that share the name Shared, each with its own
' caption, controls and embedded macro, for sameNameTest*.accdb/.mdb. Access
' keeps forms and reports in separate containers, so they may share a name.
' tblGenLog records what was created, and tblGenExport holds Access's
' SaveAsText output of the form and the report.
'
' Import this module into a new, empty .accdb and run
' CreateSameNameFormAndReport once from the Immediate window.
Option Compare Database
Option Explicit

Private Const NS As String = " xmlns='http://schemas.microsoft.com/office/accessservices/2009/11/application'"
Private Const SHARED_NAME As String = "Shared"

Public Sub CreateSameNameFormAndReport()
    CurrentDb.Execute "CREATE TABLE tblGenLog ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Result] TEXT(255))"
    CurrentDb.Execute "CREATE TABLE tblGenExport ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Content] MEMO)"
    CurrentDb.Execute "CREATE TABLE tblShared ([ID] COUNTER PRIMARY KEY, [Note] TEXT(50))"

    Dim frm As Form, rpt As Report, ctl As Control, tmpName As String

    ' The form: a caption and a button of its own.
    Set frm = CreateForm()
    frm.RecordSource = "tblShared"
    frm.Caption = "Shared form"
    Set ctl = CreateControl(frm.Name, acCommandButton, acDetail, , , 500, 500, 1500, 400)
    ctl.Name = "btnFormOnly"
    tmpName = frm.Name
    DoCmd.Close acForm, tmpName, acSaveYes
    DoCmd.Rename SHARED_NAME, acForm, tmpName
    LogResult "form", SHARED_NAME, "created"

    ' The report: a caption and a label of its own.
    Set rpt = CreateReport()
    rpt.RecordSource = "tblShared"
    rpt.Caption = "Shared report"
    Set ctl = CreateReportControl(rpt.Name, acLabel, acDetail, , "Report only", 500, 500, 1500, 400)
    ctl.Name = "lblReportOnly"
    tmpName = rpt.Name
    DoCmd.Close acReport, tmpName, acSaveYes
    DoCmd.Rename SHARED_NAME, acReport, tmpName
    LogResult "report", SHARED_NAME, "created"

    ' An embedded macro on each, with a message of its own.
    AddEmbeddedMacro acForm, "form", "OnLoadEmMacro", _
        "<UserInterfaceMacro Event='OnLoad'" & NS & "><Statements><Action Name='MessageBox'><Argument Name='Message'>form loaded</Argument></Action></Statements></UserInterfaceMacro>"
    AddEmbeddedMacro acReport, "report", "OnOpenEmMacro", _
        "<UserInterfaceMacro Event='OnOpen'" & NS & "><Statements><Action Name='MessageBox'><Argument Name='Message'>report opened</Argument></Action></Statements></UserInterfaceMacro>"

    Debug.Print "done; see tblGenLog"
End Sub

' Adds an embedded macro to the form or report Shared by exporting it with
' SaveAsText, inserting an <Event>EmMacro block after its RecordSource line,
' and loading the text back. Then stores its SaveAsText output.
Private Sub AddEmbeddedMacro(ByVal objectType As AcObjectType, ByVal kind As String, ByVal propertyName As String, ByVal xml As String)
    Const ANCHOR As String = "RecordSource =""tblShared"""
    Dim f As String, text As String, pos As Long, lineStart As Long, lineEnd As Long, indent As String
    f = TempFile(kind & "_" & SHARED_NAME)
    On Error Resume Next
    Application.SaveAsText objectType, SHARED_NAME, f
    If Err.Number <> 0 Then
        LogResult kind, SHARED_NAME, "export error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    text = ReadText(f)

    pos = InStr(1, text, ANCHOR)
    If pos = 0 Then
        LogResult kind, SHARED_NAME, "anchor not found: " & ANCHOR
        Kill f
        Exit Sub
    End If
    lineStart = InStrRev(text, vbCrLf, pos) + 2
    lineEnd = InStr(pos, text, vbCrLf)
    indent = Left(Mid(text, lineStart), Len(Mid(text, lineStart)) - Len(LTrim(Mid(text, lineStart))))
    text = Left(text, lineEnd + 1) & EmbeddedMacroBlock(indent, propertyName, xml) & Mid(text, lineEnd + 2)

    WriteUnicode f, text
    On Error Resume Next
    Application.LoadFromText objectType, SHARED_NAME, f
    If Err.Number <> 0 Then
        LogResult kind, SHARED_NAME, "load error " & Err.Number & ": " & Err.Description
        Err.Clear
    Else
        LogResult kind, SHARED_NAME, "embedded macro ok"
        ExportObject objectType, kind
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

' Stores Access's SaveAsText output of the form or report Shared in tblGenExport.
Private Sub ExportObject(ByVal objectType As AcObjectType, ByVal kind As String)
    Dim f As String, rs As DAO.Recordset
    f = TempFile("export_" & kind & "_" & SHARED_NAME)
    On Error Resume Next
    Application.SaveAsText objectType, SHARED_NAME, f
    If Err.Number <> 0 Then
        LogResult kind & "-export", SHARED_NAME, "error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    Set rs = CurrentDb.OpenRecordset("tblGenExport", dbOpenDynaset)
    rs.AddNew
    rs!Kind = kind
    rs!ObjName = SHARED_NAME
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
    rs!Result = Left$(result, 255)
    rs.Update
    rs.Close
End Sub
