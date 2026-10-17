Attribute VB_Name = "PropertyIdGenerator"
' Creates a base form and a base report, and for each property in Specs a
' form or report that differs from its base in that property only, set to
' one value, for propertyIdTest*.accdb. Comparing the design of each with its
' base shows which property ID holds the property. Each is named
' <Target>_<Property>_<n>, for the n-th value of the property in Specs.
' tblGenLog records what was created and any property Access did not take,
' and tblGenExport holds Access's SaveAsText output of each.
'
' Import this module into a new, empty .accdb and run
' CreatePropertyIdForms once from the Immediate window.
Option Compare Database
Option Explicit

' <Target>|<Property>|<value>|<value>...
' Targets: Form and Report (the object itself), TextBox, Label and Button (a
' control of the form), ReportTextBox (a control of the report) and Detail
' (the detail section of the form). The property Window moves the design
' window to Left;Top;Width;Height, and ItemSuffix creates and deletes that
' many controls. A value is a number, True or False.
Private Function Specs() As Collection
    Dim c As New Collection
    c.Add "Form|BorderStyle|0|3"
    c.Add "Form|GridX|10|30"
    c.Add "Form|GridY|12|40"
    c.Add "Form|Width|5000|7000"
    c.Add "Form|Window|1000;800;6000;4000|1500;1200;7000;5000"
    c.Add "Form|ScrollBars|0|1"
    c.Add "Form|ItemSuffix|3|7"
    c.Add "Form|DatasheetFontHeight|14|18"
    c.Add "Form|HelpContextId|1234|4321"
    c.Add "Form|DatasheetGridlinesColor|255|65280"
    c.Add "Form|DatasheetGridlinesColor12|255|65280"
    c.Add "Form|DatasheetGridlinesBehavior|0|1"
    c.Add "Form|DatasheetAlternateBackColor|255|65280"
    c.Add "Form|FilterOnLoad|True"
    c.Add "Form|AllowLayoutView|False"
    c.Add "Form|AllowDatasheetView|False"
    c.Add "Form|AllowFormView|False"
    c.Add "Form|ViewsAllowed|1|2"
    c.Add "Form|DefaultView|1|2"
    c.Add "Form|PictureAlignment|0|4"
    c.Add "Form|TimerInterval|1234|4321"
    c.Add "Report|GridX|10|30"
    c.Add "Report|GridY|12|40"
    c.Add "Report|Width|5000|7000"
    c.Add "Report|Window|1000;800;6000;4000|1500;1200;7000;5000"
    c.Add "Report|ItemSuffix|3|7"
    c.Add "Report|FilterOnLoad|True"
    c.Add "Report|AllowLayoutView|False"
    c.Add "Report|DateGrouping|1"
    c.Add "Report|GrpKeepTogether|0"
    c.Add "TextBox|Left|1700|2300"
    c.Add "TextBox|Top|300|900"
    c.Add "TextBox|Width|2100|2700"
    c.Add "TextBox|Height|350|450"
    c.Add "TextBox|TabIndex|1"
    c.Add "TextBox|VerticalAnchor|1|2"
    c.Add "TextBox|HorizontalAnchor|1|2"
    c.Add "TextBox|LeftPadding|100|200"
    c.Add "TextBox|TopPadding|110|210"
    c.Add "TextBox|RightPadding|120|220"
    c.Add "TextBox|BottomPadding|130|230"
    c.Add "TextBox|GridlineColor|255|65280"
    c.Add "TextBox|ThemeFontIndex|0|1"
    c.Add "TextBox|ForeThemeColorIndex|1|2"
    c.Add "TextBox|BorderThemeColorIndex|1|2"
    c.Add "Label|ThemeFontIndex|0|1"
    c.Add "Label|ForeThemeColorIndex|1|2"
    c.Add "Label|GridlineColor|255|65280"
    c.Add "Button|WebImagePaddingLeft|11|12"
    c.Add "Button|WebImagePaddingTop|21|22"
    c.Add "Button|WebImagePaddingRight|31|32"
    c.Add "Button|WebImagePaddingBottom|41|42"
    c.Add "Button|Overlaps|True"
    c.Add "Detail|AlternateBackColor|255|65280"
    c.Add "Detail|AlternateBackThemeColorIndex|1|2"
    c.Add "ReportTextBox|Left|1700|2300"
    c.Add "ReportTextBox|Width|2100|2700"
    Set Specs = c
End Function

Public Sub CreatePropertyIdForms()
    CurrentDb.Execute "CREATE TABLE tblGenLog ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Result] TEXT(255))"
    CurrentDb.Execute "CREATE TABLE tblGenExport ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Content] MEMO)"

    Dim spec As Variant, parts() As String, i As Long
    MakeObject "Form", "", 0, ""
    MakeObject "Report", "", 0, ""
    For Each spec In Specs()
        parts = Split(spec, "|")
        For i = 2 To UBound(parts)
            MakeObject parts(0), parts(1), i - 1, parts(i)
        Next
    Next
    Debug.Print "done; see tblGenLog"
End Sub

Private Function IsReport(ByVal target As String) As Boolean
    IsReport = (target = "Report" Or target = "ReportTextBox")
End Function

