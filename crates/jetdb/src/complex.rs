//! The values of complex columns: multiple values and the version history of
//! append-only Long Text columns (Access 2007 and later).
//!
//! A complex column holds a number in each row. Its values are rows of a
//! hidden "flat" table, whose Long column that is not an AutoNumber holds
//! that number; the columns holding the values are those named as in the
//! column's "type" table (`MSysComplexType_*`), and the kind of the values
//! follows from the types of those columns. The row of MSysComplexColumns
//! whose `ComplexID` is in the column definition names both tables.

use std::collections::BTreeMap;

use crate::catalog::{find_table, CatalogEntry};
use crate::data::{read_table_rows_with, Value};
use crate::file::{FileError, PageReader};
use crate::format::{ColumnType, ObjectType};
use crate::table::{read_table_def, ColumnDef, TableDef};

/// One version of an append-only Long Text column.
#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    /// The text of the column in this version.
    pub value: Value,
    /// When the version was written, as a [`Value::Timestamp`].
    pub modified: Value,
}

/// The values of one complex column, by the number the column holds in each
/// row. A number with no values is not in the map.
#[derive(Debug, Clone, PartialEq)]
pub enum ComplexValues {
    /// Multiple values of the column's value type, in the order Access
    /// stored them.
    MultiValue(BTreeMap<i32, Vec<Value>>),
    /// The versions of an append-only Long Text column, oldest first.
    VersionHistory(BTreeMap<i32, Vec<Version>>),
    /// A kind of complex column jetdb does not read, such as attachments,
    /// with the name of its type table.
    Unknown { type_table: String },
}

/// Read the values of the complex column `column` of the table `table`,
/// or return `None` if it is not a complex column.
///
/// The values are keyed by the number the column holds in each row, as
/// [`read_table_rows`](crate::read_table_rows) returns it ([`Value::Long`]).
pub fn read_complex_values(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
    table: &TableDef,
    column: &ColumnDef,
) -> Result<Option<ComplexValues>, FileError> {
    let Some(complex_id) = column.complex_id else {
        return Ok(None);
    };
    let link = find_link(reader, catalog, complex_id)?;
    if link.table_id != find_table(catalog, &table.name)?.table_page {
        return Err(FileError::InvalidTableDef {
            reason: "MSysComplexColumns links the complex column to another table",
        });
    }
    let type_def = read_table_by_id(reader, catalog, link.type_table_id)?;
    let flat = FlatTable::new(
        &type_def,
        read_table_by_id(reader, catalog, link.flat_table_id)?,
    )?;
    let rows = flat.read_rows_by_number(reader)?;
    Ok(Some(values_of_kind(type_def, &flat, rows)))
}

/// The flat table of a complex column, with the positions of its columns.
struct FlatTable {
    def: TableDef,
    /// The columns named as in the type table, which hold the values.
    value_columns: Vec<usize>,
    /// The Long column that holds the number of the row of the main table.
    key_column: usize,
    /// The AutoNumber column, in the order Access added the values.
    order_column: Option<usize>,
}

impl FlatTable {
    fn new(type_def: &TableDef, def: TableDef) -> Result<Self, FileError> {
        let value_columns: Vec<usize> = (0..def.columns.len())
            .filter(|&i| {
                type_def
                    .columns
                    .iter()
                    .any(|t| t.name == def.columns[i].name)
            })
            .collect();
        let key_column = (0..def.columns.len())
            .find(|&i| {
                let c = &def.columns[i];
                !value_columns.contains(&i) && c.col_type == ColumnType::Long && !c.is_auto_number()
            })
            .ok_or(FileError::InvalidTableDef {
                reason: "flat table of a complex column has no column for the row number",
            })?;
        let order_column = def.columns.iter().position(|c| c.is_auto_number());
        Ok(Self {
            def,
            value_columns,
            key_column,
            order_column,
        })
    }

    /// The value column of type `t`.
    fn value_column_of_type(&self, t: ColumnType) -> Option<usize> {
        self.value_columns
            .iter()
            .copied()
            .find(|&i| self.def.columns[i].col_type == t)
    }

    /// The rows of the table in the order Access added them, grouped by the
    /// number of the row of the main table.
    fn read_rows_by_number(
        &self,
        reader: &mut PageReader,
    ) -> Result<BTreeMap<i32, Vec<Vec<Value>>>, FileError> {
        let result = read_table_rows_with(reader, &self.def, &Default::default())?;
        result.warn_skipped(&self.def.name);
        let mut rows = result.rows;
        if let Some(i) = self.order_column {
            rows.sort_by_key(|row| long_at(row, i).unwrap_or(i32::MAX));
        }
        let mut grouped: BTreeMap<i32, Vec<Vec<Value>>> = BTreeMap::new();
        for row in rows {
            if let Some(number) = long_at(&row, self.key_column) {
                grouped.entry(number).or_default().push(row);
            }
        }
        Ok(grouped)
    }
}

