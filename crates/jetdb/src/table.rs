//! Table definition (TDEF page) parsing: columns, indexes, and data page lists.

use std::collections::HashSet;

use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::format::{ColumnType, JetFormat, PageType, MAX_INDEX_COLUMNS};
use crate::map;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Sort order for an index column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexColumnOrder {
    Ascending,
    Descending,
}

/// A single column within an index definition.
#[derive(Debug, Clone)]
pub struct IndexColumn {
    /// Column number (corresponds to `ColumnDef.col_num`).
    pub col_num: u16,
    /// Sort order.
    pub order: IndexColumnOrder,
}

/// Foreign key reference information (for `index_type == 2`).
#[derive(Debug, Clone)]
pub struct ForeignKeyReference {
    /// FK index type (0x00 or 0x01).
    pub fk_index_type: u8,
    /// FK index number.
    pub fk_index_number: u32,
    /// FK table page number.
    pub fk_table_page: u32,
    /// Update action flag.
    pub update_action: u8,
    /// Delete action flag.
    pub delete_action: u8,
}

/// A single index definition parsed from a TDEF page.
#[derive(Debug, Clone)]
pub struct IndexDef {
    /// Index name.
    pub name: String,
    /// Logical index number.
    pub index_num: u16,
    /// Index type: 0x00 = ordinary, 0x01 = primary key, 0x02 = FK reference (see `format::index_type`).
    pub index_type: u8,
    /// Columns in this index (empty for FK type=2).
    pub columns: Vec<IndexColumn>,
    /// Index flags (UNIQUE, IGNORE_NULLS, REQUIRED).
    pub flags: u8,
    /// B-tree root page number (0 for FK type=2 indexes).
    pub first_data_page: u32,
    /// Foreign key info (only for type=2 indexes).
    pub foreign_key: Option<ForeignKeyReference>,
}

/// A single column definition parsed from a TDEF page.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub col_type: ColumnType,
    pub col_num: u16,
    pub var_col_num: u16,
    pub fixed_offset: u16,
    pub col_size: u16,
    pub flags: u8,
    pub is_fixed: bool,
    /// Scale for Numeric columns (number of decimal places).
    pub scale: u8,
    /// Precision for Numeric columns.
    pub precision: u8,
}

/// A parsed table definition.
#[derive(Debug, Clone)]
pub struct TableDef {
    pub name: String,
    pub num_rows: u32,
    pub num_cols: u16,
    pub num_var_cols: u16,
    pub columns: Vec<ColumnDef>,
    pub indexes: Vec<IndexDef>,
    pub data_pages: Vec<u32>,
}

/// Return `true` if the column has the REPLICATION flag set.
pub fn is_replication_column(col: &ColumnDef) -> bool {
    (col.flags & crate::format::column_flags::REPLICATION) != 0
}

// ---------------------------------------------------------------------------
// Private types
// ---------------------------------------------------------------------------

/// Physical index entry: (columns, flags, first_data_page).
type PhysicalIndexEntry = (Vec<IndexColumn>, u8, u32);

/// Logical index entry parsed from TDEF section [6].
struct LogicalIndex {
    index_num: u16,
    index_col_entry: u32,
    fk_index_type: u8,
    fk_index_number: u32,
    fk_table_page: u32,
    update_action: u8,
    delete_action: u8,
    index_type: u8,
}

// ---------------------------------------------------------------------------
// read_table_def
// ---------------------------------------------------------------------------

