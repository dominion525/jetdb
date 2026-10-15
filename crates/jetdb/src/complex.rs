//! The values of complex columns: multiple values, attachments and the
//! version history of append-only Long Text columns (Access 2007 and later).
//!
//! A complex column holds a number in each row. Its values are rows of a
//! hidden "flat" table, whose Long column that is not an AutoNumber holds
//! that number; the columns holding the values are those named as in the
//! column's "type" table (`MSysComplexType_*`), and the kind of the values
//! follows from the types of those columns. The row of MSysComplexColumns
//! whose `ComplexID` is in the column definition names both tables.

use std::collections::{BTreeMap, HashMap};

use crate::catalog::{find_table, CatalogEntry};
use crate::data::{read_row_at, read_table_rows_with, RowPosition, TableRows, Value};
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

/// The values of a complex column in one row.
#[derive(Debug, Clone, PartialEq)]
pub enum ComplexValues {
    /// Multiple values of the column's value type, in the order Access
    /// stored them.
    MultiValue(Vec<Value>),
    /// The files of an attachment column, in the order Access added them.
    Attachment(Vec<Attachment>),
    /// The versions of an append-only Long Text column, oldest first.
    VersionHistory(Vec<Version>),
    /// A kind of complex column jetdb does not know; see
    /// [`ComplexColumn::type_table`].
    Unknown,
}

/// The kind of a complex column, as its type table shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexKind {
    MultiValue,
    Attachment,
    VersionHistory,
    /// A kind jetdb does not know.
    Unknown,
}

/// A complex column, with where the values of each row are, read by
/// [`read_complex_column`]. [`ComplexColumn::values`] reads the values of one
/// row, so the values of the rows not asked for are never read.
#[derive(Debug, Clone)]
pub struct ComplexColumn {
    flat: FlatTable,
    kind: Kind,
    type_table: String,
    /// The positions of the rows of the flat table, by the number of the row
    /// of the main table, in the order Access added them.
    positions: BTreeMap<i32, Vec<RowPosition>>,
}

/// The kind of a complex column, with the value columns of its flat table.
#[derive(Debug, Clone, Copy)]
enum Kind {
    MultiValue {
        value: usize,
    },
    Attachment {
        name: Option<usize>,
        file_type: Option<usize>,
        data: Option<usize>,
        url: Option<usize>,
        timestamp: Option<usize>,
        flags: Option<usize>,
    },
    VersionHistory {
        value: usize,
        modified: usize,
    },
    Unknown,
}

/// Read where the values of the complex column `column` of the table `table`
/// are, or return `None` if it is not a complex column.
///
/// This reads, of the column's flat table, only the column holding the
/// number of each row and the column of the order Access added the values
/// in, so it keeps a few bytes for each value, not the values.
pub fn read_complex_column(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
    table: &TableDef,
    column: &ColumnDef,
) -> Result<Option<ComplexColumn>, FileError> {
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
    let positions = flat.read_positions(reader)?;
    Ok(Some(ComplexColumn {
        kind: kind_of(&type_def, &flat),
        type_table: type_def.name,
        flat,
        positions,
    }))
}

impl ComplexColumn {
    /// The kind of the column.
    pub fn kind(&self) -> ComplexKind {
        match self.kind {
            Kind::MultiValue { .. } => ComplexKind::MultiValue,
            Kind::Attachment { .. } => ComplexKind::Attachment,
            Kind::VersionHistory { .. } => ComplexKind::VersionHistory,
            Kind::Unknown => ComplexKind::Unknown,
        }
    }

    /// The name of the column's type table (`MSysComplexType_*`).
    pub fn type_table(&self) -> &str {
        &self.type_table
    }

    /// The values of the row whose complex column holds `number`, as
    /// [`read_table_rows`](crate::read_table_rows) returns it
    /// ([`Value::Long`]): empty for a number with no values.
    pub fn values(&self, reader: &mut PageReader, number: i32) -> Result<ComplexValues, FileError> {
        if let Kind::Unknown = self.kind {
            return Ok(ComplexValues::Unknown);
        }
        let positions = self.positions.get(&number).map_or(&[][..], Vec::as_slice);
        let mut rows = Vec::with_capacity(positions.len());
        for &position in positions {
            match read_row_at(
                reader,
                &self.flat.def,
                &HashMap::new(),
                position,
                Some(&self.flat.value_columns),
            )? {
                Some(row) => rows.push(row),
                None => log::warn!("{}: a value could not be read", self.flat.def.name),
            }
        }
        Ok(self.values_of_rows(rows))
    }

