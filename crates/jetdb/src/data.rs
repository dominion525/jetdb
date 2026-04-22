//! Data row reading and value extraction from table pages.

use std::collections::HashSet;

use crate::encoding;
use crate::file::{find_row, FileError, PageReader};
use crate::format::{row, ColumnType};
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

// ---------------------------------------------------------------------------
// read_table_rows — public entry point
// ---------------------------------------------------------------------------

/// Result of reading data rows from a table.
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
/// Returns a `ReadResult` containing the successfully parsed rows and a count
/// of rows that were skipped due to errors (e.g. corrupt row data).
pub fn read_table_rows(reader: &mut PageReader, table: &TableDef) -> Result<ReadResult, FileError> {
    let format = reader.format();
    let is_jet3 = reader.header().version.is_jet3();
    let mut rows = Vec::new();
    let mut skipped_rows = 0usize;

    for &page_num in &table.data_pages {
        let page_data = reader.read_page_copy(page_num)?;

        // Validate page type (Data = 1)
        if page_data.is_empty() || page_data[0] != 0x01 {
            continue;
        }

        let row_count_pos = format.data_row_count_pos;
        if page_data.len() < row_count_pos + 2 {
            continue;
        }
        let num_rows = u16::from_le_bytes([page_data[row_count_pos], page_data[row_count_pos + 1]]);

        for row_idx in 0..num_rows {
            // Read the raw row pointer to check flags before find_row
            let table_start = row_count_pos + 2;
            let entry_pos = table_start + (row_idx as usize) * 2;
            if entry_pos + 2 > page_data.len() {
                break;
            }
            let row_ptr = u16::from_le_bytes([page_data[entry_pos], page_data[entry_pos + 1]]);

            // Skip deleted rows
            if row_ptr & row::DELETE_FLAG != 0 {
                continue;
            }
            // Lookup/overflow rows: the row was relocated to this page from
            // another page, or this is a stale pointer to a row that was
            // moved away. If find_row succeeds, the data at the offset is
            // valid row data to be read normally. Stale entries where
            // find_row fails are skipped (not counted as errors).
            let is_lookup = row_ptr & row::LOOKUP_FLAG != 0;

            let (start, size) = match find_row(format, &page_data, page_num, row_idx) {
                Ok(v) => v,
                Err(e) => {
                    if is_lookup {
                        log::debug!(
                            "skipping stale lookup row on page {page_num} row {row_idx}: {e}"
                        );
                    } else {
                        log::debug!("skipping row on page {page_num} row {row_idx}: {e}");
                        skipped_rows += 1;
                    }
                    continue;
                }
            };

            let row_data = &page_data[start..start + size];
            let cracked = match crack_row(row_data, is_jet3) {
                Ok(c) => c,
                Err(e) => {
                    log::debug!("skipping row on page {page_num} row {row_idx}: {e}");
                    skipped_rows += 1;
                    continue;
                }
            };

            let mut values = Vec::with_capacity(table.columns.len());
            for col in &table.columns {
                let val = read_column_value(&cracked, col, is_jet3, reader);
                values.push(val);
            }
            rows.push(values);
        }
    }

    Ok(ReadResult { rows, skipped_rows })
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
fn crack_row<'a>(row_data: &'a [u8], is_jet3: bool) -> Result<CrackedRow<'a>, FileError> {
    if is_jet3 {
        crack_row_jet3(row_data)
    } else {
        crack_row_jet4(row_data)
    }
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
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for column count",
        });
    }

    let col_count = u16::from_le_bytes([row_data[0], row_data[1]]);
    let null_mask_len = (col_count as usize).div_ceil(8);

    // Read from end: null_mask, then var_col_count
    let tail_min = null_mask_len + 2; // null_mask + var_col_count
    if len < 2 + tail_min {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for null mask and var col count",
        });
    }

    let null_mask_start = len - null_mask_len;
    let null_mask = &row_data[null_mask_start..];

    let vcc_pos = null_mask_start - 2;
    let var_col_count = u16::from_le_bytes([row_data[vcc_pos], row_data[vcc_pos + 1]]);

    // Read offset table backwards from vcc_pos.
    // Entry count = var_col_count + 1 (includes EOD).
    // var_offsets[0] = EOD at (vcc_pos - 2)
    // var_offsets[k+1] = start offset of var col k at (vcc_pos - 2*(k+2))
    let offset_entries = var_col_count as usize + 1;
    let mut var_offsets = Vec::with_capacity(offset_entries);
    for i in 0..offset_entries {
        // `wrapping_sub` keeps us from panicking when the offset
        // table would extend past the start of the row. When that
        // happens `pos` wraps to a near-`usize::MAX` value; the
        // `pos + 2 > len` check below MUST use `checked_add` so
        // the compare itself doesn't overflow-panic in debug
        // (the original code's `pos + 2` did exactly that, which
        // is how MM's ScoringImprovement / ScoreLanes tables were
        // bringing down every parser run).
        let pos = vcc_pos.wrapping_sub(2 + i * 2);
        match pos.checked_add(2) {
            Some(end) if end <= len => {}
            _ => break,
        }
        var_offsets.push(u16::from_le_bytes([row_data[pos], row_data[pos + 1]]));
    }

    Ok(CrackedRow {
        row_data,
        col_count,
        null_mask,
        var_col_count,
        var_offsets,
    })
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
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for column count",
        });
    }

    let col_count = row_data[0] as u16;
    let null_mask_len = (col_count as usize).div_ceil(8);

    let null_mask_start = len - null_mask_len;
    if null_mask_start == 0 {
        return Ok(CrackedRow {
            row_data,
            col_count,
            null_mask: &row_data[null_mask_start..],
            var_col_count: 0,
            var_offsets: Vec::new(),
        });
    }
    let null_mask = &row_data[null_mask_start..];

    // var_col_count is at null_mask_start - 1
    let vcc_pos = null_mask_start - 1;
    if vcc_pos == 0 {
        return Ok(CrackedRow {
            row_data,
            col_count,
            null_mask,
            var_col_count: 0,
            var_offsets: Vec::new(),
        });
    }
    let var_col_count = row_data[vcc_pos] as u16;

    // Jump table is between var_col_count and the offset table.
    // num_jumps = (row_len - 1) / 256
    let num_jumps = if len > 1 { (len - 1) / 256 } else { 0 };

    // col_ptr = vcc_pos - num_jumps - 1 (start of offset table, reading backwards)
    let col_ptr = vcc_pos.saturating_sub(num_jumps + 1);

    // Offset entries: var_col_count + 1 (includes EOD), each 1 byte
    let offset_entries = var_col_count as usize + 1;

    // Dummy jump check:
    // If last jump is a dummy value, ignore it
    let mut actual_num_jumps = num_jumps;
    if actual_num_jumps > 0 && col_ptr.saturating_sub(offset_entries) / 256 < actual_num_jumps {
        actual_num_jumps -= 1;
    }

    if col_ptr < offset_entries {
        return Err(FileError::InvalidRow {
            page: 0,
            row: 0,
            reason: "row too short for variable offset table (Jet3)",
        });
    }

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

    Ok(CrackedRow {
        row_data,
        col_count,
        null_mask,
        var_col_count,
        var_offsets,
    })
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
) -> Value {
    // Boolean is special: value comes from the null mask
    if col.col_type == ColumnType::Boolean {
        return Value::Bool(!is_null(cracked.null_mask, col.col_num));
    }

    // All other types: check null first
    if is_null(cracked.null_mask, col.col_num) {
        return Value::Null;
    }

    if col.is_fixed {
        read_fixed_value(cracked, col, is_jet3)
    } else {
        read_variable_value(cracked, col, is_jet3, reader)
    }
}

