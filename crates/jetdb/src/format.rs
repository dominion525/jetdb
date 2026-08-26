//! Jet/ACE database format constants, version detection, and layout definitions.

use std::fmt;

// ---------------------------------------------------------------------------
// FormatError
// ---------------------------------------------------------------------------

/// Parsing errors for format-level enums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    UnknownVersion(u8),
    UnknownPageType(u8),
    UnknownColumnType(u8),
    UnknownObjectType(i32),
    InvalidEncoding,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownVersion(v) => write!(f, "unknown Jet version byte: 0x{v:02X}"),
            Self::UnknownPageType(v) => write!(f, "unknown page type byte: 0x{v:02X}"),
            Self::UnknownColumnType(v) => write!(f, "unknown column type byte: 0x{v:02X}"),
            Self::UnknownObjectType(v) => write!(f, "unknown object type value: {v}"),
            Self::InvalidEncoding => write!(f, "invalid text encoding"),
        }
    }
}

impl std::error::Error for FormatError {}

// ---------------------------------------------------------------------------
// JetVersion
// ---------------------------------------------------------------------------

/// Database engine version, read from file offset 0x14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum JetVersion {
    /// Access 97 (.mdb)
    Jet3 = 0x00,
    /// Access 2000 / 2003 (.mdb)
    Jet4 = 0x01,
    /// Access 2007 (.accdb)
    Ace12 = 0x02,
    /// Access 2010 (.accdb)
    Ace14 = 0x03,
    /// Access 2013 (.accdb)
    Ace15 = 0x04,
    /// Access 2016 (.accdb)
    Ace16 = 0x05,
    /// Access 2019 (.accdb)
    Ace17 = 0x06,
}

impl JetVersion {
    /// Parse a version byte (from file offset 0x14).
    pub fn from_byte(b: u8) -> Result<Self, FormatError> {
        Self::try_from(b)
    }

    /// Return the format constants for this version.
    pub fn format(&self) -> &'static JetFormat {
        match self {
            Self::Jet3 => &JET3,
            _ => &JET4,
        }
    }

    /// `true` if this is the Jet 3 engine (Access 97).
    pub fn is_jet3(&self) -> bool {
        *self == Self::Jet3
    }

    /// `true` if this is the Jet 4 engine (Access 2000/2003).
    pub fn is_jet4(&self) -> bool {
        *self == Self::Jet4
    }

    /// `true` if this is an ACE engine (Access 2007+, .accdb).
    pub fn is_accdb(&self) -> bool {
        !matches!(self, Self::Jet3 | Self::Jet4)
    }
}

impl TryFrom<u8> for JetVersion {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Jet3),
            0x01 => Ok(Self::Jet4),
            0x02 => Ok(Self::Ace12),
            0x03 => Ok(Self::Ace14),
            0x04 => Ok(Self::Ace15),
            0x05 => Ok(Self::Ace16),
            0x06 => Ok(Self::Ace17),
            _ => Err(FormatError::UnknownVersion(value)),
        }
    }
}

impl fmt::Display for JetVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Jet3 => "Jet3 (Access 97)",
            Self::Jet4 => "Jet4 (Access 2000/2003)",
            Self::Ace12 => "ACE12 (Access 2007)",
            Self::Ace14 => "ACE14 (Access 2010)",
            Self::Ace15 => "ACE15 (Access 2013)",
            Self::Ace16 => "ACE16 (Access 2016)",
            Self::Ace17 => "ACE17 (Access 2019)",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// JetFormat — version-dependent layout constants
// ---------------------------------------------------------------------------

