//! Object property reading from MSysObjects.LvProp blobs.

use crate::catalog::CatalogEntry;
use crate::data::{self, format_guid, Value};
use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::format::{ObjectType, CATALOG_PAGE};
use crate::money;
use crate::table;

// ---------------------------------------------------------------------------
// Public data structures
// ---------------------------------------------------------------------------

/// Property map classification based on chunk type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropMapType {
    /// Table-level properties (chunk type 0x0000).
    Default,
    /// Column-level properties (chunk type 0x0001).
    Column,
    /// Additional properties (chunk type 0x0002).
    Additional,
}

/// A single property entry.
#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub value: Value,
    pub ddl: bool,
}

/// A named group of properties (table-level or per-column).
#[derive(Debug, Clone)]
pub struct PropertyMap {
    pub map_type: PropMapType,
    /// Map name (empty for table-level, column name for column-level).
    pub name: String,
    pub properties: Vec<Property>,
}

/// All properties for a single database object.
#[derive(Debug, Clone)]
pub struct ObjectProperties {
    pub object_name: String,
    pub maps: Vec<PropertyMap>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// The properties of `entry`, from the LvProp [`crate::read_catalog`] keeps,
/// without reading MSysObjects again. `is_jet3` tells the version of the file
/// the catalog was read from.
pub fn entry_properties(
    entry: &CatalogEntry,
    is_jet3: bool,
) -> Result<ObjectProperties, FileError> {
    let maps = match &entry.lv_prop {
        Some(data) => parse_lvprop(data, is_jet3)?,
        None => Vec::new(),
    };
    Ok(ObjectProperties {
        object_name: entry.name.clone(),
        maps,
    })
}

/// Read properties for a named object from MSysObjects.LvProp.
///
/// Objects of different types can share a name, such as a table and a form
/// both named `Customers`; this reads the first object of that name. Use
/// [`read_object_properties_of_type`] to name the type too.
pub fn read_object_properties(
    reader: &mut PageReader,
    object_name: &str,
) -> Result<ObjectProperties, FileError> {
    read_properties(reader, object_name, None)
}

/// Read properties for the object of `object_type` named `object_name` from
/// MSysObjects.LvProp. An object that is not there has no properties.
pub fn read_object_properties_of_type(
    reader: &mut PageReader,
    object_name: &str,
    object_type: ObjectType,
) -> Result<ObjectProperties, FileError> {
    read_properties(reader, object_name, Some(object_type))
}

/// The properties of the first object named `object_name`, of `object_type`
/// when it is given.
fn read_properties(
    reader: &mut PageReader,
    object_name: &str,
    object_type: Option<ObjectType>,
) -> Result<ObjectProperties, FileError> {
    let is_jet3 = reader.header().version.is_jet3();
    let tdef = table::read_table_def(reader, "MSysObjects", CATALOG_PAGE)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysObjects");

    // Locate Name, Type and LvProp column indices
    let (mut name_idx, mut type_idx, mut lvprop_idx) = (None, None, None);
    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "Name" => name_idx = Some(i),
            "Type" => type_idx = Some(i),
            "LvProp" => lvprop_idx = Some(i),
            _ => {}
        }
    }
    let name_idx = name_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysObjects missing Name column",
    })?;
    let type_idx = type_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysObjects missing Type column",
    })?;

    let lvprop_idx = match lvprop_idx {
        Some(i) => i,
        None => {
            return Ok(ObjectProperties {
                object_name: object_name.to_string(),
                maps: Vec::new(),
            });
        }
    };

    // The object not found, or without properties, has none.
    let data = result
        .rows
        .iter()
        .find(|row| is_object(row, (name_idx, type_idx), object_name, object_type))
        .and_then(|row| match row.get(lvprop_idx) {
            Some(Value::Binary(b)) => Some(b),
            _ => None,
        });
    let maps = match data {
        Some(data) => parse_lvprop(data, is_jet3)?,
        None => Vec::new(),
    };
    Ok(ObjectProperties {
        object_name: object_name.to_string(),
        maps,
    })
}

/// Whether `row` of MSysObjects, with its Name and Type at `name_idx` and
/// `type_idx`, is the object named `object_name`, of `object_type` when it
/// is given.
fn is_object(
    row: &[Value],
    (name_idx, type_idx): (usize, usize),
    object_name: &str,
    object_type: Option<ObjectType>,
) -> bool {
    let name_matches = matches!(row.get(name_idx), Some(Value::Text(s)) if s == object_name);
    let type_matches = object_type.is_none_or(|wanted| {
        matches!(row.get(type_idx), Some(Value::Int(v)) if ObjectType::try_from(i32::from(*v)).ok() == Some(wanted))
    });
    name_matches && type_matches
}