    /// The rows of the flat table as the values of the column's kind.
    fn values_of_rows(&self, rows: Vec<Vec<Value>>) -> ComplexValues {
        match self.kind {
            Kind::MultiValue { value } => ComplexValues::MultiValue(
                rows.into_iter()
                    .map(|mut row| take(&mut row, value))
                    .collect(),
            ),
            Kind::Attachment { .. } => ComplexValues::Attachment(self.attachments(rows)),
            Kind::VersionHistory { value, modified } => ComplexValues::VersionHistory(
                rows.into_iter()
                    .map(|mut row| Version {
                        value: take(&mut row, value),
                        modified: take(&mut row, modified),
                    })
                    .collect(),
            ),
            Kind::Unknown => ComplexValues::Unknown,
        }
    }

    /// The rows of the flat table of an attachment column as files.
    fn attachments(&self, rows: Vec<Vec<Value>>) -> Vec<Attachment> {
        let Kind::Attachment {
            name,
            file_type,
            data,
            url,
            timestamp,
            flags,
        } = self.kind
        else {
            return Vec::new();
        };
        let field =
            |row: &mut Vec<Value>, i: Option<usize>| i.map_or(Value::Null, |i| take(row, i));
        rows.into_iter()
            .map(|mut row| Attachment {
                name: field(&mut row, name),
                file_type: field(&mut row, file_type),
                data: match field(&mut row, data) {
                    Value::Binary(stored) => {
                        decode_attachment(&stored).map_or(Value::Null, Value::Binary)
                    }
                    _ => Value::Null,
                },
                url: field(&mut row, url),
                timestamp: field(&mut row, timestamp),
                flags: field(&mut row, flags),
            })
            .collect()
    }
}

/// The flat table of a complex column, with the positions of its columns.
#[derive(Debug, Clone)]
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

    /// The positions of the rows of the table, grouped by the number of the
    /// row of the main table, in the order Access added them. Only the
    /// number and the order of each row are read.
    fn read_positions(
        &self,
        reader: &mut PageReader,
    ) -> Result<BTreeMap<i32, Vec<RowPosition>>, FileError> {
        let columns: Vec<usize> = std::iter::once(self.key_column)
            .chain(self.order_column)
            .collect();
        let calculated = HashMap::new();
        let mut rows = TableRows::new(&self.def, &calculated).with_columns(&columns);
        let mut found: Vec<(i32, i32, RowPosition)> = Vec::new();
        while let Some(row) = rows.next_row(reader)? {
            if let Some(number) = long_at(&row.values, self.key_column) {
                let order = self
                    .order_column
                    .and_then(|i| long_at(&row.values, i))
                    .unwrap_or(i32::MAX);
                found.push((number, order, row.position));
            }
        }
        rows.warn_skipped(&self.def.name);
        found.sort();
        let mut positions: BTreeMap<i32, Vec<RowPosition>> = BTreeMap::new();
        for (number, _, position) in found {
            positions.entry(number).or_default().push(position);
        }
        Ok(positions)
    }
}

