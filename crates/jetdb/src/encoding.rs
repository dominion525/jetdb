//! Text decoding: the code page of the database (Jet3), UTF-16LE (Jet4+),
//! and Unicode compression.

use crate::format::FormatError;

/// Compressed-text header: `0xFF 0xFE`.
const COMPRESSED_HEADER: [u8; 2] = [0xFF, 0xFE];

/// How a file stores text, as [`crate::PageReader::text_encoding`] tells
/// it from the header of the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// Jet3 (Access 97): bytes in the code page of the database, such as
    /// 1252 for Western European languages or 932 for Japanese.
    Jet3 { code_page: u16 },
    /// Jet4 and later: UTF-16LE, which may be compressed.
    Unicode,
}

impl TextEncoding {
    /// `true` for a Jet3 (Access 97) file.
    pub fn is_jet3(self) -> bool {
        matches!(self, Self::Jet3 { .. })
    }

    /// The code page of a Jet3 file, `None` for a later one.
    pub fn jet3_code_page(self) -> Option<u16> {
        match self {
            Self::Jet3 { code_page } => Some(code_page),
            Self::Unicode => None,
        }
    }
}

/// Decode `bytes` in the code page `code_page`, or as Latin-1 when jetdb
/// does not know the code page.
pub fn decode_code_page(bytes: &[u8], code_page: u16) -> String {
    match codepage::to_encoding(code_page) {
        Some(encoding) => encoding.decode_without_bom_handling(bytes).0.into_owned(),
        None => decode_latin1(bytes),
    }
}

/// Decode a Latin-1 (ISO 8859-1) byte slice into a `String`: each byte is
/// the code point of the same value, in U+0000..U+00FF.
pub fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Decode a UTF-16LE byte slice into a `String`.
///
/// Used for Jet4/ACE column names. Returns an error if the byte slice has
/// an odd length or contains invalid surrogate pairs.
pub fn decode_utf16le(bytes: &[u8]) -> Result<String, FormatError> {
    if bytes.len() % 2 != 0 {
        return Err(FormatError::InvalidEncoding);
    }
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u16s).map_err(|_| FormatError::InvalidEncoding)
}

/// Decode a text column value of a file that stores text as `text` tells.
///
/// Jet3 text is in the code page of the database (see [`decode_code_page`]).
/// Later text is compressed (prefixed with `0xFF 0xFE`) or raw UTF-16LE.
pub fn decode_text(data: &[u8], text: TextEncoding) -> Result<String, FormatError> {
    if let TextEncoding::Jet3 { code_page } = text {
        return Ok(decode_code_page(data, code_page));
    }
    if data.is_empty() {
        return Ok(String::new());
    }
    if data.len() >= 2 && data[0] == COMPRESSED_HEADER[0] && data[1] == COMPRESSED_HEADER[1] {
        let expanded = decompress_text(&data[2..]);
        decode_utf16le(&expanded)
    } else {
        decode_utf16le(data)
    }
}