// ---------------------------------------------------------------------------
// Internal parse functions
// ---------------------------------------------------------------------------

/// Magic bytes for Jet3 LvProp header.
const HEADER_JET3: &[u8; 4] = b"KKD\0";
/// Magic bytes for Jet4/ACE LvProp header.
const HEADER_JET4: &[u8; 4] = b"MR2\0";

/// Chunk type: name dictionary.
const CHUNK_NAME_LIST: u16 = 0x0080;

/// Validate the 4-byte LvProp header magic bytes.
fn validate_header(data: &[u8]) -> Result<(), FileError> {
    if data.len() < 4 {
        return Err(FileError::InvalidProperty {
            reason: "LvProp data too short for header",
        });
    }
    if &data[..4] == HEADER_JET3 || &data[..4] == HEADER_JET4 {
        Ok(())
    } else {
        Err(FileError::InvalidProperty {
            reason: "LvProp header: unknown magic bytes",
        })
    }
}

/// Parse the entire LvProp binary blob.
pub(crate) fn parse_lvprop(data: &[u8], is_jet3: bool) -> Result<Vec<PropertyMap>, FileError> {
    validate_header(data)?;

    let mut offset = 4; // skip header
    let mut names: Vec<String> = Vec::new();
    let mut maps: Vec<PropertyMap> = Vec::new();

    while offset + 6 <= data.len() {
        let chunk_len = u32::from_le_bytes(data[offset..offset + 4].try_into().map_err(|_| {
            FileError::InvalidProperty {
                reason: "chunk_len slice mismatch",
            }
        })?) as usize;
        let chunk_type =
            u16::from_le_bytes(data[offset + 4..offset + 6].try_into().map_err(|_| {
                FileError::InvalidProperty {
                    reason: "chunk_type slice mismatch",
                }
            })?);

        if chunk_len < 6 {
            return Err(FileError::InvalidProperty {
                reason: "LvProp chunk_len < 6",
            });
        }
        // A chunk running past the end, or so long that offset + chunk_len
        // overflows a 32-bit usize, is truncated — stop parsing rather than
        // error.
        let Some(chunk_end) = offset
            .checked_add(chunk_len)
            .filter(|&end| end <= data.len())
        else {
            break;
        };

        let chunk_data = &data[offset + 6..chunk_end];

        match chunk_type {
            CHUNK_NAME_LIST => {
                names = parse_name_list(chunk_data, is_jet3)?;
            }
            0x0000..=0x0002 => {
                let map = parse_value_chunk(chunk_data, chunk_type, &names, is_jet3)?;
                maps.push(map);
            }
            _ => {
                // Unknown chunk type — skip
            }
        }

        offset = chunk_end;
    }

    Ok(maps)
}

/// Parse a name-list chunk (type 0x0080).
fn parse_name_list(chunk_data: &[u8], is_jet3: bool) -> Result<Vec<String>, FileError> {
    let mut names = Vec::new();
    let mut pos = 0;

    while pos + 2 <= chunk_data.len() {
        let name_len = u16::from_le_bytes(chunk_data[pos..pos + 2].try_into().map_err(|_| {
            FileError::InvalidProperty {
                reason: "name_len slice mismatch",
            }
        })?) as usize;
        pos += 2;
        if pos + name_len > chunk_data.len() {
            break;
        }
        let name_bytes = &chunk_data[pos..pos + name_len];
        let name = if is_jet3 {
            encoding::decode_latin1(name_bytes)
        } else {
            encoding::decode_utf16le(name_bytes).map_err(FileError::Format)?
        };
        names.push(name);
        pos += name_len;
    }

    Ok(names)
}

