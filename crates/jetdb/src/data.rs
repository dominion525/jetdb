//! Data row reading and value extraction from table pages.

use std::collections::{HashMap, HashSet};

use crate::catalog::CatalogEntry;
use crate::encoding::{self, TextEncoding};
use crate::file::{find_row, FileError, PageReader};
use crate::format::{row, ColumnType, ObjectType};
use crate::money;
use crate::table::{ColumnDef, TableDef};
use crate::timestamp;

/// Maximum initial capacity for LVAL multi-page buffer (16 MB).
const MAX_LVAL_INITIAL_CAP: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Value enum
// ---------------------------------------------------------------------------

/// A single column value read from a data row.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    Null,
    Bool(bool),
    Byte(u8),
    Int(i16),
    Long(i32),
    BigInt(i64),
    Float(f32),
    Double(f64),
    Text(String),
    Binary(Vec<u8>),
    /// Money: fixed-point string with 4 decimal places (e.g. `"12345.6789"`).
    Money(String),
    /// Numeric: fixed-point string whose scale depends on the column definition.
    Numeric(String),
    /// Timestamp: f64 days since 1899-12-30.
    Timestamp(f64),
    /// GUID: `"{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}"` format.
    Guid(String),
    /// DateTimeExtended: ISO 8601 string (e.g. `"2021-06-14 22:45:12.3456789"`).
    ///
    /// Stored as `String` because:
    /// - f64 cannot represent 100-nanosecond precision without loss.
    /// - strftime has no nanosecond directive, so `--datetime-format` cannot
    ///   be applied even with a structured representation.
    DateTimeExtended(String),
}

impl Value {
    /// The text of a Text value.
    pub(crate) fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The number of a Long value.
    pub(crate) fn as_long(&self) -> Option<i32> {
        match self {
            Value::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// The bytes of a Binary value.
    pub(crate) fn as_binary(&self) -> Option<&[u8]> {
        match self {
            Value::Binary(b) => Some(b),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// read_table_rows — public entry point
// ---------------------------------------------------------------------------

/// Result of reading data rows from a table.
#[non_exhaustive]
pub struct ReadResult {
    /// Successfully parsed rows.
    pub rows: Vec<Vec<Value>>,
    /// Number of rows that were skipped due to parse errors.
    pub skipped_rows: usize,
}

impl ReadResult {
    /// Log a warning if any rows were skipped during parsing.
    pub fn warn_skipped(&self, table: &str) {
        if self.skipped_rows > 0 {
            log::warn!(
                "{table}: {n} row(s) skipped due to parse errors",
                n = self.skipped_rows
            );
        }
    }
}

/// Read all data rows from the table's data pages.
///
/// Also decodes Access "Calculated" field values (an expression cached
/// alongside the row, introduced in Access 2010). When the table has a
/// column marked [`ColumnDef::is_calculated`], one extra `MSysObjects.LvProp`
/// read finds each calculated column's result type -- see
/// `read_calculated_value`'s doc comment for the byte format. Tables
/// without calculated columns skip that read. System tables (`MSys*`) always
/// skip it: [`crate::prop::read_object_properties`] itself reads
/// `MSysObjects` via this same function, so the lookup must never run while
/// reading `MSysObjects`.
///
/// Returns a `ReadResult` containing the successfully parsed rows and a count
/// of rows that were skipped due to errors (e.g. corrupt row data).
pub fn read_table_rows(reader: &mut PageReader, table: &TableDef) -> Result<ReadResult, FileError> {
    let calculated = calculated_column_types(reader, table);
    read_table_rows_impl(reader, table, &calculated)
}

/// [`read_table_rows`] with the result types of the calculated columns
/// already found, by [`calculated_column_types`] or
/// [`calculated_column_types_in`], rather than read MSysObjects again. A
/// calculated column whose result type `calculated` does not give is `Null`.
pub fn read_table_rows_with(
    reader: &mut PageReader,
    table: &TableDef,
    calculated: &HashMap<String, ColumnType>,
) -> Result<ReadResult, FileError> {
    read_table_rows_impl(reader, table, calculated)
}

/// The result types of the calculated columns of `table`, keyed by
/// lowercased column name, as [`read_table_rows`] reads their values.
///
/// A calculated column's [`ColumnDef::col_type`] is only a placeholder:
/// Access declares most numeric result types as `Double`. The type its values
/// have is the column's `ResultType` property, which this returns. Columns
/// that are not calculated are not in the map, and the map is empty when the
/// table's properties cannot be read.
pub fn calculated_column_types(
    reader: &mut PageReader,
    table: &TableDef,
) -> HashMap<String, ColumnType> {
    if !has_calculated_columns(table) {
        return HashMap::new();
    }
    crate::prop::read_object_properties_of_type(reader, &table.name, ObjectType::Table)
        .ok()
        .map(|props| calculated_result_types(&props))
        .unwrap_or_default()
}

/// [`calculated_column_types`] with the catalog already read, whose entry of
/// the table holds its properties, rather than read MSysObjects again.
pub fn calculated_column_types_in(
    catalog: &[CatalogEntry],
    table: &TableDef,
) -> HashMap<String, ColumnType> {
    if !has_calculated_columns(table) {
        return HashMap::new();
    }
    catalog
        .iter()
        .find(|e| e.object_type == ObjectType::Table && e.name == table.name)
        .and_then(|e| crate::prop::entry_properties(e, table.text_encoding).ok())
        .map(|props| calculated_result_types(&props))
        .unwrap_or_default()
}

/// Whether to look up the result types of calculated columns: a system table
/// (MSys*) never has any, and reading MSysObjects itself must not look them
/// up.
fn has_calculated_columns(table: &TableDef) -> bool {
    table.columns.iter().any(|c| c.is_calculated) && !table.name.starts_with("MSys")
}

fn read_table_rows_impl(
    reader: &mut PageReader,
    table: &TableDef,
    calculated: &HashMap<String, ColumnType>,
) -> Result<ReadResult, FileError> {
    let mut table_rows = TableRows::new(table, calculated);
    let mut rows = Vec::new();
    while let Some(row) = table_rows.next_row(reader)? {
        rows.push(row.values);
    }
    Ok(ReadResult {
        rows,
        skipped_rows: table_rows.skipped_rows(),
    })
}

// ---------------------------------------------------------------------------
// TableRows — the rows of a table, one at a time
// ---------------------------------------------------------------------------

/// The position of a row of a table: the index of its data page in
/// [`TableDef::data_pages`] and the index of the row on that page.
///
/// The bytes of a file do not change while jetdb reads them, so a position
/// stays the position of the same row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RowPosition {
    /// The index of the page in [`TableDef::data_pages`].
    pub page: u32,
    /// The index of the row on the page.
    pub row: u16,
}

/// A row read by [`TableRows`], with its position.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub position: RowPosition,
    /// The values in the order of [`TableDef::columns`]; `Null` for the
    /// columns not read (see [`TableRows::with_columns`]).
    pub values: Vec<Value>,
}

/// The rows of a table, read one at a time, as [`read_table_rows`] reads
/// them all.
///
/// The [`PageReader`] is passed to each call rather than kept, so between
/// rows the same reader can read other things, such as the values of a
/// complex column ([`crate::read_complex_column`]).
///
/// A deleted row, and a stale pointer to a row that was moved to another
/// page, is passed over. A row that cannot be read is passed over and
/// counted in [`TableRows::skipped_rows`]. A data page that cannot be read
/// is an error.
pub struct TableRows<'a> {
    table: &'a TableDef,
    calculated: &'a HashMap<String, ColumnType>,
    /// For each column, whether its values are read.
    read: Vec<bool>,
    /// The position to look at next.
    next: RowPosition,
    /// The index and the bytes of the page `next` is on, once read.
    page: Option<(u32, Vec<u8>)>,
    skipped: usize,
}

impl<'a> TableRows<'a> {
    /// The rows of `table` from its first row, with `calculated` the result
    /// types of its calculated columns, as [`read_table_rows_with`] takes
    /// them.
    pub fn new(table: &'a TableDef, calculated: &'a HashMap<String, ColumnType>) -> Self {
        Self {
            table,
            calculated,
            read: vec![true; table.columns.len()],
            next: RowPosition::default(),
            page: None,
            skipped: 0,
        }
    }

    /// Read only the columns at the indexes in `columns` of
    /// [`TableDef::columns`]; the others are `Null` and are not read, so
    /// the long values of Long Text and OLE Object columns not asked for are
    /// not read from their pages.
    pub fn with_columns(mut self, columns: &[usize]) -> Self {
        self.read = (0..self.table.columns.len())
            .map(|i| columns.contains(&i))
            .collect();
        self
    }

    /// Start at `position`, as [`TableRows::position`] gave it.
    pub fn starting_at(mut self, position: RowPosition) -> Self {
        self.next = position;
        self.page = None;
        self
    }

    /// Where the next call continues: the position of the next row, or of
    /// the rows before it that are passed over.
    pub fn position(&self) -> RowPosition {
        self.next
    }

    /// The number of rows passed over because they could not be read.
    pub fn skipped_rows(&self) -> usize {
        self.skipped
    }

    /// Log a warning if any rows were passed over because they could not be
    /// read, as [`ReadResult::warn_skipped`] does.
    pub fn warn_skipped(&self, table: &str) {
        if self.skipped > 0 {
            log::warn!(
                "{table}: {n} row(s) skipped due to parse errors",
                n = self.skipped
            );
        }
    }

    /// The position of the next row, passing over the rows before it that
    /// are deleted or cannot be read, without reading its values; `None`
    /// when there are no more rows.
    pub fn peek(&mut self, reader: &mut PageReader) -> Result<Option<RowPosition>, FileError> {
        let is_jet3 = reader.header().version.is_jet3();
        let has_var_cols = has_var_cols(self.table);
        loop {
            let Some(&page_num) = self.table.data_pages.get(self.next.page as usize) else {
                return Ok(None);
            };
            match self.row_at_next(reader, page_num, is_jet3, has_var_cols)? {
                RowAt::EndOfPage => {
                    self.next = RowPosition {
                        page: self.next.page + 1,
                        row: 0,
                    };
                }
                RowAt::Gone => self.next.row += 1,
                RowAt::Unreadable => {
                    self.skipped += 1;
                    self.next.row += 1;
                }
                RowAt::Row { .. } => return Ok(Some(self.next)),
            }
        }
    }

    /// What is at the next row, on the data page `page_num`, which is read
    /// unless it is the page read last.
    fn row_at_next(
        &mut self,
        reader: &mut PageReader,
        page_num: u32,
        is_jet3: bool,
        has_var_cols: bool,
    ) -> Result<RowAt, FileError> {
        if self.page.as_ref().map(|(i, _)| *i) != Some(self.next.page) {
            self.page = Some((self.next.page, reader.read_page_copy(page_num)?));
        }
        let data = self.page.as_ref().map_or(&[][..], |(_, d)| d.as_slice());
        let row = self.next.row;
        Ok(row_at(
            reader.format(),
            data,
            page_num,
            row,
            is_jet3,
            has_var_cols,
        ))
    }

    /// The next row, or `None` when there are no more rows.
    pub fn next_row(&mut self, reader: &mut PageReader) -> Result<Option<TableRow>, FileError> {
        let Some(position) = self.peek(reader)? else {
            return Ok(None);
        };
        let page_num = self.table.data_pages[position.page as usize];
        let data = self.page.as_ref().map_or(&[][..], |(_, d)| d.as_slice());
        let values = row_values(
            reader,
            data,
            page_num,
            position.row,
            self.table,
            self.calculated,
            &self.read,
        );
        self.next.row += 1;
        Ok(values.map(|values| TableRow { position, values }))
    }
}

/// The values of the row of `table` at `position`, reading only the columns
/// at the indexes in `columns` if given, as [`TableRows`] reads them; `None`
/// if there is no row there or it cannot be read.
pub fn read_row_at(
    reader: &mut PageReader,
    table: &TableDef,
    calculated: &HashMap<String, ColumnType>,
    position: RowPosition,
    columns: Option<&[usize]>,
) -> Result<Option<Vec<Value>>, FileError> {
    let Some(&page_num) = table.data_pages.get(position.page as usize) else {
        return Ok(None);
    };
    let data = reader.read_page_copy(page_num)?;
    let read: Vec<bool> = (0..table.columns.len())
        .map(|i| columns.is_none_or(|c| c.contains(&i)))
        .collect();
    Ok(row_values(
        reader,
        &data,
        page_num,
        position.row,
        table,
        calculated,
        &read,
    ))
}

/// Whether the table declares a variable-length column. See `crack_row`.
fn has_var_cols(table: &TableDef) -> bool {
    table.columns.iter().any(|c| !c.is_fixed)
}

/// What is at a row index of a data page.
enum RowAt {
    /// No more rows on the page, or it is not a data page.
    EndOfPage,
    /// A deleted row, or a stale pointer to a row moved to another page.
    Gone,
    /// A row that cannot be read.
    Unreadable,
    /// A row, at `data[start..start + size]`.
    Row { start: usize, size: usize },
}

/// What is at row `row` of the data page `page_num`, whose bytes are `data`.
fn row_at(
    format: &crate::format::JetFormat,
    data: &[u8],
    page_num: u32,
    row: u16,
    is_jet3: bool,
    has_var_cols: bool,
) -> RowAt {
    let Some(row_ptr) = row_pointer(format, data, row) else {
        return RowAt::EndOfPage;
    };
    // Skip deleted rows
    if row_ptr & row::DELETE_FLAG != 0 {
        return RowAt::Gone;
    }
    // Lookup/overflow rows: the row was relocated to this page from
    // another page, or this is a stale pointer to a row that was
    // moved away. If find_row succeeds, the data at the offset is
    // valid row data to be read normally. Stale entries where
    // find_row fails are skipped (not counted as errors).
    let is_lookup = row_ptr & row::LOOKUP_FLAG != 0;
    let (start, size) = match find_row(format, data, page_num, row) {
        Ok(v) => v,
        Err(e) if is_lookup => {
            log::debug!("skipping stale lookup row on page {page_num} row {row}: {e}");
            return RowAt::Gone;
        }
        Err(e) => {
            log::debug!("skipping row on page {page_num} row {row}: {e}");
            return RowAt::Unreadable;
        }
    };
    if let Err(e) = crack_row(&data[start..start + size], is_jet3, has_var_cols) {
        log::debug!("skipping row on page {page_num} row {row}: {e}");
        return RowAt::Unreadable;
    }
    RowAt::Row { start, size }
}

/// The raw pointer of row `row` of the data page whose bytes are `data`,
/// with its flags; `None` if it is not a data page or has no such row.
fn row_pointer(format: &crate::format::JetFormat, data: &[u8], row: u16) -> Option<u16> {
    // Validate page type (Data = 1)
    if data.first() != Some(&0x01) {
        return None;
    }
    let row_count_pos = format.data_row_count_pos;
    let count = data.get(row_count_pos..row_count_pos + 2)?;
    if row >= u16::from_le_bytes([count[0], count[1]]) {
        return None;
    }
    let entry_pos = row_count_pos + 2 + (row as usize) * 2;
    let entry = data.get(entry_pos..entry_pos + 2)?;
    Some(u16::from_le_bytes([entry[0], entry[1]]))
}

/// The values of row `row` of the data page `page_num`, whose bytes are
/// `data`, reading the columns marked in `read` and leaving the others
/// `Null`; `None` if there is no row there or it cannot be read.
fn row_values(
    reader: &mut PageReader,
    data: &[u8],
    page_num: u32,
    row: u16,
    table: &TableDef,
    calculated: &HashMap<String, ColumnType>,
    read: &[bool],
) -> Option<Vec<Value>> {
    let is_jet3 = reader.header().version.is_jet3();
    let has_var_cols = has_var_cols(table);
    let RowAt::Row { start, size } =
        row_at(reader.format(), data, page_num, row, is_jet3, has_var_cols)
    else {
        return None;
    };
    let cracked = crack_row(&data[start..start + size], is_jet3, has_var_cols).ok()?;
    Some(
        table
            .columns
            .iter()
            .zip(read)
            .map(|(col, &read)| {
                if read {
                    read_column_value(&cracked, col, is_jet3, reader, calculated)
                } else {
                    Value::Null
                }
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// CrackedRow — parsed row structure
// ---------------------------------------------------------------------------

/// Parsed structure of a single data row.
#[allow(dead_code)]
struct CrackedRow<'a> {
    row_data: &'a [u8],
    col_count: u16,
    null_mask: &'a [u8],
    var_col_count: u16,
    /// Variable-column offset table, read backwards from the var_col_count
    /// position. In Jet4/ACE, variable data grows downward from the offset
    /// table, so lower-numbered variable columns have lower offsets.
    ///
    /// - `var_offsets[0]` = start offset of var col 0's data (the "EOD" marker)
    /// - `var_offsets[k]` = start of var col `k`'s data
    /// - `var_offsets[k+1]` = end of var col `k`'s data
    ///
    /// Data for variable column `k`: `row_data[var_offsets[k]..var_offsets[k+1]]`
    var_offsets: Vec<u16>,
}

// ---------------------------------------------------------------------------
// crack_row
// ---------------------------------------------------------------------------

/// Parse the internal structure of a data row.
///
/// `has_var_cols` is whether the *table definition* declares at least one
/// variable-length column. A row only carries the `var_col_count` field and
/// the offset table that follows it when the table has variable-length
/// columns; for an all-fixed-length table the row is just the column count,
/// the fixed data and the null mask. Nothing in the row bytes distinguishes
/// the two cases, so the caller supplies the answer from the schema.
fn crack_row<'a>(
    row_data: &'a [u8],
    is_jet3: bool,
    has_var_cols: bool,
) -> Result<CrackedRow<'a>, FileError> {
    if !has_var_cols {
        crack_row_no_var_cols(row_data, is_jet3)
    } else if is_jet3 {
        crack_row_jet3(row_data)
    } else {
        crack_row_jet4(row_data)
    }
}

/// Row layout for a table with no variable-length columns (both Jet3 and
/// Jet4/ACE):
/// ```text
/// [col_count: u8 (Jet3) / u16 (Jet4)]  ← row start
/// [fixed data ...]
/// [null_mask: ceil(col_count/8)]       ← row end
/// ```
///
/// The byte preceding the null mask belongs to the last fixed column, not to
/// a `var_col_count` field: reading it as a count yields an arbitrary value
/// taken from real column data.
fn crack_row_no_var_cols(row_data: &[u8], is_jet3: bool) -> Result<CrackedRow<'_>, FileError> {
    let len = row_data.len();
    let col_count_size = if is_jet3 { 1usize } else { 2usize };
    if len < col_count_size {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for column count",
        });
    }