/// Version-dependent byte offsets and sizes for the Jet/ACE page layout.
///
/// ACE versions (2007+) share the same physical layout as Jet 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JetFormat {
    /// Database page size in bytes.
    pub page_size: usize,

    // -- Data page -----------------------------------------------------------
    /// Position of the row-count field inside a data page.
    pub data_row_count_pos: usize,

    // -- Table definition (TDEF) page ----------------------------------------
    /// Position of the "number of rows" field in TDEF.
    pub tdef_row_count_pos: usize,
    /// Position of the "number of variable columns" field in TDEF.
    pub tdef_var_col_count_pos: usize,
    /// Position of the "number of columns" field in TDEF.
    pub tdef_column_count_pos: usize,
    /// Position of the "number of indexes" field in TDEF.
    pub tdef_index_count_pos: usize,
    /// Position of the "number of real indexes" field in TDEF.
    pub tdef_real_index_count_pos: usize,
    /// Position of the owned-pages map pointer in TDEF.
    pub tdef_owned_pages_pos: usize,
    /// Position of the first data-page pointer in TDEF.
    pub tdef_first_data_page_pos: usize,
    /// Position where the index entry definitions start in TDEF.
    pub tdef_index_entries_pos: usize,
    /// Position of the free-pages map in TDEF.
    pub tdef_free_pages_pos: usize,
    /// Byte span of one real-index entry in TDEF.
    pub tdef_index_entry_span: usize,
    /// Byte span of one column entry in TDEF.
    pub tdef_column_entry_span: usize,

    // -- Index definitions (TDEF sections [5][6]) ----------------------------
    /// Byte span of one index-column definition entry (section \[5\]).
    pub idx_col_block_size: usize,
    /// Byte span of one logical index definition entry (section \[6\]).
    pub idx_info_block_size: usize,
    /// Bytes to skip at the start of each section \[5\] entry (type marker).
    pub idx_col_skip_before: usize,
    /// Bytes to skip before the flags field in section \[5\].
    pub idx_col_skip_before_flags: usize,
    /// Bytes to skip after the flags field in section \[5\].
    pub idx_col_skip_after_flags: usize,
    /// Bytes to skip at the start of each section \[6\] entry (type marker).
    pub idx_info_skip_before: usize,
    /// Byte offset of the index_type field within a section \[6\] entry.
    pub idx_info_type_offset: usize,

    // -- Column entry fields -------------------------------------------------
    /// Position of the column-number field within a column entry.
    pub coldef_number_pos: usize,
    /// Position of the column-length field within a column entry.
    pub coldef_length_pos: usize,
    /// Position of the column-flags field within a column entry.
    pub coldef_flags_pos: usize,
    /// Position of the variable-column index within a column entry.
    pub coldef_var_col_index_pos: usize,
    /// Position of the fixed-data offset within a column entry.
    pub coldef_fixed_data_pos: usize,
    /// Position of the column-count field in a data row.
    pub data_column_count_pos: usize,

    // -- Numeric column ------------------------------------------------------
    /// Position of the scale field for numeric columns.
    pub coldef_scale_pos: usize,
    /// Position of the precision field for numeric columns.
    pub coldef_precision_pos: usize,
}

/// Format constants for the Jet 3 engine (Access 97, page size 2048).
pub static JET3: JetFormat = JetFormat {
    page_size: 2048,
    data_row_count_pos: 8,
    tdef_row_count_pos: 12,
    tdef_var_col_count_pos: 23,
    tdef_column_count_pos: 25,
    tdef_index_count_pos: 27,
    tdef_real_index_count_pos: 31,
    tdef_owned_pages_pos: 35,
    tdef_first_data_page_pos: 36,
    tdef_index_entries_pos: 43,
    tdef_free_pages_pos: 39,
    tdef_index_entry_span: 8,
    tdef_column_entry_span: 18,
    idx_col_block_size: 39,
    idx_info_block_size: 20,
    idx_col_skip_before: 0,
    idx_col_skip_before_flags: 0,
    idx_col_skip_after_flags: 0,
    // Jet3 has no type marker at the start of section \[6\] entries (see HACKING.md).
    // Note: offset 4 (index_col_def_num) is used instead of 0 (index_num),
    // which works because these values are normally identical.
    idx_info_skip_before: 0,
    idx_info_type_offset: 19,
    coldef_number_pos: 1,
    coldef_length_pos: 16,
    coldef_flags_pos: 13,
    coldef_var_col_index_pos: 3,
    coldef_fixed_data_pos: 14,
    data_column_count_pos: 0,
    // Not independently verified against real bytes -- unlike JET4 (see
    // below), Jet3/Access 97 has no Decimal number type at all, so these
    // two fields are never actually read for real data.
    coldef_scale_pos: 9,
    coldef_precision_pos: 10,
};