/// Read a fixed-length column value.
fn read_fixed_value(cracked: &CrackedRow<'_>, col: &ColumnDef, is_jet3: bool) -> Value {
    let col_count_size = if is_jet3 { 1usize } else { 2usize };
    let offset = col_count_size + col.fixed_offset as usize;
    let data = cracked.row_data;

    match col.col_type {
        ColumnType::Boolean => unreachable!("handled above"),
        ColumnType::Byte => {
            if offset < data.len() {
                Value::Byte(data[offset])
            } else {
                Value::Null
            }
        }
        ColumnType::Int => {
            if offset + 2 <= data.len() {
                Value::Int(i16::from_le_bytes([data[offset], data[offset + 1]]))
            } else {
                Value::Null
            }
        }
        ColumnType::Long => {
            if offset + 4 <= data.len() {
                Value::Long(i32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]))
            } else {
                Value::Null
            }
        }
        ColumnType::BigInt => {
            if offset + 8 <= data.len() {
                let Ok(bytes) = data[offset..offset + 8].try_into() else {
                    return Value::Null;
                };
                Value::BigInt(i64::from_le_bytes(bytes))
            } else {
                Value::Null
            }
        }
        ColumnType::Float => {
            if offset + 4 <= data.len() {
                let Ok(bytes) = data[offset..offset + 4].try_into() else {
                    return Value::Null;
                };
                Value::Float(f32::from_le_bytes(bytes))
            } else {
                Value::Null
            }
        }
        ColumnType::Double => {
            if offset + 8 <= data.len() {
                let Ok(bytes) = data[offset..offset + 8].try_into() else {
                    return Value::Null;
                };
                Value::Double(f64::from_le_bytes(bytes))
            } else {
                Value::Null
            }
        }
        ColumnType::Money => {
            if offset + 8 <= data.len() {
                let Ok(bytes): Result<[u8; 8], _> = data[offset..offset + 8].try_into() else {
                    return Value::Null;
                };
                Value::Money(money::money_to_string(&bytes))
            } else {
                Value::Null
            }
        }
        ColumnType::Numeric => {
            if offset + 17 <= data.len() {
                let Ok(bytes): Result<[u8; 17], _> = data[offset..offset + 17].try_into() else {
                    return Value::Null;
                };
                Value::Numeric(money::numeric_to_string(&bytes, col.scale))
            } else {
                Value::Null
            }
        }
        ColumnType::Timestamp => {
            if offset + 8 <= data.len() {
                let Ok(bytes) = data[offset..offset + 8].try_into() else {
                    return Value::Null;
                };
                Value::Timestamp(f64::from_le_bytes(bytes))
            } else {
                Value::Null
            }
        }
        ColumnType::Guid => {
            if offset + 16 <= data.len() {
                Value::Guid(format_guid(&data[offset..offset + 16]))
            } else {
                Value::Null
            }
        }
        ColumnType::ComplexType => {
            if offset + 4 <= data.len() {
                let Ok(bytes) = data[offset..offset + 4].try_into() else {
                    return Value::Null;
                };
                Value::Long(i32::from_le_bytes(bytes))
            } else {
                Value::Null
            }
        }
        ColumnType::DateTimeExtended => {
            if offset + 42 <= data.len() {
                match parse_ext_datetime(&data[offset..offset + 42]) {
                    Some(s) => Value::DateTimeExtended(s),
                    None => Value::Binary(data[offset..offset + 42].to_vec()),
                }
            } else {
                Value::Null
            }
        }
        // Unknown fixed-size types: read as raw binary
        ColumnType::Unknown(_) => {
            let size = col.col_size as usize;
            if size > 0 && offset + size <= data.len() {
                Value::Binary(data[offset..offset + size].to_vec())
            } else {
                Value::Null
            }
        }
        // Variable-length types should not reach here, but handle gracefully
        _ => Value::Null,
    }
}

