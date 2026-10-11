Attribute VB_Name = "VbaJapaneseGenerator"
' Creates a standard module and a class module with Japanese names and source
' for vbaJapanese*.accdb, and stores in tblVbaExpected the source Access holds
' for each. Import this module into a new, empty .accdb on Windows whose
' language for non-Unicode programs is Japanese (code page 932), and run
' CreateJapaneseVbaModules once from the Immediate window.
'
' This file is ASCII only, so that importing it does not depend on its
' encoding: the Japanese text is written with \uXXXX escapes, which J turns
' into the characters.
Option Compare Database
Option Explicit

Private Const STANDARD_MODULE As Long = 1 ' vbext_ct_StdModule
Private Const CLASS_MODULE As Long = 2 ' vbext_ct_ClassModule

Public Sub CreateJapaneseVbaModules()
    CurrentDb.Execute "CREATE TABLE tblVbaExpected ([ID] COUNTER PRIMARY KEY, [ModuleName] TEXT(64), [Source] MEMO)"

    ' A standard module: a Japanese comment with full-width letters,
    ' half-width katakana, and symbols whose code page 932 mapping differs
    ' between converters (a wave dash, a circled digit, an IBM extension
    ' kanji), and a function with a Japanese name, parameter and string.
    AddModule STANDARD_MODULE, J("\u65E5\u672C\u8A9E\u30E2\u30B8\u30E5\u30FC\u30EB"), Array( _
        "Option Compare Database", _
        "Option Explicit", _
        "", _
        J("' \u65E5\u672C\u8A9E\u306E\u30B3\u30E1\u30F3\u30C8\uFF1A\u5168\u89D2\uFF21\uFF22\uFF23\u3001\u534A\u89D2\uFF76\uFF85\u3001\u8A18\u53F7\uFF5E\u2460\u9AD9"), _
        J("Public Function \u3042\u3044\u3055\u3064(\u540D\u524D As String) As String"), _
        J("    \u3042\u3044\u3055\u3064 = ""\u3053\u3093\u306B\u3061\u306F\u3001"" & \u540D\u524D & ""\u3055\u3093"""), _
        "End Function")

    ' A class module with a Japanese property.
    AddModule CLASS_MODULE, J("\u9867\u5BA2\u30AF\u30E9\u30B9"), Array( _
        "Option Compare Database", _
        "Option Explicit", _
        "", _
        J("' \u9867\u5BA2\u306E\u6C0F\u540D\u3092\u6301\u3064"), _
        J("Private m\u6C0F\u540D As String"), _
        "", _
        J("Public Property Get \u6C0F\u540D() As String"), _
        J("    \u6C0F\u540D = m\u6C0F\u540D"), _
        "End Property", _
        "", _
        J("Public Property Let \u6C0F\u540D(\u5024 As String)"), _
        J("    m\u6C0F\u540D = \u5024"), _
        "End Property")

    DoCmd.RunCommand acCmdCompileAndSaveAllModules
    Debug.Print "Japanese VBA modules created"
End Sub

' Adds a module of kind named moduleName with lines as its source, and records the
' source Access then holds for it.
Private Sub AddModule(ByVal kind As Long, ByVal moduleName As String, ByVal lines As Variant)
    Dim component As Object, code As Object
    Set component = Application.VBE.ActiveVBProject.VBComponents.Add(kind)
    component.Name = moduleName
    Set code = component.CodeModule
    ' Access may start a new module with Option lines of its own.
    If code.CountOfLines > 0 Then code.DeleteLines 1, code.CountOfLines
    code.AddFromString Join(lines, vbCrLf)

    Dim rs As DAO.Recordset
    Set rs = CurrentDb.OpenRecordset("tblVbaExpected", dbOpenDynaset)
    rs.AddNew
    rs!ModuleName = moduleName
    rs!Source = code.lines(1, code.CountOfLines)
    rs.Update
    rs.Close
End Sub

' text with each \uXXXX escape turned into its character.
Private Function J(ByVal text As String) As String
    Dim i As Long, out As String
    i = 1
    Do While i <= Len(text)
        If Mid$(text, i, 2) = "\u" Then
            out = out & ChrW(CLng("&H" & Mid$(text, i + 2, 4)))
            i = i + 6
        Else
            out = out & Mid$(text, i, 1)
            i = i + 1
        End If
    Loop
    J = out
End Function