/// Format constants for the Jet 4 / ACE engine (Access 2000+, page size 4096).
pub static JET4: JetFormat = JetFormat {
    page_size: 4096,
    data_row_count_pos: 12,
    tdef_row_count_pos: 16,
    tdef_var_col_count_pos: 43,
    tdef_column_count_pos: 45,
    tdef_index_count_pos: 47,
    tdef_real_index_count_pos: 51,
    tdef_owned_pages_pos: 55,
    tdef_first_data_page_pos: 56,
    tdef_index_entries_pos: 63,
    tdef_free_pages_pos: 59,
    tdef_index_entry_span: 12,
    tdef_column_entry_span: 25,
    idx_col_block_size: 52,
    idx_info_block_size: 28,
    idx_col_skip_before: 4,
    idx_col_skip_before_flags: 4,
    idx_col_skip_after_flags: 5,
    idx_info_skip_before: 4,
    idx_info_type_offset: 23,
    coldef_number_pos: 5,
    coldef_length_pos: 23,
    coldef_flags_pos: 15,
    coldef_var_col_index_pos: 7,
    coldef_fixed_data_pos: 21,
    data_column_count_pos: 0,
    // Verified against real Decimal-column bytes (see
    // `testdata/SOURCES.md`): offset 11 holds precision, offset 12 holds
    // scale -- the two names below were swapped prior to this fix.
    coldef_scale_pos: 12,
    coldef_precision_pos: 11,
};

// ---------------------------------------------------------------------------
// PageType
// ---------------------------------------------------------------------------

/// Type tag stored at the beginning of each database page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PageType {
    DatabaseDefinition = 0,
    Data = 1,
    TableDefinition = 2,
    IntermediateIndex = 3,
    LeafIndex = 4,
    PageUsageBitmap = 5,
}

impl TryFrom<u8> for PageType {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::DatabaseDefinition),
            1 => Ok(Self::Data),
            2 => Ok(Self::TableDefinition),
            3 => Ok(Self::IntermediateIndex),
            4 => Ok(Self::LeafIndex),
            5 => Ok(Self::PageUsageBitmap),
            _ => Err(FormatError::UnknownPageType(value)),
        }
    }
}

impl fmt::Display for PageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::DatabaseDefinition => "Database Definition",
            Self::Data => "Data",
            Self::TableDefinition => "Table Definition",
            Self::IntermediateIndex => "Intermediate Index",
            Self::LeafIndex => "Leaf Index",
            Self::PageUsageBitmap => "Page Usage Bitmap",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// ColumnType
// ---------------------------------------------------------------------------

/// Column data type stored in the column definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnType {
    Boolean,
    Byte,
    Int,
    Long,
    Money,
    Float,
    Double,
    Timestamp,
    Binary,
    Text,
    Ole,
    Memo,
    Guid,
    Numeric,
    ComplexType,
    BigInt,
    DateTimeExtended,
    Unknown(u8),
}

impl ColumnType {
    /// Return the byte value for this column type.
    pub fn to_byte(&self) -> u8 {
        match self {
            Self::Boolean => 0x01,
            Self::Byte => 0x02,
            Self::Int => 0x03,
            Self::Long => 0x04,
            Self::Money => 0x05,
            Self::Float => 0x06,
            Self::Double => 0x07,
            Self::Timestamp => 0x08,
            Self::Binary => 0x09,
            Self::Text => 0x0A,
            Self::Ole => 0x0B,
            Self::Memo => 0x0C,
            Self::Guid => 0x0F,
            Self::Numeric => 0x10,
            Self::ComplexType => 0x12,
            Self::BigInt => 0x13,
            Self::DateTimeExtended => 0x14,
            Self::Unknown(b) => *b,
        }
    }

    /// Return the fixed byte-size for this column type, or `None` for
    /// variable-length types.
    pub fn fixed_size(&self) -> Option<usize> {
        match self {
            Self::Boolean => Some(1),
            Self::Byte => Some(1),
            Self::Int => Some(2),
            Self::Long => Some(4),
            Self::Money => Some(8),
            Self::Float => Some(4),
            Self::Double => Some(8),
            Self::Timestamp => Some(8),
            Self::Binary => None,
            Self::Text => None,
            Self::Ole => None,
            Self::Memo => None,
            Self::Guid => Some(16),
            Self::Numeric => Some(17),
            Self::ComplexType => Some(4),
            Self::BigInt => Some(8),
            Self::DateTimeExtended => Some(42),
            Self::Unknown(_) => None,
        }
    }

    /// `true` if this column type has variable-length storage.
    pub fn is_variable_length(&self) -> bool {
        self.fixed_size().is_none()
    }
}