/// Read a variable-length column value.
fn read_variable_value(
    cracked: &CrackedRow<'_>,
    col: &ColumnDef,
    is_jet3: bool,
    reader: &mut PageReader,
) -> Value {
    // var_offsets is read backwards from vcc_pos:
    // Data for var col k: row_data[var_offsets[k]..var_offsets[k+1]]
    let var_idx = col.var_col_num as usize;

    // Need var_offsets[var_idx] (start) and var_offsets[var_idx+1] (end)
    if var_idx + 1 >= cracked.var_offsets.len() {
        return Value::Null;
    }

    let start = cracked.var_offsets[var_idx] as usize;
    let end = cracked.var_offsets[var_idx + 1] as usize;

    if start >= end || end > cracked.row_data.len() {
        return Value::Null;
    }

    let var_data = &cracked.row_data[start..end];

    match col.col_type {
        ColumnType::Text => match encoding::decode_text(var_data, is_jet3) {
            Ok(s) => Value::Text(s),
            Err(_) => Value::Null,
        },
        ColumnType::Binary | ColumnType::Unknown(_) => Value::Binary(var_data.to_vec()),
        ColumnType::Memo => read_memo_value(var_data, is_jet3, Some(reader)),
        ColumnType::Ole => read_ole_value(var_data, Some(reader)),
        // Fixed-size types sometimes stored as variable-length (e.g. system tables)
        ColumnType::Byte if !var_data.is_empty() => Value::Byte(var_data[0]),
        ColumnType::Int if var_data.len() >= 2 => {
            Value::Int(i16::from_le_bytes([var_data[0], var_data[1]]))
        }
        ColumnType::Long if var_data.len() >= 4 => {
            let Ok(bytes) = var_data[..4].try_into() else {
                return Value::Null;
            };
            Value::Long(i32::from_le_bytes(bytes))
        }
        ColumnType::BigInt if var_data.len() >= 8 => {
            let Ok(bytes) = var_data[..8].try_into() else {
                return Value::Null;
            };
            Value::BigInt(i64::from_le_bytes(bytes))
        }
        ColumnType::Float if var_data.len() >= 4 => {
            let Ok(bytes) = var_data[..4].try_into() else {
                return Value::Null;
            };
            Value::Float(f32::from_le_bytes(bytes))
        }
        ColumnType::Double if var_data.len() >= 8 => {
            let Ok(bytes) = var_data[..8].try_into() else {
                return Value::Null;
            };
            Value::Double(f64::from_le_bytes(bytes))
        }
        ColumnType::Money if var_data.len() >= 8 => {
            let Ok(bytes): Result<[u8; 8], _> = var_data[..8].try_into() else {
                return Value::Null;
            };
            Value::Money(money::money_to_string(&bytes))
        }
        ColumnType::Numeric if var_data.len() >= 17 => {
            let Ok(bytes): Result<[u8; 17], _> = var_data[..17].try_into() else {
                return Value::Null;
            };
            Value::Numeric(money::numeric_to_string(&bytes, col.scale))
        }
        ColumnType::Timestamp if var_data.len() >= 8 => {
            let Ok(bytes) = var_data[..8].try_into() else {
                return Value::Null;
            };
            Value::Timestamp(f64::from_le_bytes(bytes))
        }
        ColumnType::Guid if var_data.len() >= 16 => Value::Guid(format_guid(&var_data[..16])),
        ColumnType::ComplexType if var_data.len() >= 4 => {
            let Ok(bytes) = var_data[..4].try_into() else {
                return Value::Null;
            };
            Value::Long(i32::from_le_bytes(bytes))
        }
        ColumnType::DateTimeExtended if var_data.len() >= 42 => {
            match parse_ext_datetime(&var_data[..42]) {
                Some(s) => Value::DateTimeExtended(s),
                None => Value::Binary(var_data.to_vec()),
            }
        }
        _ => Value::Null,
    }
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
    if var_data.len() < 4 {
        return None;
    }
    let length_with_flags = u32::from_le_bytes(var_data[..4].try_into().ok()?);
    let memo_type = length_with_flags & LVAL_TYPE_MASK;
    let data_len = (length_with_flags & !LVAL_TYPE_MASK) as usize;

    if memo_type == LVAL_INLINE {
        // Inline: data starts at offset 12
        let data_start = LVAL_INLINE_HEADER.min(var_data.len());
        let data_end = (data_start + data_len).min(var_data.len());
        if data_start > var_data.len() {
            return None;
        }
        Some(var_data[data_start..data_end].to_vec())
    } else if memo_type == LVAL_SINGLE_PAGE {
        // Single-page overflow: read from the referenced LVAL page row
        let reader = reader?;
        if var_data.len() < 8 {
            return None;
        }
        let pg_row = u32::from_le_bytes(var_data[4..8].try_into().ok()?);
        reader.read_pg_row(pg_row).ok()
    } else if memo_type == LVAL_MULTI_PAGE {
        // Multi-page overflow: chain of LVAL page rows
        let reader = reader?;
        if var_data.len() < 8 {
            return None;
        }
        let mut pg_row = u32::from_le_bytes(var_data[4..8].try_into().ok()?);
        let mut buf = Vec::with_capacity(data_len.min(MAX_LVAL_INITIAL_CAP));
        let mut visited = HashSet::new();

        while pg_row != 0 {
            if !visited.insert(pg_row) {
                return None; // circular reference — partial data is unreliable
            }
            let row_data = reader.read_pg_row(pg_row).ok()?;
            if row_data.len() < 4 {
                return None;
            }
            let next_pg_row = u32::from_le_bytes(row_data[..4].try_into().ok()?);
            buf.extend_from_slice(&row_data[4..]);
            pg_row = next_pg_row;

            // Safety: stop if we've already collected enough data
            if buf.len() >= data_len {
                break;
            }
        }

        if buf.len() > data_len {
            buf.truncate(data_len);
        }

        Some(buf)
    } else {
        // Unknown LVAL type
        None
    }
}