/// Read and parse a table definition (TDEF) from the database.
///
/// `name` is the table name (stored in the returned `TableDef`).
/// `tdef_page` is the first TDEF page number.
pub fn read_table_def(
    reader: &mut PageReader,
    name: &str,
    tdef_page: u32,
) -> Result<TableDef, FileError> {
    let is_jet3 = reader.header().version.is_jet3();

    // 3a. Build TDEF buffer (multi-page support)
    let tdef_buf = build_tdef_buffer(reader, tdef_page)?;

    let format = reader.format();
    let cursor = &mut TdefCursor::new(&tdef_buf, 0);

    // 3b. Header fields (positional reads)
    let num_rows = cursor.u32_le_at(format.tdef_row_count_pos)?;
    let num_var_cols = cursor.u16_le_at(format.tdef_var_col_count_pos)?;
    let num_cols = cursor.u16_le_at(format.tdef_column_count_pos)?;
    let num_idxs = cursor.u32_le_at(format.tdef_index_count_pos)?;
    let num_real_idxs = cursor.u32_le_at(format.tdef_real_index_count_pos)?;

    // 3c. Data pages via owned-pages usage map
    let pg_row = cursor.u32_le_at(format.tdef_owned_pages_pos)?;
    let data_pages = if pg_row != 0 {
        let map_data = reader.read_pg_row(pg_row)?;
        map::collect_page_numbers(reader, &map_data)?
    } else {
        Vec::new()
    };

    // 3d. Column entries
    let col_entry_start =
        format.tdef_index_entries_pos + (num_real_idxs as usize) * format.tdef_index_entry_span;
    cursor.set_position(col_entry_start);
    let mut columns = parse_column_entries(
        cursor,
        format.tdef_column_entry_span,
        num_cols as usize,
        is_jet3,
        format,
    )?;

    // 3e. Column names (cursor is already at correct position)
    let col_names = read_names(cursor, num_cols as usize, is_jet3)?;
    for (col, col_name) in columns.iter_mut().zip(col_names) {
        col.name = col_name;
    }

    // 3f. Index column definitions
    let mut idx_col_defs = parse_index_column_defs(cursor, num_real_idxs, format)?;

    // 3g. Logical index definitions
    let logical_indexes = parse_logical_indexes(cursor, num_idxs, format)?;

    // Adjust idx_col_defs length based on actual non-FK count in section [6].
    let non_fk_count = logical_indexes
        .iter()
        .filter(|li| li.index_type != crate::format::index_type::FOREIGN_KEY)
        .count();
    if non_fk_count != idx_col_defs.len() {
        idx_col_defs.truncate(non_fk_count);
    }

    // 3h. Index names
    let idx_names = read_names(cursor, num_idxs as usize, is_jet3)?;

    // 3i. Build index defs
    let indexes = build_index_defs(&logical_indexes, &idx_col_defs, idx_names);

    // 3j. Sort columns by col_num
    columns.sort_by_key(|c| c.col_num);

    Ok(TableDef {
        name: name.to_string(),
        num_rows,
        num_cols,
        num_var_cols,
        columns,
        indexes,
        data_pages,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Build a contiguous TDEF buffer by following the next-page chain.
fn build_tdef_buffer(reader: &mut PageReader, tdef_page: u32) -> Result<Vec<u8>, FileError> {
    let first_page = reader.read_page_copy(tdef_page)?;

    // Validate page type
    if first_page.is_empty() || first_page[0] != PageType::TableDefinition as u8 {
        return Err(FileError::InvalidTableDef {
            reason: "first page is not a TableDefinition page",
        });
    }

    // Next-page pointer at offset 4 of the first page
    let mut next = u32::from_le_bytes([first_page[4], first_page[5], first_page[6], first_page[7]]);
    let mut buf = first_page;

    // Follow continuation pages (skip their 8-byte header)
    let mut visited = HashSet::new();
    while next != 0 {
        if !visited.insert(next) {
            return Err(FileError::InvalidTableDef {
                reason: "circular page reference in TDEF chain",
            });
        }
        let cont_page = reader.read_page_copy(next)?;
        if cont_page.len() > 8 {
            buf.extend_from_slice(&cont_page[8..]);
        }
        next = u32::from_le_bytes([cont_page[4], cont_page[5], cont_page[6], cont_page[7]]);
    }

    Ok(buf)
}

// ---------------------------------------------------------------------------
// TdefCursor — byte I/O abstraction for TDEF buffer parsing
// ---------------------------------------------------------------------------

struct TdefCursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> TdefCursor<'a> {
    fn new(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }

    fn position(&self) -> usize {
        self.pos
    }

    fn set_position(&mut self, pos: usize) {
        self.pos = pos;
    }

    // --- Sequential reads (advance cursor position) ---

    fn read_u8(&mut self) -> Result<u8, FileError> {
        if self.pos >= self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u16_le(&mut self) -> Result<u16, FileError> {
        if self.pos + 2 > self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        let v = u16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32_le(&mut self) -> Result<u32, FileError> {
        if self.pos + 4 > self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        let v = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], FileError> {
        if self.pos + n > self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        let slice = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn skip(&mut self, n: usize) -> Result<(), FileError> {
        if self.pos + n > self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        self.pos += n;
        Ok(())
    }

    // --- Positional reads (cursor position unchanged) ---

    fn u8_at(&self, pos: usize) -> Result<u8, FileError> {
        if pos >= self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        Ok(self.buf[pos])
    }

    fn u16_le_at(&self, pos: usize) -> Result<u16, FileError> {
        if pos + 2 > self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        Ok(u16::from_le_bytes([self.buf[pos], self.buf[pos + 1]]))
    }

    fn u32_le_at(&self, pos: usize) -> Result<u32, FileError> {
        if pos + 4 > self.buf.len() {
            return Err(FileError::InvalidTableDef {
                reason: "unexpected end of TDEF buffer",
            });
        }
        Ok(u32::from_le_bytes([
            self.buf[pos],
            self.buf[pos + 1],
            self.buf[pos + 2],
            self.buf[pos + 3],
        ]))
    }
}

/// Read a sequence of names from the TDEF buffer via cursor.
///
/// Jet3 uses `[len: u8][Latin-1 bytes]`, Jet4+ uses `[len: u16 LE][UTF-16LE bytes]`.
fn read_names(
    cursor: &mut TdefCursor,
    count: usize,
    is_jet3: bool,
) -> Result<Vec<String>, FileError> {
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        if is_jet3 {
            let name_len = cursor.read_u8()? as usize;
            let bytes = cursor.read_bytes(name_len)?;
            names.push(encoding::decode_latin1(bytes));
        } else {
            let name_len = cursor.read_u16_le()? as usize;
            let bytes = cursor.read_bytes(name_len)?;
            names.push(encoding::decode_utf16le(bytes).map_err(|_| {
                FileError::InvalidTableDef {
                    reason: "invalid UTF-16LE name",
                }
            })?);
        }
    }
    Ok(names)
}

/// Parse column definition entries from the TDEF buffer via cursor.
fn parse_column_entries(
    cursor: &mut TdefCursor,
    span: usize,
    count: usize,
    is_jet3: bool,
    format: &JetFormat,
) -> Result<Vec<ColumnDef>, FileError> {
    let mut columns = Vec::with_capacity(count);
    for _ in 0..count {
        let entry_start = cursor.position();

        let col_type = ColumnType::try_from(cursor.u8_at(entry_start)?)?;

        let (col_num, var_col_num) = if is_jet3 {
            (
                cursor.u8_at(entry_start + format.coldef_number_pos)? as u16,
                cursor.u16_le_at(entry_start + format.coldef_var_col_index_pos)?,
            )
        } else {
            (
                cursor.u16_le_at(entry_start + format.coldef_number_pos)?,
                cursor.u16_le_at(entry_start + format.coldef_var_col_index_pos)?,
            )
        };

        let flags = cursor.u8_at(entry_start + format.coldef_flags_pos)?;
        let is_fixed = (flags & crate::format::column_flags::FIXED) != 0;
        let fixed_offset = cursor.u16_le_at(entry_start + format.coldef_fixed_data_pos)?;
        let col_size = cursor.u16_le_at(entry_start + format.coldef_length_pos)?;
        let scale = cursor.u8_at(entry_start + format.coldef_scale_pos)?;
        let precision = cursor.u8_at(entry_start + format.coldef_precision_pos)?;

        columns.push(ColumnDef {
            name: String::new(), // filled by read_names
            col_type,
            col_num,
            var_col_num,
            fixed_offset,
            col_size,
            flags,
            is_fixed,
            scale,
            precision,
        });

        cursor.set_position(entry_start + span);
    }
    Ok(columns)
}

/// Parse index column definitions from TDEF section [5].
fn parse_index_column_defs(
    cursor: &mut TdefCursor,
    count: u32,
    format: &JetFormat,
) -> Result<Vec<PhysicalIndexEntry>, FileError> {
    let mut idx_col_defs = Vec::with_capacity(count as usize);

    for _ in 0..count {
        cursor.skip(format.idx_col_skip_before)?;

        let mut idx_columns = Vec::new();
        for _ in 0..MAX_INDEX_COLUMNS {
            let col_id = cursor.read_u16_le()?;
            let order_flag = cursor.read_u8()?;

            if col_id != 0xFFFF {
                let order = if order_flag == 0x01 {
                    IndexColumnOrder::Ascending
                } else {
                    IndexColumnOrder::Descending
                };
                idx_columns.push(IndexColumn {
                    col_num: col_id,
                    order,
                });
            }
        }

        cursor.skip(4)?; // usage map reference
        let first_pg = cursor.read_u32_le()?;
        cursor.skip(format.idx_col_skip_before_flags)?;
        let idx_flags = cursor.read_u8()?;
        cursor.skip(format.idx_col_skip_after_flags)?;

        idx_col_defs.push((idx_columns, idx_flags, first_pg));
    }

    Ok(idx_col_defs)
}

/// Parse logical index definitions from TDEF section [6].
fn parse_logical_indexes(
    cursor: &mut TdefCursor,
    count: u32,
    format: &JetFormat,
) -> Result<Vec<LogicalIndex>, FileError> {
    let mut logical_indexes = Vec::with_capacity(count as usize);

    for _ in 0..count {
        let entry_start = cursor.position();

        cursor.skip(format.idx_info_skip_before)?;
        let index_num = cursor.read_u16_le()?;
        cursor.skip(2)?; // padding
        let index_col_entry = cursor.read_u32_le()?;
        let fk_index_type = cursor.read_u8()?;
        let fk_index_number = cursor.read_u32_le()?;
        let fk_table_page = cursor.read_u32_le()?;
        let update_action = cursor.read_u8()?;
        let delete_action = cursor.read_u8()?;
        let index_type = cursor.u8_at(entry_start + format.idx_info_type_offset)?;

        logical_indexes.push(LogicalIndex {
            index_num,
            index_col_entry,
            fk_index_type,
            fk_index_number,
            fk_table_page,
            update_action,
            delete_action,
            index_type,
        });

        cursor.set_position(entry_start + format.idx_info_block_size);
    }

    Ok(logical_indexes)
}

/// Combine logical indexes, column definitions, and names into `IndexDef` entries.
fn build_index_defs(
    logical_indexes: &[LogicalIndex],
    idx_col_defs: &[PhysicalIndexEntry],
    idx_names: Vec<String>,
) -> Vec<IndexDef> {
    let mut indexes = Vec::with_capacity(logical_indexes.len());
    for (i, logical) in logical_indexes.iter().enumerate() {
        let name = idx_names.get(i).cloned().unwrap_or_default();

        if logical.index_type == crate::format::index_type::FOREIGN_KEY {
            indexes.push(IndexDef {
                name,
                index_num: logical.index_num,
                index_type: logical.index_type,
                columns: Vec::new(),
                flags: 0,
                first_data_page: 0,
                foreign_key: Some(ForeignKeyReference {
                    fk_index_type: logical.fk_index_type,
                    fk_index_number: logical.fk_index_number,
                    fk_table_page: logical.fk_table_page,
                    update_action: logical.update_action,
                    delete_action: logical.delete_action,
                }),
            });
        } else {
            let col_entry_idx = logical.index_col_entry as usize;
            let (cols, flags, first_pg) = if col_entry_idx < idx_col_defs.len() {
                idx_col_defs[col_entry_idx].clone()
            } else {
                log::warn!(
                    "index '{}': column entry index {} out of range (max {})",
                    name,
                    col_entry_idx,
                    idx_col_defs.len()
                );
                (Vec::new(), 0, 0)
            };
            indexes.push(IndexDef {
                name,
                index_num: logical.index_num,
                index_type: logical.index_type,
                columns: cols,
                flags,
                first_data_page: first_pg,
                foreign_key: None,
            });
        }
    }
    indexes
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ColumnType;
    use crate::format::{column_flags, CATALOG_PAGE};

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

    fn assert_msysobjects(tdef: &TableDef) {
        assert!(
            tdef.num_cols > 0,
            "MSysObjects should have at least one column"
        );

        let col_names: Vec<&str> = tdef.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(
            col_names.contains(&"Id"),
            "MSysObjects should have 'Id' column, found: {col_names:?}"
        );
        assert!(
            col_names.contains(&"Name"),
            "MSysObjects should have 'Name' column, found: {col_names:?}"
        );
        assert!(
            col_names.contains(&"Type"),
            "MSysObjects should have 'Type' column, found: {col_names:?}"
        );

        assert!(
            !tdef.data_pages.is_empty(),
            "MSysObjects should have at least one data page"
        );
    }

    #[test]
    fn jet3_msysobjects() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn jet4_msysobjects() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn ace12_msysobjects() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn ace14_msysobjects() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn ace17_msysobjects() {
        let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        assert_msysobjects(&tdef);
    }

    #[test]
    fn columns_sorted_by_col_num() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let tdef = read_table_def(&mut reader, "MSysObjects", CATALOG_PAGE).unwrap();
        for w in tdef.columns.windows(2) {
            assert!(
                w[0].col_num <= w[1].col_num,
                "columns should be sorted by col_num"
            );
        }
    }

    #[test]
    fn invalid_page_type_error() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        // Page 1 is typically a data/bitmap page, not TDEF
        let result = read_table_def(&mut reader, "bad", 1);
        assert!(result.is_err());
    }

    // -- Index tests ----------------------------------------------------------

    /// Helper: find a user table's TDEF page from the catalog.
    fn find_table_page(reader: &mut PageReader, table_name: &str) -> Option<u32> {
        let catalog = crate::catalog::read_catalog(reader).ok()?;
        catalog
            .iter()
            .find(|e| e.name == table_name)
            .map(|e| e.table_page)
    }

    fn assert_user_table_indexes(path: &std::path::Path, table_name: &str) -> TableDef {
        let mut reader = PageReader::open(path).unwrap();
        let page = find_table_page(&mut reader, table_name)
            .unwrap_or_else(|| panic!("table '{table_name}' not found in catalog"));
        read_table_def(&mut reader, table_name, page).unwrap()
    }

    #[test]
    fn jet4_index_count() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "Table1 should have at least one index"
        );
    }

    #[test]
    fn jet3_index_count() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "Jet3 Table1 should have at least one index"
        );
    }

    #[test]
    fn ace12_index_count() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "ACE12 Table1 should have at least one index"
        );
    }

    #[test]
    fn ace14_index_count() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let tdef = assert_user_table_indexes(&path, "Table1");
        assert!(
            !tdef.indexes.is_empty(),
            "ACE14 Table1 should have at least one index"
        );
    }

    #[test]
    fn jet4_primary_key() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        let pk = tdef
            .indexes
            .iter()
            .find(|idx| idx.name == "PrimaryKey")
            .expect("Table1 should have a PrimaryKey index");

        assert_ne!(
            pk.flags & crate::format::index_flags::UNIQUE,
            0,
            "PrimaryKey should have UNIQUE flag"
        );
        assert_ne!(
            pk.flags & crate::format::index_flags::REQUIRED,
            0,
            "PrimaryKey should have REQUIRED flag"
        );
        assert!(
            !pk.columns.is_empty(),
            "PrimaryKey should have at least one column"
        );
    }

    #[test]
    fn jet4_index_columns() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        for idx in &tdef.indexes {
            if idx.index_type != crate::format::index_type::FOREIGN_KEY {
                assert!(
                    !idx.columns.is_empty(),
                    "non-FK index '{}' should have columns",
                    idx.name
                );
                for col in &idx.columns {
                    assert!(
                        (col.col_num as usize) < tdef.columns.len() + 256,
                        "index column number should be reasonable"
                    );
                }
            }
        }
    }

    #[test]
    fn index_fk_type() {
        // indexTestV2003.mdb has FK (type=2) indexes
        let path = skip_if_missing!("V2003/indexTestV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        let fk_indexes: Vec<&IndexDef> = tdef
            .indexes
            .iter()
            .filter(|idx| idx.index_type == crate::format::index_type::FOREIGN_KEY)
            .collect();

        assert!(
            !fk_indexes.is_empty(),
            "indexTest Table1 should have FK indexes"
        );

        for fk in &fk_indexes {
            assert!(
                fk.foreign_key.is_some(),
                "FK index '{}' should have foreign_key info",
                fk.name
            );
            assert!(
                fk.columns.is_empty(),
                "FK index '{}' should have no columns",
                fk.name
            );
        }
    }

    #[test]
    fn jet3_index_fk_type() {
        let path = skip_if_missing!("V1997/indexTestV1997.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        let fk_indexes: Vec<&IndexDef> = tdef
            .indexes
            .iter()
            .filter(|idx| idx.index_type == crate::format::index_type::FOREIGN_KEY)
            .collect();

        assert!(
            !fk_indexes.is_empty(),
            "Jet3 indexTest Table1 should have FK indexes"
        );
        for fk in &fk_indexes {
            assert!(fk.foreign_key.is_some());
        }
    }

    // -- is_replication_column tests ----------------------------------------

    #[test]
    fn is_replication_true() {
        let col = ColumnDef {
            name: "s_GUID".to_string(),
            col_type: ColumnType::Guid,
            col_num: 1,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 16,
            flags: column_flags::REPLICATION | column_flags::NULLABLE,
            is_fixed: false,
            precision: 0,
            scale: 0,
        };
        assert!(is_replication_column(&col));
    }

    #[test]
    fn is_replication_false() {
        let col = ColumnDef {
            name: "ID".to_string(),
            col_type: ColumnType::Long,
            col_num: 1,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 4,
            flags: column_flags::FIXED,
            is_fixed: true,
            precision: 0,
            scale: 0,
        };
        assert!(!is_replication_column(&col));
    }

    #[test]
    fn index_names_are_nonempty() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let tdef = assert_user_table_indexes(&path, "Table1");

        for idx in &tdef.indexes {
            assert!(!idx.name.is_empty(), "index name should not be empty");
        }
    }

    // -- read_names tests -----------------------------------------------------

    #[test]
    fn read_names_jet3_latin1() {
        // Jet3 format: [len: u8][Latin-1 bytes]
        let buf = [3, b'F', b'o', b'o', 3, b'B', b'a', b'r'];
        let mut cursor = TdefCursor::new(&buf, 0);
        let names = read_names(&mut cursor, 2, true).unwrap();
        assert_eq!(names, vec!["Foo", "Bar"]);
        assert_eq!(cursor.position(), 8);
    }

    #[test]
    fn read_names_jet4_utf16le() {
        // Jet4 format: [len: u16 LE][UTF-16LE bytes]
        // "Ab" = 4 bytes UTF-16LE
        let buf = [
            4, 0, // len=4
            b'A', 0, b'b', 0, // "Ab"
            2, 0, // len=2
            b'X', 0, // "X"
        ];
        let mut cursor = TdefCursor::new(&buf, 0);
        let names = read_names(&mut cursor, 2, false).unwrap();
        assert_eq!(names, vec!["Ab", "X"]);
        assert_eq!(cursor.position(), 10);
    }

    #[test]
    fn read_names_boundary_error() {
        // Buffer too short for the name data
        let buf = [3, b'A', b'B'];
        let mut cursor = TdefCursor::new(&buf, 0);
        let result = read_names(&mut cursor, 1, true);
        assert!(result.is_err());
    }

    #[test]
    fn read_names_empty_count() {
        let buf = [];
        let mut cursor = TdefCursor::new(&buf, 0);
        let names = read_names(&mut cursor, 0, true).unwrap();
        assert!(names.is_empty());
        assert_eq!(cursor.position(), 0);
    }

    // -- parse_column_entries tests -------------------------------------------

    #[test]
    fn parse_column_entries_jet3() {
        use crate::format::JET3;
        // Build a minimal Jet3 column entry (18 bytes)
        let mut entry = vec![0u8; JET3.tdef_column_entry_span];
        entry[0] = ColumnType::Long.to_byte(); // col_type
        entry[JET3.coldef_number_pos] = 5; // col_num (1 byte for Jet3)
        entry[JET3.coldef_flags_pos] = column_flags::FIXED;
        entry[JET3.coldef_length_pos] = 4;
        entry[JET3.coldef_length_pos + 1] = 0;

        let mut cursor = TdefCursor::new(&entry, 0);
        let cols =
            parse_column_entries(&mut cursor, JET3.tdef_column_entry_span, 1, true, &JET3).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].col_type, ColumnType::Long);
        assert_eq!(cols[0].col_num, 5);
        assert!(cols[0].is_fixed);
        assert_eq!(cols[0].col_size, 4);
    }

    #[test]
    fn parse_column_entries_jet4() {
        use crate::format::JET4;
        // Build a minimal Jet4 column entry (25 bytes)
        let mut entry = vec![0u8; JET4.tdef_column_entry_span];
        entry[0] = ColumnType::Text.to_byte();
        // col_num: 2 bytes LE
        entry[JET4.coldef_number_pos] = 3;
        entry[JET4.coldef_number_pos + 1] = 0;
        entry[JET4.coldef_flags_pos] = column_flags::NULLABLE;
        entry[JET4.coldef_length_pos] = 0xFF;
        entry[JET4.coldef_length_pos + 1] = 0;

        let mut cursor = TdefCursor::new(&entry, 0);
        let cols = parse_column_entries(&mut cursor, JET4.tdef_column_entry_span, 1, false, &JET4)
            .unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].col_type, ColumnType::Text);
        assert_eq!(cols[0].col_num, 3);
        assert!(!cols[0].is_fixed);
        assert_eq!(cols[0].col_size, 255);
    }

    // -- TdefCursor unit tests ------------------------------------------------

    #[test]
    fn cursor_read_u8() {
        let buf = [0xAB, 0xCD];
        let mut cursor = TdefCursor::new(&buf, 0);
        assert_eq!(cursor.read_u8().unwrap(), 0xAB);
        assert_eq!(cursor.position(), 1);
        assert_eq!(cursor.read_u8().unwrap(), 0xCD);
        assert_eq!(cursor.position(), 2);
    }

    #[test]
    fn cursor_read_u16_le() {
        let buf = [0x34, 0x12, 0x78, 0x56];
        let mut cursor = TdefCursor::new(&buf, 0);
        assert_eq!(cursor.read_u16_le().unwrap(), 0x1234);
        assert_eq!(cursor.position(), 2);
        assert_eq!(cursor.read_u16_le().unwrap(), 0x5678);
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn cursor_read_u32_le() {
        let buf = [0x78, 0x56, 0x34, 0x12];
        let mut cursor = TdefCursor::new(&buf, 0);
        assert_eq!(cursor.read_u32_le().unwrap(), 0x12345678);
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn cursor_read_bytes() {
        let buf = [1, 2, 3, 4, 5];
        let mut cursor = TdefCursor::new(&buf, 1);
        let bytes = cursor.read_bytes(3).unwrap();
        assert_eq!(bytes, &[2, 3, 4]);
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn cursor_skip() {
        let buf = [0u8; 10];
        let mut cursor = TdefCursor::new(&buf, 0);
        cursor.skip(5).unwrap();
        assert_eq!(cursor.position(), 5);
        cursor.skip(5).unwrap();
        assert_eq!(cursor.position(), 10);
    }

    #[test]
    fn cursor_out_of_bounds() {
        let buf = [0xAB];
        let mut cursor = TdefCursor::new(&buf, 0);
        assert!(cursor.read_u16_le().is_err());
        assert!(cursor.read_u32_le().is_err());
        cursor.read_u8().unwrap(); // consume the one byte
        assert!(cursor.read_u8().is_err());
        assert!(cursor.read_bytes(1).is_err());
        assert!(cursor.skip(1).is_err());
    }

    #[test]
    fn cursor_u8_at() {
        let buf = [0x10, 0x20, 0x30];
        let cursor = TdefCursor::new(&buf, 0);
        assert_eq!(cursor.u8_at(1).unwrap(), 0x20);
        assert_eq!(cursor.position(), 0); // position unchanged
        assert!(cursor.u8_at(3).is_err());
    }

    #[test]
    fn cursor_u16_le_at() {
        let buf = [0x00, 0x34, 0x12];
        let cursor = TdefCursor::new(&buf, 0);
        assert_eq!(cursor.u16_le_at(1).unwrap(), 0x1234);
        assert_eq!(cursor.position(), 0); // position unchanged
        assert!(cursor.u16_le_at(2).is_err());
    }

    // -----------------------------------------------------------------------
    // build_index_defs tests
    // -----------------------------------------------------------------------

    #[test]
    fn build_index_defs_normal_index() {
        let logical = vec![LogicalIndex {
            index_num: 1,
            index_col_entry: 0,
            fk_index_type: 0,
            fk_index_number: 0,
            fk_table_page: 0,
            update_action: 0,
            delete_action: 0,
            index_type: crate::format::index_type::ORDINARY,
        }];
        let col = IndexColumn {
            col_num: 3,
            order: IndexColumnOrder::Ascending,
        };
        let physical: Vec<PhysicalIndexEntry> = vec![(vec![col], 0x01, 100)];
        let names = vec!["PK_Id".to_string()];

        let result = build_index_defs(&logical, &physical, names);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "PK_Id");
        assert_eq!(result[0].index_num, 1);
        assert_eq!(result[0].columns.len(), 1);
        assert_eq!(result[0].columns[0].col_num, 3);
        assert_eq!(result[0].flags, 0x01);
        assert_eq!(result[0].first_data_page, 100);
        assert!(result[0].foreign_key.is_none());
    }

    #[test]
    fn build_index_defs_foreign_key() {
        let logical = vec![LogicalIndex {
            index_num: 2,
            index_col_entry: 0,
            fk_index_type: 1,
            fk_index_number: 5,
            fk_table_page: 42,
            update_action: 1,
            delete_action: 2,
            index_type: crate::format::index_type::FOREIGN_KEY,
        }];
        let physical: Vec<PhysicalIndexEntry> = vec![];
        let names = vec!["FK_Ref".to_string()];

        let result = build_index_defs(&logical, &physical, names);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "FK_Ref");
        assert!(result[0].columns.is_empty());
        let fk = result[0].foreign_key.as_ref().unwrap();
        assert_eq!(fk.fk_index_type, 1);
        assert_eq!(fk.fk_index_number, 5);
        assert_eq!(fk.fk_table_page, 42);
        assert_eq!(fk.update_action, 1);
        assert_eq!(fk.delete_action, 2);
    }

    #[test]
    fn build_index_defs_out_of_range_warning() {
        // index_col_entry points beyond idx_col_defs → warning path
        let logical = vec![LogicalIndex {
            index_num: 3,
            index_col_entry: 99, // out of range
            fk_index_type: 0,
            fk_index_number: 0,
            fk_table_page: 0,
            update_action: 0,
            delete_action: 0,
            index_type: crate::format::index_type::ORDINARY,
        }];
        let physical: Vec<PhysicalIndexEntry> = vec![]; // empty → 99 is out of range
        let names = vec!["BadIdx".to_string()];

        let result = build_index_defs(&logical, &physical, names);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "BadIdx");
        assert!(result[0].columns.is_empty());
        assert_eq!(result[0].flags, 0);
        assert_eq!(result[0].first_data_page, 0);
        assert!(result[0].foreign_key.is_none());
    }

    #[test]
    fn japanese_column_names() {
        let path = skip_if_missing!("formPropTest.accdb");
        let tdef = assert_user_table_indexes(&path, "jp_テーブル2");
        let col_names: Vec<&str> = tdef.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(
            col_names.contains(&"商品名"),
            "should contain column 商品名, found: {col_names:?}"
        );
        assert!(
            col_names.contains(&"単価"),
            "should contain column 単価, found: {col_names:?}"
        );
        assert!(
            col_names.contains(&"個数"),
            "should contain column 個数, found: {col_names:?}"
        );
    }

    #[test]
    fn build_index_defs_name_missing_uses_default() {
        // More logical indexes than names → fallback to empty string
        let logical = vec![
            LogicalIndex {
                index_num: 0,
                index_col_entry: 0,
                fk_index_type: 0,
                fk_index_number: 0,
                fk_table_page: 0,
                update_action: 0,
                delete_action: 0,
                index_type: crate::format::index_type::ORDINARY,
            },
            LogicalIndex {
                index_num: 1,
                index_col_entry: 0,
                fk_index_type: 0,
                fk_index_number: 0,
                fk_table_page: 0,
                update_action: 0,
                delete_action: 0,
                index_type: crate::format::index_type::ORDINARY,
            },
        ];
        let col = IndexColumn {
            col_num: 1,
            order: IndexColumnOrder::Ascending,
        };
        let physical: Vec<PhysicalIndexEntry> = vec![(vec![col], 0, 0)];
        let names = vec!["OnlyOne".to_string()]; // only 1 name for 2 indexes

        let result = build_index_defs(&logical, &physical, names);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "OnlyOne");
        assert_eq!(result[1].name, ""); // fallback to empty
    }
}