' Creates the base form or report of `target` with `prop` set to `value`, or
' the base itself when `prop` is empty, and saves it under its name.
Private Sub MakeObject(ByVal target As String, ByVal prop As String, ByVal n As Long, ByVal value As String)
    Dim obj As Object, tmpName As String, objName As String, kind As String, result As String
    If prop = "" Then objName = "base_" & target Else objName = target & "_" & prop & "_" & n
    If IsReport(target) Then
        kind = "report"
        Set obj = NewReport()
    Else
        kind = "form"
        Set obj = NewForm()
    End If
    tmpName = obj.Name

    result = "ok"
    If prop <> "" And prop <> "Window" Then
        On Error Resume Next
        ApplyProperty obj, target, prop, value
        If Err.Number <> 0 Then result = "error " & Err.Number & ": " & Err.Description
        Err.Clear
        On Error GoTo 0
    End If

    If kind = "report" Then
        DoCmd.Close acReport, tmpName, acSaveYes
        DoCmd.Rename objName, acReport, tmpName
    Else
        DoCmd.Close acForm, tmpName, acSaveYes
        DoCmd.Rename objName, acForm, tmpName
    End If
    If prop = "Window" Then result = MoveWindow(kind, objName, value)
    LogResult kind, objName, prop & "=" & value & ": " & result
    ExportObject kind, objName
End Sub

' A form with a label, two text boxes and a button, open in design view.
Private Function NewForm() As Form
    Dim frm As Form, ctl As Control
    Set frm = CreateForm()
    Set ctl = CreateControl(frm.Name, acLabel, acDetail, , , 200, 300, 1200, 300)
    ctl.Name = "lblA"
    ctl.Caption = "Label"
    Set ctl = CreateControl(frm.Name, acTextBox, acDetail, , , 1500, 300, 2000, 300)
    ctl.Name = "txtA"
    Set ctl = CreateControl(frm.Name, acTextBox, acDetail, , , 1500, 800, 2000, 300)
    ctl.Name = "txtB"
    Set ctl = CreateControl(frm.Name, acCommandButton, acDetail, , , 200, 1300, 1500, 400)
    ctl.Name = "btnA"
    Set NewForm = frm
End Function

' A report with a label and a text box, open in design view.
Private Function NewReport() As Report
    Dim rpt As Report, ctl As Control
    Set rpt = CreateReport()
    Set ctl = CreateReportControl(rpt.Name, acLabel, acDetail, , "Label", 200, 300, 1200, 300)
    ctl.Name = "lblA"
    Set ctl = CreateReportControl(rpt.Name, acTextBox, acDetail, , , 1500, 300, 2000, 300)
    ctl.Name = "txtA"
    Set NewReport = rpt
End Function

Private Sub ApplyProperty(ByVal obj As Object, ByVal target As String, ByVal prop As String, ByVal value As String)
    Select Case target
    Case "Form", "Report"
        If prop = "ItemSuffix" Then
            AddAndDeleteControls obj, CLng(value), target = "Report"
        Else
            obj.Properties(prop) = ParseValue(value)
        End If
    Case "TextBox", "ReportTextBox"
        If prop = "TabIndex" Then
            obj.Controls("txtB").TabIndex = 0
        Else
            obj.Controls("txtA").Properties(prop) = ParseValue(value)
        End If
    Case "Label"
        obj.Controls("lblA").Properties(prop) = ParseValue(value)
    Case "Button"
        obj.Controls("btnA").Properties(prop) = ParseValue(value)
    Case "Detail"
        obj.Section(acDetail).Properties(prop) = ParseValue(value)
    End Select
End Sub

' Creates and deletes `count` controls, which moves ItemSuffix on.
Private Sub AddAndDeleteControls(ByVal obj As Object, ByVal count As Long, ByVal onReport As Boolean)
    Dim i As Long, ctl As Control
    For i = 1 To count
        If onReport Then
            Set ctl = CreateReportControl(obj.Name, acTextBox, acDetail, , , 4000, 300, 500, 300)
            DeleteReportControl obj.Name, ctl.Name
        Else
            Set ctl = CreateControl(obj.Name, acTextBox, acDetail, , , 4000, 300, 500, 300)
            DeleteControl obj.Name, ctl.Name
        End If
    Next
End Sub

' Moves the design window of the saved form or report to `value`
' (Left;Top;Width;Height) and saves it again.
Private Function MoveWindow(ByVal kind As String, ByVal objName As String, ByVal value As String) As String
    Dim v() As String
    v = Split(value, ";")
    On Error Resume Next
    If kind = "report" Then DoCmd.OpenReport objName, acViewDesign Else DoCmd.OpenForm objName, acDesign
    DoCmd.MoveSize CLng(v(0)), CLng(v(1)), CLng(v(2)), CLng(v(3))
    If kind = "report" Then DoCmd.Close acReport, objName, acSaveYes Else DoCmd.Close acForm, objName, acSaveYes
    If Err.Number <> 0 Then MoveWindow = "error " & Err.Number & ": " & Err.Description Else MoveWindow = "ok"
    Err.Clear
End Function

Private Function ParseValue(ByVal value As String) As Variant
    Select Case value
    Case "True": ParseValue = True
    Case "False": ParseValue = False
    Case Else: ParseValue = CLng(value)
    End Select
End Function

' Stores Access's SaveAsText output of the form or report in tblGenExport.
Private Sub ExportObject(ByVal kind As String, ByVal objName As String)
    Dim f As String, rs As DAO.Recordset
    f = Environ("TEMP") & "\jetdb_export_" & objName & ".txt"
    On Error Resume Next
    If kind = "report" Then Application.SaveAsText acReport, objName, f Else Application.SaveAsText acForm, objName, f
    If Err.Number <> 0 Then
        LogResult kind & "-export", objName, "error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    Set rs = CurrentDb.OpenRecordset("tblGenExport", dbOpenDynaset)
    rs.AddNew
    rs!Kind = kind
    rs!ObjName = objName
    rs!Content = ReadText(f)
    rs.Update
    rs.Close
    Kill f
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