/// Read a Memo field value.
fn read_memo_value(var_data: &[u8], is_jet3: bool, reader: Option<&mut PageReader>) -> Value {
    match read_lval_data(var_data, reader) {
        Some(data) => match encoding::decode_text(&data, is_jet3) {
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
    if buf.len() < 42 {
        return None;
    }
    let days_str = std::str::from_utf8(&buf[0..19]).ok()?;
    let secs_str = std::str::from_utf8(&buf[20..32]).ok()?;
    let nanos_str = std::str::from_utf8(&buf[32..39]).ok()?;

    let days: i64 = days_str.trim_start_matches('0').parse().unwrap_or(0);
    let seconds: i64 = secs_str.trim_start_matches('0').parse().unwrap_or(0);
    let nanos100: i64 = nanos_str.trim_start_matches('0').parse().unwrap_or(0);

    Some(timestamp::format_ext_datetime(days, seconds, nanos100))
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

        let val = read_memo_value(&var_data, false, None);
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

        let val = read_memo_value(&var_data, true, None);
        assert_eq!(val, Value::Text("Hi".to_string()));
    }

    #[test]
    fn memo_overflow_without_reader_returns_null() {
        // Single-page overflow (bit 30 set), no reader provided
        let flags: u32 = LVAL_SINGLE_PAGE | 100;
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]); // page ref + padding

        let val = read_memo_value(&var_data, false, None);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn memo_multi_page_without_reader_returns_null() {
        // Multi-page overflow (no type bits set), no reader provided
        let flags: u32 = 500; // no high bits
        let mut var_data = Vec::new();
        var_data.extend_from_slice(&flags.to_le_bytes());
        var_data.extend_from_slice(&[0u8; 8]);

        let val = read_memo_value(&var_data, false, None);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn memo_too_short_returns_null() {
        // Less than 4 bytes
        let val = read_memo_value(&[0x01, 0x02], false, None);
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

    fn assert_msysobjects_rows(rows: &[Vec<Value>], table: &TableDef) {
        assert!(!rows.is_empty(), "MSysObjects should have at least one row");

        // Find column indices
        let id_idx = table
            .columns
            .iter()
            .position(|c| c.name == "Id")
            .expect("Id column");
        let name_idx = table
            .columns
            .iter()
            .position(|c| c.name == "Name")
            .expect("Name column");
        let type_idx = table
            .columns
            .iter()
            .position(|c| c.name == "Type")
            .expect("Type column");

        for row in rows {
            assert_eq!(row.len(), table.columns.len());

            // Id should be a non-null Long
            match &row[id_idx] {
                Value::Long(_) => {}
                other => panic!("Expected Long for Id, got: {other:?}"),
            }

            // Name should be a non-null non-empty Text
            match &row[name_idx] {
                Value::Text(s) => assert!(!s.is_empty(), "Name should not be empty"),
                other => panic!("Expected Text for Name, got: {other:?}"),
            }

            // Type should be a non-null Int
            match &row[type_idx] {
                Value::Int(_) => {}
                other => panic!("Expected Int for Type, got: {other:?}"),
            }
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

        let val = read_memo_value(&var_data, false, None);
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

    #[test]
    fn dispatch_boolean_from_null_mask() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let _table =
            crate::table::read_table_def(&mut reader, "MSysObjects", crate::format::CATALOG_PAGE)
                .unwrap();

        // Synthesize a cracked row with a Boolean ColumnDef.
        let mut row_data = vec![0x02, 0x00]; // col_count = 2
        row_data.extend_from_slice(&[0x00, 0x00]); // fixed data placeholder
        row_data.extend_from_slice(&4u16.to_le_bytes()); // EOD
        row_data.extend_from_slice(&0u16.to_le_bytes()); // var_col_count = 0
        row_data.push(0b00000010); // null_mask: col 1 NOT NULL, col 0 NULL

        let cracked = crack_row_jet4(&row_data).unwrap();
        let bool_col = ColumnDef {
            name: "Flag".into(),
            col_type: ColumnType::Boolean,
            col_num: 1, // bit 1 is set → true
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 0,
            flags: 0x01,
            is_fixed: true,
            scale: 0,
            precision: 0,
        };
        assert_eq!(
            read_column_value(&cracked, &bool_col, false, &mut reader),
            Value::Bool(true)
        );

        // col_num 0 → bit 0 is clear → false
        let bool_col_false = ColumnDef {
            col_num: 0,
            ..bool_col.clone()
        };
        assert_eq!(
            read_column_value(&cracked, &bool_col_false, false, &mut reader),
            Value::Bool(false)
        );
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
        };
        assert_eq!(
            read_column_value(&cracked, &col, false, &mut reader),
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
        };
        assert_eq!(
            read_column_value(&cracked, &col, false, &mut reader),
            Value::Int(-42)
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
        }
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
            read_variable_value(&cracked, &col, false, &mut reader),
            Value::Null
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
        let mut buf = [0u8; 42];
        let s = b"0000000000000737954:000000045900000000000:7";
        buf[..42].copy_from_slice(&s[..42]);
        buf[41] = 0x00;
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
        // Non-digit ASCII chars → parse fails → unwrap_or(0) → epoch
        let mut buf = [b'x'; 42];
        buf[19] = b':';
        buf[39] = b':';
        buf[40] = b'7';
        buf[41] = 0x00;
        let result = parse_ext_datetime(&buf);
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("0001-01-01"));
    }

    #[test]
    fn read_fixed_datetime_extended() {
        // Build a 42-byte ASCII payload for 2020-06-17 (date only)
        let ascii = b"0000000000000737592:000000000000000000000:7\x00";
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
}
