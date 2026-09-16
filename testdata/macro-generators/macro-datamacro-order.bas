Option Compare Database
Option Explicit

' Run ProbeDataMacroOrder once in the database used by ProbeMacroActions.
' Loads data macros onto two new tables in scrambled orders and stores Access's
' SaveAsText output of each table's data macros in tblGenExport (kind datamacro-order).
' No macro is run.

Private Const NS As String = " xmlns='http://schemas.microsoft.com/office/accessservices/2009/11/application'"

Public Sub ProbeDataMacroOrder()
    CurrentDb.Execute "CREATE TABLE tblOrderA ([ID] COUNTER PRIMARY KEY, [Memo1] TEXT(50))"
    CurrentDb.Execute "CREATE TABLE tblOrderB ([ID] COUNTER PRIMARY KEY, [Memo1] TEXT(50))"

    LoadDataMacros "tblOrderA", Array("Name:zzNamed", "Event:AfterUpdate", "Event:BeforeDelete", _
        "Name:aaNamed", "Event:AfterDelete", "Event:BeforeChange", "Event:AfterInsert")
    LoadDataMacros "tblOrderB", Array("Event:AfterInsert", "Name:mmNamed", "Event:BeforeChange", _
        "Event:AfterDelete", "Name:bbNamed", "Event:BeforeDelete", "Event:AfterUpdate")

    Debug.Print "done"
End Sub

' Each item is "Event:<event>" or "Name:<name>", in the order the XML lists them.
Private Sub LoadDataMacros(ByVal table As String, ByVal items As Variant)
    Dim xml As String, item As Variant, f As String, parts() As String
    xml = "<?xml version='1.0' encoding='UTF-16' standalone='no'?><DataMacros" & NS & ">"
    For Each item In items
        parts = Split(item, ":")
        xml = xml & "<DataMacro " & parts(0) & "='" & parts(1) & "'><Statements>" & _
            "<Action Name='SetLocalVar'><Argument Name='Name'>v</Argument><Argument Name='Expression'>1</Argument></Action>" & _
            "</Statements></DataMacro>"
    Next
    xml = xml & "</DataMacros>"

    f = Environ("TEMP") & "\jetdb_order_" & table & ".txt"
    WriteUnicode f, xml
    On Error Resume Next
    Application.LoadFromText acTableDataMacro, table, f
    If Err.Number <> 0 Then
        LogResult "datamacro-order", table, "load error " & Err.Number & ": " & Err.Description
        Err.Clear
        On Error GoTo 0
        Kill f
        Exit Sub
    End If
    On Error GoTo 0
    Kill f
    LogResult "datamacro-order", table, "ok"

    f = Environ("TEMP") & "\jetdb_order_export_" & table & ".txt"
    On Error Resume Next
    Application.SaveAsText acTableDataMacro, table, f
    If Err.Number <> 0 Then
        LogResult "datamacro-order-export", table, "error " & Err.Number & ": " & Err.Description
        Err.Clear
        Exit Sub
    End If
    On Error GoTo 0
    Dim rs As DAO.Recordset
    Set rs = CurrentDb.OpenRecordset("tblGenExport", dbOpenDynaset)
    rs.AddNew
    rs!Kind = "datamacro-order"
    rs!ObjName = table
    rs!Content = ReadText(f)
    rs.Update
    rs.Close
    Kill f
End Sub

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
