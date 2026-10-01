Attribute VB_Name = "BigIntGenerator"
' Creates BigIntTable for bigIntTest*.accdb: a Long ID column and a Large
' Number (BigInt) column. Import this module into a new, empty .accdb and run
' CreateBigIntTestTable once from the Immediate window.
Option Compare Database
Option Explicit

Public Sub CreateBigIntTestTable()
    Dim db As DAO.Database
    Dim td As DAO.TableDef

    Set db = CurrentDb
    Set td = db.CreateTableDef("BigIntTable")
    td.Fields.Append td.CreateField("ID", dbLong)
    td.Fields.Append td.CreateField("Big", dbBigInt)
    db.TableDefs.Append td

    ' 2^53 + 1 is not exactly a Double, so these rows show whether a reader
    ' keeps every digit. Row 4 leaves Big empty (NULL).
    db.Execute "INSERT INTO BigIntTable (ID, Big) VALUES (1, 9007199254740993)", dbFailOnError
    db.Execute "INSERT INTO BigIntTable (ID, Big) VALUES (2, -9007199254740993)", dbFailOnError
    db.Execute "INSERT INTO BigIntTable (ID, Big) VALUES (3, 0)", dbFailOnError
    db.Execute "INSERT INTO BigIntTable (ID) VALUES (4)", dbFailOnError

    Debug.Print "BigIntTable created"
End Sub
