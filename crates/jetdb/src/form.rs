//! Form and report design stream extraction from MSysAccessStorage.
//!
//! Also provides Blob binary parsing to extract form/report properties
//! (RecordSource, ControlSource, Filter, etc.) and per-control properties.

use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::storage;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Form or report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormObjectType {
    Form,
    Report,
}

/// Which binary stream to extract from a form/report storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// Main design binary (layout, controls, properties, events).
    Blob,
    /// Control name and type list.
    TypeInfo,
    /// Small property metadata.
    PropData,
    /// Delta data (usually empty).
    BlobDelta,
}

impl StreamKind {
    /// The stream name as stored in MSysAccessStorage.
    fn storage_name(&self) -> &'static str {
        match self {
            Self::Blob => "Blob",
            Self::TypeInfo => "TypeInfo",
            Self::PropData => "PropData",
            Self::BlobDelta => "BlobDelta",
        }
    }
}

/// A form/report entry (for listing).
#[derive(Debug, Clone)]
pub struct FormEntry {
    pub name: String,
    pub object_type: FormObjectType,
}

/// Raw binary stream from a form/report.
#[derive(Debug, Clone)]
pub struct FormStream {
    pub name: String,
    pub object_type: FormObjectType,
    pub stream_kind: StreamKind,
    pub data: Vec<u8>,
}

/// A single control entry from TypeInfo.
#[derive(Debug, Clone)]
pub struct ControlInfo {
    pub name: String,
    pub type_code: u16,
    pub index: u32,
}

/// Parsed TypeInfo for a form/report.
#[derive(Debug, Clone)]
pub struct FormTypeInfo {
    pub form_name: String,
    pub object_type: FormObjectType,
    pub controls: Vec<ControlInfo>,
}

/// Property value extracted from a Blob stream.
#[derive(Debug, Clone)]
pub enum BlobValue {
    Bool(bool),
    Short(i16),
    Long(i32),
    Color(u32),
    Double(f64),
    Guid(String),
    Text(String),
    Binary(Vec<u8>),
}

/// A single property entry from the Blob binary.
#[derive(Debug, Clone)]
pub struct BlobProperty {
    pub prop_id: u16,
    pub value: BlobValue,
}

/// Properties for a single control, extracted from the Blob.
#[derive(Debug, Clone)]
pub struct ControlProperties {
    /// Control name (from Name property 0x14 in Blob).
    pub name: String,
    /// Control type code (from TypeInfo).
    pub type_code: u16,
    /// Properties for this control.
    pub properties: Vec<BlobProperty>,
}

/// All properties for a form or report, including per-control properties.
#[derive(Debug, Clone)]
pub struct FormProperties {
    pub form_name: String,
    pub object_type: FormObjectType,
    /// Form/report-level properties (RecordSource, Filter, etc.).
    pub properties: Vec<BlobProperty>,
    /// Per-control properties.
    pub controls: Vec<ControlProperties>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// List all form and report names in the database.
pub fn list_forms(reader: &mut PageReader) -> Result<Vec<FormEntry>, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let root_id = storage::find_root_id(&entries);

    let mut result = Vec::new();

    // Collect forms
    if let Some(forms_folder) = entries
        .iter()
        .find(|e| e.parent_id == root_id && e.name == "Forms" && storage::is_storage(e))
    {
        let dir = storage::find_dir_data(&entries, forms_folder.id);
        if let Some(dir_data) = dir {
            let mapping = storage::parse_dir_data(&dir_data.data)?;
            for (name, _storage_num) in mapping {
                result.push(FormEntry {
                    name,
                    object_type: FormObjectType::Form,
                });
            }
        }
    }

    // Collect reports
    if let Some(reports_folder) = entries
        .iter()
        .find(|e| e.parent_id == root_id && e.name == "Reports" && storage::is_storage(e))
    {
        let dir = storage::find_dir_data(&entries, reports_folder.id);
        if let Some(dir_data) = dir {
            let mapping = storage::parse_dir_data(&dir_data.data)?;
            for (name, _storage_num) in mapping {
                result.push(FormEntry {
                    name,
                    object_type: FormObjectType::Report,
                });
            }
        }
    }

    Ok(result)
}

/// Read a raw binary stream from a named form or report.
pub fn read_form_stream(
    reader: &mut PageReader,
    name: &str,
    stream_kind: StreamKind,
) -> Result<FormStream, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let (object_type, stream_data) = find_stream(&entries, name, stream_kind)?;

    Ok(FormStream {
        name: name.to_string(),
        object_type,
        stream_kind,
        data: stream_data,
    })
}

/// Read and parse TypeInfo for a named form or report.
pub fn read_form_type_info(reader: &mut PageReader, name: &str) -> Result<FormTypeInfo, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let (object_type, stream_data) = find_stream(&entries, name, StreamKind::TypeInfo)?;
    let controls = parse_type_info(&stream_data)?;

    Ok(FormTypeInfo {
        form_name: name.to_string(),
        object_type,
        controls,
    })
}

/// Return the known property name for a given prop_id, or `None`.
pub fn prop_id_name(prop_id: u16) -> Option<&'static str> {
    match prop_id {
        0x0011 => Some("Caption"),
        0x0012 => Some("ColumnWidths"),
        0x0014 => Some("Name"),
        0x001B => Some("ControlSource"),
        0x0022 => Some("FontName"),
        0x0026 => Some("Format"),
        0x005B => Some("RowSource"),
        0x005D => Some("RowSourceType"),
        0x0068 => Some("OnKeyDown"),
        0x0069 => Some("OnKeyUp"),
        0x006A => Some("OnKeyPress"),
        0x006B => Some("OnMouseDown"),
        0x006C => Some("OnMouseUp"),
        0x006D => Some("OnMouseMove"),
        0x0073 => Some("OnGotFocus"),
        0x0074 => Some("OnLostFocus"),
        0x007E => Some("OnClick"),
        0x009C => Some("RecordSource"),
        0x00A0 => Some("FontName"),
        0x00DE => Some("OnEnter"),
        0x00DF => Some("OnExit"),
        0x00E0 => Some("OnDblClick"),
        0x00F5 => Some("Filter"),
        0x010A => Some("LabelType"),
        0x015A => Some("InputMask"),
        _ => None,
    }
}

