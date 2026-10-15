//! The values of complex columns: multiple values, attachments and the
//! version history of append-only Long Text columns (Access 2007 and later).
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

/// One file attached to an attachment column.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// The file name, as a [`Value::Text`].
    pub name: Value,
    /// The file type Access gives the file, its extension without the dot.
    pub file_type: Value,
    /// The content of the file, as a [`Value::Binary`]; `Null` if Access
    /// stored none or it could not be decoded.
    pub data: Value,
    /// The `FileURL` Access keeps for the file, usually `Null`.
    pub url: Value,
    /// The `FileTimeStamp` Access keeps for the file, usually `Null`.
    pub timestamp: Value,
    /// The `FileFlags` Access keeps for the file, usually `Null`.
    pub flags: Value,
}

/// The values of one complex column, by the number the column holds in each
/// row. A number with no values is not in the map.
#[derive(Debug, Clone, PartialEq)]
pub enum ComplexValues {
    /// Multiple values of the column's value type, in the order Access
    /// stored them.
    MultiValue(BTreeMap<i32, Vec<Value>>),
    /// The files of an attachment column, in the order Access added them.
    Attachment(BTreeMap<i32, Vec<Attachment>>),
    /// The versions of an append-only Long Text column, oldest first.
    VersionHistory(BTreeMap<i32, Vec<Version>>),
    /// A kind of complex column jetdb does not know, with the name of its
    /// type table.
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