impl TryFrom<u8> for ColumnType {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::Boolean),
            0x02 => Ok(Self::Byte),
            0x03 => Ok(Self::Int),
            0x04 => Ok(Self::Long),
            0x05 => Ok(Self::Money),
            0x06 => Ok(Self::Float),
            0x07 => Ok(Self::Double),
            0x08 => Ok(Self::Timestamp),
            0x09 => Ok(Self::Binary),
            0x0A => Ok(Self::Text),
            0x0B => Ok(Self::Ole),
            0x0C => Ok(Self::Memo),
            0x0F => Ok(Self::Guid),
            0x10 => Ok(Self::Numeric),
            0x12 => Ok(Self::ComplexType),
            0x13 => Ok(Self::BigInt),
            0x14 => Ok(Self::DateTimeExtended),
            other => Ok(Self::Unknown(other)),
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(v) => write!(f, "Unknown(0x{v:02X})"),
            other => fmt::Debug::fmt(other, f),
        }
    }
}

// ---------------------------------------------------------------------------
// ObjectType
// ---------------------------------------------------------------------------

/// Object type stored in the MSysObjects system catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ObjectType {
    Form = 0,
    Table = 1,
    Macro = 2,
    SystemTable = 3,
    Report = 4,
    Query = 5,
    LinkedTable = 6,
    Module = 7,
    Relationship = 8,
    DatabaseProperty = 11,
}

impl TryFrom<i32> for ObjectType {
    type Error = FormatError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Form),
            1 => Ok(Self::Table),
            2 => Ok(Self::Macro),
            3 => Ok(Self::SystemTable),
            4 => Ok(Self::Report),
            5 => Ok(Self::Query),
            6 => Ok(Self::LinkedTable),
            7 => Ok(Self::Module),
            8 => Ok(Self::Relationship),
            11 => Ok(Self::DatabaseProperty),
            _ => Err(FormatError::UnknownObjectType(value)),
        }
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

// ---------------------------------------------------------------------------
// Constant sub-modules
// ---------------------------------------------------------------------------

/// Row-pointer bit masks.
pub mod row {
    /// Mask to extract the byte offset from a row pointer.
    pub const OFFSET_MASK: u16 = 0x1FFF;
    /// Flag indicating a deleted row.
    pub const DELETE_FLAG: u16 = 0x4000;
    /// Flag indicating a lookup row.
    pub const LOOKUP_FLAG: u16 = 0x8000;
}

/// Column-flags bit definitions.
pub mod column_flags {
    /// Column has a fixed-length representation.
    pub const FIXED: u8 = 0x01;
    /// Column allows NULL values.
    pub const NULLABLE: u8 = 0x02;
    /// Column is an auto-increment long integer.
    pub const AUTO_LONG: u8 = 0x04;
    /// Column is used for replication.
    pub const REPLICATION: u8 = 0x10;
    /// Column is an auto-generated UUID.
    pub const AUTO_UUID: u8 = 0x40;
    /// Column contains hyperlink data.
    pub const HYPERLINK: u8 = 0x80;
}

/// Usage-map constants.
pub mod usage_map {
    /// Inline bitmap type (small tables).
    pub const TYPE_INLINE: u8 = 0x00;
    /// Reference bitmap type (large tables).
    pub const TYPE_REFERENCE: u8 = 0x01;
    /// Byte offset where the inline bitmap starts.
    pub const INLINE_BITMAP_OFFSET: usize = 5;
    /// Byte offset where the reference page pointer starts.
    pub const REFERENCE_BITMAP_OFFSET: usize = 4;
}

/// Database header offsets.
pub mod db_header {
    /// Offset of the version byte.
    pub const VERSION: usize = 0x14;
    /// Start of the encrypted region.
    pub const ENCRYPTED_START: usize = 0x18;
    /// Language ID offset for Jet 3.
    pub const LANG_ID_JET3: usize = 0x3A;
    /// Language ID offset for Jet 4.
    pub const LANG_ID_JET4: usize = 0x6E;
    /// Code-page offset.
    pub const CODE_PAGE: usize = 0x3C;
    /// Database encryption key offset.
    pub const DB_KEY: usize = 0x3E;
    /// Password offset for Jet 3.
    pub const PASSWORD_JET3: usize = 0x42;
    /// Offset of the EncryptionInfo length field (Agile Encryption, .accdb).
    pub const ENCRYPTION_INFO_OFFSET: usize = 0x299;
}