/// Return the known control type name for a given TypeInfo type_code, or `None`.
pub fn control_type_name(type_code: u16) -> Option<&'static str> {
    match type_code {
        // Form controls
        0x066A => Some("CheckBox"),
        0x0A7A => Some("ToggleButton"),
        0x0B68 => Some("CommandButton"),
        0x0C64 | 0x0D64 => Some("Label"),
        0x0E65 => Some("Rectangle"),
        0x0F67 => Some("Image"),
        0x126D => Some("TextBox"),
        0x136F => Some("ComboBox"),
        0x1470 => Some("SubForm"),
        0x1666 => Some("Line"),
        0x1898 | 0x1998 => Some("Detail"),
        0x1899 => Some("FormHeader"),
        0x189A => Some("FormFooter"),
        0x247F => Some("EmptyCell"),
        // Report controls
        0x1B64 => Some("Label"),
        0x1B65 => Some("Rectangle"),
        0x1B66 => Some("Line"),
        0x1B67 => Some("Image"),
        0x1B68 => Some("CommandButton"),
        0x1B6A => Some("CheckBox"),
        0x1B6D => Some("TextBox"),
        0x1B6F => Some("ComboBox"),
        0x1B70 => Some("SubReport"),
        // Report sections
        0x1999 => Some("ReportHeader"),
        0x199A => Some("ReportFooter"),
        0x199D => Some("GroupHeader"),
        0x199E => Some("GroupFooter"),
        0x1F9B => Some("PageHeader"),
        0x1F9C => Some("PageFooter"),
        _ => None,
    }
}

impl BlobProperty {
    /// Return the known property name, or `None` for unknown IDs.
    pub fn name(&self) -> Option<&'static str> {
        prop_id_name(self.prop_id)
    }

    /// Return a display label: the known name or `0xXXXX`.
    pub fn display_name(&self) -> String {
        match self.name() {
            Some(n) => n.to_string(),
            None => format!("0x{:04X}", self.prop_id),
        }
    }
}

impl std::fmt::Display for BlobValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{}", if *v { "yes" } else { "no" }),
            Self::Short(v) => write!(f, "{v}"),
            Self::Long(v) => write!(f, "{v}"),
            Self::Color(v) => write!(f, "#{:06X}", v & 0x00FF_FFFF),
            Self::Double(v) => write!(f, "{v}"),
            Self::Guid(v) => write!(f, "{v}"),
            Self::Text(v) => write!(f, "{v}"),
            Self::Binary(v) => write!(f, "({} bytes)", v.len()),
        }
    }
}