/// The values of the rows of a flat table, as the kind the type table's
/// columns show.
fn values_of_kind(
    type_def: TableDef,
    flat: &FlatTable,
    rows: BTreeMap<i32, Vec<Vec<Value>>>,
) -> ComplexValues {
    let types: Vec<ColumnType> = type_def.columns.iter().map(|c| c.col_type).collect();
    if let ([value_type], [value]) = (types.as_slice(), flat.value_columns.as_slice()) {
        if is_multi_value_type(*value_type) {
            return ComplexValues::MultiValue(map_rows(rows, |row| take(row, *value)));
        }
    }
    let memo = flat.value_column_of_type(ColumnType::Memo);
    let timestamp = flat.value_column_of_type(ColumnType::Timestamp);
    if let (false, Some(value), Some(modified)) = (is_attachment_type(&types), memo, timestamp) {
        return ComplexValues::VersionHistory(map_rows(rows, |row| Version {
            value: take(row, value),
            modified: take(row, modified),
        }));
    }
    ComplexValues::Unknown {
        type_table: type_def.name,
    }
}

/// `rows` with each row made into a value by `f`.
fn map_rows<T>(
    rows: BTreeMap<i32, Vec<Vec<Value>>>,
    f: impl Fn(&mut Vec<Value>) -> T,
) -> BTreeMap<i32, Vec<T>> {
    rows.into_iter()
        .map(|(number, rows)| {
            (
                number,
                rows.into_iter().map(|mut row| f(&mut row)).collect(),
            )
        })
        .collect()
}

/// The value of column `i` of `row`, leaving `Null` in its place.
fn take(row: &mut [Value], i: usize) -> Value {
    std::mem::replace(&mut row[i], Value::Null)
}

/// The value of column `i` of `row`, if it is a Long.
fn long_at(row: &[Value], i: usize) -> Option<i32> {
    match row.get(i) {
        Some(Value::Long(n)) => Some(*n),
        _ => None,
    }
}

/// The value types a column of multiple values may have.
fn is_multi_value_type(t: ColumnType) -> bool {
    matches!(
        t,
        ColumnType::Byte
            | ColumnType::Int
            | ColumnType::Long
            | ColumnType::Float
            | ColumnType::Double
            | ColumnType::Guid
            | ColumnType::Numeric
            | ColumnType::Text
            | ColumnType::BigInt
    )
}

/// Whether the type table has the columns of an attachment: FileURL (Long
/// Text), FileName and FileType (Text), FileData (OLE Object),
/// FileTimeStamp (Date/Time) and FileFlags (Long).
fn is_attachment_type(types: &[ColumnType]) -> bool {
    let count = |t: ColumnType| types.iter().filter(|&&c| c == t).count();
    count(ColumnType::Memo) >= 1
        && count(ColumnType::Text) >= 2
        && count(ColumnType::Ole) >= 1
        && count(ColumnType::Timestamp) >= 1
        && count(ColumnType::Long) >= 1
}

/// The tables a row of MSysComplexColumns names, by their MSysObjects IDs.
struct ComplexLink {
    table_id: u32,
    flat_table_id: u32,
    type_table_id: u32,
}

/// The row of MSysComplexColumns whose `ComplexID` is `complex_id`.
fn find_link(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
    complex_id: u32,
) -> Result<ComplexLink, FileError> {
    let entry = find_table(catalog, "MSysComplexColumns")?;
    let tdef = read_table_def(reader, &entry.name, entry.table_page)?;
    let [id, table, flat, type_table] = [
        "ComplexID",
        "ConceptualTableID",
        "FlatTableID",
        "ComplexTypeObjectID",
    ]
    .map(|name| tdef.columns.iter().position(|c| c.name == name));
    let rows = read_table_rows_with(reader, &tdef, &Default::default())?;
    rows.warn_skipped(&tdef.name);
    let id_at = |row: &[Value], i: Option<usize>| {
        i.and_then(|i| long_at(row, i))
            .and_then(|n| u32::try_from(n).ok())
    };
    rows.rows
        .iter()
        .find(|row| id_at(row, id) == Some(complex_id))
        .and_then(|row| {
            Some(ComplexLink {
                table_id: id_at(row, table)?,
                flat_table_id: id_at(row, flat)?,
                type_table_id: id_at(row, type_table)?,
            })
        })
        .ok_or(FileError::InvalidTableDef {
            reason: "complex column not found in MSysComplexColumns",
        })
}