/// Decompress Jet4/ACE compressed text.
///
/// The input is the byte sequence *after* the `0xFF 0xFE` header.
/// Compressed mode expands each byte to `[byte, 0x00]` (Latin-1 → UTF-16LE).
/// A `0x00` byte toggles between compressed and uncompressed modes.
/// In uncompressed mode bytes are copied as-is (pairs of UTF-16LE bytes).
fn decompress_text(data: &[u8]) -> Vec<u8> {
    let mut compressed = true;
    let mut output = Vec::with_capacity(data.len() * 2);
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == 0x00 {
            compressed = !compressed;
            i += 1;
        } else if compressed {
            output.push(b);
            output.push(0x00);
            i += 1;
        } else {
            // Uncompressed mode: copy 2 bytes at a time.
            // If only 1 byte remains, ignore it.
            if i + 1 < data.len() {
                output.push(b);
                output.push(data[i + 1]);
            }
            i += 2;
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin1_ascii() {
        assert_eq!(decode_latin1(b"Hello"), "Hello");
    }

    #[test]
    fn latin1_special_chars() {
        // À = 0xC0, é = 0xE9
        assert_eq!(decode_latin1(&[0xC0, 0xE9]), "Àé");
    }

    #[test]
    fn latin1_empty() {
        assert_eq!(decode_latin1(b""), "");
    }

    #[test]
    fn latin1_full_range() {
        // Every byte 0x00..=0xFF should produce a valid char
        let bytes: Vec<u8> = (0..=255).collect();
        let s = decode_latin1(&bytes);
        assert_eq!(s.chars().count(), 256);
    }

    #[test]
    fn utf16le_ascii() {
        // "Hi" in UTF-16LE
        let bytes = [0x48, 0x00, 0x69, 0x00];
        assert_eq!(decode_utf16le(&bytes).unwrap(), "Hi");
    }

    #[test]
    fn utf16le_japanese() {
        // "日本" = U+65E5 U+672C
        let bytes = [0xE5, 0x65, 0x2C, 0x67];
        assert_eq!(decode_utf16le(&bytes).unwrap(), "日本");
    }

    #[test]
    fn utf16le_empty() {
        assert_eq!(decode_utf16le(&[]).unwrap(), "");
    }

    #[test]
    fn utf16le_odd_length_error() {
        let bytes = [0x48, 0x00, 0x69];
        assert_eq!(decode_utf16le(&bytes), Err(FormatError::InvalidEncoding));
    }

    #[test]
    fn utf16le_invalid_surrogate() {
        // Lone high surrogate: U+D800
        let bytes = [0x00, 0xD8];
        assert_eq!(decode_utf16le(&bytes), Err(FormatError::InvalidEncoding));
    }

    // -- decompress_text tests ------------------------------------------------

    #[test]
    fn decompress_ascii_only() {
        // "Hello" compressed: each byte → [byte, 0x00]
        let input = b"Hello";
        let result = decompress_text(input);
        assert_eq!(
            result,
            vec![0x48, 0x00, 0x65, 0x00, 0x6C, 0x00, 0x6C, 0x00, 0x6F, 0x00]
        );
    }

    #[test]
    fn decompress_empty() {
        assert_eq!(decompress_text(b""), Vec::<u8>::new());
    }

    #[test]
    fn decompress_mixed_segments() {
        // Compressed "AB" → 0x00 → Uncompressed [0xE5, 0x65] → 0x00 → Compressed "C"
        let input = [0x41, 0x42, 0x00, 0xE5, 0x65, 0x00, 0x43];
        let result = decompress_text(&input);
        // A=0x41,0x00  B=0x42,0x00  日=0xE5,0x65  C=0x43,0x00
        assert_eq!(result, vec![0x41, 0x00, 0x42, 0x00, 0xE5, 0x65, 0x43, 0x00]);
    }

    #[test]
    fn decompress_uncompressed_odd_trailing_byte() {
        // Switch to uncompressed, then only 1 byte remains → ignored
        let input = [0x00, 0xAB];
        let result = decompress_text(&input);
        assert_eq!(result, Vec::<u8>::new());
    }

    // -- decode_text tests ----------------------------------------------------

    /// A Jet3 file of code page 1252, as Access 97 makes in Western Europe.
    const WESTERN: TextEncoding = TextEncoding::Jet3 { code_page: 1252 };

    #[test]
    fn decode_text_jet3_in_its_code_page() {
        // 0x80 is the euro sign in code page 1252, a control in Latin-1.
        assert_eq!(decode_text(&[0x80, 0x41], WESTERN).unwrap(), "\u{20AC}A");
        // Code page 932 (Japanese): 顧客, half-width katakana テ, and ～,
        // which Windows reads as U+FF5E.
        let japanese = TextEncoding::Jet3 { code_page: 932 };
        let bytes = [0x8C, 0xDA, 0x8B, 0x71, 0xC3, 0x81, 0x60];
        assert_eq!(decode_text(&bytes, japanese).unwrap(), "顧客ﾃ～");
        // A code page jetdb does not know is read as Latin-1.
        let unknown = TextEncoding::Jet3 { code_page: 1 };
        assert_eq!(decode_text(&[0x80, 0xE9], unknown).unwrap(), "\u{80}é");
    }

    #[test]
    fn decode_text_compressed_hello() {
        // 0xFF 0xFE header + compressed "Hello"
        let data = [0xFF, 0xFE, 0x48, 0x65, 0x6C, 0x6C, 0x6F];
        assert_eq!(decode_text(&data, TextEncoding::Unicode).unwrap(), "Hello");
    }

    #[test]
    fn decode_text_raw_utf16le() {
        // No compression header: raw "Hi" in UTF-16LE
        let data = [0x48, 0x00, 0x69, 0x00];
        assert_eq!(decode_text(&data, TextEncoding::Unicode).unwrap(), "Hi");
    }

    #[test]
    fn decode_text_mixed_ascii_japanese() {
        // Compressed "A" → 0x00 → Uncompressed "日" (U+65E5 = 0xE5 0x65) → 0x00 → Compressed "B"
        let mut data = vec![0xFF, 0xFE]; // header
        data.push(0x41); // compressed 'A'
        data.push(0x00); // switch to uncompressed
        data.extend_from_slice(&[0xE5, 0x65]); // 日 in UTF-16LE
        data.push(0x00); // switch to compressed
        data.push(0x42); // compressed 'B'
        assert_eq!(decode_text(&data, TextEncoding::Unicode).unwrap(), "A日B");
    }

    #[test]
    fn decode_text_empty() {
        assert_eq!(decode_text(&[], TextEncoding::Unicode).unwrap(), "");
    }

    #[test]
    fn decode_text_header_only() {
        let data = [0xFF, 0xFE];
        assert_eq!(decode_text(&data, TextEncoding::Unicode).unwrap(), "");
    }

    #[test]
    fn decode_text_jet3_latin1() {
        // Jet3: direct Latin-1 decode, no compression
        let data = [0x48, 0x65, 0x6C, 0x6C, 0x6F];
        assert_eq!(decode_text(&data, WESTERN).unwrap(), "Hello");
    }

    #[test]
    fn decode_text_jet3_special_chars() {
        // Jet3: Latin-1 with accented characters
        let data = [0xC0, 0xE9]; // À, é
        assert_eq!(decode_text(&data, WESTERN).unwrap(), "Àé");
    }
}