    /// The value column named `name`.
    fn value_column_named(&self, name: &str) -> Option<usize> {
        self.value_columns
            .iter()
            .copied()
            .find(|&i| self.def.columns[i].name == name)
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
    if is_attachment_type(&types) {
        let column = |name| flat.value_column_named(name);
        let [name, file_type, data, url, timestamp, flags] = [
            "FileName",
            "FileType",
            "FileData",
            "FileURL",
            "FileTimeStamp",
            "FileFlags",
        ]
        .map(column);
        let field =
            |row: &mut Vec<Value>, i: Option<usize>| i.map_or(Value::Null, |i| take(row, i));
        return ComplexValues::Attachment(map_rows(rows, |row| Attachment {
            name: field(row, name),
            file_type: field(row, file_type),
            data: match field(row, data) {
                Value::Binary(stored) => {
                    decode_attachment(&stored).map_or(Value::Null, Value::Binary)
                }
                _ => Value::Null,
            },
            url: field(row, url),
            timestamp: field(row, timestamp),
            flags: field(row, flags),
        }));
    }
    let memo = flat.value_column_of_type(ColumnType::Memo);
    let timestamp = flat.value_column_of_type(ColumnType::Timestamp);
    if let (Some(value), Some(modified)) = (memo, timestamp) {
        return ComplexValues::VersionHistory(map_rows(rows, |row| Version {
            value: take(row, value),
            modified: take(row, modified),
        }));
    }
    ComplexValues::Unknown {
        type_table: type_def.name,
    }
}

/// The content of a file as Access stores it in `FileData`: a 32-bit flag
/// (0 for the content as it is, 1 for zlib), the 32-bit length of what
/// follows once inflated, then a header (its own 32-bit length, a 32-bit
/// value, and the extension in UTF-16 ending with a zero) and the content.
fn decode_attachment(stored: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |bytes: &[u8], at: usize| -> Option<usize> {
        let b = bytes.get(at..at + 4)?;
        usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok()
    };
    let length = u32_at(stored, 4)?;
    let body = stored.get(8..)?;
    let body = match u32_at(stored, 0)? {
        0 => body.to_vec(),
        1 => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(body, length).ok()?,
        _ => return None,
    };
    let header = u32_at(&body, 0)?;
    body.get(header..length).map(<[u8]>::to_vec)
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

    /// The name and content of a file.
    type File = (Value, Vec<u8>);

    /// The name and content of each attachment, by number.
    fn files(values: Option<ComplexValues>) -> Vec<(i32, Vec<File>)> {
        let Some(ComplexValues::Attachment(files)) = values else {
            panic!("not attachments: {values:?}");
        };
        files
            .into_iter()
            .map(|(n, files)| {
                let files = files
                    .into_iter()
                    .map(|f| match f.data {
                        Value::Binary(data) => (f.name, data),
                        other => panic!("no content: {other:?}"),
                    })
                    .collect();
                (n, files)
            })
            .collect()
    }

    #[test]
    fn attachments() {
        for path in [
            "V2007/complexDataTestV2007.accdb",
            "V2010/complexDataTestV2010.accdb",
        ] {
            let path = skip_if_missing!(path);
            let data = |s: &str| s.as_bytes().to_vec();
            assert_eq!(
                files(values(&path, "Table1", "attach-data")),
                [
                    (
                        2,
                        vec![
                            (
                                text("test_data.txt"),
                                data("this is some test data for attachment.")
                            ),
                            (
                                text("test_data2.txt"),
                                data("this is some more test data for attachment.")
                            ),
                        ]
                    ),
                    (
                        4,
                        vec![(
                            text("test_data2.txt"),
                            data("this is some more test data for attachment.")
                        )]
                    ),
                ]
            );
        }
    }

    #[test]
    fn attachments_stored_as_they_are_and_compressed() {
        // test_image.jpg is stored as it is, the others with zlib; the
        // extension accdb makes a longer header than txt.
        let path = skip_if_missing!("V2007/testOleV2007.accdb");
        let files = files(values(&path, "Table1", "attach_data"));
        let summary: Vec<(i32, Value, usize, Vec<u8>)> = files
            .into_iter()
            .flat_map(|(n, files)| {
                files
                    .into_iter()
                    .map(move |(name, data)| (n, name, data.len(), data[..4].to_vec()))
            })
            .collect();
        assert_eq!(
            summary,
            [
                (1, text("test_data.txt"), 38, b"this".to_vec()),
                (
                    2,
                    text("test_image.jpg"),
                    94188,
                    vec![0xFF, 0xD8, 0xFF, 0xE0]
                ),
                (
                    3,
                    text("test_word.doc"),
                    22016,
                    vec![0xD0, 0xCF, 0x11, 0xE0]
                ),
                (4, text("test_image.bmp"), 768054, b"BM6\xB8".to_vec()),
                (5, text("test_data\u{16B}2.txt"), 38, b"this".to_vec()),
                (6, text("test_data.txt"), 38, b"this".to_vec()),
                (
                    10,
                    text("test_embed_access.accdb"),
                    376832,
                    vec![0x00, 0x01, 0x00, 0x00]
                ),
                (11, text("test_pdf.pdf"), 82683, b"%PDF".to_vec()),
                (
                    12,
                    text("test_excel.xls"),
                    23040,
                    vec![0xD0, 0xCF, 0x11, 0xE0]
                ),
            ]
        );
    }

    #[test]
    fn file_data_that_cannot_be_decoded() {
        // Flag 0, length 6: a header of 4 bytes, then "ab".
        let stored = [0, 0, 0, 0, 6, 0, 0, 0, 4, 0, 0, 0, b'a', b'b'];
        assert_eq!(decode_attachment(&stored), Some(b"ab".to_vec()));
        // A flag other than 0 and 1.
        assert_eq!(
            decode_attachment(&[2, 0, 0, 0, 6, 0, 0, 0, 4, 0, 0, 0, b'a', b'b']),
            None
        );
        // A length past the data, a header past the length, and too few bytes.
        assert_eq!(
            decode_attachment(&[0, 0, 0, 0, 9, 0, 0, 0, 4, 0, 0, 0, b'a', b'b']),
            None
        );
        assert_eq!(
            decode_attachment(&[0, 0, 0, 0, 6, 0, 0, 0, 8, 0, 0, 0, b'a', b'b']),
            None
        );
        assert_eq!(decode_attachment(&[0, 0, 0, 0]), None);
        // Data that is not zlib.
        assert_eq!(decode_attachment(&[1, 0, 0, 0, 6, 0, 0, 0, 1, 2, 3]), None);
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
