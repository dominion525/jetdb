Attribute VB_Name = "LpSampleGenerator"
' Creates jetdb-sample.accdb, the sample database whose command output the
' jetdb site shows: a small sales application with tables, relationships, a
' saved query, a standard module, a form with its own module, and a macro.
'
' Import this module into a new, empty .accdb and run CreateLpSample once from
' the Immediate window. The sample is created as a separate file in the same
' folder, by a second Access instance, so that neither this module nor the log
' ends up in the sample. Each step is recorded in tblLpSampleLog in this
' database and printed to the Immediate window; a failed step does not stop the
' steps after it.
Option Compare Database
Option Explicit

Private Const SAMPLE_FILE As String = "jetdb-sample.accdb"
Private Const STANDARD_MODULE As Long = 1 ' vbext_ct_StdModule
Private Const FORMAT_ACCESS2007 As Long = 12 ' acNewDatabaseFormatAccess2007 (the 2007-2016 format)
Private Const NS As String = " xmlns='http://schemas.microsoft.com/office/accessservices/2009/11/application'"

Private app As Object ' Access.Application that holds the sample open
Private db As DAO.Database ' the sample, as app sees it

Public Sub CreateLpSample()
    Dim path As String
    path = CurrentProject.Path & "\" & SAMPLE_FILE
    If Dir(path) <> "" Then
        Debug.Print SAMPLE_FILE & " already exists; delete or move it first. Nothing was created."
        Exit Sub
    End If

    On Error Resume Next
    CurrentDb.Execute "CREATE TABLE tblLpSampleLog ([ID] COUNTER PRIMARY KEY, [Step] TEXT(100), [Result] TEXT(255))"
    Err.Clear
    On Error GoTo 0

    Set app = CreateObject("Access.Application")
    ' Visible, so that a dialog Access shows unexpectedly can be seen and closed.
    app.Visible = True
    app.NewCurrentDatabase path, FORMAT_ACCESS2007
    Set db = app.CurrentDb

    RunStep "table tblCustomers"
    RunStep "table tblProducts"
    RunStep "table tblOrders"
    RunStep "relationship fkOrdersCustomers"
    RunStep "relationship fkOrdersProducts"
    RunStep "descriptions"
    RunStep "rows tblCustomers"
    RunStep "rows tblProducts"
    RunStep "rows tblOrders"
    RunStep "query qryMonthlySales"
    RunStep "module modBilling"
    RunStep "form frmOrders"
    RunStep "macro mcrOpenOrders"
    RunStep "compile"
    RunStep "save module modBilling"

    Set db = Nothing
    app.CloseCurrentDatabase
    app.Quit
    Set app = Nothing
    Debug.Print "done: " & path
End Sub