/// MSysObjects flag-bit definitions.
pub mod catalog_flags {
    /// System object (0x80000000).
    pub const SYSTEM: u32 = 0x8000_0000;
    /// Hidden object (0x02).
    pub const HIDDEN: u32 = 0x02;
}

/// Index flags (section \[5\] flags field in TDEF).
pub mod index_flags {
    /// Unique index.
    pub const UNIQUE: u8 = 0x01;
    /// Ignore NULL values.
    pub const IGNORE_NULLS: u8 = 0x02;
    /// Required index (all columns must be non-NULL).
    pub const REQUIRED: u8 = 0x08;
}

/// Index type (section \[6\] index_type field in TDEF).
pub mod index_type {
    /// Normal index or PRIMARY KEY.
    pub const NORMAL: u8 = 0x01;
    /// Foreign key reference (no physical B-tree index).
    pub const FOREIGN_KEY: u8 = 0x02;
}

/// Page number of the system catalog (MSysObjects).
pub const CATALOG_PAGE: u32 = 2;

/// Maximum length of an object name.
pub const MAX_OBJECT_NAME: usize = 256;

/// Maximum number of columns in a table.
pub const MAX_COLUMNS: usize = 256;

/// Maximum number of columns in a single index.
pub const MAX_INDEX_COLUMNS: usize = 10;