/// Parse a value chunk (type 0x0000 / 0x0001 / 0x0002).
fn parse_value_chunk(
    chunk_data: &[u8],
    chunk_type: u16,
    names: &[String],
    is_jet3: bool,
) -> Result<PropertyMap, FileError> {
    let map_type = match chunk_type {
        0x0000 => PropMapType::Default,
        0x0001 => PropMapType::Column,
        0x0002 => PropMapType::Additional,
        _ => PropMapType::Default,
    };

    // Sub-header: name_block_len (4 bytes)
    if chunk_data.len() < 4 {
        return Ok(PropertyMap {
            map_type,
            name: String::new(),
            properties: Vec::new(),
        });
    }

    let name_block_len =
        u32::from_le_bytes(
            chunk_data[..4]
                .try_into()
                .map_err(|_| FileError::InvalidProperty {
                    reason: "name_block_len slice mismatch",
                })?,
        ) as usize;

    // Extract block name
    let block_name = if name_block_len > 6 && chunk_data.len() >= 6 {
        let block_name_len = u16::from_le_bytes(chunk_data[4..6].try_into().map_err(|_| {
            FileError::InvalidProperty {
                reason: "block_name_len slice mismatch",
            }
        })?) as usize;
        let name_end = (6 + block_name_len).min(chunk_data.len());
        let name_bytes = &chunk_data[6..name_end];
        if is_jet3 {
            encoding::decode_latin1(name_bytes)
        } else {
            encoding::decode_utf16le(name_bytes).unwrap_or_default()
        }
    } else {
        String::new()
    };

    // Property entries start after name_block_len bytes
    let entries_start = name_block_len.min(chunk_data.len());
    let mut properties = Vec::new();
    let mut pos = entries_start;

    while pos + 8 <= chunk_data.len() {
        let val_len = u16::from_le_bytes(chunk_data[pos..pos + 2].try_into().map_err(|_| {
            FileError::InvalidProperty {
                reason: "val_len slice mismatch",
            }
        })?) as usize;
        if val_len < 8 {
            break;
        }

        let ddl_flag = chunk_data[pos + 2];
        let data_type = chunk_data[pos + 3];
        let name_idx =
            u16::from_le_bytes(chunk_data[pos + 4..pos + 6].try_into().map_err(|_| {
                FileError::InvalidProperty {
                    reason: "name_idx slice mismatch",
                }
            })?) as usize;
        let data_size =
            u16::from_le_bytes(chunk_data[pos + 6..pos + 8].try_into().map_err(|_| {
                FileError::InvalidProperty {
                    reason: "data_size slice mismatch",
                }
            })?) as usize;

        let prop_name = names.get(name_idx).cloned().unwrap_or_default();

        let value_end = (pos + 8 + data_size).min(chunk_data.len());
        let raw = &chunk_data[pos + 8..value_end];

        let value = decode_prop_value(data_type, raw, &prop_name, is_jet3);

        properties.push(Property {
            name: prop_name,
            value,
            ddl: ddl_flag != 0,
        });

        // Advance by val_len (entry total length)
        pos += val_len;
    }

    Ok(PropertyMap {
        map_type,
        name: block_name,
        properties,
    })
}