' Runs one step and records its result. An error inside the step ends only that step.
Private Sub RunStep(ByVal stepName As String)
    On Error GoTo Failed
    Select Case stepName
        Case "table tblCustomers"
            db.Execute "CREATE TABLE tblCustomers (" & _
                "CustomerID COUNTER CONSTRAINT pkCustomers PRIMARY KEY, " & _
                "CustomerName TEXT(50) NOT NULL, " & _
                "Email TEXT(100), " & _
                "CreatedAt DATETIME)", dbFailOnError
        Case "table tblProducts"
            db.Execute "CREATE TABLE tblProducts (" & _
                "ProductID COUNTER CONSTRAINT pkProducts PRIMARY KEY, " & _
                "ProductName TEXT(50) NOT NULL, " & _
                "UnitPrice CURRENCY, " & _
                "Discontinued BIT)", dbFailOnError
        Case "table tblOrders"
            db.Execute "CREATE TABLE tblOrders (" & _
                "OrderID COUNTER CONSTRAINT pkOrders PRIMARY KEY, " & _
                "CustomerID LONG NOT NULL, " & _
                "ProductID LONG NOT NULL, " & _
                "Quantity SMALLINT NOT NULL, " & _
                "OrderDate DATETIME NOT NULL, " & _
                "Shipped BIT)", dbFailOnError
        Case "relationship fkOrdersCustomers"
            db.Execute "ALTER TABLE tblOrders ADD CONSTRAINT fkOrdersCustomers " & _
                "FOREIGN KEY (CustomerID) REFERENCES tblCustomers (CustomerID)", dbFailOnError
        Case "relationship fkOrdersProducts"
            db.Execute "ALTER TABLE tblOrders ADD CONSTRAINT fkOrdersProducts " & _
                "FOREIGN KEY (ProductID) REFERENCES tblProducts (ProductID)", dbFailOnError
        Case "descriptions"
            AddDescriptions
        Case "rows tblCustomers"
            db.Execute "INSERT INTO tblCustomers (CustomerName, Email, CreatedAt) VALUES ('Northwind Traders', 'orders@northwind.example', #2026-04-01 09:30:00#)", dbFailOnError
            db.Execute "INSERT INTO tblCustomers (CustomerName, Email, CreatedAt) VALUES ('Contoso Ltd.', 'buyer@contoso.example', #2026-05-12 14:05:00#)", dbFailOnError
            db.Execute "INSERT INTO tblCustomers (CustomerName, Email, CreatedAt) VALUES ('Fabrikam Inc.', NULL, #2026-06-20 11:00:00#)", dbFailOnError
        Case "rows tblProducts"
            db.Execute "INSERT INTO tblProducts (ProductName, UnitPrice, Discontinued) VALUES ('Coffee Beans 1kg', 2400, False)", dbFailOnError
            db.Execute "INSERT INTO tblProducts (ProductName, UnitPrice, Discontinued) VALUES ('Paper Filters (100)', 380, False)", dbFailOnError
            db.Execute "INSERT INTO tblProducts (ProductName, UnitPrice, Discontinued) VALUES ('Hand Grinder', 6800, True)", dbFailOnError
        Case "rows tblOrders"
            db.Execute "INSERT INTO tblOrders (CustomerID, ProductID, Quantity, OrderDate, Shipped) VALUES (1, 1, 3, #2026-07-03#, True)", dbFailOnError
            db.Execute "INSERT INTO tblOrders (CustomerID, ProductID, Quantity, OrderDate, Shipped) VALUES (2, 2, 10, #2026-07-18#, True)", dbFailOnError
            db.Execute "INSERT INTO tblOrders (CustomerID, ProductID, Quantity, OrderDate, Shipped) VALUES (1, 3, 1, #2026-08-02#, True)", dbFailOnError
            db.Execute "INSERT INTO tblOrders (CustomerID, ProductID, Quantity, OrderDate, Shipped) VALUES (3, 1, 2, #2026-08-25#, False)", dbFailOnError
            db.Execute "INSERT INTO tblOrders (CustomerID, ProductID, Quantity, OrderDate, Shipped) VALUES (2, 1, 5, #2026-09-09#, False)", dbFailOnError
        Case "query qryMonthlySales"
            db.CreateQueryDef "qryMonthlySales", _
                "SELECT Format(tblOrders.OrderDate, 'yyyy-mm') AS SalesMonth, tblCustomers.CustomerName, " & _
                "Sum(tblOrders.Quantity * tblProducts.UnitPrice) AS Total " & _
                "FROM (tblOrders INNER JOIN tblCustomers ON tblOrders.CustomerID = tblCustomers.CustomerID) " & _
                "INNER JOIN tblProducts ON tblOrders.ProductID = tblProducts.ProductID " & _
                "GROUP BY Format(tblOrders.OrderDate, 'yyyy-mm'), tblCustomers.CustomerName " & _
                "ORDER BY Format(tblOrders.OrderDate, 'yyyy-mm'), tblCustomers.CustomerName;"
        Case "module modBilling"
            AddStandardModule "modBilling", Array( _
                "Option Compare Database", _
                "Option Explicit", _
                "", _
                "Public Const TAX_RATE As Double = 0.1", _
                "", _
                "' Returns the amount of one order line, tax included.", _
                "Public Function LineTotal(ByVal quantity As Long, ByVal unitPrice As Currency) As Currency", _
                "    LineTotal = quantity * unitPrice * (1 + TAX_RATE)", _
                "End Function", _
                "", _
                "' Returns the amount of an order, tax included, or 0 if there is no such order.", _
                "Public Function OrderTotal(ByVal orderId As Long) As Currency", _
                "    Dim rs As DAO.Recordset", _
                "    Set rs = CurrentDb.OpenRecordset( _", _
                "        ""SELECT o.Quantity, p.UnitPrice FROM tblOrders AS o INNER JOIN tblProducts AS p "" & _", _
                "        ""ON o.ProductID = p.ProductID WHERE o.OrderID = "" & orderId)", _
                "    If Not rs.EOF Then OrderTotal = LineTotal(rs!Quantity, rs!UnitPrice)", _
                "    rs.Close", _
                "End Function")
        Case "form frmOrders"
            CreateOrdersForm
        Case "macro mcrOpenOrders"
            LoadMacro "mcrOpenOrders", "<UserInterfaceMacro" & NS & "><Statements>" & _
                "<Action Name='OpenForm'><Argument Name='FormName'>frmOrders</Argument></Action>" & _
                "<Action Name='MessageBox'><Argument Name='Message'>Showing the orders.</Argument><Argument Name='Type'>Information</Argument><Argument Name='Title'>Sales</Argument></Action>" & _
                "</Statements></UserInterfaceMacro>"
        Case "compile"
            ' Compiling needs the VBA editor open, which an automated instance does not have yet.
            app.DoCmd.OpenModule "modBilling"
            app.DoCmd.RunCommand acCmdCompileAndSaveAllModules
        Case "save module modBilling"
            ' A separate step, so that the module is saved even if compiling failed.
            app.DoCmd.Close acModule, "modBilling", acSaveYes
        Case Else
            Err.Raise vbObjectError + 1, , "unknown step"
    End Select
    LogResult stepName, "ok"
    Exit Sub
Failed:
    LogResult stepName, "error " & Err.Number & ": " & Err.Description
End Sub