    let col_count = if is_jet3 {
        row_data[0] as u16
    } else {
        u16::from_le_bytes([row_data[0], row_data[1]])
    };
    let null_mask_len = (col_count as usize).div_ceil(8);

    let Some(null_mask_start) = len.checked_sub(null_mask_len) else {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for null mask",
        });
    };
    if null_mask_start < col_count_size {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for null mask",
        });
    }

    Ok(CrackedRow {
        row_data,
        col_count,
        null_mask: &row_data[null_mask_start..],
        var_col_count: 0,
        var_offsets: Vec::new(),
    })
}

/// Jet4/ACE row layout (reading from the end):
/// ```text
/// [col_count: u16]           ← row start
/// [fixed data ...]
/// [variable data ...]
/// --- from end ---
/// [null_mask: ceil(col_count/8)]
/// [var_col_count: u16]
/// [eod: u16]                 ← end-of-data marker
/// [var_offset[N-1]: u16]
/// ...
/// [var_offset[0]: u16]
/// ```
///
/// The offset table is read **backwards** from `var_col_count` so that
/// `var_offsets[0] = EOD` and `var_offsets[k+1] = start of var col k`.
fn crack_row_jet4(row_data: &[u8]) -> Result<CrackedRow<'_>, FileError> {
    let len = row_data.len();
    if len < 2 {
        return Err(row_error("row too short for column count"));
    }

    let col_count = u16::from_le_bytes([row_data[0], row_data[1]]);
    let null_mask_len = (col_count as usize).div_ceil(8);

    // Read from end: null_mask, then var_col_count
    let tail_min = null_mask_len + 2; // null_mask + var_col_count
    if len < 2 + tail_min {
        return Err(row_error("row too short for null mask and var col count"));
    }

    let null_mask_start = len - null_mask_len;
    let null_mask = &row_data[null_mask_start..];

    let vcc_pos = null_mask_start - 2;
    let var_col_count = u16::from_le_bytes([row_data[vcc_pos], row_data[vcc_pos + 1]]);

    Ok(CrackedRow {
        row_data,
        col_count,
        null_mask,
        var_col_count,
        var_offsets: read_var_offsets_jet4(row_data, vcc_pos, var_col_count),
    })
}

/// The error of a row that cannot be cracked, for `reason`.
fn row_error(reason: &'static str) -> FileError {
    FileError::InvalidRow {
        page: 0,
        row: 0,
        reason,
    }
}

/// The variable-column offset table of a Jet4 row, read backwards from
/// `vcc_pos`, the position of its `var_col_count`.
fn read_var_offsets_jet4(row_data: &[u8], vcc_pos: usize, var_col_count: u16) -> Vec<u16> {
    let len = row_data.len();
    // Entry count = var_col_count + 1 (includes EOD).
    // var_offsets[0] = EOD at (vcc_pos - 2)
    // var_offsets[k+1] = start offset of var col k at (vcc_pos - 2*(k+2))
    let offset_entries = var_col_count as usize + 1;
    let mut var_offsets = Vec::with_capacity(offset_entries);
    for i in 0..offset_entries {
        let Some(pos) = vcc_pos.checked_sub(2 + i * 2) else {
            break;
        };
        if pos > len.saturating_sub(2) {
            break;
        }
        var_offsets.push(u16::from_le_bytes([row_data[pos], row_data[pos + 1]]));
    }
    var_offsets
}

/// Jet3 row layout (reading from the end):
/// ```text
/// [col_count: u8]            ← row start
/// [fixed data ...]
/// [variable data ...]
/// [offset_table ...]         ← 1 byte per entry, var_col_count+1 entries
/// --- from end ---
/// [null_mask: ceil(col_count/8)]
/// [var_col_count: u8]        ← null_mask の直前
/// [jump_table: num_jumps bytes]  ← var_col_count の直前
/// ```
///
/// Jump table entries contain **column numbers** (not page indices).
/// The dynamic `while` loop method is used to
/// resolve offsets that span 256-byte boundaries.
///
/// Same backward-read convention: `var_offsets[0] = EOD`, `var_offsets[k+1] = start of var col k`.
fn crack_row_jet3(row_data: &[u8]) -> Result<CrackedRow<'_>, FileError> {
    let len = row_data.len();
    if len < 1 {
        return Err(row_error("row too short for column count"));
    }

    let col_count = row_data[0] as u16;
    let null_mask_len = (col_count as usize).div_ceil(8);

    let Some(null_mask_start) = len.checked_sub(null_mask_len) else {
        return Err(row_error("row too short for null mask"));
    };
    let null_mask = &row_data[null_mask_start..];
    let without_var_cols = CrackedRow {
        row_data,
        col_count,
        null_mask,
        var_col_count: 0,
        var_offsets: Vec::new(),
    };
    if null_mask_start == 0 {
        return Ok(without_var_cols);
    }

    // var_col_count is at null_mask_start - 1
    let vcc_pos = null_mask_start - 1;
    if vcc_pos == 0 {
        return Ok(without_var_cols);
    }
    let var_col_count = row_data[vcc_pos] as u16;
    let (col_ptr, actual_num_jumps) = jet3_offset_table(len, vcc_pos, var_col_count);

    // Offset entries: var_col_count + 1 (includes EOD), each 1 byte
    let offset_entries = var_col_count as usize + 1;
    if col_ptr < offset_entries {
        return Err(row_error("row too short for variable offset table (Jet3)"));
    }

    Ok(CrackedRow {
        var_col_count,
        var_offsets: read_var_offsets_jet3(
            row_data,
            (vcc_pos, col_ptr),
            offset_entries,
            actual_num_jumps,
        ),
        ..without_var_cols
    })
}

/// Where the offset table of a Jet3 row of `len` bytes starts, reading
/// backwards, and how many entries of its jump table are used, with
/// `vcc_pos` the position of its var_col_count.
fn jet3_offset_table(len: usize, vcc_pos: usize, var_col_count: u16) -> (usize, usize) {
    // Jump table is between var_col_count and the offset table.
    // num_jumps = (row_len - 1) / 256
    let num_jumps = if len > 1 { (len - 1) / 256 } else { 0 };

    // col_ptr = vcc_pos - num_jumps - 1 (start of offset table, reading backwards)
    let col_ptr = vcc_pos.saturating_sub(num_jumps + 1);

    // Dummy jump check:
    // If last jump is a dummy value, ignore it.
    //
    // Subtract var_col_count, not the number of offset entries: when the
    // variable data ends exactly on a 256-byte boundary, subtracting one more
    // discards a real jump and the EOD offset comes out 256 short.
    let mut actual_num_jumps = num_jumps;
    if actual_num_jumps > 0
        && col_ptr.saturating_sub(var_col_count as usize) / 256 < actual_num_jumps
    {
        actual_num_jumps -= 1;
    }
    (col_ptr, actual_num_jumps)
}

/// The `offset_entries` offsets of a Jet3 row, read backwards from `col_ptr`,
/// each past the 256-byte boundaries the jump table before `vcc_pos` gives.
fn read_var_offsets_jet3(
    row_data: &[u8],
    (vcc_pos, col_ptr): (usize, usize),
    offset_entries: usize,
    actual_num_jumps: usize,
) -> Vec<u16> {
    // Read offsets using the dynamic while-loop method.
    // Jump table entries are at vcc_pos - 1 - k (for k = 0..actual_num_jumps-1)
    // and contain column numbers where jumps_used should increment.
    let mut var_offsets = Vec::with_capacity(offset_entries);
    let mut jumps_used = 0usize;
    for i in 0..offset_entries {
        while jumps_used < actual_num_jumps && i == row_data[vcc_pos - 1 - jumps_used] as usize {
            jumps_used += 1;
        }
        let raw_offset = row_data[col_ptr - i] as u16;
        var_offsets.push(raw_offset + (jumps_used as u16) * 256);
    }
    var_offsets
}

// ---------------------------------------------------------------------------
// Null mask
// ---------------------------------------------------------------------------

/// Check if a column is NULL based on the null bit mask.
///
/// Bit = 1 means NOT NULL; bit = 0 means NULL.
fn is_null(null_mask: &[u8], col_num: u16) -> bool {
    let byte_idx = col_num as usize / 8;
    let bit_idx = col_num as usize % 8;
    if byte_idx >= null_mask.len() {
        return true; // out of range → treat as null
    }
    (null_mask[byte_idx] & (1 << bit_idx)) == 0
}

// ---------------------------------------------------------------------------
// read_column_value
// ---------------------------------------------------------------------------

/// Read a single column value from a cracked row.
fn read_column_value(
    cracked: &CrackedRow<'_>,
    col: &ColumnDef,
    is_jet3: bool,
    reader: &mut PageReader,
    calculated: &HashMap<String, ColumnType>,
) -> Value {
    let text = reader.text_encoding();
    // Boolean is special: value comes from the null mask. A calculated
    // Boolean is not stored there; its cached value is read below.
    if col.col_type == ColumnType::Boolean && !col.is_calculated {
        return Value::Bool(!is_null(cracked.null_mask, col.col_num));
    }

    // All other types: check null first
    if is_null(cracked.null_mask, col.col_num) {
        return Value::Null;
    }

    // Access "Calculated" fields are always delivered through the
    // variable-length mechanism, whatever their nominal `col.col_type`
    // says (often just a generic placeholder -- see
    // `read_calculated_value`'s doc comment). Without its result type the
    // cached value cannot be read, so it is Null rather than the bytes read
    // as the placeholder type.
    if col.is_calculated && !col.is_fixed {
        return read_calculated_column(cracked, col, reader, calculated).unwrap_or(Value::Null);
    }

    if col.is_fixed {
        read_fixed_value(cracked, col, is_jet3)
    } else {
        read_variable_value(cracked, col, text, reader)
    }
}

/// The cached value of the calculated column `col`, of the result type that
/// `calculated` gives it; `None` when it gives none or the row has no data
/// for the column.
fn read_calculated_column(
    cracked: &CrackedRow<'_>,
    col: &ColumnDef,
    reader: &mut PageReader,
    calculated: &HashMap<String, ColumnType>,
) -> Option<Value> {
    let text = reader.text_encoding();
    let &result_type = calculated.get(&col.name.to_ascii_lowercase())?;
    let var_data = extract_var_data(cracked, col)?;
    // A calculated Memo is stored as a long value like any other Memo, so
    // resolve the inline or separate-page data first; the envelope is inside
    // the resolved bytes.
    if result_type == ColumnType::Memo {
        return Some(match read_lval_data(var_data, Some(reader)) {
            Some(data) => read_calculated_value(&data, result_type, text),
            None => Value::Null,
        });
    }
    Some(read_calculated_value(var_data, result_type, text))
}

/// Read a fixed-length column value.
fn read_fixed_value(cracked: &CrackedRow<'_>, col: &ColumnDef, is_jet3: bool) -> Value {
    let col_count_size = if is_jet3 { 1usize } else { 2usize };
    let offset = col_count_size + col.fixed_offset as usize;
    let Some(bytes) = cracked.row_data.get(offset..) else {
        return Value::Null;
    };

    match col.col_type {
        ColumnType::Boolean => unreachable!("handled above"),
        ColumnType::DateTimeExtended => ext_datetime_or(bytes, &bytes[..bytes.len().min(42)]),
        // Unknown fixed-size types: read as raw binary
        ColumnType::Unknown(_) => match bytes.get(..col.col_size as usize) {
            Some(raw) if !raw.is_empty() => Value::Binary(raw.to_vec()),
            _ => Value::Null,
        },
        // Variable-length types should not reach here, and are Null
        other => stored_value(other, bytes, col.scale).unwrap_or(Value::Null),
    }
}

/// The first `N` bytes of `bytes`, if it has them.
fn le_bytes<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.get(..N)?.try_into().ok()
}

/// The value of the fixed-size type `col_type` at the start of `bytes`, as
/// an ordinary column stores it, with `scale` that of a Numeric column;
/// `None` when `bytes` is too short for it or `col_type` is not such a type.
fn stored_value(col_type: ColumnType, bytes: &[u8], scale: u8) -> Option<Value> {
    match col_type {
        ColumnType::Byte
        | ColumnType::Int
        | ColumnType::Long
        | ColumnType::BigInt
        | ColumnType::ComplexType => integer_value(col_type, bytes),
        ColumnType::Guid => bytes.get(..16).map(format_guid).map(Value::Guid),
        _ => fractional_value(col_type, bytes, scale),
    }
}

/// An integer at the start of `bytes`, or the ComplexID of a complex column,
/// which is a Long.
fn integer_value(col_type: ColumnType, bytes: &[u8]) -> Option<Value> {
    match col_type {
        ColumnType::Byte => bytes.first().copied().map(Value::Byte),
        ColumnType::Int => le_bytes(bytes).map(i16::from_le_bytes).map(Value::Int),
        ColumnType::Long | ColumnType::ComplexType => {
            le_bytes(bytes).map(i32::from_le_bytes).map(Value::Long)
        }
        ColumnType::BigInt => le_bytes(bytes).map(i64::from_le_bytes).map(Value::BigInt),
        _ => None,
    }
}

/// A number with a fraction, or a date and time, at the start of `bytes`.
fn fractional_value(col_type: ColumnType, bytes: &[u8], scale: u8) -> Option<Value> {
    match col_type {
        ColumnType::Float => le_bytes(bytes).map(f32::from_le_bytes).map(Value::Float),
        ColumnType::Double => le_bytes(bytes).map(f64::from_le_bytes).map(Value::Double),
        ColumnType::Timestamp => le_bytes(bytes)
            .map(f64::from_le_bytes)
            .map(Value::Timestamp),
        ColumnType::Money => le_bytes(bytes)
            .as_ref()
            .map(money::money_to_string)
            .map(Value::Money),
        ColumnType::Numeric => {
            le_bytes(bytes).map(|b| Value::Numeric(money::numeric_to_string(&b, scale)))
        }
        _ => None,
    }
}

/// The DateTimeExtended at the start of `bytes`, `unreadable` as binary
/// when its 42 bytes are not in its layout, or Null when there are not 42.
fn ext_datetime_or(bytes: &[u8], unreadable: &[u8]) -> Value {
    match bytes.get(..42) {
        Some(ext) => parse_ext_datetime(ext).map_or_else(
            || Value::Binary(unreadable.to_vec()),
            Value::DateTimeExtended,
        ),
        None => Value::Null,
    }
}

/// Locates a variable-length column's raw byte slice within the row, per
/// the `var_offsets` table (`var_offsets[k]..var_offsets[k+1]` for var col
/// `k`). Shared by [`read_variable_value`] and [`read_calculated_value`]'s
/// caller, since Access "Calculated" fields also always go through this
/// storage mechanism.
fn extract_var_data<'a>(cracked: &CrackedRow<'a>, col: &ColumnDef) -> Option<&'a [u8]> {
    let var_idx = col.var_col_num as usize;
    if var_idx + 1 >= cracked.var_offsets.len() {
        return None;
    }
    let start = cracked.var_offsets[var_idx] as usize;
    let end = cracked.var_offsets[var_idx + 1] as usize;
    if start > end || end > cracked.row_data.len() {
        return None;
    }
    Some(&cracked.row_data[start..end])
}

