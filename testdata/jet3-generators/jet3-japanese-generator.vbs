' Creates japaneseJet3.mdb, an Access 97 (Jet 3) database whose text is in
' code page 932: a table, its columns, an index, a column description and a
' saved query with Japanese names and values. Access 2013 and later cannot
' create Access 97 files, so it uses DAO 3.6, which comes with Windows.
'
' Run it once with the 32-bit script host, as DAO 3.6 is 32-bit:
'   C:\Windows\SysWOW64\cscript.exe //nologo jet3-japanese-generator.vbs
' The database is created next to this script. The Japanese strings are built
' from their code points (U), so the encoding of this file does not matter;
' DAO writes them in code page 932, the code page of the database.
Option Explicit

Const dbVersion30 = 32
Const dbLangJapanese = ";LANGID=0x0411;CP=932;COUNTRY=0"
Const dbLong = 4
Const dbText = 10
Const dbMemo = 12

' The string of the code points in `hex`, separated by spaces.
Function U(hex)
    Dim s, c
    s = ""
    For Each c In Split(hex, " ")
        s = s & ChrW(CLng("&H" & c))
    Next
    U = s
End Function

Dim fso, path, engine, db, tdf, fld, idx, rs
Set fso = CreateObject("Scripting.FileSystemObject")
path = fso.GetParentFolderName(WScript.ScriptFullName) & "\japaneseJet3.mdb"
If fso.FileExists(path) Then
    WScript.Echo "already exists: " & path
    WScript.Quit 1
End If

On Error Resume Next
Set engine = CreateObject("DAO.DBEngine.36")
If Err.Number <> 0 Then
    WScript.Echo "DAO 3.6 is not available (" & Err.Description & "). Run this with C:\Windows\SysWOW64\cscript.exe."
    WScript.Quit 1
End If
Set db = engine.CreateDatabase(path, dbLangJapanese, dbVersion30)
If Err.Number <> 0 Then
    WScript.Echo "CreateDatabase failed: " & Err.Description
    WScript.Quit 1
End If
On Error GoTo 0

' Table kokyaku (customers) with columns ID, shimei (name, a text column with a
' description) and bikou (notes, a memo column).
Set tdf = db.CreateTableDef(U("9867 5BA2"))
tdf.Fields.Append tdf.CreateField("ID", dbLong)
Set fld = tdf.CreateField(U("6C0F 540D"), dbText, 50)
tdf.Fields.Append fld
tdf.Fields.Append tdf.CreateField(U("5099 8003"), dbMemo)
' Index shimei-sakuin on shimei.
Set idx = tdf.CreateIndex(U("6C0F 540D 7D22 5F15"))
idx.Fields.Append idx.CreateField(U("6C0F 540D"))
tdf.Indexes.Append idx
db.TableDefs.Append tdf
Set fld = db.TableDefs(U("9867 5BA2")).Fields(U("6C0F 540D"))
fld.Properties.Append fld.CreateProperty("Description", dbText, U("9867 5BA2 306E 6C0F 540D"))

Set rs = db.OpenRecordset(U("9867 5BA2"))
' 1, Yamada Taro, Tokyo-to Chiyoda-ku
AddRow rs, 1, U("5C71 7530 592A 90CE"), U("6771 4EAC 90FD 5343 4EE3 7530 533A")
' 2, "test" in half-width katakana, and a circled 1, a tilde and a kanji of
' code page 932 that some converters map differently
AddRow rs, 2, U("FF83 FF7D FF84"), U("2460 FF5E 9AD9")
' 3, Smith, ASCII only
AddRow rs, 3, "Smith", "ASCII only"
rs.Close

' Query kokyaku-ichiran (customer list): the names that start with yama
db.CreateQueryDef U("9867 5BA2 4E00 89A7"), "SELECT " & U("6C0F 540D") & " FROM " & U("9867 5BA2") & _
    " WHERE " & U("6C0F 540D") & " LIKE '" & U("5C71") & "*'"

db.Close
WScript.Echo "created: " & path

Sub AddRow(rs, id, name, note)
    rs.AddNew
    rs.Fields("ID").Value = id
    rs.Fields(U("6C0F 540D")).Value = name
    rs.Fields(U("5099 8003")).Value = note
    rs.Update
End Sub
