//! Page usage map (bitmap) traversal for locating table data pages.

use crate::file::{FileError, PageReader};
use crate::format::{usage_map, PageType};

// ---------------------------------------------------------------------------
// collect_page_numbers — usage map traversal
// ---------------------------------------------------------------------------

/// Collect data page numbers from a usage map byte slice.
///
/// The first byte indicates the map type:
///
/// - **Type 0 (inline):** bytes 1–4 are the start page (u32 LE), bytes 5+
///   are a bitmap where bit *i* being set means `start_page + i` is in use.
///
/// - **Type 1 (reference):** bytes 1+ are an array of u32 LE page pointers.
///   Each non-zero pointer refers to a page of type 0x05 (PageUsageBitmap)
///   whose bytes 4+ contain a bitmap covering `(page_size - 4) * 8` pages.
pub fn collect_page_numbers(
    reader: &mut PageReader,
    map_data: &[u8],
) -> Result<Vec<u32>, FileError> {
    if map_data.is_empty() {
        return Err(FileError::InvalidUsageMap {
            reason: "empty usage map data",
        });
    }

    match map_data[0] {
        usage_map::TYPE_INLINE => collect_inline(map_data),
        usage_map::TYPE_REFERENCE => collect_reference(reader, map_data),
        other => Err(FileError::InvalidUsageMap {
            reason: if other == 2 {
                "unsupported usage map type 2"
            } else {
                "unknown usage map type"
            },
        }),
    }
}

/// Type 0: inline bitmap.
fn collect_inline(map_data: &[u8]) -> Result<Vec<u32>, FileError> {
    if map_data.len() < usage_map::INLINE_BITMAP_OFFSET {
        return Err(FileError::InvalidUsageMap {
            reason: "inline map too short for start page",
        });
    }

    let start_page = u32::from_le_bytes([map_data[1], map_data[2], map_data[3], map_data[4]]);
    let bitmap = &map_data[usage_map::INLINE_BITMAP_OFFSET..];

    Ok(pages_from_bitmap(bitmap, start_page))
}

/// Type 1: reference (indirect) bitmap.
fn collect_reference(reader: &mut PageReader, map_data: &[u8]) -> Result<Vec<u32>, FileError> {
    let ptr_data = &map_data[1..];
    if !ptr_data.len().is_multiple_of(4) {
        return Err(FileError::InvalidUsageMap {
            reason: "reference map pointer data not aligned to 4 bytes",
        });
    }

    let page_size = reader.format().page_size;
    let pages_per_bitmap = (page_size - usage_map::REFERENCE_BITMAP_OFFSET) * 8;

    let mut result = Vec::new();

    for (idx, chunk) in ptr_data.chunks_exact(4).enumerate() {
        let ptr_page = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if ptr_page == 0 {
            continue;
        }

        let bitmap_page = reader.read_page_copy(ptr_page)?;
        if bitmap_page[0] != PageType::PageUsageBitmap as u8 {
            return Err(FileError::InvalidUsageMap {
                reason: "reference map pointer does not point to a usage bitmap page",
            });
        }
        let bitmap = &bitmap_page[usage_map::REFERENCE_BITMAP_OFFSET..];
        let start_page = (idx as u32) * (pages_per_bitmap as u32);

        let pages = pages_from_bitmap(bitmap, start_page);
        result.extend(pages);
    }

    Ok(result)
}