/// Read a variable-length column value.
fn read_variable_value(
    cracked: &CrackedRow<'_>,
    col: &ColumnDef,
    text: TextEncoding,
    reader: &mut PageReader,
) -> Value {
    let Some(var_data) = extract_var_data(cracked, col) else {
        return Value::Null;
    };

    match col.col_type {
        ColumnType::Text => encoding::decode_text(var_data, text).map_or(Value::Null, Value::Text),
        ColumnType::Binary | ColumnType::Unknown(_) => Value::Binary(var_data.to_vec()),
        ColumnType::Memo | ColumnType::Ole => long_value(col.col_type, var_data, text, reader),
        ColumnType::DateTimeExtended => ext_datetime_or(var_data, var_data),
        // Fixed-size types sometimes stored as variable-length (e.g. system tables)
        other => stored_value(other, var_data, col.scale).unwrap_or(Value::Null),
    }
}

/// The value of a Memo or OLE column whose variable-length data is
/// `var_data`, empty when it is.
fn long_value(
    col_type: ColumnType,
    var_data: &[u8],
    text: TextEncoding,
    reader: &mut PageReader,
) -> Value {
    match (col_type, var_data.is_empty()) {
        (ColumnType::Memo, true) => Value::Text(String::new()),
        (ColumnType::Memo, false) => read_memo_value(var_data, text, Some(reader)),
        (_, true) => Value::Binary(Vec::new()),
        (_, false) => read_ole_value(var_data, Some(reader)),
    }
}

// ---------------------------------------------------------------------------
// Access "Calculated" fields
// ---------------------------------------------------------------------------

/// Extracts, for one table, the Result Type of every column that has an
/// Access "Calculated" field expression -- i.e. every column-level property
/// map with both an `Expression` and a `ResultType` property. `Expression`
/// presence is the reliable signal that a column is calculated (a plain
/// column never has one); `ResultType` is the calculated value's actual
/// data type, encoded as the same byte values as [`ColumnType`].
///
/// Keyed by lowercased column name for case-insensitive lookup against
/// [`ColumnDef::name`].
fn calculated_result_types(props: &crate::prop::ObjectProperties) -> HashMap<String, ColumnType> {
    props
        .maps
        .iter()
        .filter(|map| map.map_type == crate::prop::PropMapType::Column)
        .filter_map(|map| Some((map.name.to_ascii_lowercase(), calculated_result_type(map)?)))
        .collect()
}

/// The Result Type of the column of the column-level property map `map`
/// when it is calculated: when the map has an `Expression` and a
/// `ResultType`.
fn calculated_result_type(map: &crate::prop::PropertyMap) -> Option<ColumnType> {
    if !map.properties.iter().any(|p| p.name == "Expression") {
        return None;
    }
    let result_type_prop = map.properties.iter().find(|p| p.name == "ResultType")?;
    let byte = match result_type_prop.value {
        Value::Byte(b) => b,
        Value::Int(i) => i as u8,
        Value::Long(l) => l as u8,
        _ => return None,
    };
    ColumnType::try_from(byte).ok()
}

/// Decodes an Access "Calculated" field's cached value, given its actual
/// `result_type` (from [`calculated_result_types`]'s `Expression`/
/// `ResultType` properties). `ColumnDef::col_type` can't be used for this:
/// Access's Calculated Field UI lets a user pick a Result Type (Short
/// Text, Long Integer, Double, Currency, ...) independently of the
/// expression, but the column's *declared* physical type ends up as a
/// generic placeholder for most numeric choices (observed: Integer, Long
/// Integer, Single, Double, and Currency Result Types all declare the
/// column as plain `Double`), so only `result_type` says how to actually
/// read the bytes.
///
/// The cached value is always delivered through the variable-length
/// storage mechanism (`var_data`), wrapped in an envelope: 16 reserved zero
/// bytes, then a 4-byte little-endian byte length, then that many bytes
/// holding the value. For a Memo result the variable-length data is a long
/// value reference, so the caller resolves it with [`read_lval_data`] first
/// and passes the resolved bytes, which carry the same envelope.
///
/// Most types' payload uses the *same* encoding a normal fixed/variable
/// column of that type uses elsewhere in this module. Numeric/Decimal is
/// the exception: a calculated Decimal result has no reliable external
/// scale to borrow (unlike an ordinary stored Decimal column, neither
/// `ColumnDef` nor the `LvProp` properties carry a usable scale for a
/// calculated field, and an expression's result scale isn't fixed anyway),
/// so its payload is self-describing instead -- see
/// [`crate::money::decimal_variant_to_string`]'s doc comment.
fn read_calculated_value(var_data: &[u8], result_type: ColumnType, text: TextEncoding) -> Value {
    let Some(payload) = extract_calculated_payload(var_data, 16) else {
        return Value::Null;
    };

    match result_type {
        ColumnType::Text => encoding::decode_text(payload, text).map_or(Value::Null, Value::Text),
        // Memo's payload is raw UTF-16LE (no FF FE compressed-text marker).
        ColumnType::Memo => encoding::decode_utf16le(payload).map_or(Value::Null, Value::Text),
        ColumnType::Boolean => payload
            .first()
            .map_or(Value::Null, |&b| Value::Bool(b != 0)),
        ColumnType::DateTimeExtended => calculated_ext_datetime(payload),
        // Self-describing OLE Automation DECIMAL structure -- see
        // `money::decimal_variant_to_string`'s doc comment for why this
        // differs from the ordinary fixed-column Numeric format.
        ColumnType::Numeric => decimal_variant_value(payload),
        // Anything else: not (yet) reliably decodable -- see this
        // function's doc comment.
        ColumnType::ComplexType => Value::Null,
        // The other types are in the same encoding as an ordinary column.
        other => stored_value(other, payload, 0).unwrap_or(Value::Null),
    }
}

/// The value of a calculated DateTimeExtended, in exactly the same 42-byte
/// text encoding `parse_ext_datetime` parses for the fixed-width column
/// format; Null for a payload of another length.
fn calculated_ext_datetime(payload: &[u8]) -> Value {
    if payload.len() != 42 {
        return Value::Null;
    }
    parse_ext_datetime(payload).map_or(Value::Null, Value::DateTimeExtended)
}

/// The Numeric value of a calculated Decimal, an OLE Automation DECIMAL.
fn decimal_variant_value(payload: &[u8]) -> Value {
    le_bytes(payload).map_or(Value::Null, |b| {
        Value::Numeric(money::decimal_variant_to_string(&b))
    })
}

/// Extracts the payload from an Access "Calculated" field's cached-value
/// envelope: `reserved_len` reserved bytes, then a 4-byte little-endian
/// byte length, then that many payload bytes. Returns `None` if the shape
/// doesn't match (empty/NULL cached value, or data that isn't actually
/// this envelope), so callers fall back to `Value::Null` rather than
/// misinterpreting unrelated bytes.
fn extract_calculated_payload(data: &[u8], reserved_len: usize) -> Option<&[u8]> {
    let header_len = reserved_len + 4;
    if data.len() < header_len {
        return None;
    }
    // The 16-byte reserved section is all zero in every sample seen.
    if reserved_len == 16 && data[..16] != [0u8; 16] {
        return None;
    }
    let len_bytes: [u8; 4] = data[reserved_len..header_len].try_into().ok()?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let end = header_len + len;
    if end > data.len() {
        return None;
    }
    Some(&data[header_len..end])
}

// ---------------------------------------------------------------------------
// LVAL (Long Value) types
// ---------------------------------------------------------------------------

/// Multi-page overflow — data split across multiple LVAL pages.
const LVAL_MULTI_PAGE: u32 = 0x00000000;
/// Inline long value — data stored directly in the row.
const LVAL_INLINE: u32 = 0x80000000;
/// Single-page overflow — data stored on one other page.
const LVAL_SINGLE_PAGE: u32 = 0x40000000;
/// Mask for the type flag bits.
const LVAL_TYPE_MASK: u32 = 0xC0000000;
/// Byte offset where inline long value data begins.
/// Inline layout: `[length_with_flags(4B)] [lval_dp(4B)] [unknown(4B)] [data...]`
const LVAL_INLINE_HEADER: usize = 12;

/// Read raw bytes from an LVAL (Long Value) field.
///
/// LVAL variable data starts with a 4-byte `length_with_flags` (u32 LE):
/// - bit 31 (0x80000000): LONG_VALUE_TYPE_THIS_PAGE — inline data
/// - bit 30 (0x40000000): LONG_VALUE_TYPE_OTHER_PAGE — single page reference
/// - both 0: LONG_VALUE_TYPE_OTHER_PAGES — multi-page chain
///
/// Inline layout: `[length_with_flags(4B)] [lval_dp(4B)] [unknown(4B)] [data...]`
///
/// Single-page (0x40): `pg_row` at `var_data[4..8]` points to the row on an
/// LVAL page whose data is the entire field value.
///
/// Multi-page (0x00): `pg_row` at `var_data[4..8]` is the first chunk.
/// Each chunk's first 4 bytes are the next `pg_row` (0 = end); bytes after
/// offset 4 are appended to the result buffer.
fn read_lval_data(var_data: &[u8], reader: Option<&mut PageReader>) -> Option<Vec<u8>> {
    let length_with_flags = u32::from_le_bytes(le_bytes(var_data)?);
    let data_len = (length_with_flags & !LVAL_TYPE_MASK) as usize;
    match length_with_flags & LVAL_TYPE_MASK {
        LVAL_INLINE => Some(lval_inline(var_data, data_len)),
        LVAL_SINGLE_PAGE => lval_single_page(var_data, reader?),
        LVAL_MULTI_PAGE => lval_multi_page(var_data, data_len, reader?),
        // Unknown LVAL type
        _ => None,
    }
}

/// An inline long value of `data_len` bytes: data starts at offset 12.
fn lval_inline(var_data: &[u8], data_len: usize) -> Vec<u8> {
    let data_start = LVAL_INLINE_HEADER.min(var_data.len());
    let data_end = (data_start + data_len).min(var_data.len());
    var_data[data_start..data_end].to_vec()
}

/// The `pg_row` of a long value stored on other pages, at `var_data[4..8]`.
fn lval_pg_row(var_data: &[u8]) -> Option<u32> {
    le_bytes(var_data.get(4..)?).map(u32::from_le_bytes)
}

/// A single-page overflow: read from the referenced LVAL page row.
fn lval_single_page(var_data: &[u8], reader: &mut PageReader) -> Option<Vec<u8>> {
    reader.read_pg_row(lval_pg_row(var_data)?).ok()
}

/// A multi-page overflow of `data_len` bytes: a chain of LVAL page rows.
fn lval_multi_page(var_data: &[u8], data_len: usize, reader: &mut PageReader) -> Option<Vec<u8>> {
    let mut pg_row = lval_pg_row(var_data)?;
    let mut buf = Vec::with_capacity(data_len.min(MAX_LVAL_INITIAL_CAP));
    let mut visited = HashSet::new();

    while pg_row != 0 {
        if !visited.insert(pg_row) {
            return None; // circular reference — partial data is unreliable
        }
        let row_data = reader.read_pg_row(pg_row).ok()?;
        let next_pg_row = u32::from_le_bytes(le_bytes(&row_data)?);
        buf.extend_from_slice(&row_data[4..]);
        pg_row = next_pg_row;

        // Safety: stop if we've already collected enough data
        if buf.len() >= data_len {
            break;
        }
    }

    buf.truncate(data_len);
    Some(buf)
}

/// Read a Memo field value.
fn read_memo_value(var_data: &[u8], text: TextEncoding, reader: Option<&mut PageReader>) -> Value {
    match read_lval_data(var_data, reader) {
        Some(data) => match encoding::decode_text(&data, text) {
            Ok(s) => Value::Text(s),
            Err(_) => Value::Null,
        },
        None => Value::Null,
    }
}

