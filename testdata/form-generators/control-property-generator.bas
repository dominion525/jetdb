Attribute VB_Name = "ControlPropertyGenerator"
' Creates, for each control type, ROUNDS forms and ROUNDS reports with one
' control of that type, and sets every property of the control, of the form
' or report itself and of its detail section that Access takes to a value of
' its own, for controlPropertyTest*.mdb. Saved in the Access 2000 format,
' comparing the design of each object with the values set shows which
' property ID of that format holds which property.
'
' The values: a string is z<round>_<index>; a number is the first of a few
' candidates Access takes that differs from the value it had; a Yes/No is
' turned over in the rounds a pattern of its index gives. The index is the
' place of the property in the Properties collection.
'
' tblGenSet records each value set, tblGenLog what was made and the
' properties Access did not take, and tblGenExport Access's SaveAsText output
' of each object.
'
' Import this module into a new, empty .accdb and run
' CreateControlPropertyForms from the Immediate window. Making some objects
' and reading or setting some properties makes Access stop; run it again
' after that, until it prints done: it goes on with the objects not yet made,
' and leaves out what it stopped on. It writes a line to jetdb_progress.txt in
' the temporary folder before making each object and before reading and
' setting each property, and it keeps what it leaves out for that in
' jetdb_skip.txt there.
Option Compare Database
Option Explicit

Private Const ROUNDS As Long = 3

' What Access stopped on, from jetdb_skip.txt: property names, and
' object:<name> for objects, each between line feeds.
Private stopped As String

' Control types, as name|constant. The unbound object frame is left out: Access
' asks for the object to insert when it is created.
Private Function ControlTypes() As Collection
    Dim c As New Collection
    c.Add "Label|100"
    c.Add "Rectangle|101"
    c.Add "Line|102"
    c.Add "Image|103"
    c.Add "CommandButton|104"
    c.Add "OptionButton|105"
    c.Add "CheckBox|106"
    c.Add "OptionGroup|107"
    c.Add "BoundObjectFrame|108"
    c.Add "TextBox|109"
    c.Add "ListBox|110"
    c.Add "ComboBox|111"
    c.Add "Subform|112"
    c.Add "PageBreak|118"
    c.Add "ToggleButton|122"
    c.Add "TabControl|123"
    Set ControlTypes = c
End Function

' Properties left as they are: what makes the object another object, binary
' data, what adds a module, and what Access stopped on.
Private Function Skipped(ByVal name As String) As Boolean
    Select Case name
    Case "Name", "ControlType", "Section", "InSelection", "HasModule", _
         "Picture", "PictureData", "PrtDevMode", "PrtDevNames", "PrtMip", _
         "Palette", "PaletteSource", "ObjectPalette", "OLEClass", "Class", _
         "SourceDoc", "SourceItem", "OLETypeAllowed", "Action", "Verb"
        Skipped = True
    Case Else
        Skipped = InStr(1, stopped, vbLf & name & vbLf, vbBinaryCompare) > 0
    End Select
End Function

Public Sub CreateControlPropertyForms()
    ' The tables are there already when this is run again.
    On Error Resume Next
    CurrentDb.Execute "CREATE TABLE tblGenLog ([ID] COUNTER PRIMARY KEY, [ObjName] TEXT(100), [Result] MEMO)"
    CurrentDb.Execute "CREATE TABLE tblGenSet ([ID] COUNTER PRIMARY KEY, [ObjName] TEXT(100), [Target] TEXT(20), [Prop] TEXT(100), [VarType] LONG, [Value] MEMO)"
    CurrentDb.Execute "CREATE TABLE tblGenExport ([ID] COUNTER PRIMARY KEY, [Kind] TEXT(20), [ObjName] TEXT(100), [Content] MEMO)"
    Err.Clear
    On Error GoTo 0
    LeaveOutStoppedProperty

    Dim spec As Variant, parts() As String, r As Long
    For Each spec In ControlTypes()
        parts = Split(spec, "|")
        For r = 1 To ROUNDS
            MakeObject False, parts(0), CLng(parts(1)), r
            MakeObject True, parts(0), CLng(parts(1)), r
        Next
    Next
    Debug.Print "done; see tblGenLog"
End Sub