/// Decode a property value based on its data type byte.
fn decode_prop_value(data_type: u8, raw: &[u8], prop_name: &str, is_jet3: bool) -> Value {
    match data_type {
        // Bool (0x01): 1 byte, 0 = false, non-zero = true
        0x01 => {
            if raw.is_empty() {
                Value::Bool(false)
            } else {
                Value::Bool(raw[0] != 0)
            }
        }
        // Byte (0x02)
        0x02 => {
            if raw.is_empty() {
                Value::Null
            } else {
                Value::Byte(raw[0])
            }
        }
        // Int (0x03): 2 bytes LE
        0x03 => {
            if raw.len() < 2 {
                Value::Null
            } else {
                Value::Int(i16::from_le_bytes([raw[0], raw[1]]))
            }
        }
        // Long (0x04): 4 bytes LE
        0x04 => {
            if raw.len() < 4 {
                Value::Null
            } else {
                let Ok(bytes) = raw[..4].try_into() else {
                    return Value::Null;
                };
                Value::Long(i32::from_le_bytes(bytes))
            }
        }
        // Money (0x05): 8 bytes LE
        0x05 => {
            if raw.len() < 8 {
                Value::Null
            } else {
                let Ok(bytes): Result<[u8; 8], _> = raw[..8].try_into() else {
                    return Value::Null;
                };
                Value::Money(money::money_to_string(&bytes))
            }
        }
        // Float (0x06): 4 bytes LE
        0x06 => {
            if raw.len() < 4 {
                Value::Null
            } else {
                let Ok(bytes) = raw[..4].try_into() else {
                    return Value::Null;
                };
                Value::Float(f32::from_le_bytes(bytes))
            }
        }
        // Double (0x07): 8 bytes LE
        0x07 => {
            if raw.len() < 8 {
                Value::Null
            } else {
                let Ok(bytes) = raw[..8].try_into() else {
                    return Value::Null;
                };
                Value::Double(f64::from_le_bytes(bytes))
            }
        }
        // Timestamp (0x08): 8 bytes LE f64
        0x08 => {
            if raw.len() < 8 {
                Value::Null
            } else {
                let Ok(bytes) = raw[..8].try_into() else {
                    return Value::Null;
                };
                Value::Timestamp(f64::from_le_bytes(bytes))
            }
        }
        // Binary (0x09): special case for GUID
        0x09 => {
            if raw.len() == 16 && prop_name == "GUID" {
                Value::Guid(format_guid(raw))
            } else {
                Value::Binary(raw.to_vec())
            }
        }
        // Text (0x0A): non-compressed decode
        0x0A => {
            if is_jet3 {
                Value::Text(encoding::decode_latin1(raw))
            } else {
                match encoding::decode_utf16le(raw) {
                    Ok(s) => Value::Text(s),
                    Err(_) => Value::Binary(raw.to_vec()),
                }
            }
        }
        // OLE (0x0B): treat as binary
        0x0B => Value::Binary(raw.to_vec()),
        // Memo (0x0C): decode as text
        0x0C => {
            if is_jet3 {
                Value::Text(encoding::decode_latin1(raw))
            } else {
                match encoding::decode_utf16le(raw) {
                    Ok(s) => Value::Text(s),
                    Err(_) => Value::Binary(raw.to_vec()),
                }
            }
        }
        // Guid (0x0F): format as GUID if 16 bytes
        0x0F => {
            if raw.len() == 16 {
                Value::Guid(format_guid(raw))
            } else {
                Value::Binary(raw.to_vec())
            }
        }
        // Unknown types: preserve as binary
        _ => Value::Binary(raw.to_vec()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- validate_header ------------------------------------------------------

    #[test]
    fn validate_header_jet3() {
        let data = b"KKD\0some extra data";
        assert!(validate_header(data).is_ok());
    }

    #[test]
    fn validate_header_jet4() {
        let data = b"MR2\0some extra data";
        assert!(validate_header(data).is_ok());
    }

    #[test]
    fn validate_header_invalid() {
        let data = b"XXXX";
        assert!(validate_header(data).is_err());
    }

    #[test]
    fn validate_header_too_short() {
        let data = b"MR";
        assert!(validate_header(data).is_err());
    }

    // -- parse_name_list ------------------------------------------------------

    #[test]
    fn parse_name_list_jet3() {
        // Two Latin-1 names: "Foo" (3 bytes), "Bar" (3 bytes)
        let mut data = Vec::new();
        data.extend_from_slice(&3u16.to_le_bytes());
        data.extend_from_slice(b"Foo");
        data.extend_from_slice(&3u16.to_le_bytes());
        data.extend_from_slice(b"Bar");

        let names = parse_name_list(&data, true).unwrap();
        assert_eq!(names, vec!["Foo", "Bar"]);
    }

    #[test]
    fn parse_name_list_jet4() {
        // Two UTF-16LE names: "Hi" (4 bytes), "Go" (4 bytes)
        let mut data = Vec::new();
        // "Hi"
        data.extend_from_slice(&4u16.to_le_bytes());
        data.extend_from_slice(&[0x48, 0x00, 0x69, 0x00]);
        // "Go"
        data.extend_from_slice(&4u16.to_le_bytes());
        data.extend_from_slice(&[0x47, 0x00, 0x6F, 0x00]);

        let names = parse_name_list(&data, false).unwrap();
        assert_eq!(names, vec!["Hi", "Go"]);
    }

    #[test]
    fn parse_name_list_empty() {
        let names = parse_name_list(&[], true).unwrap();
        assert!(names.is_empty());
    }

    // -- decode_prop_value ----------------------------------------------------

    #[test]
    fn decode_prop_value_bool_true() {
        let val = decode_prop_value(0x01, &[0x01], "x", false);
        assert_eq!(val, Value::Bool(true));
    }

    #[test]
    fn decode_prop_value_bool_false() {
        let val = decode_prop_value(0x01, &[0x00], "x", false);
        assert_eq!(val, Value::Bool(false));
    }

    #[test]
    fn decode_prop_value_text_jet4() {
        // "Ab" in UTF-16LE
        let raw = [0x41, 0x00, 0x62, 0x00];
        let val = decode_prop_value(0x0A, &raw, "x", false);
        assert_eq!(val, Value::Text("Ab".to_string()));
    }

    #[test]
    fn decode_prop_value_text_jet3() {
        let raw = b"Hello";
        let val = decode_prop_value(0x0A, raw, "x", true);
        assert_eq!(val, Value::Text("Hello".to_string()));
    }

    #[test]
    fn decode_prop_value_long() {
        let raw = 42i32.to_le_bytes();
        let val = decode_prop_value(0x04, &raw, "x", false);
        assert_eq!(val, Value::Long(42));
    }

    #[test]
    fn decode_prop_value_int() {
        let raw = (-7i16).to_le_bytes();
        let val = decode_prop_value(0x03, &raw, "x", false);
        assert_eq!(val, Value::Int(-7));
    }

    #[test]
    fn decode_prop_value_binary_as_guid() {
        let guid_bytes: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ];
        let val = decode_prop_value(0x09, &guid_bytes, "GUID", false);
        assert_eq!(
            val,
            Value::Guid("{04030201-0605-0807-090A-0B0C0D0E0F10}".to_string())
        );
    }

    #[test]
    fn decode_prop_value_binary_non_guid() {
        let raw = [0x01, 0x02, 0x03];
        let val = decode_prop_value(0x09, &raw, "SomeField", false);
        assert_eq!(val, Value::Binary(vec![0x01, 0x02, 0x03]));
    }

    #[test]
    fn decode_prop_value_memo_as_text() {
        // Memo (0x0C) in UTF-16LE: "Ok"
        let raw = [0x4F, 0x00, 0x6B, 0x00];
        let val = decode_prop_value(0x0C, &raw, "x", false);
        assert_eq!(val, Value::Text("Ok".to_string()));
    }

    // -- parse_value_chunk ----------------------------------------------------

    #[test]
    fn parse_value_chunk_with_name() {
        let names = vec!["Required".to_string(), "Description".to_string()];

        // Build chunk_data:
        // Sub-header: name_block_len = 4 + 2 + 6 = 12 (includes block name "Col" in UTF-16LE)
        // Block name: "Col" = [0x43,0x00, 0x6F,0x00, 0x6C,0x00] (6 bytes)
        // name_block_len = 12 (4 + 2 + 6)
        let mut chunk = Vec::new();
        // name_block_len
        chunk.extend_from_slice(&12u32.to_le_bytes());
        // block name length
        chunk.extend_from_slice(&6u16.to_le_bytes());
        // block name "Col" in UTF-16LE
        chunk.extend_from_slice(&[0x43, 0x00, 0x6F, 0x00, 0x6C, 0x00]);

        // Property entry: Bool "Required" = true
        // val_len = 8 + 1 = 9, ddl_flag = 1, data_type = 0x01, name_idx = 0,
        // data_size = 1, value = [0x01]
        chunk.extend_from_slice(&9u16.to_le_bytes()); // val_len
        chunk.push(0x01); // ddl_flag
        chunk.push(0x01); // data_type = Bool
        chunk.extend_from_slice(&0u16.to_le_bytes()); // name_idx = 0 -> "Required"
        chunk.extend_from_slice(&1u16.to_le_bytes()); // data_size = 1
        chunk.push(0x01); // value = true

        let map = parse_value_chunk(&chunk, 0x0001, &names, false).unwrap();
        assert_eq!(map.map_type, PropMapType::Column);
        assert_eq!(map.name, "Col");
        assert_eq!(map.properties.len(), 1);
        assert_eq!(map.properties[0].name, "Required");
        assert_eq!(map.properties[0].value, Value::Bool(true));
        assert!(map.properties[0].ddl);
    }

    #[test]
    fn parse_value_chunk_no_name() {
        let names = vec!["AccessVersion".to_string()];

        // name_block_len = 4 (no block name)
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&4u32.to_le_bytes());

        // Property entry: Text "AccessVersion" = "08.50" in UTF-16LE
        // "08.50" = 5 chars = 10 bytes
        let text_bytes = [0x30, 0x00, 0x38, 0x00, 0x2E, 0x00, 0x35, 0x00, 0x30, 0x00];
        let val_len: u16 = 8 + text_bytes.len() as u16;
        chunk.extend_from_slice(&val_len.to_le_bytes());
        chunk.push(0x00); // ddl_flag = false
        chunk.push(0x0A); // data_type = Text
        chunk.extend_from_slice(&0u16.to_le_bytes()); // name_idx = 0
        chunk.extend_from_slice(&(text_bytes.len() as u16).to_le_bytes());
        chunk.extend_from_slice(&text_bytes);

        let map = parse_value_chunk(&chunk, 0x0000, &names, false).unwrap();
        assert_eq!(map.map_type, PropMapType::Default);
        assert_eq!(map.name, "");
        assert_eq!(map.properties.len(), 1);
        assert_eq!(map.properties[0].name, "AccessVersion");
        assert_eq!(map.properties[0].value, Value::Text("08.50".to_string()));
        assert!(!map.properties[0].ddl);
    }

    // -- parse_lvprop full ----------------------------------------------------

    #[test]
    fn parse_lvprop_jet4_full() {
        // Build a complete LvProp blob:
        // [header(4)] [name_list_chunk] [value_chunk]
        let mut blob = Vec::new();

        // Header: MR2\0
        blob.extend_from_slice(b"MR2\0");

        // -- Name list chunk (type=0x0080) --
        // Names: "Title" in UTF-16LE (10 bytes)
        let mut name_payload = Vec::new();
        let title_utf16 = [0x54, 0x00, 0x69, 0x00, 0x74, 0x00, 0x6C, 0x00, 0x65, 0x00]; // "Title"
        name_payload.extend_from_slice(&(title_utf16.len() as u16).to_le_bytes());
        name_payload.extend_from_slice(&title_utf16);

        let name_chunk_len = 6 + name_payload.len();
        blob.extend_from_slice(&(name_chunk_len as u32).to_le_bytes());
        blob.extend_from_slice(&0x0080u16.to_le_bytes());
        blob.extend_from_slice(&name_payload);

        // -- Value chunk (type=0x0000, table-level) --
        let mut value_payload = Vec::new();
        // name_block_len = 4 (no block name)
        value_payload.extend_from_slice(&4u32.to_le_bytes());

        // Property: Text "Title" = "Test" in UTF-16LE
        let test_utf16 = [0x54, 0x00, 0x65, 0x00, 0x73, 0x00, 0x74, 0x00]; // "Test"
        let entry_len: u16 = 8 + test_utf16.len() as u16;
        value_payload.extend_from_slice(&entry_len.to_le_bytes());
        value_payload.push(0x00); // ddl_flag
        value_payload.push(0x0A); // data_type = Text
        value_payload.extend_from_slice(&0u16.to_le_bytes()); // name_idx = 0
        value_payload.extend_from_slice(&(test_utf16.len() as u16).to_le_bytes());
        value_payload.extend_from_slice(&test_utf16);

        let value_chunk_len = 6 + value_payload.len();
        blob.extend_from_slice(&(value_chunk_len as u32).to_le_bytes());
        blob.extend_from_slice(&0x0000u16.to_le_bytes());
        blob.extend_from_slice(&value_payload);

        let maps = parse_lvprop(&blob, false).unwrap();
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].map_type, PropMapType::Default);
        assert_eq!(maps[0].name, "");
        assert_eq!(maps[0].properties.len(), 1);
        assert_eq!(maps[0].properties[0].name, "Title");
        assert_eq!(maps[0].properties[0].value, Value::Text("Test".to_string()));
    }

    #[test]
    fn parse_lvprop_unknown_chunk_skipped() {
        // Header + unknown chunk (type=0x00FF) + name list chunk
        let mut blob = Vec::new();
        blob.extend_from_slice(b"MR2\0");

        // Unknown chunk: type=0x00FF, payload = 4 bytes of garbage
        let unknown_chunk_len: u32 = 10; // 6 header + 4 payload
        blob.extend_from_slice(&unknown_chunk_len.to_le_bytes());
        blob.extend_from_slice(&0x00FFu16.to_le_bytes());
        blob.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

        // Name list chunk (empty)
        let name_chunk_len: u32 = 6; // header only, no names
        blob.extend_from_slice(&name_chunk_len.to_le_bytes());
        blob.extend_from_slice(&0x0080u16.to_le_bytes());

        let maps = parse_lvprop(&blob, false).unwrap();
        assert!(maps.is_empty());
    }

    #[test]
    fn parse_lvprop_chunk_len_past_the_end() {
        // A value chunk after a valid one claims 0xFFFFFFFF bytes. Where usize
        // is 32 bits, as in Wasm, offset + chunk_len wraps around; it must
        // still be read as a chunk cut short, not as a slice out of range.
        let mut blob = Vec::new();
        blob.extend_from_slice(b"MR2\0");
        blob.extend_from_slice(&6u32.to_le_bytes());
        blob.extend_from_slice(&0x0080u16.to_le_bytes());
        blob.extend_from_slice(&u32::MAX.to_le_bytes());
        blob.extend_from_slice(&0x0000u16.to_le_bytes());
        blob.extend_from_slice(&[0; 8]);

        let maps = parse_lvprop(&blob, false).unwrap();
        assert!(maps.is_empty());
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

    fn assert_has_properties(props: &ObjectProperties) {
        assert!(
            !props.maps.is_empty(),
            "object '{}' should have at least one property map",
            props.object_name
        );
        // At least one map should have properties
        let total_props: usize = props.maps.iter().map(|m| m.properties.len()).sum();
        assert!(
            total_props > 0,
            "object '{}' should have at least one property",
            props.object_name
        );
    }

    #[test]
    fn jet3_table_properties() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_object_properties(&mut reader, "Table1").unwrap();
        assert_eq!(props.object_name, "Table1");
        assert_has_properties(&props);
    }

    #[test]
    fn jet4_table_properties() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_object_properties(&mut reader, "Table1").unwrap();
        assert_eq!(props.object_name, "Table1");
        assert_has_properties(&props);

        // Default マップ (テーブルプロパティ) が存在すること
        let default_map = props
            .maps
            .iter()
            .find(|m| m.map_type == PropMapType::Default);
        assert!(default_map.is_some(), "should have a Default property map");

        // GUID プロパティが存在すること
        let default_map = default_map.unwrap();
        let guid = default_map.properties.iter().find(|p| p.name == "GUID");
        assert!(guid.is_some(), "should have GUID property");
    }

    #[test]
    fn ace12_table_properties() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_object_properties(&mut reader, "Table1").unwrap();
        assert_eq!(props.object_name, "Table1");
        assert_has_properties(&props);
    }

    #[test]
    fn ace14_table_properties() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_object_properties(&mut reader, "Table1").unwrap();
        assert_eq!(props.object_name, "Table1");
        assert_has_properties(&props);
    }

    #[test]
    fn properties_of_a_catalog_entry() {
        // The same as reading MSysObjects again, for each object of
        // Customers in nwind.mdb (a table, a form and a macro).
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = crate::read_catalog(&mut reader).unwrap();
        let customers: Vec<_> = catalog.iter().filter(|e| e.name == "Customers").collect();
        assert_eq!(customers.len(), 3);
        for entry in customers {
            let read = read_object_properties_of_type(&mut reader, &entry.name, entry.object_type)
                .unwrap();
            assert!(!read.maps.is_empty(), "{:?}", entry.object_type);
            assert_eq!(
                format!("{:?}", entry_properties(entry, true).unwrap()),
                format!("{read:?}"),
                "{:?}",
                entry.object_type
            );
        }
    }

    #[test]
    fn properties_of_the_object_of_a_type() {
        // nwind.mdb has a table, a form and a macro all named Customers.
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let column_names = |props: &ObjectProperties| -> Vec<String> {
            props
                .maps
                .iter()
                .filter(|m| m.map_type == PropMapType::Column)
                .map(|m| m.name.clone())
                .collect()
        };
        let description = |props: &ObjectProperties| -> Option<String> {
            props
                .maps
                .iter()
                .filter(|m| m.map_type == PropMapType::Default)
                .flat_map(|m| &m.properties)
                .find(|p| p.name == "Description")
                .and_then(|p| match &p.value {
                    Value::Text(s) => Some(s.clone()),
                    _ => None,
                })
        };

        let table =
            read_object_properties_of_type(&mut reader, "Customers", ObjectType::Table).unwrap();
        assert!(
            column_names(&table).contains(&"CustomerID".to_string()),
            "{:?}",
            column_names(&table)
        );

        let form =
            read_object_properties_of_type(&mut reader, "Customers", ObjectType::Form).unwrap();
        assert!(column_names(&form).is_empty());
        let form_description = description(&form).unwrap();
        assert!(
            form_description.contains("Single-column form"),
            "{form_description}"
        );
        assert_ne!(description(&table), Some(form_description));

        let query =
            read_object_properties_of_type(&mut reader, "Customers", ObjectType::Query).unwrap();
        assert!(query.maps.is_empty());
    }

    #[test]
    fn nonexistent_object_returns_empty() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_object_properties(&mut reader, "NoSuchObject_XYZ_12345").unwrap();
        assert!(props.maps.is_empty());
    }

    // -- decode_prop_value additional types ------------------------------------

    #[test]
    fn decode_prop_value_money() {
        let raw = 10_000i64.to_le_bytes();
        let val = decode_prop_value(0x05, &raw, "test", false);
        assert_eq!(val, Value::Money("1.0000".to_string()));
    }

    #[test]
    fn decode_prop_value_money_short() {
        let val = decode_prop_value(0x05, &[0x01, 0x02, 0x03], "test", false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn decode_prop_value_float() {
        let raw = 1.5f32.to_le_bytes();
        let val = decode_prop_value(0x06, &raw, "test", false);
        assert!(matches!(val, Value::Float(v) if (v - 1.5).abs() < f32::EPSILON));
    }

    #[test]
    fn decode_prop_value_float_short() {
        let val = decode_prop_value(0x06, &[0x01, 0x02], "test", false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn decode_prop_value_double() {
        let raw = 3.125f64.to_le_bytes();
        let val = decode_prop_value(0x07, &raw, "test", false);
        assert!(matches!(val, Value::Double(v) if (v - 3.125).abs() < f64::EPSILON));
    }

    #[test]
    fn decode_prop_value_double_short() {
        let val = decode_prop_value(0x07, &[0x01, 0x02, 0x03, 0x04], "test", false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn decode_prop_value_timestamp() {
        let raw = 37623.0f64.to_le_bytes();
        let val = decode_prop_value(0x08, &raw, "test", false);
        assert!(matches!(val, Value::Timestamp(v) if (v - 37623.0).abs() < f64::EPSILON));
    }

    #[test]
    fn decode_prop_value_timestamp_short() {
        let val = decode_prop_value(0x08, &[0x01], "test", false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn decode_prop_value_ole_binary() {
        let raw = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let val = decode_prop_value(0x0B, &raw, "test", false);
        assert_eq!(val, Value::Binary(raw));
    }

    #[test]
    fn decode_prop_value_guid_16bytes() {
        let guid_bytes: [u8; 16] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ];
        let val = decode_prop_value(0x0F, &guid_bytes, "test", false);
        assert_eq!(
            val,
            Value::Guid("{04030201-0605-0807-090A-0B0C0D0E0F10}".to_string())
        );
    }

    #[test]
    fn decode_prop_value_guid_non16bytes() {
        let raw = vec![0x01, 0x02, 0x03];
        let val = decode_prop_value(0x0F, &raw, "test", false);
        assert_eq!(val, Value::Binary(raw));
    }

    #[test]
    fn decode_prop_value_byte_empty() {
        let val = decode_prop_value(0x02, &[], "test", false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn decode_prop_value_byte_valid() {
        let val = decode_prop_value(0x02, &[42], "test", false);
        assert_eq!(val, Value::Byte(42));
    }

    #[test]
    fn decode_prop_value_int_short() {
        let val = decode_prop_value(0x03, &[0x01], "test", false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn decode_prop_value_long_short() {
        let val = decode_prop_value(0x04, &[0x01, 0x02], "test", false);
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn decode_prop_value_unknown_type() {
        let raw = vec![0xFF, 0xFE];
        let val = decode_prop_value(0xFF, &raw, "test", false);
        assert_eq!(val, Value::Binary(raw));
    }

    #[test]
    fn decode_prop_value_bool_empty() {
        let val = decode_prop_value(0x01, &[], "test", false);
        assert_eq!(val, Value::Bool(false));
    }

    #[test]
    fn decode_prop_value_memo_jet3() {
        let raw = b"Hello Jet3";
        let val = decode_prop_value(0x0C, raw, "test", true);
        assert_eq!(val, Value::Text("Hello Jet3".to_string()));
    }

    #[test]
    fn decode_prop_value_text_invalid_utf16() {
        // Odd number of bytes cannot be valid UTF-16LE
        let raw = vec![0x41, 0x00, 0x42];
        let val = decode_prop_value(0x0A, &raw, "test", false);
        assert_eq!(val, Value::Binary(raw));
    }

    #[test]
    fn decode_prop_value_memo_invalid_utf16() {
        let raw = vec![0xFF];
        let val = decode_prop_value(0x0C, &raw, "test", false);
        assert_eq!(val, Value::Binary(raw));
    }

    // -- parse_value_chunk additional type -------------------------------------

    #[test]
    fn parse_value_chunk_type_additional() {
        let names = vec!["Prop1".to_string()];
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&4u32.to_le_bytes()); // name_block_len = 4

        // Property entry: Long "Prop1" = 99
        let raw = 99i32.to_le_bytes();
        let val_len: u16 = 8 + raw.len() as u16;
        chunk.extend_from_slice(&val_len.to_le_bytes());
        chunk.push(0x00); // ddl_flag
        chunk.push(0x04); // data_type = Long
        chunk.extend_from_slice(&0u16.to_le_bytes());
        chunk.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        chunk.extend_from_slice(&raw);

        let map = parse_value_chunk(&chunk, 0x0002, &names, false).unwrap();
        assert_eq!(map.map_type, PropMapType::Additional);
        assert_eq!(map.properties[0].value, Value::Long(99));
    }

    #[test]
    fn japanese_object_properties() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_object_properties(&mut reader, "jp_テーブル2").unwrap();
        assert_eq!(props.object_name, "jp_テーブル2");
        assert_has_properties(&props);

        // Column property maps should contain 商品名
        let col_maps: Vec<&PropertyMap> = props
            .maps
            .iter()
            .filter(|m| m.map_type == PropMapType::Column)
            .collect();
        let col_names: Vec<&str> = col_maps.iter().map(|m| m.name.as_str()).collect();
        assert!(
            col_names.contains(&"商品名"),
            "should have column property map for 商品名, found: {col_names:?}"
        );
    }

    #[test]
    fn parse_value_chunk_too_short() {
        let names: Vec<String> = vec![];
        let chunk = vec![0x01, 0x02]; // chunk_data < 4 bytes
        let map = parse_value_chunk(&chunk, 0x0000, &names, false).unwrap();
        assert!(map.properties.is_empty());
    }
}