/// Read an OLE field value.
fn read_ole_value(var_data: &[u8], reader: Option<&mut PageReader>) -> Value {
    match read_lval_data(var_data, reader) {
        Some(data) => Value::Binary(data),
        None => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// DateTimeExtended parsing
// ---------------------------------------------------------------------------

/// Parse 42-byte ASCII DateTimeExtended into an ISO 8601 string.
///
/// Layout: `[days:19][':'][seconds:12][nanos100:7][':']['7'][0x00]`
fn parse_ext_datetime(buf: &[u8]) -> Option<String> {
    let (days_str, secs_str, nanos_str) = ext_datetime_fields(buf)?;
    let days = parse_zero_padded_i64(days_str)?;
    let seconds = parse_zero_padded_i64(secs_str)?;
    let nanos100 = parse_zero_padded_i64(nanos_str)?;
    if !timestamp::is_ext_datetime_in_range(days, seconds, nanos100) {
        return None;
    }

    Some(timestamp::format_ext_datetime(days, seconds, nanos100))
}

/// The digits of the days, the seconds and the hundreds of nanoseconds of
/// 42 bytes of DateTimeExtended; `None` when `buf` is not in its layout.
fn ext_datetime_fields(buf: &[u8]) -> Option<(&str, &str, &str)> {
    if !is_ext_datetime_layout(buf) {
        return None;
    }
    Some((
        digits(&buf[0..19])?,
        digits(&buf[20..32])?,
        digits(&buf[32..39])?,
    ))
}

/// Whether `buf` is 42 bytes with the separators of DateTimeExtended:
/// `[days:19][':'][seconds:12][nanos100:7][':']['7'][0x00]`.
fn is_ext_datetime_layout(buf: &[u8]) -> bool {
    buf.len() == 42 && buf[19] == b':' && buf[39] == b':' && buf[40] == b'7' && buf[41] == 0x00
}

/// `bytes` as text, when they are all ASCII digits.
fn digits(bytes: &[u8]) -> Option<&str> {
    if !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

fn parse_zero_padded_i64(s: &str) -> Option<i64> {
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() {
        Some(0)
    } else {
        trimmed.parse().ok()
    }
}

// ---------------------------------------------------------------------------
// GUID formatting
// ---------------------------------------------------------------------------

/// Format 16 raw bytes as a GUID string.
///
/// The byte order follows the standard UUID mixed-endian layout:
/// `{AABBCCDD-EEFF-GGHH-IIJJ-KKLLMMNNOOPP}` where the first three groups
/// are byte-swapped.
pub(crate) fn format_guid(b: &[u8]) -> String {
    format!(
        "{{{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        b[3], b[2], b[1], b[0],   // 4-byte swap
        b[5], b[4],               // 2-byte swap
        b[7], b[6],               // 2-byte swap
        b[8], b[9],               // as-is
        b[10], b[11], b[12], b[13], b[14], b[15], // as-is
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_null ---------------------------------------------------------------

    #[test]
    fn null_mask_bit_set_means_not_null() {
        // Byte 0 = 0b00000010 → col 1 is NOT NULL
        let mask = [0x02u8];
        assert!(!is_null(&mask, 1));
    }

    #[test]
    fn null_mask_bit_clear_means_null() {
        let mask = [0x02u8];
        assert!(is_null(&mask, 0)); // bit 0 = 0 → NULL
    }

    #[test]
    fn null_mask_out_of_range() {
        let mask = [0xFFu8];
        assert!(is_null(&mask, 8)); // byte_idx=1, beyond mask → null
    }

    // -- format_guid -----------------------------------------------------------

    #[test]
    fn guid_formatting() {
        let bytes: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, // group 1
            0x05, 0x06, // group 2
            0x07, 0x08, // group 3
            0x09, 0x0A, // group 4
            0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, // group 5
        ];
        assert_eq!(
            format_guid(&bytes),
            "{04030201-0605-0807-090A-0B0C0D0E0F10}"
        );
    }

    #[test]
    fn guid_zero() {
        let bytes = [0u8; 16];
        assert_eq!(
            format_guid(&bytes),
            "{00000000-0000-0000-0000-000000000000}"
        );
    }

    // -- crack_row_jet4 --------------------------------------------------------

    #[test]
    fn crack_row_jet4_basic() {
        // Build a minimal Jet4 row with:
        //   col_count = 3, 1 fixed col (4 bytes), 1 var col
        //
        // Layout (forward):
        //   [0x03, 0x00]              ← col_count = 3
        //   [0xAA, 0xBB, 0xCC, 0xDD] ← fixed data (4 bytes)
        //   [0x48, 0x00, 0x69, 0x00]  ← var data "Hi" in UTF-16LE (offset 6..10)
        //   --- offset table (forward = descending order in Jet4) ---
        //   [end of var col 0 = 10]   ← furthest from vcc (highest offset)
        //   [start/EOD = 6]           ← closest to vcc (lowest offset)
        //   [var_col_count = 1]
        //   [null_mask = 0xFF]

        let mut row = Vec::new();
        // col_count
        row.extend_from_slice(&[0x03, 0x00]);
        // fixed data
        row.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        // var data: "Hi" in UTF-16LE
        row.extend_from_slice(&[0x48, 0x00, 0x69, 0x00]);
        // Offset table (forward/descending): end=10, start=6
        row.extend_from_slice(&10u16.to_le_bytes());
        row.extend_from_slice(&6u16.to_le_bytes());
        // var_col_count = 1
        row.extend_from_slice(&1u16.to_le_bytes());
        // null_mask: 1 byte, all bits set (not null)
        row.push(0xFF);

        let cracked = crack_row_jet4(&row).unwrap();
        assert_eq!(cracked.col_count, 3);
        assert_eq!(cracked.var_col_count, 1);
        // Backward read: var_offsets[0]=6 (start), var_offsets[1]=10 (end)
        assert_eq!(cracked.var_offsets, vec![6, 10]);
        assert_eq!(cracked.null_mask, &[0xFF]);
    }

    #[test]
    fn crack_row_jet4_bogus_var_col_count_does_not_overflow() {
        // Regression: a row whose `var_col_count` claims more
        // variable columns than physically fit in the row would
        // make the backward-walking offset-table loop compute a
        // `pos` that `wrapping_sub` drove below zero. The old
        // bounds check `pos + 2 > len` then overflow-panicked in
        // debug before the comparison could reject the wrapped
        // value. `checked_add` makes the bounds check itself
        // panic-safe so the loop just breaks cleanly.
        //
        // Forward layout (7 bytes total):
        //   [0x01, 0x00]        ← col_count = 1
        //   [0x42, 0x42]        ← 2 bytes of payload
        //   [0x64, 0x00]        ← var_col_count = 100 (bogus)
        //   [0xFF]              ← null_mask (1 byte)
        //
        // vcc_pos = 4, so the backward walk starts at pos = 2, 0,
        // then wraps on i=2. Old code panicked; new code must
        // return Ok with however many valid offsets it read.
        let row: &[u8] = &[0x01, 0x00, 0x42, 0x42, 0x64, 0x00, 0xFF];

        let cracked = crack_row_jet4(row).expect("must not panic on wrap");
        assert_eq!(cracked.col_count, 1);
        assert_eq!(cracked.var_col_count, 100);
        // Only 2 offsets fit before pos wraps; the loop must break
        // at that point rather than panic.
        assert_eq!(cracked.var_offsets.len(), 2);
    }

    #[test]
    fn crack_row_jet4_no_var_cols() {
        // col_count = 2, no variable columns
        // fixed data: 2 bytes
        let mut row = Vec::new();
        row.extend_from_slice(&[0x02, 0x00]); // col_count
        row.extend_from_slice(&[0x42, 0x43]); // fixed data
                                              // EOD offset (points to end of fixed data = 4)
        row.extend_from_slice(&4u16.to_le_bytes());
        // var_col_count = 0
        row.extend_from_slice(&0u16.to_le_bytes());
        // null_mask: 1 byte
        row.push(0xFF);

        let cracked = crack_row_jet4(&row).unwrap();
        assert_eq!(cracked.col_count, 2);
        assert_eq!(cracked.var_col_count, 0);
        assert_eq!(cracked.var_offsets.len(), 1); // just EOD
    }

    // -- crack_row_jet3 --------------------------------------------------------

    #[test]
    fn crack_row_jet3_basic() {
        // Build a minimal Jet3 row with:
        //   col_count = 2, 1 fixed (2 bytes), 1 var col
        //
        // Jet3 end-of-row layout (from end):
        //   [null_mask]         ← row end
        //   [var_col_count]     ← null_mask の直前
        //   (no jump_table, row < 256 bytes)
        //   [offset_table]      ← var_col_count の直前
        //
        // Full layout:
        //   [0x02]              ← col_count = 2
        //   [0xAA, 0xBB]       ← fixed data
        //   [0x48, 0x69]       ← var data "Hi" in Latin-1 (offset 3..5)
        //   [5, 3]             ← offset table (end=5, EOD=3)
        //   [1]                ← var_col_count = 1
        //   [0xFF]             ← null_mask

        let mut row = Vec::new();
        row.push(0x02); // col_count
        row.extend_from_slice(&[0xAA, 0xBB]); // fixed data
        row.extend_from_slice(&[0x48, 0x69]); // var data
                                              // offset table: end=5, EOD=3
        row.push(5);
        row.push(3);
        // var_col_count = 1
        row.push(1);
        // null_mask = 1 byte
        row.push(0xFF);

        let cracked = crack_row_jet3(&row).unwrap();
        assert_eq!(cracked.col_count, 2);
        assert_eq!(cracked.var_col_count, 1);
        assert_eq!(cracked.var_offsets, vec![3, 5]);
    }

    #[test]
    fn crack_row_jet3_keeps_jump_when_var_data_ends_exactly_on_256() {
        // Variable data ending exactly at offset 256 stores the EOD offset as
        // raw byte 0 plus one jump. Here col_ptr - var_col_count == 256, which
        // the "dummy jump" check must not treat as a dummy: doing so drops the
        // jump, EOD reads as 0, and the last variable column comes back as
        // Null because start > end.
        //
        // Layout (262 bytes, num_jumps = 261 / 256 = 1):
        //   [0]       col_count = 2
        //   [1..200]  var col 0 data
        //   [200..256] var col 1 data
        //   [256]     offset: EOD = 256 -> raw 0, needs the jump
        //   [257]     offset: start of var col 1 = 200
        //   [258]     offset: start of var col 0 = 1      (col_ptr)
        //   [259]     jump table: jump applies at offset index 2
        //   [260]     var_col_count = 2
        //   [261]     null mask
        let mut row = vec![0u8; 262];
        row[0] = 2;
        for b in &mut row[1..256] {
            *b = b'x';
        }
        row[256] = 0;
        row[257] = 200;
        row[258] = 1;
        row[259] = 2;
        row[260] = 2;
        row[261] = 0b11;

        let cracked = crack_row_jet3(&row).unwrap();
        assert_eq!(cracked.var_col_count, 2);
        assert_eq!(cracked.var_offsets, vec![1, 200, 256]);
    }

    #[test]
    fn crack_row_jet3_jump_table() {
        // Build a Jet3 row > 256 bytes to exercise the jump table logic.
        //
        // We simulate 2 variable columns whose data spans the 256-byte boundary.
        // row_len will be ~300 bytes, so num_jumps = (300-1)/256 = 1.
        //
        // col_count = 3, var_col_count = 2
        // var col 0 data: offsets 1..200   (within first 256 bytes)
        // var col 1 data: offsets 200..280 (crosses 256-byte boundary)
        //
        // Layout (from end):
        //   [null_mask: 1 byte]
        //   [var_col_count: 1 byte = 2]
        //   [jump_table: 1 byte]    ← column number where 256-boundary is crossed
        //   [offset_table: 3 bytes] ← 3 entries (var_col_count + 1)

        let col_count: u8 = 3;
        let var_col_count: u8 = 2;
        let null_mask_len = 1usize; // ceil(3/8) = 1

        // Target: var col 0 at [1..200], var col 1 at [200..280]
        // offset_table entries (read by index i):
        //   i=0: EOD = 1  (raw byte: 1)
        //   i=1: start of var col 0 end / var col 1 start = 200 (raw: 200)
        //   i=2: end of var col 1 = 280 (raw: 280 - 256 = 24, with jump correction)
        //
        // Jump table entry: column index where jumps_used increments.
        // jump entry contains the column number.
        // For i=2 (the 3rd entry), we need jumps_used=1,
        // so jump_table[0] = 2 (the column number that triggers the jump).

        // We'll construct the row as a fixed-size buffer.
        // Total row structure:
        //   [col_count(1)] [payload...] [offset_table(3)] [jump_table(1)] [vcc(1)] [null_mask(1)]
        // We need total length ~ 300. Let's target exactly 300.
        // Tail overhead = 3 + 1 + 1 + 1 = 6 bytes
        // Payload = 300 - 1 - 6 = 293 bytes (col_count + payload + tail = 300)

        let target_len = 300usize;
        let tail_size = (var_col_count as usize + 1) + 1 + 1 + null_mask_len; // offset_table + jump + vcc + null
        let payload_size = target_len - 1 - tail_size; // minus col_count byte

        let mut row = Vec::with_capacity(target_len);
        row.push(col_count);
        // Fill payload (fixed + variable data regions)
        row.extend(std::iter::repeat_n(0xAA, payload_size));

        // offset_table: 3 entries read via col_ptr - i.
        // col_ptr points to the last pushed byte (highest position).
        // Push in reverse order: entry[2] first, entry[0] last.
        row.push(24); // col_ptr-2 → entry[2]: var col 1 end = 280 - 256 = 24
        row.push(200); // col_ptr-1 → entry[1]: var col 0 end / var col 1 start = 200
        row.push(1); // col_ptr-0 → entry[0]: EOD = 1

        // jump_table: 1 entry — column number 2 triggers the jump
        row.push(2); // jump_table[0] = 2

        // var_col_count
        row.push(var_col_count);

        // null_mask
        row.push(0xFF);

        assert_eq!(row.len(), target_len);

        let cracked = crack_row_jet3(&row).unwrap();
        assert_eq!(cracked.col_count, 3);
        assert_eq!(cracked.var_col_count, 2);

        // Expected offsets:
        // i=0: raw=1,   jumps_used=0 → 1 + 0*256 = 1
        // i=1: raw=200, jumps_used=0 → 200 + 0*256 = 200
        //   (jump entry is 2, i=1 ≠ 2 so no jump increment)
        // i=2: raw=24,  but first check jump: jump_table[0]=2, i=2 matches → jumps_used=1
        //   → 24 + 1*256 = 280
        assert_eq!(cracked.var_offsets, vec![1, 200, 280]);
    }

    // -- read_memo_value -------------------------------------------------------

    #[test]
    fn memo_inline_utf16le() {
        // Inline memo: length_with_flags has bit 31 set.
        // Text "Hi" in UTF-16LE = [0x48, 0x00, 0x69, 0x00] — 4 bytes.
        let data_len: u32 = 4;
        let flags: u32 = LVAL_INLINE | data_len;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes()); // length_with_flags
        var_data.extend_from_slice(&[0u8; 8]); // lval_dp(4B) + unknown(4B)
        var_data.extend_from_slice(&[0x48, 0x00, 0x69, 0x00]); // "Hi" UTF-16LE

        let val = read_memo_value(&var_data, TextEncoding::Unicode, None);
        assert_eq!(val, Value::Text("Hi".to_string()));
    }

    #[test]
    fn memo_inline_jet3_latin1() {
        // Jet3 inline memo: "Hi" in Latin-1 = [0x48, 0x69] — 2 bytes.
        let data_len: u32 = 2;
        let flags: u32 = LVAL_INLINE | data_len;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]); // lval_dp(4B) + unknown(4B)
        var_data.extend_from_slice(&[0x48, 0x69]); // "Hi" Latin-1

        let val = read_memo_value(&var_data, TextEncoding::Jet3 { code_page: 1252 }, None);
        assert_eq!(val, Value::Text("Hi".to_string()));
    }

    #[test]
    fn memo_overflow_without_reader_returns_null() {
        // Single-page overflow (bit 30 set), no reader provided
        let flags: u32 = LVAL_SINGLE_PAGE | 100;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]); // page ref + padding

        let val = read_memo_value(&var_data, TextEncoding::Unicode, None);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn memo_multi_page_without_reader_returns_null() {
        // Multi-page overflow (no type bits set), no reader provided
        let flags: u32 = 500; // no high bits
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]);

        let val = read_memo_value(&var_data, TextEncoding::Unicode, None);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn memo_too_short_returns_null() {
        // Less than 4 bytes
        let val = read_memo_value(&[0x01, 0x02], TextEncoding::Unicode, None);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn ole_inline_returns_binary() {
        // Inline OLE: length_with_flags has bit 31 set.
        let data: [u8; 3] = [0xDE, 0xAD, 0xBE];
        let data_len: u32 = 3;
        let flags: u32 = LVAL_INLINE | data_len;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]); // lval_dp(4B) + unknown(4B)
        var_data.extend_from_slice(&data);

        let val = read_ole_value(&var_data, None);
        assert_eq!(val, Value::Binary(data.to_vec()));
    }

    // -- Boolean from null mask ------------------------------------------------

    #[test]
    fn boolean_from_null_mask() {
        // null_mask bit 0 = 1 → NOT NULL (true), bit 1 = 0 → NULL (false)
        let null_mask = [0x01u8]; // bit 0 set, bit 1 clear
        assert_eq!(Value::Bool(!is_null(&null_mask, 0)), Value::Bool(true));
        assert_eq!(Value::Bool(!is_null(&null_mask, 1)), Value::Bool(false));
    }

    // -- read_fixed_value (fixed types) ----------------------------------------

    #[test]
    fn read_fixed_int() {
        // col_count=1 (2 bytes) + fixed data at offset 0
        let mut row_data = vec![0x01, 0x00]; // col_count
        row_data.extend_from_slice(&(-42i16).to_le_bytes());
        // tail: EOD offset + var_col_count + null_mask
        row_data.extend_from_slice(&4u16.to_le_bytes()); // EOD (single entry)
        row_data.extend_from_slice(&0u16.to_le_bytes()); // var_col_count=0
        row_data.push(0xFF); // null_mask

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "x".into(),
            col_type: ColumnType::Int,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 2,
            flags: 0x01, // FIXED
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        };
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Int(-42));
    }

    #[test]
    fn read_fixed_long() {
        let mut row_data = vec![0x01, 0x00];
        row_data.extend_from_slice(&123456i32.to_le_bytes());
        row_data.extend_from_slice(&6u16.to_le_bytes());
        row_data.extend_from_slice(&0u16.to_le_bytes());
        row_data.push(0xFF);

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "id".into(),
            col_type: ColumnType::Long,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 4,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        };
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Long(123456));
    }

    #[test]
    fn read_fixed_guid() {
        let mut row_data = vec![0x01, 0x00]; // col_count
                                             // GUID bytes
        let guid_bytes: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ];
        row_data.extend_from_slice(&guid_bytes);
        row_data.extend_from_slice(&18u16.to_le_bytes()); // EOD
        row_data.extend_from_slice(&0u16.to_le_bytes());
        row_data.push(0xFF);

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "g".into(),
            col_type: ColumnType::Guid,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 16,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        };
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::Guid("{04030201-0605-0807-090A-0B0C0D0E0F10}".to_string())
        );
    }

    // -- Integration tests with real files ------------------------------------

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

    // -- TableRows -------------------------------------------------------------

    /// The database files under `dir` and its subdirectories.
    fn database_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                files.extend(database_files(&path));
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("mdb" | "accdb")
            ) {
                files.push(path);
            }
        }
        files
    }

    /// The rows of `table` read one at a time, each with the position after
    /// it, and the number of rows passed over.
    fn rows_one_at_a_time(
        reader: &mut PageReader,
        table: &TableDef,
        calculated: &HashMap<String, ColumnType>,
    ) -> (Vec<(TableRow, RowPosition)>, usize) {
        let mut rows = TableRows::new(table, calculated);
        let mut read = Vec::new();
        while let Some(row) = rows.next_row(reader).unwrap() {
            read.push((row, rows.position()));
        }
        assert_eq!(rows.peek(reader).unwrap(), None);
        (read, rows.skipped_rows())
    }

    /// Starting again after the first, middle and next to last rows gives
    /// the rows after them.
    fn assert_continues(
        reader: &mut PageReader,
        table: &TableDef,
        calculated: &HashMap<String, ColumnType>,
        read: &[(TableRow, RowPosition)],
        context: &str,
    ) {
        for k in [0, read.len() / 2, read.len().saturating_sub(2)] {
            let Some((_, after)) = read.get(k) else {
                continue;
            };
            let mut rest = TableRows::new(table, calculated).starting_at(*after);
            let mut got = Vec::new();
            while let Some(row) = rest.next_row(reader).unwrap() {
                got.push(row);
            }
            let expected: Vec<TableRow> = read[k + 1..].iter().map(|(r, _)| r.clone()).collect();
            assert_eq!(got, expected, "{context} after row {k}");
        }
    }

    /// Reading every other column gives their values and Null for the others.
    fn assert_chosen_columns(
        reader: &mut PageReader,
        table: &TableDef,
        calculated: &HashMap<String, ColumnType>,
        read: &[(TableRow, RowPosition)],
        context: &str,
    ) {
        let columns: Vec<usize> = (0..table.columns.len()).step_by(2).collect();
        let mut some = TableRows::new(table, calculated).with_columns(&columns);
        for (row, _) in read {
            let got = some.next_row(reader).unwrap().unwrap();
            assert_eq!(got.position, row.position, "{context}");
            let expected: Vec<Value> = (0..row.values.len())
                .map(|i| {
                    if columns.contains(&i) {
                        row.values[i].clone()
                    } else {
                        Value::Null
                    }
                })
                .collect();
            assert_eq!(got.values, expected, "{context} {:?}", row.position);
        }
        assert!(some.next_row(reader).unwrap().is_none(), "{context}");
    }

    #[test]
    fn table_rows_in_every_table_of_the_test_data() {
        // For each table: reading the rows one at a time gives what
        // read_table_rows gives; starting again at the position after a row
        // gives the rows after it; read_row_at gives each row at its
        // position; and reading every other column gives those values and
        // Null for the others.
        let Some(root) = test_data_path("") else {
            eprintln!("SKIP: test data not found");
            return;
        };
        let mut tables = 0;
        for path in database_files(&root) {
            // The files that need a password are left out.
            let Ok(mut reader) = PageReader::open(&path) else {
                continue;
            };
            let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
            for entry in catalog
                .iter()
                .filter(|e| e.object_type == ObjectType::Table)
            {
                let context = format!("{} {}", path.display(), entry.name);
                let table =
                    crate::table::read_table_def(&mut reader, &entry.name, entry.table_page)
                        .unwrap();
                let calculated = calculated_column_types_in(&catalog, &table);
                let all = read_table_rows_with(&mut reader, &table, &calculated).unwrap();
                let (read, skipped) = rows_one_at_a_time(&mut reader, &table, &calculated);
                let values: Vec<&Vec<Value>> = read.iter().map(|(r, _)| &r.values).collect();
                assert_eq!(values, all.rows.iter().collect::<Vec<_>>(), "{context}");
                assert_eq!(skipped, all.skipped_rows, "{context}");
                assert_continues(&mut reader, &table, &calculated, &read, &context);
                for (row, _) in &read {
                    let at =
                        read_row_at(&mut reader, &table, &calculated, row.position, None).unwrap();
                    assert_eq!(
                        at.as_ref(),
                        Some(&row.values),
                        "{context} {:?}",
                        row.position
                    );
                }
                assert_chosen_columns(&mut reader, &table, &calculated, &read, &context);
                tables += 1;
            }
        }
        // Every table of the test data, as fetched by scripts/fetch-testdata.sh.
        assert!(tables >= 2000, "only {tables} tables read");
    }

    /// Table1 of V2003/testV2003.mdb, from `bytes`.
    fn test_v2003_table1(bytes: Vec<u8>) -> (PageReader, TableDef) {
        let mut reader = PageReader::open_reader(std::io::Cursor::new(bytes)).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = crate::catalog::find_table(&catalog, "Table1").unwrap();
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        (reader, table)
    }

    /// The bytes of testV2003.mdb, and the position in them of the pointer of
    /// the second row of Table1: its two rows are on page 27 of the 4096-byte
    /// pages, and the pointer is at byte 16 of the page.
    fn test_v2003_second_row_pointer(path: &std::path::Path) -> (Vec<u8>, usize) {
        let bytes = std::fs::read(path).unwrap();
        let pos = 27 * 4096 + 16;
        assert_eq!(u16::from_le_bytes([bytes[pos], bytes[pos + 1]]), 0x0F89);
        (bytes, pos)
    }

    const SECOND_ROW: RowPosition = RowPosition { page: 0, row: 1 };

    #[test]
    fn table_rows_read_both_rows_of_table1() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let (bytes, _) = test_v2003_second_row_pointer(&path);
        let calculated = HashMap::new();
        let (mut reader, table) = test_v2003_table1(bytes);
        assert_eq!(table.data_pages, [27]);
        let mut rows = TableRows::new(&table, &calculated);
        assert!(rows.next_row(&mut reader).unwrap().is_some());
        assert_eq!(rows.peek(&mut reader).unwrap(), Some(SECOND_ROW));
        assert!(
            read_row_at(&mut reader, &table, &calculated, SECOND_ROW, None)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn table_rows_pass_over_a_deleted_row() {
        // A deleted row is passed over and not counted.
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let (mut bytes, pos) = test_v2003_second_row_pointer(&path);
        bytes[pos + 1] |= (row::DELETE_FLAG >> 8) as u8;
        let calculated = HashMap::new();
        let (mut reader, table) = test_v2003_table1(bytes);
        let mut rows = TableRows::new(&table, &calculated);
        assert_eq!(
            rows.next_row(&mut reader).unwrap().unwrap().position,
            RowPosition { page: 0, row: 0 }
        );
        assert!(rows.next_row(&mut reader).unwrap().is_none());
        assert_eq!(rows.skipped_rows(), 0);
        assert_eq!(
            read_row_at(&mut reader, &table, &calculated, SECOND_ROW, None).unwrap(),
            None
        );
    }

    #[test]
    fn table_rows_pass_over_an_unreadable_row() {
        // A row given the offset of the first has no bytes, so it cannot be
        // read; it is passed over and counted once, however often peek looks.
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let (mut bytes, pos) = test_v2003_second_row_pointer(&path);
        bytes[pos..pos + 2].copy_from_slice(&0x0FB8u16.to_le_bytes());
        let calculated = HashMap::new();
        let (mut reader, table) = test_v2003_table1(bytes);
        let mut rows = TableRows::new(&table, &calculated);
        assert!(rows.next_row(&mut reader).unwrap().is_some());
        assert_eq!(rows.peek(&mut reader).unwrap(), None);
        assert_eq!(rows.peek(&mut reader).unwrap(), None);
        assert!(rows.next_row(&mut reader).unwrap().is_none());
        assert_eq!(rows.skipped_rows(), 1);
        assert_eq!(
            read_row_at(&mut reader, &table, &calculated, SECOND_ROW, None).unwrap(),
            None
        );
    }

    #[test]
    fn table_rows_past_the_last_row() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let (mut reader, table) = test_v2003_table1(std::fs::read(path).unwrap());
        let calculated = HashMap::new();
        for position in [
            RowPosition { page: 0, row: 2 },
            RowPosition { page: 1, row: 0 },
        ] {
            let mut rows = TableRows::new(&table, &calculated).starting_at(position);
            assert!(rows.next_row(&mut reader).unwrap().is_none());
            assert_eq!(rows.skipped_rows(), 0);
            assert_eq!(
                read_row_at(&mut reader, &table, &calculated, position, None).unwrap(),
                None
            );
        }
    }

    fn assert_msysobjects_rows(rows: &[Vec<Value>], table: &TableDef) {
        assert!(!rows.is_empty(), "MSysObjects should have at least one row");
        let column = |name: &str| {
            table
                .columns
                .iter()
                .position(|c| c.name == name)
                .unwrap_or_else(|| panic!("{name} column"))
        };
        let (id, name, kind) = (column("Id"), column("Name"), column("Type"));

        for row in rows {
            assert_eq!(row.len(), table.columns.len());
            // Id is a Long, Name a Text that is not empty, and Type an Int.
            assert!(matches!(row[id], Value::Long(_)), "Id: {:?}", row[id]);
            assert!(
                matches!(&row[name], Value::Text(s) if !s.is_empty()),
                "Name: {:?}",
                row[name]
            );
            assert!(matches!(row[kind], Value::Int(_)), "Type: {:?}", row[kind]);
        }
    }

    #[test]
    fn jet3_msysobjects_rows() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn jet4_msysobjects_rows() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn ace12_msysobjects_rows() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn ace14_msysobjects_rows() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn ace17_msysobjects_rows() {
        let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_msysobjects_rows(&result.rows, &table);
    }

    #[test]
    fn ace17_datetime_extended() {
        let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "Table1")
            .expect("Table1 entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert!(!result.rows.is_empty(), "Table1 should have rows");

        // Collect all DateTimeExtended values
        let ext_values: Vec<&str> = result
            .rows
            .iter()
            .flat_map(|row| row.iter())
            .filter_map(|v| match v {
                Value::DateTimeExtended(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            ext_values.iter().any(|v| v.contains("2020-06-17")),
            "should contain date-only value 2020-06-17, found: {ext_values:?}"
        );
        assert!(
            ext_values
                .iter()
                .any(|v| v.contains("2021-06-14 22:45:12.3456789")),
            "should contain full precision datetime, found: {ext_values:?}"
        );
    }

    /// BigIntTable in bigIntTestV2016.accdb, made by
    /// testdata/bigint-generators/bigint-generator.bas. 2^53 + 1 is not
    /// exactly a Double, so it shows that every digit is kept.
    #[test]
    fn ace16_bigint() {
        let path = skip_if_missing!("V2016/bigIntTestV2016.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "BigIntTable")
            .expect("BigIntTable entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        assert_eq!(table.columns[1].name, "Big");
        assert_eq!(table.columns[1].col_type, ColumnType::BigInt);
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_eq!(
            result.rows,
            [
                vec![Value::Long(1), Value::BigInt(9007199254740993)],
                vec![Value::Long(2), Value::BigInt(-9007199254740993)],
                vec![Value::Long(3), Value::BigInt(0)],
                vec![Value::Long(4), Value::Null],
            ]
        );
    }

    /// Timestamps before 1899-12-30 in Table6 of testIndexCodesV2003.mdb, as
    /// Access for Microsoft 365 shows them in its datasheet.
    #[test]
    fn timestamps_before_the_epoch_as_access_shows_them() {
        let path = skip_if_missing!("V2003/testIndexCodesV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog.iter().find(|e| e.name == "Table6").unwrap();
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let rows = read_table_rows(&mut reader, &table).unwrap().rows;
        let shown = |name: &str| {
            let row = rows
                .iter()
                .find(|r| r[0] == Value::Text(name.to_string()))
                .unwrap();
            let Value::Timestamp(ts) = row[1] else {
                panic!("{name}: {:?}", row[1]);
            };
            timestamp::format_timestamp(ts, "%Y-%m-%d %H:%M:%S")
        };
        assert_eq!(shown("row10"), "1899-12-30 00:00:30");
        assert_eq!(shown("row19"), "1899-12-29 02:24:00");
        assert_eq!(shown("row14"), "1899-09-27 10:19:15");
        assert_eq!(shown("row3"), "1899-12-07 00:00:00");
    }

    #[test]
    fn numeric_scale_and_precision_not_swapped() {
        // Regression test for `format::JET4::coldef_scale_pos` /
        // `coldef_precision_pos` having been swapped: col2..col7 are
        // declared Numeric(precision=18, scale=0), so their values must
        // come back as plain integers, not padded with 18 digits after
        // a decimal point.
        let path = skip_if_missing!("V2007/fixedNumericTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "test")
            .expect("test table entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        for col in &table.columns[1..] {
            assert_eq!(col.precision, 18, "{}: expected precision 18", col.name);
            assert_eq!(col.scale, 0, "{}: expected scale 0", col.name);
        }
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.rows.len(), 1);
        let row = &result.rows[0];
        let expected = ["1", "0", "0", "4", "-1", "1"];
        for (col, expected) in row[1..].iter().zip(expected) {
            assert_eq!(*col, Value::Numeric(expected.to_string()));
        }
    }

    /// Same table as `table::tests::assert_columns_ordered_by_design_time_insert`:
    /// fields ID, A, C created in that order, then B inserted between A
    /// and C in Design View. Row values must line up with the
    /// display-order column list (ID, A, B, C), not creation order
    /// (ID, A, C, B) -- both derive from the same `TableDef::columns`,
    /// so this exercises that the fix applies consistently to row
    /// reading, not just schema/DDL output.
    fn assert_row_values_ordered_by_design_time_insert(sample_path: &str) {
        let path = skip_if_missing!(sample_path);
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "Table1")
            .expect("Table1 entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        assert_eq!(
            table
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["ID", "A", "B", "C"]
        );
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.rows.len(), 1);
        let row = &result.rows[0];
        assert_eq!(row[0], Value::Long(1));
        assert_eq!(row[1], Value::Text("a".to_string()));
        assert_eq!(row[2], Value::Text("b".to_string()));
        assert_eq!(row[3], Value::Text("c".to_string()));
    }

    #[test]
    fn row_values_ordered_by_design_time_insert_not_creation_order() {
        assert_row_values_ordered_by_design_time_insert("V2007/columnOrderTestV2007.accdb");
    }

    #[test]
    fn jet3_row_values_ordered_by_design_time_insert_not_creation_order() {
        assert_row_values_ordered_by_design_time_insert("V1997/columnOrderTestV1997.mdb");
    }

    // -- LVAL overflow (Memo / OLE) -------------------------------------------

    /// Expected long author text in test2 MSP_PROJECTS.
    const EXPECTED_AUTHOR: &str = "Jon Iles this is a a vawesrasoih aksdkl fas dlkjflkasjd flkjaslkdjflkajlksj dfl lkasjdf lkjaskldfj lkas dlk lkjsjdfkl; aslkdf lkasjkldjf lka skldf lka sdkjfl;kasjd falksjdfljaslkdjf laskjdfk jalskjd flkj aslkdjflkjkjasljdflkjas jf;lkasjd fjkas dasdf asd fasdf asdf asdmhf lksaiyudfoi jasodfj902384jsdf9 aw90se fisajldkfj lkasj dlkfslkd jflksjadf as";

    fn read_msp_projects_row(path: &std::path::Path) -> (Vec<Value>, TableDef) {
        let mut reader = PageReader::open(path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "MSP_PROJECTS")
            .expect("MSP_PROJECTS entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert!(
            !result.rows.is_empty(),
            "MSP_PROJECTS should have at least one row"
        );
        (result.rows.into_iter().next().unwrap(), table)
    }

    fn col_index(table: &TableDef, name: &str) -> usize {
        table
            .columns
            .iter()
            .position(|c| c.name == name)
            .unwrap_or_else(|| panic!("column {name} not found"))
    }

    #[test]
    fn jet4_memo_lval_overflow() {
        let path = skip_if_missing!("V2003/test2V2003.mdb");
        let (row, table) = read_msp_projects_row(&path);

        // PROJ_PROP_AUTHOR: long Memo text (likely single-page LVAL overflow)
        let author_idx = col_index(&table, "PROJ_PROP_AUTHOR");
        match &row[author_idx] {
            Value::Text(s) => assert_eq!(s, EXPECTED_AUTHOR),
            other => panic!("Expected Text for PROJ_PROP_AUTHOR, got: {other:?}"),
        }

        // PROJ_PROP_COMPANY: short Memo text (inline)
        let company_idx = col_index(&table, "PROJ_PROP_COMPANY");
        assert_eq!(row[company_idx], Value::Text("T".to_string()));

        // PROJ_PROP_TITLE: short Memo text (inline)
        let title_idx = col_index(&table, "PROJ_PROP_TITLE");
        assert_eq!(row[title_idx], Value::Text("Project1".to_string()));
    }

    #[test]
    fn jet4_ole_lval_overflow() {
        let path = skip_if_missing!("V2003/test2V2003.mdb");
        let (row, table) = read_msp_projects_row(&path);

        // RESERVED_BINARY_DATA: OLE binary (likely multi-page LVAL overflow)
        let bin_idx = col_index(&table, "RESERVED_BINARY_DATA");
        let expected = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/test2BinData.dat"),
        )
        .unwrap();
        match &row[bin_idx] {
            Value::Binary(b) => assert_eq!(b, &expected),
            other => panic!("Expected Binary for RESERVED_BINARY_DATA, got: {other:?}"),
        }
    }

    #[test]
    fn jet3_memo_lval_overflow() {
        let path = skip_if_missing!("V1997/test2V1997.mdb");
        let (row, table) = read_msp_projects_row(&path);

        let author_idx = col_index(&table, "PROJ_PROP_AUTHOR");
        match &row[author_idx] {
            Value::Text(s) => assert_eq!(s, EXPECTED_AUTHOR),
            other => panic!("Expected Text for PROJ_PROP_AUTHOR, got: {other:?}"),
        }

        let title_idx = col_index(&table, "PROJ_PROP_TITLE");
        assert_eq!(row[title_idx], Value::Text("Project1".to_string()));
    }

    #[test]
    fn jet3_ole_lval_overflow() {
        let path = skip_if_missing!("V1997/test2V1997.mdb");
        let (row, table) = read_msp_projects_row(&path);

        let bin_idx = col_index(&table, "RESERVED_BINARY_DATA");
        let expected = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/test2BinData.dat"),
        )
        .unwrap();
        match &row[bin_idx] {
            Value::Binary(b) => assert_eq!(b, &expected),
            other => panic!("Expected Binary for RESERVED_BINARY_DATA, got: {other:?}"),
        }
    }

    // -- LVAL inline empty data -----------------------------------------------

    #[test]
    fn lval_inline_empty_data() {
        // Header only, data_len = 0 → should return Some(vec![])
        let flags: u32 = LVAL_INLINE; // data_len = 0
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes()); // length_with_flags
        var_data.extend_from_slice(&[0u8; 8]); // lval_dp(4B) + unknown(4B)
                                               // No payload bytes — total 12 bytes (header only)

        let result = read_lval_data(&var_data, None);
        assert_eq!(result, Some(vec![]));
    }

    #[test]
    fn memo_inline_empty_returns_empty_text() {
        // Memo with inline empty data → empty string
        let flags: u32 = LVAL_INLINE;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]);

        let val = read_memo_value(&var_data, TextEncoding::Unicode, None);
        assert_eq!(val, Value::Text("".to_string()));
    }

    // -- LVAL unknown type ----------------------------------------------------

    #[test]
    fn lval_unknown_type_returns_none() {
        // Type bits = 0xC0000000 (both bit 31 and bit 30 set) — undefined type
        let flags: u32 = 0xC0000000 | 42;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]);

        let result = read_lval_data(&var_data, None);
        assert_eq!(result, None);
    }

    // -- read_column_value dispatch -------------------------------------------

    /// A Boolean column of the number `col_num`, whose value is its bit of the
    /// null mask.
    fn boolean_column(col_num: u16) -> ColumnDef {
        ColumnDef {
            name: "Flag".into(),
            col_type: ColumnType::Boolean,
            col_num,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 0,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        }
    }

    #[test]
    fn dispatch_boolean_from_null_mask() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        // Synthesize a cracked row with a Boolean ColumnDef.
        let mut row_data = vec![0x02, 0x00]; // col_count = 2
        row_data.extend_from_slice(&[0x00, 0x00]); // fixed data placeholder
        row_data.extend_from_slice(&4u16.to_le_bytes()); // EOD
        row_data.extend_from_slice(&0u16.to_le_bytes()); // var_col_count = 0
        row_data.push(0b00000010); // null_mask: col 1 NOT NULL, col 0 NULL
        let cracked = crack_row_jet4(&row_data).unwrap();

        let mut read = |col_num| {
            let column = boolean_column(col_num);
            read_column_value(&cracked, &column, false, &mut reader, &HashMap::new())
        };
        // Bit 1 is set, so column 1 is true; bit 0 is clear, so column 0 is false.
        assert_eq!(read(1), Value::Bool(true));
        assert_eq!(read(0), Value::Bool(false));
    }

    #[test]
    fn dispatch_null_returns_null() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        let mut row_data = vec![0x02, 0x00]; // col_count = 2
        row_data.extend_from_slice(&0i16.to_le_bytes()); // fixed data
        row_data.extend_from_slice(&4u16.to_le_bytes()); // EOD
        row_data.extend_from_slice(&0u16.to_le_bytes()); // var_col_count = 0
        row_data.push(0x00); // null_mask: all NULL

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "x".into(),
            col_type: ColumnType::Int,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 2,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        };
        assert_eq!(
            read_column_value(&cracked, &col, false, &mut reader, &HashMap::new()),
            Value::Null
        );
    }

    #[test]
    fn dispatch_fixed_int() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        let mut row_data = vec![0x01, 0x00]; // col_count = 1
        row_data.extend_from_slice(&(-42i16).to_le_bytes());
        row_data.extend_from_slice(&4u16.to_le_bytes()); // EOD
        row_data.extend_from_slice(&0u16.to_le_bytes()); // var_col_count = 0
        row_data.push(0xFF); // null_mask: NOT NULL

        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            name: "x".into(),
            col_type: ColumnType::Int,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 2,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        };
        assert_eq!(
            read_column_value(&cracked, &col, false, &mut reader, &HashMap::new()),
            Value::Int(-42)
        );
    }

    #[test]
    fn dispatch_calculated_boolean_reads_cached_value() {
        // A calculated column declared as Boolean keeps its value in the row,
        // not in the null mask: here the null mask bit is set (not null) but
        // the cached value is false.
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        let row_data = make_jet4_row_with_var(&calc_envelope(&[0x00]));
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = ColumnDef {
            is_fixed: false,
            is_calculated: true,
            ..make_col_def(ColumnType::Boolean, 0)
        };
        let calculated = HashMap::from([("x".to_string(), ColumnType::Boolean)]);
        assert_eq!(
            read_column_value(&cracked, &col, false, &mut reader, &calculated),
            Value::Bool(false)
        );
    }

    // -- ACE12/ACE14 LVAL overflow --------------------------------------------

    #[test]
    fn ace12_memo_lval_overflow() {
        let path = skip_if_missing!("V2007/test2V2007.accdb");
        let (row, table) = read_msp_projects_row(&path);

        let author_idx = col_index(&table, "PROJ_PROP_AUTHOR");
        match &row[author_idx] {
            Value::Text(s) => assert_eq!(s, EXPECTED_AUTHOR),
            other => panic!("Expected Text for PROJ_PROP_AUTHOR, got: {other:?}"),
        }

        let title_idx = col_index(&table, "PROJ_PROP_TITLE");
        assert_eq!(row[title_idx], Value::Text("Project1".to_string()));
    }

    #[test]
    fn ace12_ole_lval_overflow() {
        let path = skip_if_missing!("V2007/test2V2007.accdb");
        let (row, table) = read_msp_projects_row(&path);

        let bin_idx = col_index(&table, "RESERVED_BINARY_DATA");
        let expected = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/test2BinData.dat"),
        )
        .unwrap();
        match &row[bin_idx] {
            Value::Binary(b) => assert_eq!(b, &expected),
            other => panic!("Expected Binary for RESERVED_BINARY_DATA, got: {other:?}"),
        }
    }

    #[test]
    fn ace14_memo_lval_overflow() {
        let path = skip_if_missing!("V2010/test2V2010.accdb");
        let (row, table) = read_msp_projects_row(&path);

        let author_idx = col_index(&table, "PROJ_PROP_AUTHOR");
        match &row[author_idx] {
            Value::Text(s) => assert_eq!(s, EXPECTED_AUTHOR),
            other => panic!("Expected Text for PROJ_PROP_AUTHOR, got: {other:?}"),
        }

        let title_idx = col_index(&table, "PROJ_PROP_TITLE");
        assert_eq!(row[title_idx], Value::Text("Project1".to_string()));
    }

    #[test]
    fn ace14_ole_lval_overflow() {
        let path = skip_if_missing!("V2010/test2V2010.accdb");
        let (row, table) = read_msp_projects_row(&path);

        let bin_idx = col_index(&table, "RESERVED_BINARY_DATA");
        let expected = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/test2BinData.dat"),
        )
        .unwrap();
        match &row[bin_idx] {
            Value::Binary(b) => assert_eq!(b, &expected),
            other => panic!("Expected Binary for RESERVED_BINARY_DATA, got: {other:?}"),
        }
    }

    // -- crack_row_jet4 error paths -------------------------------------------

    #[test]
    fn crack_row_jet4_empty() {
        assert!(crack_row_jet4(&[]).is_err());
    }

    #[test]
    fn crack_row_jet4_too_short_for_col_count() {
        assert!(crack_row_jet4(&[0x01]).is_err());
    }

    #[test]
    fn crack_row_jet4_short_for_null_mask() {
        // col_count=8 (need 1 byte null mask + 2 byte var_col_count = 3 tail min)
        // total row must be >= 2 + 3 = 5 bytes, provide only 4
        assert!(crack_row_jet4(&[0x08, 0x00, 0x00, 0x00]).is_err());
    }

    #[test]
    fn crack_row_jet4_truncated_offset_table_stops_before_underflow() {
        // col_count=1, var_col_count=2, but there is room for only one
        // 2-byte offset entry before var_col_count. Reading the second entry
        // must stop instead of wrapping the subtraction to a huge index.
        let row = [
            0x01, 0x00, // col_count = 1 and first available offset entry
            0x02, 0x00, // var_col_count = 2
            0xff, // null mask
        ];

        let cracked = crack_row_jet4(&row).unwrap();

        assert_eq!(cracked.var_col_count, 2);
        assert_eq!(cracked.var_offsets, vec![1]);
    }

    // -- crack_row_jet3 edge cases -------------------------------------------

    #[test]
    fn crack_row_jet3_empty() {
        assert!(crack_row_jet3(&[]).is_err());
    }

    #[test]
    fn crack_row_jet3_minimal_null_mask_covers_row() {
        // col_count=8 → null_mask_len=1, row has only 1 byte
        // null_mask_start = 1 - 1 = 0 → early return
        let row = [0x08]; // col_count=8 stored as u8
        let cracked = crack_row_jet3(&row).unwrap();
        assert_eq!(cracked.col_count, 8);
        assert_eq!(cracked.var_col_count, 0);
    }

    #[test]
    fn crack_row_jet3_null_mask_longer_than_row() {
        // col_count=255 → null_mask_len=32, but the row has 3 bytes, as a
        // broken row can.
        let row = [0xFF, 0x00, 0x00];
        assert!(matches!(
            crack_row_jet3(&row),
            Err(FileError::InvalidRow { .. })
        ));
    }

    #[test]
    fn crack_row_jet3_vcc_pos_zero() {
        // col_count=1, null_mask_len=1
        // row = [0x01, null_mask]
        // null_mask_start = 2 - 1 = 1, vcc_pos = 1-1 = 0 → early return
        let row = [0x01, 0xFF];
        let cracked = crack_row_jet3(&row).unwrap();
        assert_eq!(cracked.col_count, 1);
        assert_eq!(cracked.var_col_count, 0);
    }

    #[test]
    fn crack_row_jet3_offset_table_too_short() {
        // Build a row where col_ptr < offset_entries
        // col_count=1 → null_mask_len=1
        // row = [0x01, var_col_count=5, null_mask]
        // null_mask_start = 3-1 = 2, vcc_pos = 2-1 = 1
        // var_col_count = row[1] = 5, offset_entries = 6
        // num_jumps = (3-1)/256 = 0
        // col_ptr = 1 - 0 - 1 = 0
        // col_ptr(0) < offset_entries(6) → error
        let row = [0x01, 0x05, 0xFF];
        assert!(crack_row_jet3(&row).is_err());
    }

    // -- crack_row: tables with no variable-length columns ---------------------

    #[test]
    fn crack_row_jet3_without_var_cols_keeps_last_fixed_byte_out_of_the_count() {
        // The shape of Northwind's "Order Details" (github.com/dominion525/
        // jetdb/issues/12): col_count=5, 22 bytes of fixed data, 1 byte of
        // null mask. The last fixed byte here is 0x3E, the high byte of a
        // Single holding 0.15 — read as a var_col_count it claims 62
        // variable columns, an offset table far larger than the whole row.
        let mut row = vec![0x05];
        row.extend(std::iter::repeat_n(0xAA, 21));
        row.push(0x3E);
        row.push(0xFF);
        assert_eq!(row.len(), 24);

        let cracked = crack_row(&row, true, false).unwrap();
        assert_eq!(cracked.col_count, 5);
        assert_eq!(cracked.var_col_count, 0);
        assert!(cracked.var_offsets.is_empty());
        assert_eq!(cracked.null_mask, &[0xFF]);

        // The same bytes read as a table that does have variable columns:
        // the trailing 0x3E is taken for a count and the row is rejected.
        assert!(crack_row(&row, true, true).is_err());
    }

    #[test]
    fn crack_row_jet4_without_var_cols_keeps_last_fixed_bytes_out_of_the_count() {
        // col_count=1 (u16), 4 bytes of fixed data, 1 byte of null mask.
        let mut row = vec![0x01, 0x00];
        row.extend_from_slice(&[0x00, 0x00, 0x80, 0x3E]);
        row.push(0xFF);

        let cracked = crack_row(&row, false, false).unwrap();
        assert_eq!(cracked.col_count, 1);
        assert_eq!(cracked.var_col_count, 0);
        assert!(cracked.var_offsets.is_empty());
        assert_eq!(cracked.null_mask, &[0xFF]);

        // Read as a table with variable columns, the last two fixed bytes
        // become a count of 16000 and the offsets come from fixed data.
        let wrong = crack_row(&row, false, true).unwrap();
        assert_eq!(wrong.var_col_count, 0x3E80);
    }

    /// Northwind's "Order Details" is a Jet3 table whose five columns are all
    /// fixed-length, so its rows carry no variable-column trailer. Reading the
    /// byte before the null mask as a variable-column count made the outcome
    /// depend on the value of the last fixed column: rows with a Discount of 0
    /// parsed, the rest were skipped.
    ///
    /// Requires `scripts/fetch-testdata.sh`.
    #[test]
    fn jet3_all_fixed_column_table_reads_every_row() {
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "Order Details")
            .expect("Order Details in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        assert!(table.columns.iter().all(|c| c.is_fixed));
        assert_eq!(table.num_rows, 2155);

        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0);
        assert_eq!(result.rows.len(), 2155);

        // Rows with a non-zero Discount are the ones that used to be skipped.
        let discounted = result
            .rows
            .iter()
            .filter(|row| !matches!(row[4], Value::Float(d) if d == 0.0))
            .count();
        assert_eq!(discounted, 838);
    }

    #[test]
    fn crack_row_without_var_cols_rejects_row_shorter_than_null_mask() {
        // col_count=17 → null mask is 3 bytes, leaving no room for the
        // column count byte itself.
        assert!(crack_row(&[0x11, 0xFF, 0xFF], true, false).is_err());
        assert!(crack_row(&[], true, false).is_err());
        assert!(crack_row(&[0x01], false, false).is_err());
    }

    // -- read_fixed_value additional types ------------------------------------

    fn make_jet4_row_with_fixed(fixed_data: &[u8]) -> Vec<u8> {
        let mut row_data = vec![0x01, 0x00]; // col_count = 1
        row_data.extend_from_slice(fixed_data);
        let eod = row_data.len() as u16;
        row_data.extend_from_slice(&eod.to_le_bytes()); // EOD
        row_data.extend_from_slice(&0u16.to_le_bytes()); // var_col_count = 0
        row_data.push(0xFF); // null_mask
        row_data
    }

    fn make_col_def(col_type: ColumnType, col_size: u16) -> ColumnDef {
        ColumnDef {
            name: "x".into(),
            col_type,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        }
    }

    fn make_jet4_row_with_var(var_data: &[u8]) -> Vec<u8> {
        let mut row_data = vec![0x01, 0x00]; // col_count = 1
        let start = row_data.len() as u16;
        row_data.extend_from_slice(var_data);
        let end = row_data.len() as u16;
        row_data.extend_from_slice(&end.to_le_bytes());
        row_data.extend_from_slice(&start.to_le_bytes());
        row_data.extend_from_slice(&1u16.to_le_bytes()); // var_col_count = 1
        row_data.push(0xFF); // null_mask
        row_data
    }

    fn ext_datetime_bytes(days: i64, seconds: i64, nanos100: i64) -> [u8; 42] {
        let s = format!("{days:019}:{seconds:012}{nanos100:07}:7\0");
        let mut buf = [0u8; 42];
        buf.copy_from_slice(s.as_bytes());
        buf
    }

    #[test]
    fn read_fixed_byte() {
        let row_data = make_jet4_row_with_fixed(&[42]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::Byte, 1);
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Byte(42));
    }

    #[test]
    fn read_fixed_bigint() {
        let data = 123456789i64.to_le_bytes();
        let row_data = make_jet4_row_with_fixed(&data);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::BigInt, 8);
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::BigInt(123456789)
        );
    }

    #[test]
    fn read_fixed_float() {
        let data = 1.5f32.to_le_bytes();
        let row_data = make_jet4_row_with_fixed(&data);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::Float, 4);
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Float(1.5));
    }

    #[test]
    fn read_fixed_double() {
        let data = 3.125f64.to_le_bytes();
        let row_data = make_jet4_row_with_fixed(&data);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::Double, 8);
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::Double(3.125)
        );
    }

    #[test]
    fn read_fixed_money() {
        let data = 10_000i64.to_le_bytes();
        let row_data = make_jet4_row_with_fixed(&data);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::Money, 8);
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::Money("1.0000".to_string())
        );
    }

    #[test]
    fn read_fixed_numeric() {
        let mut num_bytes = [0u8; 17];
        num_bytes[0] = 0x00; // positive
        num_bytes[13] = 0x39; // 12345 LE group
        num_bytes[14] = 0x30;
        let row_data = make_jet4_row_with_fixed(&num_bytes);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Numeric, 17);
        col.scale = 2;
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::Numeric("123.45".to_string())
        );
    }

    #[test]
    fn read_fixed_timestamp() {
        let data = 37623.0f64.to_le_bytes();
        let row_data = make_jet4_row_with_fixed(&data);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::Timestamp, 8);
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::Timestamp(37623.0)
        );
    }

    #[test]
    fn read_fixed_complex_type() {
        let data = 42i32.to_le_bytes();
        let row_data = make_jet4_row_with_fixed(&data);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::ComplexType, 4);
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Long(42));
    }

    #[test]
    fn read_fixed_unknown_type() {
        let data = [0xDE, 0xAD];
        let row_data = make_jet4_row_with_fixed(&data);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::Unknown(0x99), 2);
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::Binary(vec![0xDE, 0xAD])
        );
    }

    // -- read_fixed_value Null on out-of-range offset -------------------------

    #[test]
    fn read_fixed_byte_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Byte, 1);
        col.fixed_offset = 100; // way past data
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_int_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Int, 2);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_long_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Long, 4);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_bigint_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::BigInt, 8);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_float_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Float, 4);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_double_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Double, 8);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_money_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Money, 8);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_guid_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Guid, 16);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_numeric_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Numeric, 17);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_timestamp_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Timestamp, 8);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_complex_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::ComplexType, 4);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    #[test]
    fn read_fixed_unknown_null_offset_out_of_range() {
        let row_data = make_jet4_row_with_fixed(&[0x01]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Unknown(0x99), 2);
        col.fixed_offset = 100;
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    // -- read_variable_value edge cases ---------------------------------------

    #[test]
    fn read_variable_var_idx_out_of_range() {
        // Build row with 0 var cols, then request var_col_num=5
        let row_data = make_jet4_row_with_fixed(&[0x42]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Text, 255);
        col.is_fixed = false;
        col.var_col_num = 5;
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert_eq!(
            read_variable_value(&cracked, &col, TextEncoding::Unicode, &mut reader),
            Value::Null
        );
    }

    #[test]
    fn read_variable_empty_text_is_not_null() {
        let row_data = make_jet4_row_with_var(&[]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Text, 255);
        col.is_fixed = false;
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        assert_eq!(
            read_variable_value(&cracked, &col, TextEncoding::Unicode, &mut reader),
            Value::Text(String::new())
        );
    }

    #[test]
    fn read_variable_empty_binary_is_not_null() {
        let row_data = make_jet4_row_with_var(&[]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Binary, 255);
        col.is_fixed = false;
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        assert_eq!(
            read_variable_value(&cracked, &col, TextEncoding::Unicode, &mut reader),
            Value::Binary(Vec::new())
        );
    }

    #[test]
    fn read_variable_empty_memo_is_not_null() {
        let row_data = make_jet4_row_with_var(&[]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Memo, 255);
        col.is_fixed = false;
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        assert_eq!(
            read_variable_value(&cracked, &col, TextEncoding::Unicode, &mut reader),
            Value::Text(String::new())
        );
    }

    #[test]
    fn read_variable_empty_ole_is_not_null() {
        let row_data = make_jet4_row_with_var(&[]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let mut col = make_col_def(ColumnType::Ole, 255);
        col.is_fixed = false;
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();

        assert_eq!(
            read_variable_value(&cracked, &col, TextEncoding::Unicode, &mut reader),
            Value::Binary(Vec::new())
        );
    }

    // -- read_lval_data edge cases -------------------------------------------

    #[test]
    fn lval_too_short() {
        assert_eq!(read_lval_data(&[0x01, 0x02], None), None);
    }

    #[test]
    fn lval_single_page_too_short_for_pg_row() {
        // LVAL_SINGLE_PAGE flag but < 8 bytes
        let flags: u32 = LVAL_SINGLE_PAGE | 10;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0x00, 0x00, 0x00]); // only 3 more bytes, need 4
        assert_eq!(read_lval_data(&var_data, None), None);
    }

    #[test]
    fn lval_multi_page_too_short_for_pg_row() {
        // LVAL_MULTI_PAGE flag but < 8 bytes
        let flags: u32 = 10; // LVAL_MULTI_PAGE = 0x00000000
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0x00, 0x00, 0x00]); // only 3 more bytes
        assert_eq!(read_lval_data(&var_data, None), None);
    }

    // -- parse_ext_datetime / read_fixed_value DateTimeExtended ---------------

    #[test]
    fn parse_ext_datetime_with_time() {
        // days=737954 (2021-06-14), secs=45900 (12:45:00), nanos=0
        let buf = ext_datetime_bytes(737954, 45900, 0);
        let result = parse_ext_datetime(&buf);
        assert_eq!(result, Some("2021-06-14 12:45:00".to_string()));
    }

    #[test]
    fn parse_ext_datetime_buffer_too_short() {
        // 41 bytes (one short of required 42)
        let buf = [b'0'; 41];
        assert_eq!(parse_ext_datetime(&buf), None);
    }

    #[test]
    fn parse_ext_datetime_all_zeros() {
        // All ASCII '0' → days=0, secs=0, nanos=0 → epoch 0001-01-01
        let mut buf = [b'0'; 42];
        buf[19] = b':';
        buf[39] = b':';
        buf[40] = b'7';
        buf[41] = 0x00;
        let result = parse_ext_datetime(&buf);
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("0001-01-01"));
    }

    #[test]
    fn parse_ext_datetime_non_utf8() {
        // Invalid UTF-8 bytes in the days field
        let mut buf = [0xFFu8; 42];
        buf[19] = b':';
        buf[39] = b':';
        buf[40] = b'7';
        buf[41] = 0x00;
        assert_eq!(parse_ext_datetime(&buf), None);
    }

    #[test]
    fn parse_ext_datetime_non_digit() {
        // Non-digit ASCII chars are malformed and should preserve raw bytes.
        let mut buf = [b'x'; 42];
        buf[19] = b':';
        buf[39] = b':';
        buf[40] = b'7';
        buf[41] = 0x00;
        assert_eq!(parse_ext_datetime(&buf), None);
    }

    #[test]
    fn parse_ext_datetime_bad_separator() {
        let mut buf = [b'0'; 42];
        buf[19] = b'-';
        buf[39] = b':';
        buf[40] = b'7';
        buf[41] = 0x00;
        assert_eq!(parse_ext_datetime(&buf), None);
    }

    #[test]
    fn parse_ext_datetime_seconds_out_of_range() {
        let buf = ext_datetime_bytes(737954, 86_400, 0);
        assert_eq!(parse_ext_datetime(&buf), None);
    }

    #[test]
    fn parse_ext_datetime_days_out_of_range() {
        // 9999-12-31 is the last date; the day count of a broken file can
        // be up to 19 digits, which overflowed in the date of it.
        let last = ext_datetime_bytes(3_652_058, 0, 0);
        assert_eq!(parse_ext_datetime(&last), Some("9999-12-31".to_string()));
        for days in [3_652_059, 3_000_000_000_000_000_000] {
            let buf = ext_datetime_bytes(days, 0, 0);
            assert_eq!(parse_ext_datetime(&buf), None, "{days}");
        }
    }

    #[test]
    fn read_fixed_datetime_extended_out_of_range_is_binary() {
        let ascii = ext_datetime_bytes(3_000_000_000_000_000_000, 0, 0);
        let row_data = make_jet4_row_with_fixed(&ascii[..42]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::DateTimeExtended, 42);
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::Binary(ascii.to_vec())
        );
    }

    #[test]
    fn parse_zero_padded_i64_all_zeros() {
        assert_eq!(parse_zero_padded_i64("000000"), Some(0));
    }

    #[test]
    fn read_fixed_datetime_extended() {
        // Build a 42-byte ASCII payload for 2020-06-17 (date only)
        let ascii = ext_datetime_bytes(737592, 0, 0);
        let row_data = make_jet4_row_with_fixed(&ascii[..42]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::DateTimeExtended, 42);
        assert_eq!(
            read_fixed_value(&cracked, &col, false),
            Value::DateTimeExtended("2020-06-17".to_string())
        );
    }

    // -- read_fixed_value variable-length type fallback -----------------------

    #[test]
    fn read_fixed_text_returns_null() {
        // Text is variable-length, should not reach fixed path → returns Null
        let row_data = make_jet4_row_with_fixed(&[0x41, 0x00, 0x42, 0x00]);
        let cracked = crack_row_jet4(&row_data).unwrap();
        let col = make_col_def(ColumnType::Text, 255);
        assert_eq!(read_fixed_value(&cracked, &col, false), Value::Null);
    }

    // -- Overflow (LOOKUP_FLAG) rows ------------------------------------------

    #[test]
    fn japanese_data_values() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "jp_テーブル2")
            .expect("jp_テーブル2 entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert!(!result.rows.is_empty(), "jp_テーブル2 should have rows");

        let name_idx = table
            .columns
            .iter()
            .position(|c| c.name == "商品名")
            .expect("商品名 column");
        let text_values: Vec<&str> = result
            .rows
            .iter()
            .filter_map(|row| match &row[name_idx] {
                Value::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            text_values.iter().any(|v| v.contains("商品")),
            "should contain 商品 in text values, found: {text_values:?}"
        );
    }

    #[test]
    fn overflow_row_msysaccessstorage() {
        let path = skip_if_missing!("overflow_enc_vbaV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "MSysAccessStorage")
            .expect("MSysAccessStorage entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.skipped_rows, 0, "no rows should be skipped");
        assert!(
            result.rows.len() >= 10,
            "MSysAccessStorage should have at least 10 rows, got {}",
            result.rows.len()
        );

        // Verify that a /VBA/dir-like entry exists (Name column should contain VBA paths)
        let name_idx = table
            .columns
            .iter()
            .position(|c| c.name == "Name")
            .expect("Name column");
        let names: Vec<&str> = result
            .rows
            .iter()
            .filter_map(|row| match &row[name_idx] {
                Value::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            names.iter().any(|n| n.contains("dir")),
            "Expected a VBA dir entry among: {names:?}"
        );
    }

    // -- Access "Calculated" field envelope -------------------------------------
    // Byte sequences captured verbatim from a real Access 2019 (.accdb) with a
    // calculated column of every Result Type available in the Access UI.

    #[test]
    fn extract_calculated_payload_basic() {
        let mut data = vec![0u8; 16];
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&[1, 2, 3]);
        data.extend_from_slice(&[9, 9, 9]); // trailing padding, ignored
        assert_eq!(
            extract_calculated_payload(&data, 16),
            Some(&[1u8, 2, 3][..])
        );
    }

    #[test]
    fn extract_calculated_payload_rejects_non_zero_reserved() {
        let mut data = vec![0u8; 15];
        data.push(1); // 16th reserved byte isn't zero
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&[1, 2]);
        assert_eq!(extract_calculated_payload(&data, 16), None);
    }

    #[test]
    fn extract_calculated_payload_rejects_length_past_end() {
        let mut data = vec![0u8; 16];
        data.extend_from_slice(&100u32.to_le_bytes()); // claims more than exists
        data.extend_from_slice(&[1, 2, 3]);
        assert_eq!(extract_calculated_payload(&data, 16), None);
    }

    #[test]
    fn extract_calculated_payload_rejects_too_short() {
        assert_eq!(extract_calculated_payload(&[0u8; 10], 16), None);
    }

    /// Builds the standard (non-Memo) calculated-field envelope: 16 zero
    /// bytes, a 4-byte LE length, then `payload`.
    fn calc_envelope(payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 16];
        data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        data.extend_from_slice(payload);
        data
    }

    #[test]
    fn read_calculated_value_text() {
        // "Nancy Freehafer" -- same envelope shape as the Northwind fix,
        // reusing this module's `encoding::decode_text`.
        let payload = [
            0xFF, 0xFE, 0x4E, 0x61, 0x6E, 0x63, 0x79, 0x20, 0x46, 0x72, 0x65, 0x65, 0x68, 0x61,
            0x66, 0x65, 0x72,
        ];
        let data = calc_envelope(&payload);
        assert_eq!(
            read_calculated_value(&data, ColumnType::Text, TextEncoding::Unicode),
            Value::Text("Nancy Freehafer".to_string())
        );
    }

    #[test]
    fn read_calculated_value_memo() {
        // An inline long value (12-byte header: length with flags, then 8
        // more bytes) holding the usual 16 reserved bytes, a length, then raw
        // UTF-16LE (no FF FE marker) -- "s1 longs1 long".
        let mut data = vec![0x33, 0x00, 0x00, 0x80];
        data.extend_from_slice(&[0u8; 24]);
        let text = "s1 longs1 long";
        let payload: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        data.extend_from_slice(&payload);
        let resolved = read_lval_data(&data, None).expect("inline long value");
        assert_eq!(
            read_calculated_value(&resolved, ColumnType::Memo, TextEncoding::Unicode),
            Value::Text(text.to_string())
        );
    }

    #[test]
    fn read_calculated_value_int_expression_division() {
        // `[Number Integer]/2` where Number Integer = -32768 -> -16384,
        // stored as a plain 2-byte Int16 (not the 8-byte Double the
        // column's declared `col_type` would suggest).
        let data = calc_envelope(&(-16384i16).to_le_bytes());
        assert_eq!(
            read_calculated_value(&data, ColumnType::Int, TextEncoding::Unicode),
            Value::Int(-16384)
        );
    }

    #[test]
    fn read_calculated_value_int_rounds_to_even() {
        // `[Number Integer]/2` where Number Integer = 32767 -> 16383.5,
        // banker's-rounded to 16384 (nearest even) by Access before caching.
        let data = calc_envelope(&16384i16.to_le_bytes());
        assert_eq!(
            read_calculated_value(&data, ColumnType::Int, TextEncoding::Unicode),
            Value::Int(16384)
        );
    }

    #[test]
    fn read_calculated_value_long() {
        // `[Number Long Integer]/2` where Number Long Integer = -2147483648.
        let data = calc_envelope(&(-1073741824i32).to_le_bytes());
        assert_eq!(
            read_calculated_value(&data, ColumnType::Long, TextEncoding::Unicode),
            Value::Long(-1073741824)
        );
    }

    #[test]
    fn read_calculated_value_bigint_rounds_to_even() {
        // `[Large Number]/2` where Large Number = i64::MIN + 1 ->
        // -4611686018427387903.5, banker's-rounded to the even neighbor.
        let data = calc_envelope(&(-4611686018427387904i64).to_le_bytes());
        assert_eq!(
            read_calculated_value(&data, ColumnType::BigInt, TextEncoding::Unicode),
            Value::BigInt(-4611686018427387904)
        );
    }

    #[test]
    fn read_calculated_value_double() {
        // `[Number Double]/2`, a full 8-byte (untrimmed) f64 payload.
        let data = calc_envelope(&(-5.985e307f64).to_le_bytes());
        assert_eq!(
            read_calculated_value(&data, ColumnType::Double, TextEncoding::Unicode),
            Value::Double(-5.985e307)
        );
    }

    #[test]
    fn read_calculated_value_money() {
        // `[Currency]/2` where Currency = -99999999999999.9999 -> the
        // scaled-int64 halves to an exact .5, banker's-rounds to
        // -50000000000000.0000 before caching.
        let scaled: i64 = -500000000000000000; // -50000000000000.0000 * 10000
        let data = calc_envelope(&scaled.to_le_bytes());
        assert_eq!(
            read_calculated_value(&data, ColumnType::Money, TextEncoding::Unicode),
            Value::Money("-50000000000000.0000".to_string())
        );
    }

    #[test]
    fn read_calculated_value_timestamp() {
        // `[Date/Time]` passthrough of 0100-01-01 (days since 1899-12-30).
        let data = calc_envelope(&(-657434.0f64).to_le_bytes());
        assert_eq!(
            read_calculated_value(&data, ColumnType::Timestamp, TextEncoding::Unicode),
            Value::Timestamp(-657434.0)
        );
    }

    #[test]
    fn read_calculated_value_boolean() {
        let data_true = calc_envelope(&[0xFF]);
        assert_eq!(
            read_calculated_value(&data_true, ColumnType::Boolean, TextEncoding::Unicode),
            Value::Bool(true)
        );
        let data_false = calc_envelope(&[0x00]);
        assert_eq!(
            read_calculated_value(&data_false, ColumnType::Boolean, TextEncoding::Unicode),
            Value::Bool(false)
        );
    }

    #[test]
    fn read_calculated_value_date_time_extended() {
        // `[Date/Time Extended]` passthrough of 9999-12-31 23:59:59.9999999
        // -- the exact same 42-byte text encoding as the fixed-width
        // column format, reusing `parse_ext_datetime` unchanged.
        let payload = ext_datetime_bytes(3652058, 86399, 9999999);
        let data = calc_envelope(&payload);
        assert_eq!(
            read_calculated_value(&data, ColumnType::DateTimeExtended, TextEncoding::Unicode),
            Value::DateTimeExtended("9999-12-31 23:59:59.9999999".to_string())
        );
    }

    #[test]
    fn read_calculated_value_unsupported_type_is_null_not_garbled() {
        // A payload too short for any known type falls back to a clean
        // `Null`, never a guess.
        let data = calc_envelope(&[1, 2, 3, 4]);
        assert_eq!(
            read_calculated_value(&data, ColumnType::Guid, TextEncoding::Unicode),
            Value::Null
        );
    }

    #[test]
    fn read_calculated_value_guid() {
        // `[Number Replication ID]` passthrough -- same raw 16-byte layout
        // as the ordinary fixed-column Guid format.
        let payload = [
            0x67, 0x45, 0x3e, 0x12, 0x9b, 0xe8, 0xd3, 0x12, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17,
            0x40, 0x00,
        ];
        let data = calc_envelope(&payload);
        assert_eq!(
            read_calculated_value(&data, ColumnType::Guid, TextEncoding::Unicode),
            Value::Guid("{123E4567-E89B-12D3-A456-426614174000}".to_string())
        );
    }

    #[test]
    fn read_calculated_value_numeric() {
        // `[Number Decimal 7x2]/2` = 1.23 / 2 = 0.615 -- OLE Automation
        // DECIMAL structure, see `money::decimal_variant_to_string`.
        let payload = [
            0x0e, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x67, 0x02, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let data = calc_envelope(&payload);
        assert_eq!(
            read_calculated_value(&data, ColumnType::Numeric, TextEncoding::Unicode),
            Value::Numeric("0.615".to_string())
        );
    }

    #[test]
    fn calculated_result_types_requires_expression_property() {
        use crate::prop::{ObjectProperties, PropMapType, Property, PropertyMap};

        let props = ObjectProperties {
            object_name: "Employees".to_string(),
            maps: vec![
                // Calculated: has both Expression and ResultType.
                PropertyMap {
                    map_type: PropMapType::Column,
                    name: "FullNameFNLN".to_string(),
                    properties: vec![
                        Property {
                            name: "Expression".to_string(),
                            value: Value::Text("x".to_string()),
                            ddl: false,
                        },
                        Property {
                            name: "ResultType".to_string(),
                            value: Value::Byte(10),
                            ddl: false,
                        },
                    ],
                },
                // Not calculated: no Expression property at all.
                PropertyMap {
                    map_type: PropMapType::Column,
                    name: "EmployeeID".to_string(),
                    properties: vec![Property {
                        name: "ColumnWidth".to_string(),
                        value: Value::Long(-1),
                        ddl: false,
                    }],
                },
            ],
        };

        let result = calculated_result_types(&props);
        assert_eq!(result.get("fullnamefnln"), Some(&ColumnType::Text));
        assert_eq!(result.get("employeeid"), None);
    }

    /// The ResultType properties of Table1 in calcFieldTestV2010.accdb, where
    /// most numeric results are declared as Double.
    #[test]
    fn calculated_column_types_of_a_table() {
        let path = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog.iter().find(|e| e.name == "Table1").unwrap();
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let types = calculated_column_types(&mut reader, &table);
        // The same from the catalog already read.
        assert_eq!(calculated_column_types_in(&catalog, &table), types);
        // Rows read with the types already found are the rows read_table_rows
        // reads, finding them itself.
        assert_eq!(
            format!(
                "{:?}",
                read_table_rows_with(&mut reader, &table, &types)
                    .unwrap()
                    .rows
            ),
            format!("{:?}", read_table_rows(&mut reader, &table).unwrap().rows)
        );
        let declared = |name: &str| {
            table
                .columns
                .iter()
                .find(|c| c.name == name)
                .unwrap()
                .col_type
        };
        assert_eq!(declared("MonthlySalary"), ColumnType::Double);
        assert_eq!(types.get("monthlysalary"), Some(&ColumnType::Money));
        assert_eq!(types.get("isrich"), Some(&ColumnType::Boolean));
        assert_eq!(types.get("floattest"), Some(&ColumnType::Float));
        assert_eq!(types.get("decimaltest"), Some(&ColumnType::Numeric));
        assert_eq!(types.get("lastfirst"), Some(&ColumnType::Text));
        assert_eq!(types.get("firstname"), None);
        assert_eq!(types.len(), 11);
    }

    /// Calculated columns whose result types are not given are Null, not
    /// their cached values read as the placeholder types; the other columns
    /// are read as usual.
    #[test]
    fn calculated_columns_without_result_types_are_null() {
        let path = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog.iter().find(|e| e.name == "Table1").unwrap();
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let expected = read_table_rows(&mut reader, &table).unwrap().rows;
        let rows = read_table_rows_with(&mut reader, &table, &HashMap::new())
            .unwrap()
            .rows;
        assert_eq!(rows.len(), expected.len());
        for (row, expected_row) in rows.iter().zip(&expected) {
            for ((col, value), expected_value) in table.columns.iter().zip(row).zip(expected_row) {
                if col.is_calculated {
                    assert_eq!(*value, Value::Null, "{}", col.name);
                } else {
                    assert_eq!(value, expected_value, "{}", col.name);
                }
            }
        }
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    fn money(s: &str) -> Value {
        Value::Money(s.to_string())
    }

    fn numeric(s: &str) -> Value {
        Value::Numeric(s.to_string())
    }

    /// Checks calculated columns of the four rows of Table1 of
    /// calcFieldTestV2010.accdb against `expected`, the values Jackcess reads,
    /// column by column.
    fn assert_calculated_values_match(expected: &[(&str, [Value; 4])]) {
        let path = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = catalog
            .iter()
            .find(|e| e.name == "Table1")
            .expect("Table1 entry in catalog");
        let table =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let result = read_table_rows(&mut reader, &table).unwrap();
        assert_eq!(result.rows.len(), 4);

        let mut mismatches = Vec::new();
        for (name, values) in expected {
            let col_idx = table
                .columns
                .iter()
                .position(|c| c.name == *name)
                .unwrap_or_else(|| panic!("column {name} not found"));
            for (row_idx, (row, expected_value)) in result.rows.iter().zip(values).enumerate() {
                if row[col_idx] != *expected_value {
                    mismatches.push(format!(
                        "row {row_idx} {name}: got {:?}, expected {expected_value:?}",
                        row[col_idx]
                    ));
                }
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }

    #[test]
    fn calculated_text_values_match_jackcess() {
        assert_calculated_values_match(&[
            (
                "LastFirst",
                [
                    text("Wayne, Bruce"),
                    text("Simpson, Bart"),
                    text("Doe, John"),
                    text("User, Test"),
                ],
            ),
            (
                "AllNames",
                [
                    text("Wayne, Bruce=Wayne, Bruce"),
                    text("Simpson, Bart=Simpson, Bart"),
                    text("Doe, John=Doe, John"),
                    text("User, Test=User, Test"),
                ],
            ),
        ]);
    }

    #[test]
    fn calculated_number_values_match_jackcess() {
        assert_calculated_values_match(&[
            (
                "LastFirstLen",
                [
                    Value::Long(12),
                    Value::Long(13),
                    Value::Long(9),
                    Value::Long(10),
                ],
            ),
            (
                "WeeklySalary",
                [
                    numeric("19230.7692307692"),
                    numeric("-0.0192307692307692"),
                    numeric("0"),
                    numeric("1.92307692307692"),
                ],
            ),
            (
                "DecimalTest",
                [
                    numeric("50.325000"),
                    numeric("-36.222200"),
                    numeric("0.012300"),
                    numeric("102030405060.654321"),
                ],
            ),
            (
                "FloatTest",
                [
                    Value::Float(2583.2092),
                    Value::Float(0.0035889593),
                    Value::Float(0.0),
                    Value::Float(1.27413e-10),
                ],
            ),
            (
                "BigNumTest",
                [
                    numeric("56505085819.424791296572280180"),
                    numeric("-0.0784734499180612994241100748"),
                    numeric("0.00000000"),
                    numeric("0.0000002787019289824216980830"),
                ],
            ),
        ]);
    }

    #[test]
    fn calculated_currency_and_yes_no_values_match_jackcess() {
        let (yes, no) = (Value::Bool(true), Value::Bool(false));
        assert_calculated_values_match(&[
            (
                "MonthlySalary",
                [
                    money("83333.3333"),
                    money("-0.0833"),
                    money("0.0000"),
                    money("8.3333"),
                ],
            ),
            (
                "SalaryTest",
                [
                    money("1000000.0000"),
                    money("-1.0000"),
                    money("0.0000"),
                    money("100.0000"),
                ],
            ),
            ("IsRich", [yes.clone(), no.clone(), no.clone(), no]),
            ("BoolTest", [yes.clone(), yes.clone(), yes.clone(), yes]),
        ]);
    }

    /// The text values of every row of `table` in the Jet3 file `file`.
    fn jet3_texts(file: &std::path::Path, table: &str) -> Vec<String> {
        let mut reader = PageReader::open(file).unwrap();
        let catalog = crate::read_catalog(&mut reader).unwrap();
        let entry = crate::find_table(&catalog, table).unwrap();
        let tdef = crate::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let rows = read_table_rows(&mut reader, &tdef).unwrap().rows;
        rows.into_iter()
            .flatten()
            .filter_map(|v| match v {
                Value::Text(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn japanese_access_97_text_in_its_code_page() {
        // Made by testdata/jet3-generators/jet3-japanese-generator.vbs, in
        // code page 932: the names, values, description and query read as
        // Japanese, among them half-width katakana and ～, ① and 髙.
        let path = skip_if_missing!("V1997/japaneseTestV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert_eq!(
            reader.text_encoding(),
            TextEncoding::Jet3 { code_page: 932 }
        );
        let catalog = crate::read_catalog(&mut reader).unwrap();
        let entry = crate::find_table(&catalog, "顧客").unwrap();
        let tdef = crate::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let names: Vec<&str> = tdef.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["ID", "氏名", "備考"]);
        assert!(tdef.indexes.iter().any(|i| i.name == "氏名索引"));
        let properties = crate::entry_properties(entry, reader.text_encoding()).unwrap();
        let description = properties
            .maps
            .iter()
            .filter(|m| m.name == "氏名")
            .flat_map(|m| &m.properties)
            .find(|p| p.name == "Description")
            .map(|p| p.value.clone());
        assert!(matches!(description, Some(Value::Text(s)) if s == "顧客の氏名"));
        let queries = crate::read_queries(&mut reader).unwrap();
        let query = queries.iter().find(|q| q.name == "顧客一覧").unwrap();
        assert!(crate::query_to_sql(query).contains("WHERE 氏名 LIKE '山*'"));

        let texts = jet3_texts(&path, "顧客");
        let expected = [
            "山田太郎",
            "東京都千代田区",
            "ﾃｽﾄ",
            "①～髙",
            "Smith",
            "ASCII only",
        ];
        assert_eq!(texts, expected);
    }

    #[test]
    fn western_access_97_text_in_code_page_1252() {
        // Bytes 0x80 to 0x9F are characters such as € and ™ in code page
        // 1252, but for the five it leaves undefined, which stay controls.
        let path = skip_if_missing!("V1997/testIndexCodesV1997.mdb");
        let text: String = jet3_texts(&path, "Table14").concat();
        for c in ['€', '‚', '…', '‰', 'Š', '™', 'Ÿ'] {
            assert!(text.contains(c), "{c}");
        }
        let controls = text
            .chars()
            .filter(|c| ('\u{80}'..='\u{9F}').contains(c))
            .count();
        assert_eq!(controls, 5);
    }

    /// A column of `col_type`, the variable-length column 0 of a row.
    fn variable_column(col_type: ColumnType) -> ColumnDef {
        ColumnDef {
            name: "V".into(),
            col_type,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 0,
            flags: 0x02,
            is_fixed: false,
            scale: 2,
            precision: 18,
            is_calculated: false,
            display_index: 0,
            complex_id: None,
        }
    }

    /// The value of `bytes` read as the only variable-length column of a row,
    /// of `col_type`.
    fn variable_value(reader: &mut PageReader, col_type: ColumnType, bytes: &[u8]) -> Value {
        let cracked = CrackedRow {
            row_data: bytes,
            col_count: 1,
            null_mask: &[1],
            var_col_count: 1,
            var_offsets: vec![0, bytes.len() as u16],
        };
        read_variable_value(
            &cracked,
            &variable_column(col_type),
            TextEncoding::Unicode,
            reader,
        )
    }

    /// Checks that each of `cases`, a type with bytes and the value they
    /// hold, reads as the value, and as Null one byte short.
    fn assert_variable_values(cases: Vec<(ColumnType, Vec<u8>, Value)>) {
        // Fixed-size types are sometimes stored as variable-length, as in
        // system tables.
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        for (col_type, bytes, expected) in cases {
            assert_eq!(
                variable_value(&mut reader, col_type, &bytes),
                expected,
                "{col_type:?}"
            );
            let short = &bytes[..bytes.len() - 1];
            assert_eq!(
                variable_value(&mut reader, col_type, short),
                Value::Null,
                "{col_type:?}"
            );
        }
    }

    #[test]
    fn variable_values_of_integer_types() {
        assert_variable_values(vec![
            (ColumnType::Byte, vec![7], Value::Byte(7)),
            (ColumnType::Int, vec![0x34, 0x12], Value::Int(0x1234)),
            (ColumnType::Long, vec![1, 0, 0, 0], Value::Long(1)),
            (
                ColumnType::BigInt,
                2i64.to_le_bytes().to_vec(),
                Value::BigInt(2),
            ),
            (ColumnType::ComplexType, vec![5, 0, 0, 0], Value::Long(5)),
        ]);
    }

    #[test]
    fn variable_values_of_other_fixed_size_types() {
        let money = 12_345i64.to_le_bytes();
        let mut numeric = [0u8; 17];
        numeric[1] = 0x39;
        let guid: Vec<u8> = (1..=16).collect();
        let ext = ext_datetime_bytes(1, 2, 3);
        assert_variable_values(vec![
            (
                ColumnType::Float,
                1.5f32.to_le_bytes().to_vec(),
                Value::Float(1.5),
            ),
            (
                ColumnType::Double,
                2.5f64.to_le_bytes().to_vec(),
                Value::Double(2.5),
            ),
            (
                ColumnType::Money,
                money.to_vec(),
                Value::Money(crate::money::money_to_string(&money)),
            ),
            (
                ColumnType::Numeric,
                numeric.to_vec(),
                Value::Numeric(crate::money::numeric_to_string(&numeric, 2)),
            ),
            (
                ColumnType::Timestamp,
                3.25f64.to_le_bytes().to_vec(),
                Value::Timestamp(3.25),
            ),
            (
                ColumnType::Guid,
                guid.clone(),
                Value::Guid(format_guid(&guid)),
            ),
            (
                ColumnType::DateTimeExtended,
                ext.to_vec(),
                Value::DateTimeExtended(parse_ext_datetime(&ext).unwrap()),
            ),
        ]);
    }

    #[test]
    fn variable_values_that_cannot_be_read() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        // A DateTimeExtended that cannot be read is the bytes of the value.
        let mut broken = ext_datetime_bytes(1, 2, 3).to_vec();
        broken[19] = b'x';
        broken.push(9);
        assert_eq!(
            variable_value(&mut reader, ColumnType::DateTimeExtended, &broken),
            Value::Binary(broken.clone())
        );
        // A Boolean is not stored as variable-length.
        assert_eq!(
            variable_value(&mut reader, ColumnType::Boolean, &[1]),
            Value::Null
        );
    }

    #[test]
    fn calculated_values_too_short_or_of_other_types() {
        let read = |result_type, payload: &[u8]| {
            read_calculated_value(&calc_envelope(payload), result_type, TextEncoding::Unicode)
        };
        assert_eq!(read(ColumnType::Byte, &[7]), Value::Byte(7));
        assert_eq!(
            read(ColumnType::Float, &1.5f32.to_le_bytes()),
            Value::Float(1.5)
        );
        for (result_type, size) in [
            (ColumnType::Int, 2),
            (ColumnType::Long, 4),
            (ColumnType::Float, 4),
            (ColumnType::Double, 8),
            (ColumnType::Money, 8),
            (ColumnType::BigInt, 8),
            (ColumnType::Timestamp, 8),
            (ColumnType::Guid, 16),
            (ColumnType::Numeric, 16),
        ] {
            assert_eq!(
                read(result_type, &vec![1; size - 1]),
                Value::Null,
                "{result_type:?}"
            );
        }
        assert_eq!(read(ColumnType::Byte, &[]), Value::Null);
        assert_eq!(read(ColumnType::Boolean, &[]), Value::Null);
        // A calculated value is not of these types, or not in these forms.
        assert_eq!(read(ColumnType::ComplexType, &[1, 0, 0, 0]), Value::Null);
        assert_eq!(read(ColumnType::Numeric, &[1; 8]), Value::Null);
        let mut ext = ext_datetime_bytes(1, 2, 3).to_vec();
        ext.push(0);
        assert_eq!(read(ColumnType::DateTimeExtended, &ext), Value::Null);
    }
}