' Makes the form (or report) F_<type>_<round> (R_...) with one control of
' `ctlType`, sets the properties and saves it.
Private Sub MakeObject(ByVal isReport As Boolean, ByVal typeName As String, ByVal ctlType As Long, ByVal r As Long)
    Dim obj As Object, ctl As Control, tmpName As String, objName As String, notes As String
    objName = IIf(isReport, "R_", "F_") & typeName & "_" & r
    If Exists(isReport, objName) Then Exit Sub
    If InStr(1, stopped, vbLf & "object:" & objName & vbLf, vbBinaryCompare) > 0 Then
        LogResult objName, "left out: Access stopped while making it"
        Exit Sub
    End If
    Progress "MAKE" & vbTab & objName

    If isReport Then Set obj = CreateReport() Else Set obj = CreateForm()
    tmpName = obj.Name
    On Error Resume Next
    If isReport Then
        Set ctl = CreateReportControl(tmpName, ctlType, acDetail, , , 500, 500, 2000, 600)
    Else
        Set ctl = CreateControl(tmpName, ctlType, acDetail, , , 500, 500, 2000, 600)
    End If
    If Err.Number <> 0 Then
        notes = "control not made: " & Err.Number & " " & Err.Description & vbCrLf
        Err.Clear
        Set ctl = Nothing
    End If
    On Error GoTo 0

    If Not ctl Is Nothing Then
        ctl.Name = "ctl"
        notes = notes & SetAll(ctl, "control", objName, r)
        If ctlType = 123 Then notes = notes & SetAll(ctl.Pages(0), "page", objName, r)
    End If
    notes = notes & SetAll(obj.Section(acDetail), "section", objName, r)
    notes = notes & SetAll(obj, IIf(isReport, "report", "form"), objName, r)

    If isReport Then
        DoCmd.Close acReport, tmpName, acSaveYes
        DoCmd.Rename objName, acReport, tmpName
    Else
        DoCmd.Close acForm, tmpName, acSaveYes
        DoCmd.Rename objName, acForm, tmpName
    End If
    LogResult objName, notes
    ExportObject isReport, objName
End Sub

' Sets each property of `obj` Access takes; returns a line of what it did.
Private Function SetAll(ByVal obj As Object, ByVal target As String, ByVal objName As String, ByVal r As Long) As String
    Dim p As Object, i As Long, setCount As Long, notSet As String
    For Each p In obj.Properties
        i = i + 1
        If Not Skipped(p.Name) Then
            Select Case TrySet(p, i, r, target, objName)
            Case 1: setCount = setCount + 1
            Case -1: notSet = notSet & p.Name & " "
            End Select
        End If
    Next
    SetAll = target & ": " & setCount & " set; not taken: " & notSet & vbCrLf
End Function

' Sets the property `p`, the i-th of its object, in round `r`: 1 when a value
' was set, 0 when it was left as it is on purpose, -1 when Access took none.
Private Function TrySet(ByVal p As Object, ByVal i As Long, ByVal r As Long, ByVal target As String, ByVal objName As String) As Long
    Dim orig As Variant, vt As Integer, c As Variant
    On Error Resume Next
    Progress "GET" & vbTab & objName & vbTab & target & vbTab & p.Name
    orig = p.Value
    Progress "OK"
    If Err.Number <> 0 Then
        Err.Clear
        TrySet = -1
        Exit Function
    End If
    vt = VarType(orig)
    If (vt And vbArray) <> 0 Then Exit Function

    If vt = vbBoolean Then
        If (((i Mod 7) + 1) And CLng(2 ^ (r - 1))) = 0 Then Exit Function
        TrySet = Assign(p, Not orig, orig, target, objName)
        Exit Function
    End If

    If vt = vbString Or vt = vbNull Or vt = vbEmpty Then
        TrySet = Assign(p, "z" & r & "_" & i, orig, target, objName)
        If TrySet = 1 Or vt = vbString Then Exit Function
    End If

    For Each c In Array(50 + i * 7 + r * 1000, r, r - 1, r + 1, 255, -1)
        TrySet = Assign(p, c, orig, target, objName)
        If TrySet = 1 Then Exit Function
    Next
End Function

' Sets `p` to `v` when Access takes it and it differs from `orig`, and
' records it: 1 when set, -1 otherwise.
Private Function Assign(ByVal p As Object, ByVal v As Variant, ByVal orig As Variant, ByVal target As String, ByVal objName As String) As Long
    Dim now_ As Variant, same As Boolean
    On Error Resume Next
    Err.Clear
    Progress "SET" & vbTab & objName & vbTab & target & vbTab & p.Name
    p.Value = v
    Progress "OK"
    If Err.Number <> 0 Then
        Err.Clear
        Assign = -1
        Exit Function
    End If
    now_ = p.Value
    same = False
    If Not IsNull(now_) And Not IsNull(orig) Then same = (CStr(now_) = CStr(orig))
    If same Then
        Assign = -1
        Exit Function
    End If
    Assign = 1
    Dim rs As DAO.Recordset
    Set rs = CurrentDb.OpenRecordset("tblGenSet", dbOpenDynaset)
    rs.AddNew
    rs!ObjName = objName
    rs!Target = target
    rs!Prop = p.Name
    rs!VarType = VarType(now_)
    rs!Value = CStr(Nz(now_, ""))
    rs.Update
    rs.Close