/// Extract page numbers from a bitmap, where bit *i* set means
/// `start_page + i` is present.
fn pages_from_bitmap(bitmap: &[u8], start_page: u32) -> Vec<u32> {
    let mut pages = Vec::new();
    for (byte_idx, &byte) in bitmap.iter().enumerate() {
        if byte == 0 {
            continue;
        }
        for bit in 0u8..8 {
            if byte & (1 << bit) != 0 {
                let page_num = start_page + (byte_idx as u32) * 8 + bit as u32;
                pages.push(page_num);
            }
        }
    }
    pages
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Unit tests (inline bitmap) -------------------------------------------

    #[test]
    fn inline_empty_bitmap() {
        // Type 0, start_page = 0, no bitmap bytes set
        let map_data = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        // We need a reader but collect_inline doesn't use it.
        let pages = collect_inline(&map_data).unwrap();
        assert!(pages.is_empty());
    }

    #[test]
    fn inline_single_page() {
        // Type 0, start_page = 10, bitmap byte 0 = 0x01 → page 10
        let map_data = vec![0x00, 0x0A, 0x00, 0x00, 0x00, 0x01];
        let pages = collect_inline(&map_data).unwrap();
        assert_eq!(pages, vec![10]);
    }

    #[test]
    fn inline_multiple_pages() {
        // Type 0, start_page = 0, bitmap = [0b00000101, 0b00000010]
        // → pages 0, 2, 9
        let map_data = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x02];
        let pages = collect_inline(&map_data).unwrap();
        assert_eq!(pages, vec![0, 2, 9]);
    }

    #[test]
    fn inline_start_page_offset() {
        // Type 0, start_page = 100, bitmap byte 0 = 0xFF
        // → pages 100..107
        let map_data = vec![0x00, 0x64, 0x00, 0x00, 0x00, 0xFF];
        let pages = collect_inline(&map_data).unwrap();
        assert_eq!(pages, vec![100, 101, 102, 103, 104, 105, 106, 107]);
    }

    #[test]
    fn inline_too_short() {
        let map_data = vec![0x00, 0x01, 0x02];
        let result = collect_inline(&map_data);
        assert!(result.is_err());
    }

    // -- Unit tests (error cases) ---------------------------------------------

    #[test]
    fn inline_empty_map_data_errors() {
        let map_data: Vec<u8> = vec![0x00];
        let result = collect_inline(&map_data);
        assert!(matches!(result, Err(FileError::InvalidUsageMap { .. })));
    }

    #[test]
    fn unknown_map_type_errors() {
        // Type 0x05 is not a valid usage map type; collect_inline / collect_reference
        // won't be called, but we can verify via collect_inline that an unrecognised
        // first byte in a hand-crafted slice is caught at the call-site level.
        // Here we test that collect_inline rejects data whose length < 5.
        let map_data = vec![0x00, 0x01, 0x02, 0x03];
        let result = collect_inline(&map_data);
        assert!(matches!(result, Err(FileError::InvalidUsageMap { .. })));
    }

    #[test]
    fn pages_from_bitmap_empty() {
        let bitmap: &[u8] = &[];
        let pages = pages_from_bitmap(bitmap, 0);
        assert!(pages.is_empty());
    }

    #[test]
    fn pages_from_bitmap_all_zeros() {
        let bitmap = &[0x00, 0x00, 0x00, 0x00];
        let pages = pages_from_bitmap(bitmap, 5);
        assert!(pages.is_empty());
    }

    #[test]
    fn pages_from_bitmap_all_ones() {
        let bitmap = &[0xFF];
        let pages = pages_from_bitmap(bitmap, 0);
        assert_eq!(pages, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn pages_from_bitmap_with_offset() {
        // 0b00010010 = bits 1 and 4 → pages start_page+1, start_page+4
        let bitmap = &[0x12];
        let pages = pages_from_bitmap(bitmap, 50);
        assert_eq!(pages, vec![51, 54]);
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

    /// Read the owned-pages pg_row from the TDEF at the given page,
    /// then collect page numbers from the usage map.
    fn collect_from_tdef(reader: &mut PageReader, tdef_page: u32) -> Vec<u32> {
        let format = reader.format();
        let page_data = reader.read_page_copy(tdef_page).unwrap();
        let owned_pos = format.tdef_owned_pages_pos;
        let pg_row = u32::from_le_bytes([
            page_data[owned_pos],
            page_data[owned_pos + 1],
            page_data[owned_pos + 2],
            page_data[owned_pos + 3],
        ]);
        let map_data = reader.read_pg_row(pg_row).unwrap();
        collect_page_numbers(reader, &map_data).unwrap()
    }

    #[test]
    fn jet3_catalog_usage_map() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let pages = collect_from_tdef(&mut reader, crate::format::CATALOG_PAGE);
        assert!(
            !pages.is_empty(),
            "MSysObjects should have at least one data page (Jet3)"
        );
    }

    #[test]
    fn jet4_catalog_usage_map() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let pages = collect_from_tdef(&mut reader, crate::format::CATALOG_PAGE);
        assert!(
            !pages.is_empty(),
            "MSysObjects should have at least one data page (Jet4)"
        );
    }

    #[test]
    fn ace_catalog_usage_map() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let pages = collect_from_tdef(&mut reader, crate::format::CATALOG_PAGE);
        assert!(
            !pages.is_empty(),
            "MSysObjects should have at least one data page (ACE)"
        );
    }

    #[test]
    fn ace14_catalog_usage_map() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let pages = collect_from_tdef(&mut reader, crate::format::CATALOG_PAGE);
        assert!(
            !pages.is_empty(),
            "MSysObjects should have at least one data page (ACE14)"
        );
    }

    // -- collect_page_numbers error paths ------------------------------------

    #[test]
    fn collect_page_numbers_empty() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let result = collect_page_numbers(&mut reader, &[]);
        assert!(matches!(result, Err(FileError::InvalidUsageMap { .. })));
    }

    #[test]
    fn collect_page_numbers_type2() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let result = collect_page_numbers(&mut reader, &[0x02]);
        assert!(matches!(result, Err(FileError::InvalidUsageMap { .. })));
        if let Err(FileError::InvalidUsageMap { reason }) = result {
            assert_eq!(reason, "unsupported usage map type 2");
        }
    }

    #[test]
    fn collect_page_numbers_reference_unaligned() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        // Type 1 (reference) + 3 bytes (not a multiple of 4)
        let result = collect_page_numbers(&mut reader, &[0x01, 0xAA, 0xBB, 0xCC]);
        assert!(matches!(result, Err(FileError::InvalidUsageMap { .. })));
        if let Err(FileError::InvalidUsageMap { reason }) = result {
            assert_eq!(reason, "reference map pointer data not aligned to 4 bytes");
        }
    }

    #[test]
    fn collect_page_numbers_reference_zero_pointer_skipped() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        // Type 1 (reference) + one pointer = 0x00000000 (skipped)
        let result = collect_page_numbers(&mut reader, &[0x01, 0x00, 0x00, 0x00, 0x00]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn collect_page_numbers_reference_bad_page_type() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        // Type 1 (reference) + pointer to page 1 (which is a data page, not a usage bitmap)
        let result = collect_page_numbers(&mut reader, &[0x01, 0x01, 0x00, 0x00, 0x00]);
        assert!(matches!(result, Err(FileError::InvalidUsageMap { .. })));
        if let Err(FileError::InvalidUsageMap { reason }) = result {
            assert_eq!(
                reason,
                "reference map pointer does not point to a usage bitmap page"
            );
        }
    }

    #[test]
    fn collect_page_numbers_unknown_type() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let result = collect_page_numbers(&mut reader, &[0xFF]);
        assert!(matches!(result, Err(FileError::InvalidUsageMap { .. })));
        if let Err(FileError::InvalidUsageMap { reason }) = result {
            assert_eq!(reason, "unknown usage map type");
        }
    }
}