' Descriptions of the tables and their columns, which Access keeps as properties.
Private Sub AddDescriptions()
    SetDescription "tblCustomers", "", "Customers who place orders"
    SetDescription "tblCustomers", "CustomerName", "Company name"
    SetDescription "tblCustomers", "Email", "Address for order confirmations"
    SetDescription "tblProducts", "", "Products for sale"
    SetDescription "tblProducts", "UnitPrice", "Price per unit, tax excluded"
    SetDescription "tblProducts", "Discontinued", "No longer sold"
    SetDescription "tblOrders", "", "One row per ordered product"
    SetDescription "tblOrders", "Quantity", "Number of units"
    SetDescription "tblOrders", "Shipped", "Sent to the customer"
End Sub

' Sets the Description of a table, or of one of its columns when columnName is not empty.
Private Sub SetDescription(ByVal tableName As String, ByVal columnName As String, ByVal text As String)
    Dim tdf As DAO.TableDef, owner As Object
    Set tdf = db.TableDefs(tableName)
    If columnName = "" Then Set owner = tdf Else Set owner = tdf.Fields(columnName)
    owner.Properties.Append owner.CreateProperty("Description", dbText, text)
End Sub

' Adds a standard module named moduleName with lines as its source.
Private Sub AddStandardModule(ByVal moduleName As String, ByVal lines As Variant)
    Dim component As Object, code As Object
    Set component = app.VBE.ActiveVBProject.VBComponents.Add(STANDARD_MODULE)
    component.Name = moduleName
    Set code = component.CodeModule
    ' Access may start a new module with Option lines of its own.
    If code.CountOfLines > 0 Then code.DeleteLines 1, code.CountOfLines
    code.AddFromString Join(lines, vbCrLf)
End Sub

' Creates frmOrders on tblOrders: a labelled text box for each column, and a button
' whose click is handled in the form's own module.
Private Sub CreateOrdersForm()
    Dim frm As Object, ctl As Object, lbl As Object, tmpName As String
    Dim columns As Variant, i As Long, top As Long

    Set frm = app.CreateForm()
    frm.RecordSource = "tblOrders"
    frm.Caption = "Orders"

    columns = Array("OrderID", "CustomerID", "ProductID", "Quantity", "OrderDate", "Shipped")
    top = 300
    For i = LBound(columns) To UBound(columns)
        If columns(i) = "Shipped" Then
            Set ctl = app.CreateControl(frm.Name, acCheckBox, acDetail, , columns(i), 1800, top)
            ctl.Name = "chk" & columns(i)
        Else
            Set ctl = app.CreateControl(frm.Name, acTextBox, acDetail, , columns(i), 1800, top, 2400, 300)
            ctl.Name = "txt" & columns(i)
        End If
        Set lbl = app.CreateControl(frm.Name, acLabel, acDetail, ctl.Name, , 300, top, 1400, 300)
        lbl.Name = "lbl" & columns(i)
        lbl.Caption = columns(i)
        top = top + 420
    Next

    Set ctl = app.CreateControl(frm.Name, acCommandButton, acDetail, , , 1800, top + 120, 2400, 400)
    ctl.Name = "btnShowTotal"
    ctl.Caption = "Show total"
    ctl.OnClick = "[Event Procedure]"

    frm.HasModule = True
    frm.Module.AddFromString Join(Array( _
        "Private Sub btnShowTotal_Click()", _
        "    If IsNull(Me!OrderID) Then Exit Sub", _
        "    MsgBox ""Total: "" & Format(OrderTotal(Me!OrderID), ""Currency""), vbInformation, ""Orders""", _
        "End Sub"), vbCrLf)

    tmpName = frm.Name
    app.DoCmd.Close acForm, tmpName, acSaveYes
    app.DoCmd.Rename "frmOrders", acForm, tmpName
End Sub

' Builds a SaveAsText-style macro file whose content is the XML as _AXL: comments,
' and loads it into the sample. The XML uses single quotes so that no escaping is
' needed inside the text file.
Private Sub LoadMacro(ByVal name As String, ByVal xml As String)
    Const CHUNK As Long = 200
    Dim s As String, i As Long, f As String
    xml = "<?xml version='1.0' encoding='UTF-16' standalone='no'?>" & xml
    s = "Version =196611" & vbCrLf & "ColumnsShown =0" & vbCrLf
    For i = 1 To Len(xml) Step CHUNK
        s = s & "Begin" & vbCrLf & "    Comment =""_AXL:" & Mid(xml, i, CHUNK) & """" & vbCrLf & "End" & vbCrLf
    Next
    f = Environ("TEMP") & "\jetdb_lp_" & name & ".txt"
    WriteUnicode f, s
    app.LoadFromText acMacro, name, f
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

Private Sub LogResult(ByVal stepName As String, ByVal result As String)
    Dim rs As DAO.Recordset
    Debug.Print stepName & ": " & result
    Set rs = CurrentDb.OpenRecordset("tblLpSampleLog", dbOpenDynaset)
    rs.AddNew
    rs!Step = stepName
    rs!Result = Left(result, 255)
    rs.Update
    rs.Close
End Sub