/// Overhead bytes for memo / OLE fields.
pub const MEMO_OVERHEAD: usize = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jet_version_roundtrip() {
        let versions: &[(u8, JetVersion)] = &[
            (0x00, JetVersion::Jet3),
            (0x01, JetVersion::Jet4),
            (0x02, JetVersion::Ace12),
            (0x03, JetVersion::Ace14),
            (0x04, JetVersion::Ace15),
            (0x05, JetVersion::Ace16),
            (0x06, JetVersion::Ace17),
        ];
        for &(byte, expected) in versions {
            let v = JetVersion::from_byte(byte).unwrap();
            assert_eq!(v, expected);
            assert_eq!(v as u8, byte);
        }
    }

    #[test]
    fn jet_version_unknown_byte() {
        assert_eq!(
            JetVersion::from_byte(0xFF),
            Err(FormatError::UnknownVersion(0xFF))
        );
        assert_eq!(
            JetVersion::from_byte(0x07),
            Err(FormatError::UnknownVersion(0x07))
        );
    }

    #[test]
    fn jet3_page_size() {
        assert_eq!(JET3.page_size, 2048);
    }

    #[test]
    fn jet4_page_size() {
        assert_eq!(JET4.page_size, 4096);
    }

    #[test]
    fn ace_versions_use_jet4_format() {
        let ace_versions = [
            JetVersion::Ace12,
            JetVersion::Ace14,
            JetVersion::Ace15,
            JetVersion::Ace16,
            JetVersion::Ace17,
        ];
        for v in ace_versions {
            assert!(std::ptr::eq(v.format(), &JET4), "{v} should return &JET4");
        }
        // Jet4 itself should also return &JET4
        assert!(std::ptr::eq(JetVersion::Jet4.format(), &JET4));
        // Jet3 should return &JET3
        assert!(std::ptr::eq(JetVersion::Jet3.format(), &JET3));
    }

    #[test]
    fn version_classification() {
        assert!(JetVersion::Jet3.is_jet3());
        assert!(!JetVersion::Jet3.is_jet4());
        assert!(!JetVersion::Jet3.is_accdb());

        assert!(!JetVersion::Jet4.is_jet3());
        assert!(JetVersion::Jet4.is_jet4());
        assert!(!JetVersion::Jet4.is_accdb());

        assert!(!JetVersion::Ace12.is_jet3());
        assert!(!JetVersion::Ace12.is_jet4());
        assert!(JetVersion::Ace12.is_accdb());
    }

    #[test]
    fn column_type_fixed_sizes() {
        assert_eq!(ColumnType::Boolean.fixed_size(), Some(1));
        assert_eq!(ColumnType::Byte.fixed_size(), Some(1));
        assert_eq!(ColumnType::Int.fixed_size(), Some(2));
        assert_eq!(ColumnType::Long.fixed_size(), Some(4));
        assert_eq!(ColumnType::Money.fixed_size(), Some(8));
        assert_eq!(ColumnType::Float.fixed_size(), Some(4));
        assert_eq!(ColumnType::Double.fixed_size(), Some(8));
        assert_eq!(ColumnType::Timestamp.fixed_size(), Some(8));
        assert_eq!(ColumnType::Guid.fixed_size(), Some(16));
        assert_eq!(ColumnType::Numeric.fixed_size(), Some(17));
        assert_eq!(ColumnType::ComplexType.fixed_size(), Some(4));
        assert_eq!(ColumnType::BigInt.fixed_size(), Some(8));
        assert_eq!(ColumnType::DateTimeExtended.fixed_size(), Some(42));
    }

    #[test]
    fn column_type_variable_length() {
        assert!(ColumnType::Text.is_variable_length());
        assert!(ColumnType::Memo.is_variable_length());
        assert!(ColumnType::Ole.is_variable_length());
        assert!(ColumnType::Binary.is_variable_length());

        assert!(!ColumnType::Int.is_variable_length());
        assert!(!ColumnType::Double.is_variable_length());
    }

    #[test]
    fn page_type_roundtrip() {
        let types: &[(u8, PageType)] = &[
            (0, PageType::DatabaseDefinition),
            (1, PageType::Data),
            (2, PageType::TableDefinition),
            (3, PageType::IntermediateIndex),
            (4, PageType::LeafIndex),
            (5, PageType::PageUsageBitmap),
        ];
        for &(byte, expected) in types {
            let pt = PageType::try_from(byte).unwrap();
            assert_eq!(pt, expected);
            assert_eq!(pt as u8, byte);
        }
    }

    #[test]
    fn page_type_unknown() {
        assert_eq!(PageType::try_from(6), Err(FormatError::UnknownPageType(6)));
    }

    #[test]
    fn column_type_roundtrip() {
        let types: &[(u8, ColumnType)] = &[
            (0x01, ColumnType::Boolean),
            (0x02, ColumnType::Byte),
            (0x03, ColumnType::Int),
            (0x04, ColumnType::Long),
            (0x05, ColumnType::Money),
            (0x06, ColumnType::Float),
            (0x07, ColumnType::Double),
            (0x08, ColumnType::Timestamp),
            (0x09, ColumnType::Binary),
            (0x0A, ColumnType::Text),
            (0x0B, ColumnType::Ole),
            (0x0C, ColumnType::Memo),
            (0x0F, ColumnType::Guid),
            (0x10, ColumnType::Numeric),
            (0x12, ColumnType::ComplexType),
            (0x13, ColumnType::BigInt),
            (0x14, ColumnType::DateTimeExtended),
        ];
        for &(byte, expected) in types {
            let ct = ColumnType::try_from(byte).unwrap();
            assert_eq!(ct, expected);
            assert_eq!(ct.to_byte(), byte);
        }
    }

    #[test]
    fn column_type_unknown() {
        let ct = ColumnType::try_from(0x11).unwrap();
        assert_eq!(ct, ColumnType::Unknown(0x11));
        assert_eq!(ct.to_byte(), 0x11);
        assert_eq!(ct.fixed_size(), None);
        assert!(ct.is_variable_length());
        assert_eq!(ct.to_string(), "Unknown(0x11)");
    }

    #[test]
    fn object_type_roundtrip() {
        let types: &[(i32, ObjectType)] = &[
            (0, ObjectType::Form),
            (1, ObjectType::Table),
            (2, ObjectType::Macro),
            (3, ObjectType::SystemTable),
            (4, ObjectType::Report),
            (5, ObjectType::Query),
            (6, ObjectType::LinkedTable),
            (7, ObjectType::Module),
            (8, ObjectType::Relationship),
            (11, ObjectType::DatabaseProperty),
        ];
        for &(val, expected) in types {
            let ot = ObjectType::try_from(val).unwrap();
            assert_eq!(ot, expected);
            assert_eq!(ot as i32, val);
        }
    }

    #[test]
    fn row_offset_mask() {
        // A row pointer with offset 0x0ABC and flags set
        let ptr: u16 = 0xCABC;
        assert_eq!(ptr & row::OFFSET_MASK, 0x0ABC);
        assert_ne!(ptr & row::DELETE_FLAG, 0);
        assert!(ptr & row::LOOKUP_FLAG != 0);
    }

    #[test]
    fn format_error_display() {
        let e = FormatError::UnknownVersion(0xFF);
        assert_eq!(e.to_string(), "unknown Jet version byte: 0xFF");

        let e = FormatError::UnknownPageType(0x09);
        assert_eq!(e.to_string(), "unknown page type byte: 0x09");

        let e = FormatError::UnknownColumnType(0xAB);
        assert_eq!(e.to_string(), "unknown column type byte: 0xAB");

        let e = FormatError::UnknownObjectType(99);
        assert_eq!(e.to_string(), "unknown object type value: 99");

        let e = FormatError::InvalidEncoding;
        assert_eq!(e.to_string(), "invalid text encoding");
    }

    // -- Display trait tests --------------------------------------------------

    #[test]
    fn jet_version_display() {
        assert_eq!(JetVersion::Jet3.to_string(), "Jet3 (Access 97)");
        assert_eq!(JetVersion::Jet4.to_string(), "Jet4 (Access 2000/2003)");
        assert_eq!(JetVersion::Ace12.to_string(), "ACE12 (Access 2007)");
        assert_eq!(JetVersion::Ace14.to_string(), "ACE14 (Access 2010)");
        assert_eq!(JetVersion::Ace15.to_string(), "ACE15 (Access 2013)");
        assert_eq!(JetVersion::Ace16.to_string(), "ACE16 (Access 2016)");
        assert_eq!(JetVersion::Ace17.to_string(), "ACE17 (Access 2019)");
    }

    #[test]
    fn page_type_display() {
        assert_eq!(
            PageType::DatabaseDefinition.to_string(),
            "Database Definition"
        );
        assert_eq!(PageType::Data.to_string(), "Data");
        assert_eq!(PageType::TableDefinition.to_string(), "Table Definition");
        assert_eq!(
            PageType::IntermediateIndex.to_string(),
            "Intermediate Index"
        );
        assert_eq!(PageType::LeafIndex.to_string(), "Leaf Index");
        assert_eq!(PageType::PageUsageBitmap.to_string(), "Page Usage Bitmap");
    }

    #[test]
    fn object_type_display() {
        assert_eq!(ObjectType::Form.to_string(), "Form");
        assert_eq!(ObjectType::Table.to_string(), "Table");
        assert_eq!(ObjectType::Macro.to_string(), "Macro");
        assert_eq!(ObjectType::SystemTable.to_string(), "SystemTable");
        assert_eq!(ObjectType::Report.to_string(), "Report");
        assert_eq!(ObjectType::Query.to_string(), "Query");
        assert_eq!(ObjectType::LinkedTable.to_string(), "LinkedTable");
        assert_eq!(ObjectType::Module.to_string(), "Module");
        assert_eq!(ObjectType::Relationship.to_string(), "Relationship");
        assert_eq!(ObjectType::DatabaseProperty.to_string(), "DatabaseProperty");
    }

    #[test]
    fn column_type_display_known() {
        assert_eq!(ColumnType::Boolean.to_string(), "Boolean");
        assert_eq!(ColumnType::Byte.to_string(), "Byte");
        assert_eq!(ColumnType::Int.to_string(), "Int");
        assert_eq!(ColumnType::Long.to_string(), "Long");
        assert_eq!(ColumnType::Money.to_string(), "Money");
        assert_eq!(ColumnType::Float.to_string(), "Float");
        assert_eq!(ColumnType::Double.to_string(), "Double");
        assert_eq!(ColumnType::Timestamp.to_string(), "Timestamp");
        assert_eq!(ColumnType::Binary.to_string(), "Binary");
        assert_eq!(ColumnType::Text.to_string(), "Text");
        assert_eq!(ColumnType::Ole.to_string(), "Ole");
        assert_eq!(ColumnType::Memo.to_string(), "Memo");
        assert_eq!(ColumnType::Guid.to_string(), "Guid");
        assert_eq!(ColumnType::Numeric.to_string(), "Numeric");
        assert_eq!(ColumnType::ComplexType.to_string(), "ComplexType");
        assert_eq!(ColumnType::BigInt.to_string(), "BigInt");
        assert_eq!(ColumnType::DateTimeExtended.to_string(), "DateTimeExtended");
    }

    #[test]
    fn format_error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(FormatError::InvalidEncoding);
        assert!(e.source().is_none());
    }

    #[test]
    fn object_type_unknown() {
        assert_eq!(
            ObjectType::try_from(99),
            Err(FormatError::UnknownObjectType(99))
        );
    }
}