/// The definition of the table whose MSysObjects ID is `id`.
fn read_table_by_id(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
    id: u32,
) -> Result<TableDef, FileError> {
    let entry = catalog
        .iter()
        .find(|e| e.object_type == ObjectType::Table && e.table_page == id)
        .ok_or(FileError::InvalidTableDef {
            reason: "table of a complex column not found",
        })?;
    read_table_def(reader, &entry.name, entry.table_page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::read_catalog;

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::PathBuf::from(manifest_dir)
            .join("../../testdata")
            .join(relative);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    macro_rules! skip_if_missing {
        ($path:expr) => {
            match test_data_path($path) {
                Some(p) => p,
                None => {
                    eprintln!("SKIP: test data not found: {}", $path);
                    return;
                }
            }
        };
    }

    /// The values of the column `column` of the table `table` in `path`.
    fn values(path: &std::path::Path, table: &str, column: &str) -> Option<ComplexValues> {
        let mut reader = PageReader::open(path).unwrap();
        let catalog = read_catalog(&mut reader).unwrap();
        let entry = find_table(&catalog, table).unwrap();
        let tdef = read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let column = tdef.columns.iter().find(|c| c.name == column).unwrap();
        read_complex_values(&mut reader, &catalog, &tdef, column).unwrap()
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    #[test]
    fn complex_ids_link_the_columns_to_ms_sys_complex_columns() {
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = read_catalog(&mut reader).unwrap();
        let entry = find_table(&catalog, "Table1").unwrap();
        let tdef = read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let ids: Vec<(&str, Option<u32>)> = tdef
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c.complex_id))
            .collect();
        assert_eq!(
            ids,
            [
                ("id", None),
                (
                    "VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880",
                    Some(2)
                ),
                ("memo-data", None),
                ("append-memo-data", None),
                ("multi-value-data", Some(3)),
                ("attach-data", Some(4)),
            ]
        );
    }

    #[test]
    fn multiple_values() {
        for path in [
            "V2007/complexDataTestV2007.accdb",
            "V2010/complexDataTestV2010.accdb",
        ] {
            let path = skip_if_missing!(path);
            assert_eq!(
                values(&path, "Table1", "multi-value-data"),
                Some(ComplexValues::MultiValue(BTreeMap::from([
                    (2, vec![text("value1"), text("value4")]),
                    (
                        3,
                        vec![
                            text("value1"),
                            text("value2"),
                            text("value3"),
                            text("value4")
                        ]
                    ),
                ])))
            );
        }
    }

    #[test]
    fn version_history() {
        for path in [
            "V2007/complexDataTestV2007.accdb",
            "V2010/complexDataTestV2010.accdb",
        ] {
            let path = skip_if_missing!(path);
            let Some(ComplexValues::VersionHistory(versions)) = values(
                &path,
                "Table1",
                "VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880",
            ) else {
                panic!("not a version history");
            };
            let texts: Vec<(i32, Vec<Value>)> = versions
                .iter()
                .map(|(n, v)| (*n, v.iter().map(|v| v.value.clone()).collect()))
                .collect();
            assert_eq!(
                texts,
                [
                    (2, vec![text("row2-memo")]),
                    (
                        3,
                        vec![
                            text("row3-memo"),
                            text("row3-memo-revised"),
                            text("row3-memo-again")
                        ]
                    ),
                    (4, vec![text("row4-memo")]),
                ]
            );
            assert!(versions
                .values()
                .flatten()
                .all(|v| matches!(v.modified, Value::Timestamp(_))));
        }
    }

    #[test]
    fn attachments_are_unknown() {
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        assert_eq!(
            values(&path, "Table1", "attach-data"),
            Some(ComplexValues::Unknown {
                type_table: "MSysComplexType_Attachment".to_string()
            })
        );
    }

    #[test]
    fn values_of_an_unknown_type_are_unknown() {
        // The type table of this column has a column of a type jetdb does
        // not know (0x48).
        let path = skip_if_missing!("V2007/unsupportedFieldsTestV2007.accdb");
        assert_eq!(
            values(&path, "Test", "UnknownComplex"),
            Some(ComplexValues::Unknown {
                type_table: "MSysComplexType_Text".to_string()
            })
        );
    }

    #[test]
    fn other_columns_are_not_complex() {
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        assert_eq!(values(&path, "Table1", "memo-data"), None);
    }
}