/// Read and parse all properties from a named form or report.
///
/// Parses the Blob binary to extract form-level and per-control properties.
/// TypeInfo is used to associate control names and type codes.
pub fn read_form_properties(
    reader: &mut PageReader,
    name: &str,
) -> Result<FormProperties, FileError> {
    let entries = storage::read_storage_entries(reader)?;
    let (object_type, blob_data) = find_stream(&entries, name, StreamKind::Blob)?;

    // Try to get TypeInfo for control name/type mapping; not fatal if missing.
    let type_info_controls = match find_stream(&entries, name, StreamKind::TypeInfo) {
        Ok((_, ti_data)) => parse_type_info(&ti_data).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let (form_props, control_prop_groups) = parse_blob(&blob_data)?;

    // Merge control property groups with TypeInfo data.
    let controls = control_prop_groups
        .into_iter()
        .enumerate()
        .map(|(i, props)| {
            // Find control name from Blob's Name property (0x14).
            let blob_name = props
                .iter()
                .find(|p| p.prop_id == 0x0014)
                .and_then(|p| match &p.value {
                    BlobValue::Text(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| format!("Control_{i}"));

            // Match with TypeInfo by name, fallback to index.
            let type_code = type_info_controls
                .iter()
                .find(|c| c.name == blob_name)
                .or_else(|| type_info_controls.get(i))
                .map(|c| c.type_code)
                .unwrap_or(0);

            ControlProperties {
                name: blob_name,
                type_code,
                properties: props,
            }
        })
        .collect();

    Ok(FormProperties {
        form_name: name.to_string(),
        object_type,
        properties: form_props,
        controls,
    })
}

// ---------------------------------------------------------------------------
// Internal: stream lookup
// ---------------------------------------------------------------------------

/// Find a specific stream for a named form/report.
///
/// Searches both Forms and Reports folders. Returns the object type and
/// the stream binary data.
fn find_stream(
    entries: &[storage::StorageEntry],
    name: &str,
    stream_kind: StreamKind,
) -> Result<(FormObjectType, Vec<u8>), FileError> {
    let root_id = storage::find_root_id(entries);

    // Try Forms first, then Reports
    for (folder_name, obj_type) in [
        ("Forms", FormObjectType::Form),
        ("Reports", FormObjectType::Report),
    ] {
        if let Some(folder) = entries
            .iter()
            .find(|e| e.parent_id == root_id && e.name == folder_name && storage::is_storage(e))
        {
            if let Some(dir_data) = storage::find_dir_data(entries, folder.id) {
                let mapping = storage::parse_dir_data(&dir_data.data)?;
                if let Some((_form_name, storage_num)) = mapping.iter().find(|(n, _)| n == name) {
                    // Find the storage entry with this number under the folder
                    if let Some(form_storage) = entries.iter().find(|e| {
                        e.parent_id == folder.id && e.name == *storage_num && storage::is_storage(e)
                    }) {
                        // Find the requested stream under this form storage
                        let stream_name = stream_kind.storage_name();
                        if let Some(stream_entry) = entries.iter().find(|e| {
                            e.parent_id == form_storage.id
                                && e.name == stream_name
                                && !storage::is_storage(e)
                        }) {
                            return Ok((obj_type, stream_entry.data.clone()));
                        }
                    }
                }
            }
        }
    }

    Err(FileError::FormNotFound {
        name: name.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Internal: TypeInfo parser
// ---------------------------------------------------------------------------

/// TypeInfo magic number.
const TYPEINFO_MAGIC: u32 = 0xACCD_EAF7;

/// Parse TypeInfo binary into a list of control entries.
///
/// Format:
/// - Header (32 bytes): magic(u32) + field1(u32) + field2(i32) + count(u32) + GUID(16)
/// - Entries: ctrl_type(u16) + padding(u16) + index(u32) + name(Shift-JIS, NUL) + align(0x00)
fn parse_type_info(data: &[u8]) -> Result<Vec<ControlInfo>, FileError> {
    if data.len() < 32 {
        return Err(FileError::InvalidFormData {
            reason: "TypeInfo too short for header",
        });
    }

    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != TYPEINFO_MAGIC {
        return Err(FileError::InvalidFormData {
            reason: "TypeInfo magic mismatch",
        });
    }

    let entry_count = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;

    // Sanity check: entry_count must not exceed what the data can hold.
    // Each entry is at least 8 bytes (ctrl_type u16 + padding u16 + index u32).
    let max_entries = (data.len().saturating_sub(32)) / 8;
    if entry_count > max_entries {
        return Err(FileError::InvalidFormData {
            reason: "TypeInfo entry count exceeds data size",
        });
    }

    let mut controls = Vec::with_capacity(entry_count);
    let mut pos = 32; // skip header

    for _ in 0..entry_count {
        if pos + 8 > data.len() {
            break;
        }

        let ctrl_type = u16::from_le_bytes([data[pos], data[pos + 1]]);
        // skip padding u16 at pos+2..pos+4
        let index =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        pos += 8;

        // Each entry has two NUL-terminated Shift-JIS strings:
        //   1. EventProcPrefix (or Name if no special characters)
        //   2. RealName (actual name when it contains parentheses, else empty)
        let epp_start = pos;
        while pos < data.len() && data[pos] != 0x00 {
            pos += 1;
        }
        let epp_bytes = &data[epp_start..pos];
        if pos < data.len() {
            pos += 1; // skip NUL terminator
        }

        let real_start = pos;
        while pos < data.len() && data[pos] != 0x00 {
            pos += 1;
        }
        let real_bytes = &data[real_start..pos];
        if pos < data.len() {
            pos += 1; // skip NUL terminator
        }

        // Use RealName if present, otherwise EventProcPrefix
        let name = if real_bytes.is_empty() {
            decode_shift_jis(epp_bytes)
        } else {
            decode_shift_jis(real_bytes)
        };

        // 0x1EFF: internal metadata entries, not real controls
        if ctrl_type == 0x1EFF {
            continue;
        }

        controls.push(ControlInfo {
            name,
            type_code: ctrl_type,
            index,
        });
    }

    Ok(controls)
}

/// Decode Shift-JIS (cp932) bytes to a String.
fn decode_shift_jis(bytes: &[u8]) -> String {
    let (result, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
    result.into_owned()
}

// ---------------------------------------------------------------------------
// Internal: Blob parser
// ---------------------------------------------------------------------------

/// Parse the Blob binary into form-level and per-control property groups.
///
/// Returns (form_properties, vec_of_control_properties).
/// Each control section starts with a Name (0x14) property.
/// Parsing stops gracefully on unknown types or malformed data.
///
/// Entry format (reverse-engineered):
///   prop_id(u16) + type(u32) + B(u32) + C(u32) + data[type-dependent]
///   Header = 14 bytes, then type-specific data.
///
/// Some types (≥ 0x08) have a 4-byte trailer after the data.
fn parse_blob(data: &[u8]) -> Result<(Vec<BlobProperty>, Vec<Vec<BlobProperty>>), FileError> {
    if data.len() < 14 {
        return Ok((Vec::new(), Vec::new()));
    }

    // Skip 8-byte blob header + 6-byte section preamble.
    let mut pos = 14;
    let mut all_props = Vec::new();

    while pos + 14 <= data.len() {
        let prop_id = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let type_code =
            u32::from_le_bytes([data[pos + 2], data[pos + 3], data[pos + 4], data[pos + 5]]);
        let _b = u32::from_le_bytes([data[pos + 6], data[pos + 7], data[pos + 8], data[pos + 9]]);
        let c = u32::from_le_bytes([
            data[pos + 10],
            data[pos + 11],
            data[pos + 12],
            data[pos + 13],
        ]);
        let data_start = pos + 14;

        match type_code {
            0x01 => {
                // Bool: 4 bytes data, no trailer. Total = 18.
                if data_start + 4 > data.len() {
                    break;
                }
                let val = u32::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Bool(val != 0),
                });
                pos += 18;
            }
            0x02 => {
                // Short: 5 bytes data, no trailer. Total = 19.
                if data_start + 5 > data.len() {
                    break;
                }
                let val = i16::from_le_bytes([data[data_start], data[data_start + 1]]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Short(val),
                });
                pos += 19;
            }
            0x03 => {
                // Long: 6 bytes data, no trailer. Total = 20.
                if data_start + 6 > data.len() {
                    break;
                }
                let val = i32::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Long(val),
                });
                pos += 20;
            }
            0x04 => {
                // Color: 8 bytes data (4 color + 4 extra), no trailer. Total = 22.
                if data_start + 8 > data.len() {
                    break;
                }
                let val = u32::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Color(val),
                });
                pos += 22;
            }
            0x08 => {
                // Double: 8 bytes data + 4 byte trailer. Total = 26.
                if data_start + 12 > data.len() {
                    break;
                }
                let val = f64::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                    data[data_start + 4],
                    data[data_start + 5],
                    data[data_start + 6],
                    data[data_start + 7],
                ]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Double(val),
                });
                pos += 26;
            }
            0x09 => {
                // GUID: 16 bytes data + 4 byte trailer. Total = 34.
                if data_start + 20 > data.len() {
                    break;
                }
                let guid = format_guid(&data[data_start..data_start + 16]);
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Guid(guid),
                });
                pos += 34;
            }
            0x0A | 0x0C => {
                // Variable-length text: C bytes data + 4 byte trailer.
                let byte_len = c as usize;
                if data_start + byte_len + 4 > data.len() {
                    break;
                }
                let text_bytes = &data[data_start..data_start + byte_len];
                let text = encoding::decode_utf16le(text_bytes)
                    .unwrap_or_else(|_| String::from_utf8_lossy(text_bytes).into_owned());
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Text(text),
                });
                pos += 14 + byte_len + 4;
            }
            0x0B => {
                // Variable-length binary: C bytes data + 4 byte trailer.
                let byte_len = c as usize;
                if data_start + byte_len + 4 > data.len() {
                    break;
                }
                let bin_data = data[data_start..data_start + byte_len].to_vec();
                all_props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Binary(bin_data),
                });
                pos += 14 + byte_len + 4;
            }
            _ => {
                // Unknown type — stop parsing gracefully.
                break;
            }
        }
    }

    // The sequential parse above covers form-level properties but stops at binary
    // layout data in the middle. Control sections come later, each starting with a
    // Name (prop_id=0x14, type=0x0A) property. Scan the remaining blob for these.
    let control_groups = scan_control_sections(data, pos);

    Ok((all_props, control_groups))
}