/// The kind of a complex column, as the types of its type table's columns
/// show it, with the value columns of its flat table.
fn kind_of(type_def: &TableDef, flat: &FlatTable) -> Kind {
    let types: Vec<ColumnType> = type_def.columns.iter().map(|c| c.col_type).collect();
    if let ([value_type], [value]) = (types.as_slice(), flat.value_columns.as_slice()) {
        if is_multi_value_type(*value_type) {
            return Kind::MultiValue { value: *value };
        }
    }
    if is_attachment_type(&types) {
        let [name, file_type, data, url, timestamp, flags] = [
            "FileName",
            "FileType",
            "FileData",
            "FileURL",
            "FileTimeStamp",
            "FileFlags",
        ]
        .map(|name| flat.value_column_named(name));
        return Kind::Attachment {
            name,
            file_type,
            data,
            url,
            timestamp,
            flags,
        };
    }
    let memo = flat.value_column_of_type(ColumnType::Memo);
    let timestamp = flat.value_column_of_type(ColumnType::Timestamp);
    if let (Some(value), Some(modified)) = (memo, timestamp) {
        return Kind::VersionHistory { value, modified };
    }
    Kind::Unknown
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

    /// The complex column `column` of the table `table` in `path`, with a
    /// reader of the file.
    fn complex_column(
        path: &std::path::Path,
        table: &str,
        column: &str,
    ) -> (PageReader, Option<ComplexColumn>) {
        let mut reader = PageReader::open(path).unwrap();
        let catalog = read_catalog(&mut reader).unwrap();
        let entry = find_table(&catalog, table).unwrap();
        let tdef = read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let column = tdef.columns.iter().find(|c| c.name == column).unwrap();
        let complex = read_complex_column(&mut reader, &catalog, &tdef, column).unwrap();
        (reader, complex)
    }

    /// The values of each number of the column `column` of the table `table`
    /// in `path` that has values.
    fn values(path: &std::path::Path, table: &str, column: &str) -> BTreeMap<i32, ComplexValues> {
        let (mut reader, complex) = complex_column(path, table, column);
        let complex = complex.unwrap();
        let numbers: Vec<i32> = complex.positions.keys().copied().collect();
        numbers
            .into_iter()
            .map(|n| (n, complex.values(&mut reader, n).unwrap()))
            .collect()
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
                BTreeMap::from([
                    (
                        2,
                        ComplexValues::MultiValue(vec![text("value1"), text("value4")])
                    ),
                    (
                        3,
                        ComplexValues::MultiValue(vec![
                            text("value1"),
                            text("value2"),
                            text("value3"),
                            text("value4")
                        ])
                    ),
                ])
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
            let versions: BTreeMap<i32, Vec<Version>> = values(
                &path,
                "Table1",
                "VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880",
            )
            .into_iter()
            .map(|(n, v)| match v {
                ComplexValues::VersionHistory(v) => (n, v),
                other => panic!("not a version history: {other:?}"),
            })
            .collect();
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
    fn files(values: BTreeMap<i32, ComplexValues>) -> Vec<(i32, Vec<File>)> {
        values
            .into_iter()
            .map(|(n, files)| {
                let ComplexValues::Attachment(files) = files else {
                    panic!("not attachments: {files:?}");
                };
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
        let (_, complex) = complex_column(&path, "Test", "UnknownComplex");
        let complex = complex.unwrap();
        assert_eq!(complex.kind(), ComplexKind::Unknown);
        assert_eq!(complex.type_table(), "MSysComplexType_Text");
        let values = values(&path, "Test", "UnknownComplex");
        assert!(!values.is_empty());
        assert!(values.values().all(|v| *v == ComplexValues::Unknown));
    }

    #[test]
    fn kinds_and_type_tables() {
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        for (column, kind, type_table) in [
            (
                "multi-value-data",
                ComplexKind::MultiValue,
                "MSysComplexType_Text",
            ),
            (
                "attach-data",
                ComplexKind::Attachment,
                "MSysComplexType_Attachment",
            ),
            (
                "VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880",
                ComplexKind::VersionHistory,
                "MSysComplexTypeVH_7B4A12A4BE0B4564884754E9E31577B2",
            ),
        ] {
            let complex = complex_column(&path, "Table1", column).1.unwrap();
            assert_eq!(complex.kind(), kind, "{column}");
            assert_eq!(complex.type_table(), type_table, "{column}");
        }
    }

    #[test]
    fn numbers_without_values_are_empty() {
        // Rows 1 and 4 have no multiple values, row 1 and 3 no attachments,
        // and no row has the number 999.
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        let (mut reader, complex) = complex_column(&path, "Table1", "multi-value-data");
        let complex = complex.unwrap();
        for n in [1, 4, 999] {
            assert_eq!(
                complex.values(&mut reader, n).unwrap(),
                ComplexValues::MultiValue(Vec::new())
            );
        }
        let (mut reader, complex) = complex_column(&path, "Table1", "attach-data");
        let complex = complex.unwrap();
        for n in [1, 3, 999] {
            assert_eq!(
                complex.values(&mut reader, n).unwrap(),
                ComplexValues::Attachment(Vec::new())
            );
        }
    }

    #[test]
    fn positions_of_every_row_of_the_flat_table() {
        // Each row of a flat table is in the positions once, under the
        // number of the row of the main table it holds.
        for (path, table, column) in [
            (
                "V2007/complexDataTestV2007.accdb",
                "Table1",
                "multi-value-data",
            ),
            ("V2007/complexDataTestV2007.accdb", "Table1", "attach-data"),
            (
                "V2007/complexDataTestV2007.accdb",
                "Table1",
                "VersionHistory_F5F8918F-0A3F-4DA9-AE71-184EE5012880",
            ),
            ("V2007/testOleV2007.accdb", "Table1", "attach_data"),
        ] {
            let path = skip_if_missing!(path);
            let (mut reader, complex) = complex_column(&path, table, column);
            let complex = complex.unwrap();
            let rows = read_table_rows_with(&mut reader, &complex.flat.def, &HashMap::new())
                .unwrap()
                .rows;
            let mut expected: Vec<i32> = rows
                .iter()
                .filter_map(|r| long_at(r, complex.flat.key_column))
                .collect();
            expected.sort();
            let mut numbers: Vec<i32> = complex
                .positions
                .iter()
                .flat_map(|(n, p)| std::iter::repeat_n(*n, p.len()))
                .collect();
            numbers.sort();
            assert_eq!(numbers, expected, "{column}");
        }
    }

    #[test]
    fn other_columns_are_not_complex() {
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        assert!(complex_column(&path, "Table1", "memo-data").1.is_none());
    }
}
