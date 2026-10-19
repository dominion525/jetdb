Attribute VB_Name = "SaveAsTextExporter"
' Writes Access's SaveAsText output of every form and report of the open
' database to a folder next to it, <database file name>_saveastext, with the
' forms under forms\ and the reports under reports\. It changes no form or
' report.
'
' Import this module into a copy of the database and run ExportSaveAsText
' from the Immediate window. It prints done and the number of objects at the
' end.
Option Compare Database
Option Explicit

Public Sub ExportSaveAsText()
    Dim root As String, obj As AccessObject, n As Long
    root = CurrentProject.Path & "\" & CurrentProject.Name & "_saveastext"
    MakeDir root
    MakeDir root & "\forms"
    MakeDir root & "\reports"
    For Each obj In CurrentProject.AllForms
        Application.SaveAsText acForm, obj.Name, root & "\forms\" & obj.Name & ".txt"
        n = n + 1
    Next
    For Each obj In CurrentProject.AllReports
        Application.SaveAsText acReport, obj.Name, root & "\reports\" & obj.Name & ".txt"
        n = n + 1
    Next
    Debug.Print "done: " & n & " objects to " & root
End Sub

Private Sub MakeDir(ByVal path As String)
    If Dir(path, vbDirectory) = "" Then MkDir path
End Sub