/// Scan the blob for control property sections starting with Name (0x14) entries.
///
/// Each control section begins with prop_id=0x14 (Name) + type=0x0A (Text).
/// The byte pattern is `[14, 00, 0A, 00, 00, 00]`.
fn scan_control_sections(data: &[u8], start: usize) -> Vec<Vec<BlobProperty>> {
    // Pattern: prop_id(0x14, 0x00) + type(0x0A, 0x00, 0x00, 0x00)
    let pattern: [u8; 6] = [0x14, 0x00, 0x0A, 0x00, 0x00, 0x00];

    // Find all positions where control sections start.
    let mut section_starts = Vec::new();
    let mut search_pos = start;
    while search_pos + 6 <= data.len() {
        if data[search_pos..search_pos + 6] == pattern {
            section_starts.push(search_pos);
            search_pos += 6; // skip past this match
        } else {
            search_pos += 1;
        }
    }

    // Parse properties from each section start.
    let mut control_groups = Vec::new();
    for (i, &sec_start) in section_starts.iter().enumerate() {
        let sec_end = section_starts.get(i + 1).copied().unwrap_or(data.len());
        let props = parse_section_props(data, sec_start, sec_end);
        if !props.is_empty() {
            control_groups.push(props);
        }
    }

    control_groups
}

/// Parse property entries from a section of the blob.
///
/// Same entry format as `parse_blob`, but limited to the given range.
fn parse_section_props(data: &[u8], start: usize, end: usize) -> Vec<BlobProperty> {
    let mut props = Vec::new();
    let mut pos = start;

    while pos + 14 <= end {
        let prop_id = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let type_code =
            u32::from_le_bytes([data[pos + 2], data[pos + 3], data[pos + 4], data[pos + 5]]);
        let _b = u32::from_le_bytes([data[pos + 6], data[pos + 7], data[pos + 8], data[pos + 9]]);
        let c = u32::from_le_bytes([
            data[pos + 10],
            data[pos + 11],
            data[pos + 12],
            data[pos + 13],
        ]);
        let data_start = pos + 14;

        match type_code {
            0x01 => {
                if data_start + 4 > end {
                    break;
                }
                let val = u32::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                ]);
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Bool(val != 0),
                });
                pos += 18;
            }
            0x02 => {
                if data_start + 5 > end {
                    break;
                }
                let val = i16::from_le_bytes([data[data_start], data[data_start + 1]]);
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Short(val),
                });
                pos += 19;
            }
            0x03 => {
                if data_start + 6 > end {
                    break;
                }
                let val = i32::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                ]);
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Long(val),
                });
                pos += 20;
            }
            0x04 => {
                if data_start + 8 > end {
                    break;
                }
                let val = u32::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                ]);
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Color(val),
                });
                pos += 22;
            }
            0x06 => {
                // Type 6: observed in control sections. 6 bytes data, no trailer. Total = 20.
                if data_start + 6 > end {
                    break;
                }
                let val = i32::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                ]);
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Long(val),
                });
                pos += 20;
            }
            0x08 => {
                if data_start + 12 > end {
                    break;
                }
                let val = f64::from_le_bytes([
                    data[data_start],
                    data[data_start + 1],
                    data[data_start + 2],
                    data[data_start + 3],
                    data[data_start + 4],
                    data[data_start + 5],
                    data[data_start + 6],
                    data[data_start + 7],
                ]);
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Double(val),
                });
                pos += 26;
            }
            0x09 => {
                if data_start + 20 > end {
                    break;
                }
                let guid = format_guid(&data[data_start..data_start + 16]);
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Guid(guid),
                });
                pos += 34;
            }
            0x0A | 0x0C => {
                let byte_len = c as usize;
                if data_start + byte_len + 4 > end {
                    break;
                }
                let text_bytes = &data[data_start..data_start + byte_len];
                let text = encoding::decode_utf16le(text_bytes)
                    .unwrap_or_else(|_| String::from_utf8_lossy(text_bytes).into_owned());
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Text(text),
                });
                pos += 14 + byte_len + 4;
            }
            0x0B => {
                let byte_len = c as usize;
                if data_start + byte_len + 4 > end {
                    break;
                }
                let bin_data = data[data_start..data_start + byte_len].to_vec();
                props.push(BlobProperty {
                    prop_id,
                    value: BlobValue::Binary(bin_data),
                });
                pos += 14 + byte_len + 4;
            }
            _ => {
                break;
            }
        }
    }

    props
}