End Function

' Stores Access's SaveAsText output of the object in tblGenExport.
Private Sub ExportObject(ByVal isReport As Boolean, ByVal objName As String)
    Dim f As String, rs As DAO.Recordset
    f = Environ("TEMP") & "\jetdb_export_" & objName & ".txt"
    On Error Resume Next
    If isReport Then Application.SaveAsText acReport, objName, f Else Application.SaveAsText acForm, objName, f
    If Err.Number <> 0 Then
        LogResult objName, "export error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    Set rs = CurrentDb.OpenRecordset("tblGenExport", dbOpenDynaset)
    rs.AddNew
    rs!Kind = IIf(isReport, "report", "form")
    rs!ObjName = objName
    rs!Content = ReadText(f)
    rs.Update
    rs.Close
    Kill f
End Sub

' Whether the form (or report) `objName` is there.
Private Function Exists(ByVal isReport As Boolean, ByVal objName As String) As Boolean
    Dim item As Object
    On Error Resume Next
    If isReport Then Set item = CurrentProject.AllReports(objName) Else Set item = CurrentProject.AllForms(objName)
    Exists = (Err.Number = 0)
    Err.Clear
End Function

Private Function TempPath(ByVal name As String) As String
    TempPath = Environ("TEMP") & "\" & name
End Function

' Appends `line` to jetdb_progress.txt, closing it so the line stays when
' Access stops.
Private Sub Progress(ByVal line As String)
    Dim fso As Object, ts As Object
    Set fso = CreateObject("Scripting.FileSystemObject")
    Set ts = fso.OpenTextFile(TempPath("jetdb_progress.txt"), 8, True)
    ts.WriteLine line
    ts.Close
End Sub

Private Sub AddToSkip(ByVal fso As Object, ByVal line As String)
    Dim ts As Object
    Set ts = fso.OpenTextFile(TempPath("jetdb_skip.txt"), 8, True)
    ts.WriteLine line
    ts.Close
End Sub

' When jetdb_progress.txt ends while making an object, or reading or setting
' a property, adds that object or property to jetdb_skip.txt; then reads
' jetdb_skip.txt into `stopped`.
Private Sub LeaveOutStoppedProperty()
    Dim fso As Object, ts As Object, text As String, pos As Long, last As String, fields() As String
    Set fso = CreateObject("Scripting.FileSystemObject")
    If fso.FileExists(TempPath("jetdb_progress.txt")) Then
        Set ts = fso.OpenTextFile(TempPath("jetdb_progress.txt"), 1)
        If Not ts.AtEndOfStream Then text = ts.ReadAll
        ts.Close
        If Right$(text, 2) = vbCrLf Then text = Left$(text, Len(text) - 2)
        pos = InStrRev(text, vbCrLf)
        last = Mid$(text, IIf(pos > 0, pos + 2, 1)) & vbTab & vbTab & vbTab
        fields = Split(last, vbTab)
        If fields(0) = "GET" Or fields(0) = "SET" Then
            AddToSkip fso, fields(3)
            LogResult fields(1), "Access stopped on " & fields(0) & " " & fields(2) & " " & fields(3) & "; left out from now on"
        ElseIf fields(0) = "MAKE" Then
            AddToSkip fso, "object:" & fields(1)
        End If
        fso.DeleteFile TempPath("jetdb_progress.txt")
    End If
    stopped = vbLf
    If fso.FileExists(TempPath("jetdb_skip.txt")) Then
        Set ts = fso.OpenTextFile(TempPath("jetdb_skip.txt"), 1)
        If Not ts.AtEndOfStream Then stopped = vbLf & Replace(ts.ReadAll, vbCrLf, vbLf)
        ts.Close
    End If
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

Private Sub LogResult(ByVal name As String, ByVal result As String)
    Dim rs As DAO.Recordset
    Set rs = CurrentDb.OpenRecordset("tblGenLog", dbOpenDynaset)
    rs.AddNew
    rs!ObjName = name
    rs!Result = result
    rs.Update
    rs.Close
End Sub