/// Format 16 bytes as a GUID string `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`.
fn format_guid(bytes: &[u8]) -> String {
    if bytes.len() < 16 {
        return format!("({} bytes)", bytes.len());
    }
    let d1 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let d2 = u16::from_le_bytes([bytes[4], bytes[5]]);
    let d3 = u16::from_le_bytes([bytes[6], bytes[7]]);
    format!(
        "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
        d1,
        d2,
        d3,
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    // -- DirData parser unit tests -------------------------------------------

    #[test]
    fn parse_dir_data_empty() {
        // Just a 4-byte header
        let data = [0x00, 0x00, 0x00, 0x00];
        let result = storage::parse_dir_data(&data).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_dir_data_too_short() {
        let result = storage::parse_dir_data(&[0x00]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_dir_data_single_entry() {
        // Header + one entry: name "AB" (UTF-16LE) + storage_index=5 + null
        let mut data = vec![0x00, 0x00, 0x00, 0x00]; // header
        data.push(0x04); // marker
        data.push(0x08); // len = 4 (name) + 2 (index) + 2 (null)
                         // name "AB" in UTF-16LE
        data.extend_from_slice(&[0x41, 0x00, 0x42, 0x00]);
        // storage_index = 5
        data.extend_from_slice(&[0x05, 0x00]);
        // null terminator
        data.extend_from_slice(&[0x00, 0x00]);

        let result = storage::parse_dir_data(&data).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "AB");
        assert_eq!(result[0].1, "5");
    }

    // -- TypeInfo parser unit tests ------------------------------------------

    #[test]
    fn parse_type_info_too_short() {
        let data = [0u8; 16];
        let result = parse_type_info(&data);
        assert!(result.is_err());
    }

    #[test]
    fn parse_type_info_bad_magic() {
        let mut data = [0u8; 32];
        data[0] = 0xFF; // wrong magic
        let result = parse_type_info(&data);
        assert!(result.is_err());
    }

    #[test]
    fn parse_type_info_empty_entries() {
        let mut data = vec![0u8; 32];
        // Set magic
        data[0..4].copy_from_slice(&TYPEINFO_MAGIC.to_le_bytes());
        // entry_count = 0 (at offset 12)
        data[12..16].copy_from_slice(&0u32.to_le_bytes());

        let result = parse_type_info(&data).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_type_info_single_entry() {
        let mut data = vec![0u8; 32];
        // Set magic
        data[0..4].copy_from_slice(&TYPEINFO_MAGIC.to_le_bytes());
        // entry_count = 1 (at offset 12)
        data[12..16].copy_from_slice(&1u32.to_le_bytes());

        // Entry: ctrl_type=0x0B68, padding=0, index=0, name="Btn1\0\0"
        data.extend_from_slice(&[0x68, 0x0B]); // ctrl_type
        data.extend_from_slice(&[0x00, 0x00]); // padding
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // index
        data.extend_from_slice(b"Btn1"); // ASCII name (valid Shift-JIS)
        data.push(0x00); // NUL terminator
        data.push(0x00); // alignment

        let result = parse_type_info(&data).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Btn1");
        assert_eq!(result[0].type_code, 0x0B68);
        assert_eq!(result[0].index, 0);
    }

    // -- Integration tests with real files -----------------------------------

    #[test]
    fn list_forms_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let forms = list_forms(&mut reader).unwrap();
        // vbaV2007.accdb has Form1
        assert!(
            forms.iter().any(|e| e.name == "Form1"),
            "expected Form1 in form list, got: {:?}",
            forms.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn list_forms_access_2000_format() {
        // No MSysAccessStorage: the forms come from the compound file in
        // MSysAccessObjects.
        let path = skip_if_missing!("V2000/macroTestV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let forms = list_forms(&mut reader).unwrap();
        assert!(
            forms
                .iter()
                .any(|e| e.name == "tblItems" && e.object_type == FormObjectType::Form),
            "got: {:?}",
            forms.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn read_form_blob_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let stream = read_form_stream(&mut reader, "Form1", StreamKind::Blob).unwrap();
        assert!(!stream.data.is_empty(), "Blob should not be empty");
        assert_eq!(stream.object_type, FormObjectType::Form);
    }

    #[test]
    fn read_form_type_info_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let type_info = read_form_type_info(&mut reader, "Form1").unwrap();
        assert!(
            !type_info.controls.is_empty(),
            "TypeInfo should have at least one control"
        );
        assert_eq!(type_info.object_type, FormObjectType::Form);
    }

    #[test]
    fn form_not_found() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let result = read_form_stream(&mut reader, "NoSuchForm", StreamKind::Blob);
        assert!(matches!(result, Err(FileError::FormNotFound { .. })));
    }

    #[test]
    fn no_forms_in_plain_db() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let forms = list_forms(&mut reader).unwrap();
        assert!(forms.is_empty(), "expected no forms in plain test database");
    }

    // -- Blob parser unit tests (synthetic binary) ----------------------------

    /// Build a minimal blob with 8-byte header + 6-byte preamble + entries.
    fn make_blob(entries: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 14]; // 8 header + 6 preamble
        data.extend_from_slice(entries);
        data
    }

    /// Build a single Blob entry for a given type.
    fn make_entry(prop_id: u16, type_code: u32, b: u32, c: u32, payload: &[u8]) -> Vec<u8> {
        let mut entry = Vec::new();
        entry.extend_from_slice(&prop_id.to_le_bytes());
        entry.extend_from_slice(&type_code.to_le_bytes());
        entry.extend_from_slice(&b.to_le_bytes());
        entry.extend_from_slice(&c.to_le_bytes());
        entry.extend_from_slice(payload);
        entry
    }

    #[test]
    fn parse_blob_empty() {
        let data = vec![0u8; 14];
        let (form_props, controls) = parse_blob(&data).unwrap();
        assert!(form_props.is_empty());
        assert!(controls.is_empty());
    }

    #[test]
    fn parse_blob_too_short() {
        let (form_props, controls) = parse_blob(&[0u8; 5]).unwrap();
        assert!(form_props.is_empty());
        assert!(controls.is_empty());
    }

    #[test]
    fn parse_blob_bool_entry() {
        // Bool (type 0x01): 4 bytes data, total entry = 18
        let payload = [0x01, 0x00, 0x00, 0x00]; // true
        let entry = make_entry(0x0013, 0x01, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x0013);
        assert!(matches!(props[0].value, BlobValue::Bool(true)));
    }

    #[test]
    fn parse_blob_short_entry() {
        // Short (type 0x02): 5 bytes data, total entry = 19
        let payload = [0x2A, 0x00, 0x00, 0x00, 0x00]; // 42
        let entry = make_entry(0x0098, 0x02, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert!(matches!(props[0].value, BlobValue::Short(42)));
    }

    #[test]
    fn parse_blob_long_entry() {
        // Long (type 0x03): 6 bytes data, total entry = 20
        let payload = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00]; // 256
        let entry = make_entry(0x002A, 0x03, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert!(matches!(props[0].value, BlobValue::Long(256)));
    }

    #[test]
    fn parse_blob_text_entry() {
        // Text (type 0x0A): C bytes data + 4 byte trailer
        let text = "AB"; // UTF-16LE: [0x41, 0x00, 0x42, 0x00]
        let text_bytes = [0x41, 0x00, 0x42, 0x00];
        let c = text_bytes.len() as u32;
        let mut payload = Vec::new();
        payload.extend_from_slice(&text_bytes);
        payload.extend_from_slice(&[0x00; 4]); // trailer
        let entry = make_entry(0x009C, 0x0A, 0, c, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x009C); // RecordSource
        match &props[0].value {
            BlobValue::Text(s) => assert_eq!(s, text),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn parse_blob_binary_entry() {
        // Binary (type 0x0B): C bytes data + 4 byte trailer
        let bin = [0xDE, 0xAD, 0xBE, 0xEF];
        let c = bin.len() as u32;
        let mut payload = Vec::new();
        payload.extend_from_slice(&bin);
        payload.extend_from_slice(&[0x00; 4]); // trailer
        let entry = make_entry(0x00BD, 0x0B, 0, c, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        match &props[0].value {
            BlobValue::Binary(v) => assert_eq!(v.as_slice(), &bin),
            other => panic!("expected Binary, got {:?}", other),
        }
    }

    #[test]
    fn parse_blob_multiple_entries() {
        let mut entries = Vec::new();
        // Bool entry
        entries.extend_from_slice(&make_entry(0x0013, 0x01, 0, 0, &[0x00; 4]));
        // Short entry
        entries.extend_from_slice(&make_entry(
            0x0098,
            0x02,
            0,
            0,
            &[0x07, 0x00, 0x00, 0x00, 0x00],
        ));
        // Long entry
        entries.extend_from_slice(&make_entry(
            0x002A,
            0x03,
            0,
            0,
            &[0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00],
        ));

        let blob = make_blob(&entries);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 3);
        assert!(matches!(props[0].value, BlobValue::Bool(false)));
        assert!(matches!(props[1].value, BlobValue::Short(7)));
        assert!(matches!(props[2].value, BlobValue::Long(-1)));
    }

    #[test]
    fn parse_blob_stops_on_unknown_type() {
        let mut entries = Vec::new();
        // Valid Bool entry
        entries.extend_from_slice(&make_entry(0x0013, 0x01, 0, 0, &[0x01; 4]));
        // Unknown type 0xFF
        entries.extend_from_slice(&make_entry(0x9999, 0xFF, 0, 0, &[0x00; 10]));

        let blob = make_blob(&entries);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1, "should stop at unknown type");
    }

    #[test]
    fn parse_blob_guid_entry() {
        // GUID (type 0x09): 16 bytes data + 4 byte trailer, total = 34
        let guid_bytes = [
            0x50, 0xA6, 0x64, 0x8D, 0xE7, 0x62, 0x03, 0x49, 0x97, 0x33, 0x0D, 0x8C, 0xE8, 0x49,
            0x78, 0xBF,
        ];
        let mut payload = Vec::new();
        payload.extend_from_slice(&guid_bytes);
        payload.extend_from_slice(&[0x00; 4]); // trailer
        let entry = make_entry(0x0178, 0x09, 0, 0, &payload);
        let blob = make_blob(&entry);
        let (props, _) = parse_blob(&blob).unwrap();
        assert_eq!(props.len(), 1);
        match &props[0].value {
            BlobValue::Guid(s) => assert!(s.starts_with('{') && s.ends_with('}')),
            other => panic!("expected Guid, got {:?}", other),
        }
    }

    #[test]
    fn prop_id_name_known() {
        assert_eq!(prop_id_name(0x009C), Some("RecordSource"));
        assert_eq!(prop_id_name(0x001B), Some("ControlSource"));
        assert_eq!(prop_id_name(0x00F5), Some("Filter"));
        assert_eq!(prop_id_name(0x0014), Some("Name"));
    }

    #[test]
    fn prop_id_name_unknown() {
        assert_eq!(prop_id_name(0xFFFF), None);
    }

    #[test]
    fn blob_property_display_name() {
        let known = BlobProperty {
            prop_id: 0x009C,
            value: BlobValue::Bool(true),
        };
        assert_eq!(known.display_name(), "RecordSource");

        let unknown = BlobProperty {
            prop_id: 0x1234,
            value: BlobValue::Bool(true),
        };
        assert_eq!(unknown.display_name(), "0x1234");
    }

    #[test]
    fn blob_value_display() {
        assert_eq!(format!("{}", BlobValue::Bool(true)), "yes");
        assert_eq!(format!("{}", BlobValue::Bool(false)), "no");
        assert_eq!(format!("{}", BlobValue::Short(42)), "42");
        assert_eq!(format!("{}", BlobValue::Long(-1)), "-1");
        assert_eq!(format!("{}", BlobValue::Color(0x00FF0000)), "#FF0000");
        assert_eq!(format!("{}", BlobValue::Text("hello".into())), "hello");
        assert_eq!(format!("{}", BlobValue::Binary(vec![0; 10])), "(10 bytes)");
    }

    // -- Integration test: read_form_properties with real file ----------------

    #[test]
    fn read_form_properties_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "Form1").unwrap();
        assert_eq!(props.object_type, FormObjectType::Form);
        // Should have at least some form-level properties.
        assert!(
            !props.properties.is_empty(),
            "expected form-level properties, got empty"
        );
    }

    // -- Integration tests with formPropTest.accdb -------------------------

    /// Helper: find ControlSource (0x001B) text value for a named control.
    fn find_control_source(props: &FormProperties, ctrl_name: &str) -> Option<String> {
        props
            .controls
            .iter()
            .find(|c| c.name == ctrl_name)
            .and_then(|c| c.properties.iter().find(|p| p.prop_id == 0x001B))
            .and_then(|p| match &p.value {
                BlobValue::Text(s) => Some(s.clone()),
                _ => None,
            })
    }

    #[test]
    fn list_forms_form_prop_test() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let forms = list_forms(&mut reader).unwrap();
        let mut names: Vec<&str> = forms.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            ["F_Buttons", "F_Table0", "F_Table1", "jp_フォーム_2"]
        );
        assert!(forms.iter().all(|e| e.object_type == FormObjectType::Form));
    }

    #[test]
    fn read_form_properties_empty_form() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "F_Table0").unwrap();
        assert_eq!(props.form_name, "F_Table0");
        assert_eq!(props.object_type, FormObjectType::Form);
        assert!(
            !props.properties.iter().any(|p| p.prop_id == 0x009C),
            "empty form should not have RecordSource"
        );
        assert!(
            !props.properties.iter().any(|p| p.prop_id == 0x00F5),
            "empty form should not have Filter"
        );
    }

    #[test]
    fn read_form_properties_record_source_and_filter() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "F_Table1").unwrap();

        let rs = props
            .properties
            .iter()
            .find(|p| p.prop_id == 0x009C)
            .expect("RecordSource should exist");
        match &rs.value {
            BlobValue::Text(s) => assert_eq!(s.trim(), "SELECT * FROM Table1;"),
            other => panic!("expected Text, got {:?}", other),
        }

        let filter = props
            .properties
            .iter()
            .find(|p| p.prop_id == 0x00F5)
            .expect("Filter should exist");
        match &filter.value {
            BlobValue::Text(s) => assert_eq!(s, "[ID] > 0"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn read_form_properties_control_source() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "F_Table1").unwrap();

        assert_eq!(find_control_source(&props, "ID").as_deref(), Some("ID"));
        assert_eq!(
            find_control_source(&props, "ProductName").as_deref(),
            Some("ProductName")
        );
        assert_eq!(
            find_control_source(&props, "Price").as_deref(),
            Some("Price")
        );
        assert_eq!(find_control_source(&props, "Qty").as_deref(), Some("Qty"));
    }

    #[test]
    fn read_form_properties_calculated_field() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "F_Table1").unwrap();
        assert_eq!(
            find_control_source(&props, "Text_01_SubTotal").as_deref(),
            Some("=[Price]*[Qty]")
        );
    }

    #[test]
    fn read_form_properties_format() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "F_Table1").unwrap();

        let price = props
            .controls
            .iter()
            .find(|c| c.name == "Price")
            .expect("Price control should exist");
        let fmt = price
            .properties
            .iter()
            .find(|p| p.prop_id == 0x0026)
            .expect("Format property should exist on Price");
        match &fmt.value {
            BlobValue::Text(s) => assert_eq!(s, "¥#,##0;-¥#,##0"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn read_form_properties_japanese_form() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "jp_フォーム_2").unwrap();
        assert_eq!(props.form_name, "jp_フォーム_2");

        let rs = props
            .properties
            .iter()
            .find(|p| p.prop_id == 0x009C)
            .expect("RecordSource should exist");
        match &rs.value {
            BlobValue::Text(s) => assert_eq!(s, "jp_クエリ_02"),
            other => panic!("expected Text, got {:?}", other),
        }

        assert_eq!(
            find_control_source(&props, "商品名").as_deref(),
            Some("商品名")
        );
        assert_eq!(find_control_source(&props, "単価").as_deref(), Some("単価"));
        assert_eq!(find_control_source(&props, "個数").as_deref(), Some("個数"));
    }

    #[test]
    fn read_form_properties_japanese_calculated_field() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "jp_フォーム_2").unwrap();
        assert_eq!(
            find_control_source(&props, "小計").as_deref(),
            Some("=[単価]*[個数]")
        );
        // NOTE: 小計の Format は Access 側の不具合で壊れた値が格納されている
        // ("=jp_[単価]"*["個数]") ためテスト対象外とする
    }

    #[test]
    fn read_form_properties_onclick_event() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "F_Table1").unwrap();

        let btn = props
            .controls
            .iter()
            .find(|c| c.name == "btn_msg")
            .expect("btn_msg should exist");
        let onclick = btn
            .properties
            .iter()
            .find(|p| p.prop_id == 0x007E)
            .expect("OnClick (0x007E) should exist on btn_msg");
        match &onclick.value {
            BlobValue::Text(s) => assert_eq!(s, "[Event Procedure]"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn read_form_properties_japanese_onclick_event() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "jp_フォーム_2").unwrap();

        let cmd = props
            .controls
            .iter()
            .find(|c| c.name == "コマンド22")
            .expect("コマンド22 should exist");
        let onclick = cmd
            .properties
            .iter()
            .find(|p| p.prop_id == 0x007E)
            .expect("OnClick (0x007E) should exist on コマンド22");
        match &onclick.value {
            BlobValue::Text(s) => assert_eq!(s, "[Event Procedure]"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn read_form_properties_all_event_types() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let props = read_form_properties(&mut reader, "F_Buttons").unwrap();

        let event_cases: &[(&str, u16)] = &[
            ("btn_Click", 0x007E),
            ("btn_GotFocus", 0x0073),
            ("btn_LostFocus", 0x0074),
            ("btn_DblClick", 0x00E0),
            ("btn_MouseDown", 0x006B),
            ("btn_MouseUp", 0x006C),
            ("btn_MouseMove", 0x006D),
            ("btn_KeyDown", 0x0068),
            ("btn_KeyUp", 0x0069),
            ("btn_KeyPress", 0x006A),
            ("btn_Enter", 0x00DE),
            ("btn_Exit", 0x00DF),
        ];
        for (btn_name, expected_prop_id) in event_cases {
            let ctrl = props
                .controls
                .iter()
                .find(|c| c.name == *btn_name)
                .unwrap_or_else(|| panic!("control '{}' not found", btn_name));
            let event = ctrl
                .properties
                .iter()
                .find(|p| p.prop_id == *expected_prop_id)
                .unwrap_or_else(|| {
                    panic!(
                        "prop_id 0x{:04X} not found on '{}'",
                        expected_prop_id, btn_name
                    )
                });
            match &event.value {
                BlobValue::Text(s) => assert_eq!(
                    s, "[Event Procedure]",
                    "event value mismatch on '{}'",
                    btn_name
                ),
                other => panic!("expected Text on '{}', got {:?}", btn_name, other),
            }
        }
    }

    // TODO: formPropTest.accdb にはレポートが含まれていないため、
    // レポートプロパティのテストは別のテストデータで実施する

    #[test]
    fn control_type_name_known_form_controls() {
        let cases = [
            (0x0B68, "CommandButton"),
            (0x126D, "TextBox"),
            (0x136F, "ComboBox"),
            (0x066A, "CheckBox"),
            (0x0A7A, "ToggleButton"),
            (0x0E65, "Rectangle"),
            (0x0F67, "Image"),
            (0x1470, "SubForm"),
            (0x1666, "Line"),
            (0x247F, "EmptyCell"),
            (0x1898, "Detail"),
            (0x1998, "Detail"),
            (0x1899, "FormHeader"),
            (0x189A, "FormFooter"),
        ];
        for (code, expected) in cases {
            assert_eq!(
                control_type_name(code),
                Some(expected),
                "form control 0x{:04X} should be {:?}",
                code,
                expected
            );
        }
    }

    #[test]
    fn control_type_name_known_report_controls() {
        let cases = [
            (0x1B6D, "TextBox"),
            (0x1B64, "Label"),
            (0x1B70, "SubReport"),
            (0x1B68, "CommandButton"),
            (0x1B6A, "CheckBox"),
            (0x1B6F, "ComboBox"),
            (0x1B65, "Rectangle"),
            (0x1B66, "Line"),
            (0x1B67, "Image"),
            (0x1999, "ReportHeader"),
            (0x199A, "ReportFooter"),
            (0x199D, "GroupHeader"),
            (0x199E, "GroupFooter"),
            (0x1F9B, "PageHeader"),
            (0x1F9C, "PageFooter"),
        ];
        for (code, expected) in cases {
            assert_eq!(
                control_type_name(code),
                Some(expected),
                "report control 0x{:04X} should be {:?}",
                code,
                expected
            );
        }
    }

    #[test]
    fn control_type_name_unknown_returns_none() {
        assert_eq!(control_type_name(0x0000), None);
        assert_eq!(control_type_name(0xFFFF), None);
        assert_eq!(control_type_name(0x1EFF), None);
    }

    #[test]
    fn control_type_name_label_variants() {
        // Both form label codes should return "Label"
        assert_eq!(control_type_name(0x0C64), Some("Label"));
        assert_eq!(control_type_name(0x0D64), Some("Label"));
        // Report label too
        assert_eq!(control_type_name(0x1B64), Some("Label"));
    }

    // -- BlobValue Display: Double and Guid -----------------------------------

    #[test]
    fn blob_value_display_double_and_guid() {
        assert_eq!(format!("{}", BlobValue::Double(1.23)), "1.23");
        assert_eq!(format!("{}", BlobValue::Guid("{ABC}".into())), "{ABC}");
    }

    // -- parse_section_props: type 0x06 / 0x08 / 0x04 ------------------------

    #[test]
    fn parse_section_props_type_06_long() {
        // Type 0x06: 6 bytes data, no trailer. Total entry = 20 bytes.
        let mut data = Vec::new();
        // 14-byte header: prop_id=0x0099, type=0x06, B=0, C=0
        data.extend_from_slice(&0x0099u16.to_le_bytes());
        data.extend_from_slice(&0x00000006u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // B
        data.extend_from_slice(&0u32.to_le_bytes()); // C
                                                     // 6 bytes data: value 42 as i32 + 2 padding bytes
        data.extend_from_slice(&42i32.to_le_bytes());
        data.extend_from_slice(&[0x00, 0x00]);

        let props = parse_section_props(&data, 0, data.len());
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x0099);
        assert!(matches!(props[0].value, BlobValue::Long(42)));
    }

    #[test]
    fn parse_section_props_type_08_double() {
        // Type 0x08: 8 bytes data + 4 byte trailer. Total entry = 26 bytes.
        let mut data = Vec::new();
        // 14-byte header: prop_id=0x00AA, type=0x08, B=0, C=0
        data.extend_from_slice(&0x00AAu16.to_le_bytes());
        data.extend_from_slice(&0x00000008u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // B
        data.extend_from_slice(&0u32.to_le_bytes()); // C
                                                     // 8 bytes data: f64 value 1.23
        data.extend_from_slice(&1.23f64.to_le_bytes());
        // 4 byte trailer
        data.extend_from_slice(&[0x00; 4]);

        let props = parse_section_props(&data, 0, data.len());
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x00AA);
        match &props[0].value {
            BlobValue::Double(v) => assert!((v - 1.23).abs() < f64::EPSILON),
            other => panic!("expected Double, got {:?}", other),
        }
    }

    #[test]
    fn parse_section_props_type_04_color() {
        // Type 0x04: 8 bytes data (4 color + 4 extra), no trailer. Total entry = 22 bytes.
        let mut data = Vec::new();
        // 14-byte header: prop_id=0x00BB, type=0x04, B=0, C=0
        data.extend_from_slice(&0x00BBu16.to_le_bytes());
        data.extend_from_slice(&0x00000004u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // B
        data.extend_from_slice(&0u32.to_le_bytes()); // C
                                                     // 8 bytes data: color 0x00FF0000 + 4 extra bytes
        data.extend_from_slice(&0x00FF0000u32.to_le_bytes());
        data.extend_from_slice(&[0x00; 4]);

        let props = parse_section_props(&data, 0, data.len());
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x00BB);
        assert!(matches!(props[0].value, BlobValue::Color(0x00FF0000)));
    }
}
