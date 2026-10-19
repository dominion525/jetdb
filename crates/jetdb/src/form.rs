//! Form and report design stream extraction from MSysAccessStorage.
//!
//! Also provides Blob binary parsing to extract form/report properties
//! (RecordSource, ControlSource, Filter, etc.) and per-control properties.

use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::storage::{self, AccessStorage};

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
    /// A Yes/No property set to the value that is not its default, stored
    /// without a value; `Application.SaveAsText` writes it as `NotDefault`.
    NotDefault,
    Short(i16),
    /// A 32-bit integer: of type 0x03, or of type 0x04, which holds colors
    /// (`0x00BBGGRR`, or negative for a system color) and other numbers, such
    /// as `HelpContextId`. SaveAsText writes both as this signed number.
    Long(i32),
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
    list_forms_in(&storage::read_access_storage(reader)?)
}

/// [`list_forms`] in the storage already read.
pub fn list_forms_in(storage: &AccessStorage) -> Result<Vec<FormEntry>, FileError> {
    let entries = &storage.entries;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let root_id = storage::find_root_id(entries);

    let mut result = Vec::new();

    // Collect forms
    if let Some(forms_folder) = entries
        .iter()
        .find(|e| e.parent_id == root_id && e.name == "Forms" && storage::is_storage(e))
    {
        let dir = storage::find_dir_data(entries, forms_folder.id);
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
        let dir = storage::find_dir_data(entries, reports_folder.id);
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

/// Read a raw binary stream from a named form or report: the form when a
/// form and a report share the name.
pub fn read_form_stream(
    reader: &mut PageReader,
    name: &str,
    stream_kind: StreamKind,
) -> Result<FormStream, FileError> {
    read_stream(reader, name, None, stream_kind)
}

/// [`read_form_stream`] of the form or the report named `name`.
pub fn read_form_stream_of_type(
    reader: &mut PageReader,
    name: &str,
    object_type: FormObjectType,
    stream_kind: StreamKind,
) -> Result<FormStream, FileError> {
    read_stream(reader, name, Some(object_type), stream_kind)
}

/// [`read_form_stream`] in the storage already read: of `object_type` when
/// it is given, and otherwise the form when a form and a report share the
/// name.
pub fn read_form_stream_in(
    storage: &AccessStorage,
    name: &str,
    object_type: Option<FormObjectType>,
    stream_kind: StreamKind,
) -> Result<FormStream, FileError> {
    stream_in(storage, name, object_type, stream_kind)
}

/// A stream of the form or report named `name`, of `object_type` when it
/// is given and otherwise the form when a form and a report share the name.
pub(crate) fn read_stream(
    reader: &mut PageReader,
    name: &str,
    object_type: Option<FormObjectType>,
    stream_kind: StreamKind,
) -> Result<FormStream, FileError> {
    stream_in(
        &storage::read_access_storage(reader)?,
        name,
        object_type,
        stream_kind,
    )
}

/// [`read_stream`] in the storage already read.
pub(crate) fn stream_in(
    storage: &AccessStorage,
    name: &str,
    object_type: Option<FormObjectType>,
    stream_kind: StreamKind,
) -> Result<FormStream, FileError> {
    let (object_type, stream_data) = find_stream(&storage.entries, name, object_type, stream_kind)?;

    Ok(FormStream {
        name: name.to_string(),
        object_type,
        stream_kind,
        data: stream_data,
    })
}

/// Read and parse TypeInfo for a named form or report: the form when a form
/// and a report share the name.
pub fn read_form_type_info(reader: &mut PageReader, name: &str) -> Result<FormTypeInfo, FileError> {
    type_info(reader, name, None)
}

/// [`read_form_type_info`] of the form or the report named `name`.
pub fn read_form_type_info_of_type(
    reader: &mut PageReader,
    name: &str,
    object_type: FormObjectType,
) -> Result<FormTypeInfo, FileError> {
    type_info(reader, name, Some(object_type))
}

fn type_info(
    reader: &mut PageReader,
    name: &str,
    object_type: Option<FormObjectType>,
) -> Result<FormTypeInfo, FileError> {
    read_form_type_info_in(&storage::read_access_storage(reader)?, name, object_type)
}

/// [`read_form_type_info`] in the storage already read: of `object_type`
/// when it is given, and otherwise the form when a form and a report share
/// the name.
pub fn read_form_type_info_in(
    storage: &AccessStorage,
    name: &str,
    object_type: Option<FormObjectType>,
) -> Result<FormTypeInfo, FileError> {
    let (object_type, stream_data) =
        find_stream(&storage.entries, name, object_type, StreamKind::TypeInfo)?;
    let controls = parse_type_info(&stream_data, storage.code_page)?;

    Ok(FormTypeInfo {
        form_name: name.to_string(),
        object_type,
        controls,
    })
}

/// Return the known name of the property `prop_id` whose value is text, or
/// `None`.
///
/// The same ID can hold a value of another type where the property does not
/// apply, as a different property: 0x0068 is `OnKeyDown` on a form, but holds
/// a color on a label, which has no key events. [`BlobProperty::name`] names
/// a text value by this table, and an integer or Yes/No value by
/// [`number_prop_id_name`].
pub fn prop_id_name(prop_id: u16) -> Option<&'static str> {
    match prop_id {
        0x0011 => Some("Caption"),
        0x0012 => Some("ColumnWidths"),
        0x0014 => Some("Name"),
        0x0016 => Some("EventProcPrefix"),
        0x0017 => Some("DefaultValue"),
        0x001B => Some("ControlSource"),
        0x0022 => Some("FontName"),
        0x0026 => Some("Format"),
        0x004C => Some("OLEClass"),
        0x004D => Some("OnOpen"),
        0x004E => Some("OnClose"),
        0x0056 => Some("AfterUpdate"),
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
        0x0079 => Some("OnNoData"),
        0x007E => Some("OnClick"),
        0x007F => Some("OnLoad"),
        0x0084 => Some("SourceObject"),
        0x0087 => Some("StatusBarText"),
        0x009C => Some("RecordSource"),
        0x00A0 => Some("FontName"),
        0x00DA => Some("HelpFile"),
        0x00DE => Some("OnEnter"),
        0x00DF => Some("OnExit"),
        0x00E0 => Some("OnDblClick"),
        0x00F5 => Some("Filter"),
        0x010A => Some("LabelType"),
        0x0112 => Some("Class"),
        0x0130 => Some("ShortcutMenuBar"),
        0x013D => Some("ControlTipText"),
        0x0161 => Some("Toolbar"),
        _ => None,
    }
}

/// Return the known name of the property `prop_id` whose value is an integer
/// or Yes/No, or `None`.
///
/// Each name is the SaveAsText key whose values agree with every value of the
/// ID in the forms and reports of Sports.accdb (ruddj/SportsAdmin), where the
/// values vary, so that one key fits them all, or that changes the value of
/// the ID when it is set in testdata/V2007/propertyIdTestV2007.accdb, and
/// agrees with its values there and in Sports.accdb. Only IDs up to 0x01C4 are
/// named: Access for Microsoft 365 writes the IDs above, such as those of
/// the anchors and the layout, one higher than the Access that made
/// Sports.accdb.
pub fn number_prop_id_name(prop_id: u16) -> Option<&'static str> {
    match prop_id {
        0x0009 => Some("BorderStyle"),
        0x000C => Some("Bottom"),
        0x0029 => Some("GridX"),
        0x002A => Some("GridY"),
        0x0036 => Some("Left"),
        0x006F => Some("TimerInterval"),
        0x0081 => Some("Right"),
        0x008D => Some("Top"),
        0x0092 => Some("ViewsAllowed"),
        0x0093 => Some("DefaultView"),
        0x0096 => Some("Width"),
        0x0098 => Some("ScrollBars"),
        0x009E => Some("ItemSuffix"),
        0x00A1 => Some("DatasheetFontHeight"),
        0x00DB => Some("HelpContextId"),
        0x0105 => Some("TabIndex"),
        0x0135 => Some("PictureAlignment"),
        0x013E => Some("DatasheetGridlinesBehavior"),
        0x013F => Some("DatasheetGridlinesColor"),
        0x01A6 => Some("AllowDatasheetView"),
        0x01C4 => Some("FilterOnLoad"),
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
        // The types of the Access 2000 format: the acControlType of Access
        0x0064 => Some("Label"),
        0x0065 => Some("Rectangle"),
        0x0066 => Some("Line"),
        0x0067 => Some("Image"),
        0x0068 => Some("CommandButton"),
        0x0069 => Some("OptionButton"),
        0x006A => Some("CheckBox"),
        0x006B => Some("OptionGroup"),
        0x006C => Some("BoundObjectFrame"),
        0x006D => Some("TextBox"),
        0x006E => Some("ListBox"),
        0x006F => Some("ComboBox"),
        0x0070 => Some("SubForm"),
        0x0072 => Some("ObjectFrame"),
        0x0076 => Some("PageBreak"),
        0x007A => Some("ToggleButton"),
        0x007B => Some("TabControl"),
        0x007C => Some("Page"),
        0x0098 => Some("Detail"),
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
    /// Return the known property name, or `None` for unknown IDs: a text
    /// value is named by [`prop_id_name`], an integer or Yes/No value by
    /// [`number_prop_id_name`], and other values not at all.
    pub fn name(&self) -> Option<&'static str> {
        match self.value {
            BlobValue::Text(_) => prop_id_name(self.prop_id),
            BlobValue::Bool(_) | BlobValue::Short(_) | BlobValue::Long(_) => {
                number_prop_id_name(self.prop_id)
            }
            _ => None,
        }
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
            Self::NotDefault => write!(f, "NotDefault"),
            Self::Short(v) => write!(f, "{v}"),
            Self::Long(v) => write!(f, "{v}"),
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
/// TypeInfo is used to associate control names and type codes. The form is
/// read when a form and a report share the name.
pub fn read_form_properties(
    reader: &mut PageReader,
    name: &str,
) -> Result<FormProperties, FileError> {
    form_properties(reader, name, None)
}

/// [`read_form_properties`] of the form or the report named `name`.
pub fn read_form_properties_of_type(
    reader: &mut PageReader,
    name: &str,
    object_type: FormObjectType,
) -> Result<FormProperties, FileError> {
    form_properties(reader, name, Some(object_type))
}

fn form_properties(
    reader: &mut PageReader,
    name: &str,
    object_type: Option<FormObjectType>,
) -> Result<FormProperties, FileError> {
    read_form_properties_in(&storage::read_access_storage(reader)?, name, object_type)
}

/// [`read_form_properties`] in the storage already read: of `object_type`
/// when it is given, and otherwise the form when a form and a report share
/// the name.
pub fn read_form_properties_in(
    storage: &AccessStorage,
    name: &str,
    object_type: Option<FormObjectType>,
) -> Result<FormProperties, FileError> {
    let entries = &storage.entries;
    let (object_type, blob_data) = find_stream(entries, name, object_type, StreamKind::Blob)?;

    // The Access 2000 format keeps the type of each control in the Blob.
    if blob_data.first() == Some(&crate::form2000::VERSION) {
        let (properties, controls) = crate::form2000::read(&blob_data).unwrap_or_default();
        return Ok(FormProperties {
            form_name: name.to_string(),
            object_type,
            properties,
            controls,
        });
    }

    // Try to get TypeInfo for control name/type mapping; not fatal if missing.
    // It is that of the object the Blob is of, even when a report shares the
    // name of a form.
    let type_info_controls =
        match find_stream(entries, name, Some(object_type), StreamKind::TypeInfo) {
            Ok((_, ti_data)) => parse_type_info(&ti_data, storage.code_page).unwrap_or_default(),
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
            let type_code = type_info_by_name(&type_info_controls, &blob_name)
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

/// The control of TypeInfo named `name`. Access does not tell names apart by
/// case, and TypeInfo can keep an old spelling of a name, such as `H_Code`
/// for the control the Blob and SaveAsText name `H_code`.
fn type_info_by_name<'a>(controls: &'a [ControlInfo], name: &str) -> Option<&'a ControlInfo> {
    let name = name.to_lowercase();
    controls.iter().find(|c| c.name.to_lowercase() == name)
}

// ---------------------------------------------------------------------------
// Internal: stream lookup
// ---------------------------------------------------------------------------

/// Find a specific stream for a named form/report.
///
/// Searches the Forms folder and then the Reports folder, or only the one
/// of `object_type` when it is given. Returns the object type and the
/// stream binary data.
fn find_stream(
    entries: &[storage::StorageEntry],
    name: &str,
    object_type: Option<FormObjectType>,
    stream_kind: StreamKind,
) -> Result<(FormObjectType, Vec<u8>), FileError> {
    let root_id = storage::find_root_id(entries);

    // Try Forms first, then Reports
    for (folder_name, obj_type) in [
        ("Forms", FormObjectType::Form),
        ("Reports", FormObjectType::Report),
    ] {
        if object_type.is_some_and(|t| t != obj_type) {
            continue;
        }
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
/// - Entries: ctrl_type(u16) + padding(u16) + index(u32) + name(NUL) + align(0x00)
///
/// The names are bytes in a code page, read in `code_page`, that of the
/// header of the database. The code page they are written in is that of
/// the Access that saved the form, which the file does not tell: the
/// Russian names of Strings.mdb are in 1251, while its header has 1252.
fn parse_type_info(data: &[u8], code_page: u16) -> Result<Vec<ControlInfo>, FileError> {
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

        // Each entry has two NUL-terminated strings:
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
            encoding::decode_code_page(epp_bytes, code_page)
        } else {
            encoding::decode_code_page(real_bytes, code_page)
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

// ---------------------------------------------------------------------------
// Internal: Blob parser
// ---------------------------------------------------------------------------

/// Parse the Blob binary into form-level and per-control property groups.
///
/// Returns (form_properties, vec_of_control_properties).
/// Each control section starts with a Name (0x14) property.
/// Parsing stops gracefully on unknown types or malformed data.
///
/// Entry format (reverse-engineered): see [`parse_entry`].
fn parse_blob(data: &[u8]) -> Result<(Vec<BlobProperty>, Vec<Vec<BlobProperty>>), FileError> {
    if data.len() < 14 {
        return Ok((Vec::new(), Vec::new()));
    }

    // Skip 8-byte blob header + 6-byte section preamble.
    let (all_props, pos) = parse_group(data, 14, data.len());

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

/// The trailers that end a group of entries: the properties of the form or
/// report itself end with 255, those of a control with 253.
const GROUP_END: std::ops::RangeInclusive<u32> = 253..=255;

/// The entries of the group from `start`, which ends by `end`: through the
/// entry whose trailer ends the group (see [`GROUP_END`]), or up to the
/// bytes that are not an entry. Returns them and where the reading stopped.
fn parse_group(data: &[u8], start: usize, end: usize) -> (Vec<BlobProperty>, usize) {
    let mut props = Vec::new();
    let mut pos = start;
    while let Some((property, next, last)) = parse_entry(data, pos, end) {
        props.push(property);
        pos = next;
        if last {
            break;
        }
    }
    (props, pos)
}

/// The property entry at `pos`, which ends by `end`, where the next entry
/// starts, and whether it ends its group; `None` when there is no entry
/// there.
///
/// An entry is reverse-engineered as `prop_id(u16) + type(u32) + B(u32) +
/// C(u32)`, then C bytes of value and a 4-byte trailer. The type tells how
/// to read the value (see [`entry_value`]); B is not used. The trailer grows
/// from one entry to the next, and is one of [`GROUP_END`] for the last entry
/// of a group.
fn parse_entry(data: &[u8], pos: usize, end: usize) -> Option<(BlobProperty, usize, bool)> {
    let header = data.get(pos..pos.checked_add(14)?)?;
    let prop_id = u16::from_le_bytes([header[0], header[1]]);
    let type_code = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    let len = u32::from_le_bytes([header[10], header[11], header[12], header[13]]);
    let data_start = pos + 14;
    let data_end = variable_data_end(data_start, len, end)?;
    let value = entry_value(type_code, &data[data_start..data_end]);
    let t = &data[data_end..data_end + 4];
    let trailer = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
    let property = BlobProperty { prop_id, value };
    Some((property, data_end + 4, GROUP_END.contains(&trailer)))
}

/// The value of an entry of type `type_code` whose value bytes are `bytes`.
///
/// - 0x01 Yes/No: no bytes for [`BlobValue::NotDefault`], one byte for a
///   value (0 for No), and 2 or 4 bytes for an integer, such as `Overlaps`.
/// - 0x02, 0x03 integers and 0x04 a 32-bit value (often a color, given as
///   [`BlobValue::Long`]): a little-endian integer of 1, 2 or 4 bytes, signed
///   for 2 and 4 bytes.
/// - 0x06 a single of 4 bytes, such as `AlternateBackShade`, given as a
///   double.
/// - 0x08 a double of 8 bytes, 0x09 a GUID of 16 bytes.
/// - 0x0A, 0x0C UTF-16LE text, 0x0B bytes.
///
/// Bytes of a length the type does not have, and of a type jetdb does not
/// know, are given as they are.
fn entry_value(type_code: u32, bytes: &[u8]) -> BlobValue {
    let binary = || BlobValue::Binary(bytes.to_vec());
    match type_code {
        0x01 => yes_no_value(bytes),
        0x02 => integer_value(bytes)
            .and_then(|v| i16::try_from(v).ok())
            .map_or_else(binary, BlobValue::Short),
        0x03 | 0x04 => integer_value(bytes).map_or_else(binary, BlobValue::Long),
        0x06 => <[u8; 4]>::try_from(bytes).map_or_else(
            |_| binary(),
            |b| BlobValue::Double(f64::from(f32::from_le_bytes(b))),
        ),
        0x08 => <[u8; 8]>::try_from(bytes)
            .map_or_else(|_| binary(), |b| BlobValue::Double(f64::from_le_bytes(b))),
        0x09 if bytes.len() == 16 => BlobValue::Guid(format_guid(bytes)),
        0x0A | 0x0C => BlobValue::Text(
            encoding::decode_utf16le(bytes)
                .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned()),
        ),
        _ => binary(),
    }
}

/// The value of a Yes/No entry (type 0x01): [`BlobValue::NotDefault`] for
/// no bytes, a Yes/No for one, and an integer for more.
fn yes_no_value(bytes: &[u8]) -> BlobValue {
    match *bytes {
        [] => BlobValue::NotDefault,
        [b] => BlobValue::Bool(b != 0),
        _ => {
            integer_value(bytes).map_or_else(|| BlobValue::Binary(bytes.to_vec()), BlobValue::Long)
        }
    }
}

/// A little-endian integer of 1 byte (unsigned), 2 or 4 bytes (signed).
fn integer_value(bytes: &[u8]) -> Option<i32> {
    match *bytes {
        [b] => Some(i32::from(b)),
        [a, b] => Some(i32::from(i16::from_le_bytes([a, b]))),
        [a, b, c, d] => Some(i32::from_le_bytes([a, b, c, d])),
        _ => None,
    }
}

/// The end of `len` bytes of variable-length data from `start`, when they and
/// the 4-byte trailer after them end by `limit`. The sums are checked: `len`
/// comes from the file, and `usize` has 32 bits in Wasm, where an unchecked
/// sum could wrap around and pass the check.
fn variable_data_end(start: usize, len: u32, limit: usize) -> Option<usize> {
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    (end.checked_add(4)? <= limit).then_some(end)
}

/// Parse property entries from a section of the blob: the group of entries
/// from `start`, limited to the given range.
fn parse_section_props(data: &[u8], start: usize, end: usize) -> Vec<BlobProperty> {
    parse_group(data, start, end).0
}

/// Format 16 bytes as a GUID string `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`.
pub(crate) fn format_guid(bytes: &[u8]) -> String {
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
    use std::collections::{HashMap, HashSet};

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
    fn type_info_is_found_by_name_of_any_case() {
        let control = |name: &str, type_code| ControlInfo {
            name: name.to_string(),
            type_code,
            index: 0,
        };
        let controls = [control("Label7", 0x0C64), control("H_Code", 0x136F)];
        let found = |name| type_info_by_name(&controls, name).map(|c| c.type_code);
        assert_eq!(found("H_code"), Some(0x136F));
        assert_eq!(found("h_code"), Some(0x136F));
        assert_eq!(found("Sex"), None);
    }

    #[test]
    fn parse_type_info_too_short() {
        let data = [0u8; 16];
        let result = parse_type_info(&data, 932);
        assert!(result.is_err());
    }

    #[test]
    fn parse_type_info_bad_magic() {
        let mut data = [0u8; 32];
        data[0] = 0xFF; // wrong magic
        let result = parse_type_info(&data, 932);
        assert!(result.is_err());
    }

    #[test]
    fn parse_type_info_empty_entries() {
        let mut data = vec![0u8; 32];
        // Set magic
        data[0..4].copy_from_slice(&TYPEINFO_MAGIC.to_le_bytes());
        // entry_count = 0 (at offset 12)
        data[12..16].copy_from_slice(&0u32.to_le_bytes());

        let result = parse_type_info(&data, 932).unwrap();
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
        data.extend_from_slice(b"Btn1"); // ASCII name
        data.push(0x00); // NUL terminator
        data.push(0x00); // alignment

        let result = parse_type_info(&data, 932).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "Btn1");
        assert_eq!(result[0].type_code, 0x0B68);
        assert_eq!(result[0].index, 0);
    }

    #[test]
    fn type_info_names_are_read_in_the_code_page() {
        // One entry named `name`, in bytes of a code page.
        let type_info = |name: &[u8]| {
            let mut data = vec![0u8; 32];
            data[0..4].copy_from_slice(&TYPEINFO_MAGIC.to_le_bytes());
            data[12..16].copy_from_slice(&1u32.to_le_bytes());
            data.extend_from_slice(&[0x64, 0x0D, 0, 0, 0, 0, 0, 0]);
            data.extend_from_slice(name);
            data.extend_from_slice(&[0, 0]);
            data
        };
        let name =
            |data: &[u8], code_page| parse_type_info(data, code_page).unwrap()[0].name.clone();
        // 詳細 in 932, Café in 1252, Заголовок in 1251.
        assert_eq!(name(&type_info(&[0x8F, 0xDA, 0x8D, 0xD7]), 932), "詳細");
        assert_eq!(name(&type_info(b"Caf\xE9"), 1252), "Café");
        let russian = [0xC7, 0xE0, 0xE3, 0xEE, 0xEB, 0xEE, 0xE2, 0xEE, 0xEA];
        assert_eq!(name(&type_info(&russian), 1251), "Заголовок");
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

    /// One Blob entry: the header with the length of `value` as C, the value,
    /// and a trailer.
    fn entry(prop_id: u16, type_code: u32, value: &[u8]) -> Vec<u8> {
        let mut payload = value.to_vec();
        payload.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]);
        make_entry(prop_id, type_code, 1, value.len() as u32, &payload)
    }

    /// The integer properties of a control in a SaveAsText file, by key.
    type SavedIntegers = Vec<(String, i64)>;

    /// A block of a SaveAsText file being read: its depth, the control name,
    /// its integer properties and its `NotDefault` lines.
    type SaveAsTextBlock = (usize, Option<String>, SavedIntegers, usize);

    /// The integer properties a SaveAsText file of a form or report writes
    /// for each control, by its name ("" for the form or report itself), and
    /// the number of its `NotDefault` lines.
    fn save_as_text_integers(path: &std::path::Path) -> HashMap<String, (SavedIntegers, usize)> {
        let text = std::fs::read_to_string(path).unwrap();
        let mut found: HashMap<String, (SavedIntegers, usize)> = HashMap::new();
        let mut blocks: Vec<SaveAsTextBlock> = Vec::new();
        // The depth of the binary data (`= Begin` ... `End`) being passed over.
        let mut binary = 0;
        for line in text.trim_start_matches('\u{feff}').lines() {
            let trimmed = line.trim();
            if binary > 0 {
                binary += usize::from(trimmed.ends_with("= Begin"));
                binary -= usize::from(trimmed == "End");
            } else if trimmed.ends_with("= Begin") || trimmed.ends_with("=Begin") {
                binary = 1;
            } else if trimmed == "Begin" || trimmed.starts_with("Begin ") {
                blocks.push((blocks.len(), None, Vec::new(), 0));
            } else if trimmed == "End" {
                if let Some(block) = blocks.pop() {
                    close_save_as_text_block(&mut found, block);
                }
            } else if let Some(block) = blocks.last_mut() {
                save_as_text_property(block, trimmed);
            }
        }
        found
    }

    /// Adds what `block` holds to `found`, under its control name ("" for
    /// the form or report itself); a block without a name is left out.
    fn close_save_as_text_block(
        found: &mut HashMap<String, (SavedIntegers, usize)>,
        (depth, name, ints, not_default): SaveAsTextBlock,
    ) {
        let Some(name) = (if depth == 0 {
            Some(String::new())
        } else {
            name
        }) else {
            return;
        };
        let entry = found.entry(name).or_default();
        entry.0.extend(ints);
        entry.1 += not_default;
    }

    /// Adds the property line `line` of a SaveAsText file to `block`.
    fn save_as_text_property(block: &mut SaveAsTextBlock, line: &str) {
        let Some((key, value)) = line.split_once('=') else {
            return;
        };
        let (key, value) = (key.trim(), value.trim());
        if key == "Name" && block.1.is_none() {
            block.1 = Some(value.trim_matches('"').to_string());
        } else if value == "NotDefault" {
            block.3 += 1;
        } else if let Ok(v) = value.parse::<i64>() {
            block.2.push((key.to_string(), v));
        }
    }

    /// The keys of `saved` whose value is `value` as SaveAsText writes it:
    /// `None` for a value it does not write as an integer. A Yes value is -1,
    /// 1 or 255.
    fn keys_of(value: &BlobValue, saved: &SavedIntegers) -> Option<HashSet<String>> {
        let values: &[i64] = match *value {
            BlobValue::Short(v) => &[i64::from(v)],
            BlobValue::Long(v) => &[i64::from(v)],
            BlobValue::Bool(false) => &[0],
            BlobValue::Bool(true) => &[-1, 1, 255],
            _ => return None,
        };
        Some(
            saved
                .iter()
                .filter(|(_, v)| values.contains(v))
                .map(|(k, _)| k.clone())
                .collect(),
        )
    }

    /// Checks the values of the form or report `name` against its SaveAsText
    /// file `file`: narrows the keys each property may be in `keys`, and
    /// returns how many values were checked.
    fn check_against_save_as_text(
        path: &std::path::Path,
        file: &std::path::Path,
        object_type: FormObjectType,
        name: &str,
        keys: &mut HashMap<u16, HashSet<String>>,
    ) -> usize {
        let written = save_as_text_integers(file);
        let mut reader = PageReader::open(path).unwrap();
        let read = read_form_properties_of_type(&mut reader, name, object_type).unwrap();
        let groups = std::iter::once((String::new(), read.properties))
            .chain(read.controls.into_iter().map(|c| (c.name, c.properties)));
        let mut checked = 0;
        for (control, properties) in groups {
            let (saved, not_default) = written.get(&control).cloned().unwrap_or_default();
            // SaveAsText writes the right and bottom of a form only.
            let report_side = object_type == FormObjectType::Report && control.is_empty();
            let compared = properties.iter().filter(|p| {
                p.prop_id != 0x0000 && !(report_side && matches!(p.prop_id, 0x0081 | 0x000C))
            });
            for p in compared {
                let Some(found) = keys_of(&p.value, &saved) else {
                    continue;
                };
                assert!(
                    !found.is_empty(),
                    "{name} {control:?} {:#06x} {:?}",
                    p.prop_id,
                    p.value
                );
                let narrowed = keys.entry(p.prop_id).or_insert_with(|| found.clone());
                narrowed.retain(|k| found.contains(k));
                checked += 1;
            }
            if control.is_empty() {
                let read_not_default = properties
                    .iter()
                    .filter(|p| matches!(p.value, BlobValue::NotDefault))
                    .count();
                assert_eq!(read_not_default, not_default, "{name}");
            }
        }
        checked
    }

    #[test]
    fn values_are_those_save_as_text_writes() {
        // Every integer and Yes/No value of a form and a report of
        // Sports.accdb is one SaveAsText writes for the same control under a
        // key that is the same for every value of the property, and the form
        // or report itself has as many NotDefault values as SaveAsText has
        // lines, and a named property is under its name. Left out: property 0x0000, which SaveAsText does not write,
        // and the right and bottom (0x0081, 0x000C) of a report. Few
        // properties of the controls of the report come after their names,
        // where jetdb reads them, so fewer values are checked there.
        let path = skip_if_missing!("saveastext/SportsAdmin/Sports.accdb");
        let dir = path.parent().unwrap().to_path_buf();
        let mut keys: HashMap<u16, HashSet<String>> = HashMap::new();
        for (file, object_type, name, at_least) in [
            (
                "forms/Statistical Reports.bas",
                FormObjectType::Form,
                "Statistical Reports",
                50,
            ),
            (
                "reports/Field (High Jump).bas",
                FormObjectType::Report,
                "Field (High Jump)",
                5,
            ),
        ] {
            let checked =
                check_against_save_as_text(&path, &dir.join(file), object_type, name, &mut keys);
            assert!(checked >= at_least, "{name}: only {checked} values");
        }
        for (id, found) in &keys {
            assert!(!found.is_empty(), "{id:#06x}: no key fits every value");
        }
        let mut named = 0;
        for (id, found) in &keys {
            if let Some(key) = number_prop_id_name(*id) {
                assert!(found.contains(key), "{id:#06x} {key}: {found:?}");
                named += 1;
            }
        }
        assert!(named >= 10, "only {named} named properties checked");
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
        // Yes/No (type 0x01): no value for NotDefault, one byte for a value,
        // and four bytes for an integer such as Overlaps.
        let mut entries = entry(0x0013, 0x01, &[]);
        entries.extend(entry(0x01A6, 0x01, &[0x00]));
        entries.extend(entry(0x01C4, 0x01, &[0xFF]));
        entries.extend(entry(0x02E0, 0x01, &[0x01, 0x00, 0x00, 0x00]));
        let (props, _) = parse_blob(&make_blob(&entries)).unwrap();
        assert_eq!(props.len(), 4);
        assert_eq!(props[0].prop_id, 0x0013);
        assert!(matches!(props[0].value, BlobValue::NotDefault));
        assert!(matches!(props[1].value, BlobValue::Bool(false)));
        assert!(matches!(props[2].value, BlobValue::Bool(true)));
        assert!(matches!(props[3].value, BlobValue::Long(1)));
    }
    #[test]
    fn parse_blob_short_entry() {
        // Short (type 0x02): one byte, unsigned.
        let mut entries = entry(0x0098, 0x02, &[0x2A]);
        entries.extend(entry(0x0099, 0x02, &[0xFF]));
        let (props, _) = parse_blob(&make_blob(&entries)).unwrap();
        assert_eq!(props.len(), 2);
        assert!(matches!(props[0].value, BlobValue::Short(42)));
        assert!(matches!(props[1].value, BlobValue::Short(255)));
    }
    #[test]
    fn parse_blob_long_entry() {
        // Long (type 0x03): two or four bytes, signed.
        let mut entries = entry(0x0096, 0x03, &[0x3D, 0x1B]);
        entries.extend(entry(0x0036, 0x03, &[0xDF, 0xB7]));
        entries.extend(entry(0x013F, 0x03, &[0xC0, 0xC0, 0xC0, 0x00]));
        let (props, _) = parse_blob(&make_blob(&entries)).unwrap();
        assert_eq!(props.len(), 3);
        assert!(matches!(props[0].value, BlobValue::Long(6973)));
        assert!(matches!(props[1].value, BlobValue::Long(-18465)));
        assert!(matches!(props[2].value, BlobValue::Long(12632256)));
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
    fn variable_data_of_a_length_too_long_stops() {
        // A length near u32::MAX, as a broken file may hold, ends the
        // properties rather than wrapping around in 32-bit usize (Wasm) and
        // slicing out of range.
        for kind in [0x0A, 0x0B] {
            let entry = make_entry(
                0x009C,
                kind,
                0,
                u32::MAX,
                &[0x41, 0x00, 0x00, 0x00, 0x00, 0x00],
            );
            let (props, _) = parse_blob(&make_blob(&entry)).unwrap();
            assert!(props.is_empty(), "{kind:#x}");
            let section = parse_section_props(&entry, 0, entry.len());
            assert!(section.is_empty(), "{kind:#x}");
        }
        assert_eq!(variable_data_end(10, 4, 18), Some(14));
        assert_eq!(variable_data_end(10, 4, 17), None);
        assert_eq!(variable_data_end(usize::MAX - 2, 1, usize::MAX), None);
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
        // Each value is its own length, and the trailer after it is not part
        // of it: 20 rather than 0x00620014, as the trailer 0x62 follows.
        let mut entries = entry(0x0013, 0x01, &[]);
        entries.extend(make_entry(
            0x0029,
            0x03,
            4,
            2,
            &[0x14, 0x00, 0x62, 0x00, 0x00, 0x00],
        ));
        entries.extend(entry(0x0098, 0x02, &[0x07]));
        entries.extend(entry(0x002A, 0x03, &[0xFF, 0xFF]));
        let (props, _) = parse_blob(&make_blob(&entries)).unwrap();
        assert_eq!(props.len(), 4);
        assert!(matches!(props[0].value, BlobValue::NotDefault));
        assert!(matches!(props[1].value, BlobValue::Long(20)));
        assert!(matches!(props[2].value, BlobValue::Short(7)));
        assert!(matches!(props[3].value, BlobValue::Long(-1)));
    }
    #[test]
    fn parse_blob_reads_on_past_unknown_types() {
        // An entry of a type jetdb does not know (such as 0x11) is read by its
        // length as bytes, and the entries after it are read too.
        let mut entries = entry(0x0013, 0x01, &[]);
        entries.extend(entry(0x0201, 0x11, &[1, 2, 3]));
        entries.extend(entry(0x0230, 0x01, &[0x00]));
        let (props, _) = parse_blob(&make_blob(&entries)).unwrap();
        assert_eq!(props.len(), 3);
        assert!(matches!(props[0].value, BlobValue::NotDefault));
        assert!(matches!(&props[1].value, BlobValue::Binary(b) if b == &[1, 2, 3]));
        assert!(matches!(props[2].value, BlobValue::Bool(false)));
    }

    #[test]
    fn a_group_ends_with_a_trailer_of_253_to_255() {
        // The entry whose trailer is 253, 254 or 255 is the last of its group:
        // what follows is not read as an entry of it.
        for end in [253u32, 254, 255] {
            let mut entries = entry(0x0029, 0x03, &[0x14, 0x00]);
            let mut last = vec![0x01];
            last.extend_from_slice(&end.to_le_bytes());
            entries.extend(make_entry(0x02E6, 0x01, 1, 1, &last));
            entries.extend(entry(0x0230, 0x01, &[0x00]));
            let (props, _) = parse_blob(&make_blob(&entries)).unwrap();
            assert_eq!(props.len(), 2, "{end}");
            assert_eq!(props[1].prop_id, 0x02E6);
            let section = parse_section_props(&entries, 0, entries.len());
            assert_eq!(section.len(), 2, "{end}");
        }
    }

    #[test]
    fn properties_after_an_entry_of_an_unknown_type() {
        // The form Shared has an entry 0x0201 of type 0x11 (730 bytes), and
        // twelve more after it, through 0x02E6, whose trailer 255 ends the
        // properties of the form.
        let path = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let read =
            read_form_properties_of_type(&mut reader, "Shared", FormObjectType::Form).unwrap();
        let ids: Vec<u16> = read.properties.iter().map(|p| p.prop_id).collect();
        let at = ids.iter().position(|&id| id == 0x0201).unwrap();
        assert!(matches!(&read.properties[at].value, BlobValue::Binary(b) if b.len() == 730));
        assert_eq!(ids.len() - at - 1, 12);
        assert_eq!(ids.last(), Some(&0x02E6));
        assert!(read
            .properties
            .iter()
            .any(|p| p.prop_id == 0x0277 && matches!(p.value, BlobValue::Double(_))));
    }
    #[test]
    fn parse_blob_guid_entry() {
        // GUID (type 0x09): 16 bytes.
        let guid_bytes = [
            0x50, 0xA6, 0x64, 0x8D, 0xE7, 0x62, 0x03, 0x49, 0x97, 0x33, 0x0D, 0x8C, 0xE8, 0x49,
            0x78, 0xBF,
        ];
        let (props, _) = parse_blob(&make_blob(&entry(0x0178, 0x09, &guid_bytes))).unwrap();
        assert_eq!(props.len(), 1);
        match &props[0].value {
            BlobValue::Guid(s) => assert_eq!(s, "{8d64a650-62e7-4903-9733-0d8ce84978bf}"),
            other => panic!("expected Guid, got {:?}", other),
        }
    }

    #[test]
    fn values_of_a_length_their_type_does_not_have_are_bytes() {
        let mut entries = entry(0x0029, 0x03, &[1, 2, 3]);
        entries.extend(entry(0x0178, 0x09, &[1; 4]));
        entries.extend(entry(0x00AA, 0x08, &[1; 4]));
        let (props, _) = parse_blob(&make_blob(&entries)).unwrap();
        let values: Vec<&BlobValue> = props.iter().map(|p| &p.value).collect();
        assert!(matches!(values[..], [
            BlobValue::Binary(a),
            BlobValue::Binary(b),
            BlobValue::Binary(c),
        ] if a == &[1, 2, 3] && b == &[1; 4] && c == &[1; 4]));
    }
    #[test]
    fn prop_id_name_known() {
        assert_eq!(prop_id_name(0x009C), Some("RecordSource"));
        assert_eq!(prop_id_name(0x001B), Some("ControlSource"));
        assert_eq!(prop_id_name(0x00F5), Some("Filter"));
        assert_eq!(prop_id_name(0x0014), Some("Name"));
    }

    #[test]
    fn names_by_the_type_of_the_value() {
        // 0x0068 is OnKeyDown on a form, but holds a color on a label.
        let property = |prop_id, value| BlobProperty { prop_id, value };
        let event = property(0x0068, BlobValue::Text("[Event Procedure]".to_string()));
        assert_eq!(event.name(), Some("OnKeyDown"));
        let color = property(0x0068, BlobValue::Long(0));
        assert_eq!(color.name(), None);
        assert_eq!(color.display_name(), "0x0068");
        // 0x0036 is Left, an integer.
        assert_eq!(property(0x0036, BlobValue::Short(120)).name(), Some("Left"));
        assert_eq!(
            property(0x0036, BlobValue::Long(70000)).name(),
            Some("Left")
        );
        assert_eq!(
            property(0x01C4, BlobValue::Bool(false)).name(),
            Some("FilterOnLoad")
        );
        assert_eq!(property(0x0036, BlobValue::NotDefault).name(), None);
        assert_eq!(
            property(0x0036, BlobValue::Text("x".to_string())).name(),
            None
        );
    }

    #[test]
    fn prop_id_name_unknown() {
        assert_eq!(prop_id_name(0xFFFF), None);
    }

    #[test]
    fn blob_property_display_name() {
        let known = BlobProperty {
            prop_id: 0x009C,
            value: BlobValue::Text("Orders".to_string()),
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
    fn parse_section_props_type_06_single() {
        // Type 0x06: a single of four bytes, such as AlternateBackShade.
        let data = entry(0x0276, 0x06, &95.0f32.to_le_bytes());
        let props = parse_section_props(&data, 0, data.len());
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x0276);
        assert!(matches!(props[0].value, BlobValue::Double(v) if v == 95.0));
    }
    #[test]
    fn parse_section_props_type_08_double() {
        // Type 0x08: a double of eight bytes.
        let data = entry(0x00AA, 0x08, &1.23f64.to_le_bytes());
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
        // Type 0x04: a 32-bit value of four bytes, often a color, given as the
        // signed number SaveAsText writes: 0x8000000F is a system color.
        let data = entry(0x00BB, 0x04, &0x8000_000Fu32.to_le_bytes());
        let props = parse_section_props(&data, 0, data.len());
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].prop_id, 0x00BB);
        assert!(matches!(props[0].value, BlobValue::Long(-2_147_483_633)));
        assert_eq!(props[0].value.to_string(), "-2147483633");
    }
    /// An object with a name in the SaveAsText output of Access: the type of
    /// its `Begin` block and its string properties.
    struct SavedObject {
        name: String,
        type_name: String,
        properties: HashMap<String, String>,
    }

    /// A form or report in the SaveAsText output of Access: the string
    /// properties of the form or report itself, and its named objects.
    struct Saved {
        properties: HashMap<String, String>,
        objects: Vec<SavedObject>,
    }

    /// A quoted string of SaveAsText, with `\"` and `\\` for a quote and a
    /// backslash, and `\ooo` for a character by its octal code, such as
    /// `\015\012` for a line break.
    fn saved_string(text: &str) -> Option<String> {
        let inner = text.strip_prefix('"')?.strip_suffix('"')?;
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match (c, chars.clone().next()) {
                ('\\', Some(next @ ('"' | '\\'))) => {
                    out.push(next);
                    chars.next();
                }
                ('\\', Some('0'..='7')) => {
                    let octal: String = chars.by_ref().take(3).collect();
                    out.push(char::from(u8::from_str_radix(&octal, 8).unwrap()));
                }
                _ => out.push(c),
            }
        }
        Some(out)
    }

    /// Reads the string properties of a form or report from its SaveAsText
    /// output. A long string goes on in quoted lines of its own.
    fn read_saveastext(text: &str) -> Saved {
        let mut saved = Saved {
            properties: HashMap::new(),
            objects: Vec::new(),
        };
        // The open blocks: `Begin <Type>`, or `<Key> = Begin` for binary data.
        let mut blocks: Vec<(String, HashMap<String, String>)> = Vec::new();
        let mut last_key: Option<String> = None;
        for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
            let properties = match blocks.len() {
                0 => None,
                1 => Some(&mut saved.properties),
                _ => blocks.last_mut().map(|(_, p)| p),
            };
            if let (Some(key), Some(more), Some(properties)) =
                (&last_key, saved_string(line), properties)
            {
                properties.get_mut(key).unwrap().push_str(&more);
                continue;
            }
            last_key = None;
            if line == "End" {
                if let Some((type_name, properties)) = blocks.pop() {
                    if let Some(name) = properties.get("Name").cloned() {
                        saved.objects.push(SavedObject {
                            name,
                            type_name,
                            properties,
                        });
                    }
                }
            } else if line == "Begin" || line.starts_with("Begin ") {
                blocks.push((line["Begin".len()..].trim().to_string(), HashMap::new()));
            } else if line.ends_with(" = Begin") {
                blocks.push((String::new(), HashMap::new()));
            } else if let Some((key, value)) = line.split_once(" =") {
                let properties = match blocks.len() {
                    0 => continue,
                    1 => &mut saved.properties,
                    _ => &mut blocks.last_mut().unwrap().1,
                };
                if let Some(value) = saved_string(value) {
                    properties.insert(key.to_string(), value);
                    last_key = Some(key.to_string());
                } else {
                    properties.insert(key.to_string(), value.trim().to_string());
                }
            }
        }
        saved
    }

    /// The ways SaveAsText writes `value`: a Yes value is -1, 1 or 255.
    fn saved_forms(value: &BlobValue) -> Vec<String> {
        match *value {
            BlobValue::Bool(true) => vec!["-1".into(), "1".into(), "255".into()],
            BlobValue::Bool(false) => vec!["0".into()],
            ref other => vec![other.to_string()],
        }
    }

    /// The name jetdb gives the type that SaveAsText names `saved`.
    fn jetdb_type_name(saved: &str) -> &str {
        match saved {
            "Section" => "Detail",
            "BreakHeader" => "GroupHeader",
            "Subform" => "SubForm",
            other => other,
        }
    }

    #[test]
    fn form_and_report_of_the_same_name() {
        // Made by testdata/form-generators/same-name-generator.bas: a form and
        // a report both named Shared, each with a caption and a control of
        // its own.
        let path = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let caption = |p: &FormProperties| {
            p.properties
                .iter()
                .find(|x| x.name() == Some("Caption"))
                .map(|x| x.value.to_string())
        };
        let has_control =
            |p: &FormProperties, name: &str| p.controls.iter().any(|c| c.name == name);

        let form =
            read_form_properties_of_type(&mut reader, "Shared", FormObjectType::Form).unwrap();
        assert_eq!(form.object_type, FormObjectType::Form);
        assert_eq!(caption(&form).as_deref(), Some("Shared form"));
        assert!(has_control(&form, "btnFormOnly") && !has_control(&form, "lblReportOnly"));

        let report =
            read_form_properties_of_type(&mut reader, "Shared", FormObjectType::Report).unwrap();
        assert_eq!(report.object_type, FormObjectType::Report);
        assert_eq!(caption(&report).as_deref(), Some("Shared report"));
        assert!(has_control(&report, "lblReportOnly") && !has_control(&report, "btnFormOnly"));

        // Without a type, the form, as before.
        let either = read_form_properties(&mut reader, "Shared").unwrap();
        assert_eq!(either.object_type, FormObjectType::Form);

        // The TypeInfo of the report. (Objects made with CreateForm and
        // CreateReport have an empty TypeInfo, so only which object it is of
        // is checked; the Blob above shows the two apart.)
        let report_type_info = read_form_stream_of_type(
            &mut reader,
            "Shared",
            FormObjectType::Report,
            StreamKind::TypeInfo,
        )
        .unwrap();
        assert_eq!(report_type_info.object_type, FormObjectType::Report);

        let form_blob = read_form_stream_of_type(
            &mut reader,
            "Shared",
            FormObjectType::Form,
            StreamKind::Blob,
        )
        .unwrap();
        let report_blob = read_form_stream_of_type(
            &mut reader,
            "Shared",
            FormObjectType::Report,
            StreamKind::Blob,
        )
        .unwrap();
        assert_eq!(report_blob.object_type, FormObjectType::Report);
        assert_ne!(form_blob.data, report_blob.data);

        // A type that the name does not have is not found.
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        assert!(matches!(
            read_form_properties_of_type(&mut reader, "F_Buttons", FormObjectType::Report),
            Err(FileError::FormNotFound { .. })
        ));
    }

    /// Forms and reports of propertyIdTestV2007.accdb, made by
    /// testdata/form-generators/property-id-generator.bas, with a property set to
    /// a value of their own, and the value SaveAsText writes for it: a copy named
    /// Report_* of the base report, the others of the base form.
    const SET_VALUES: &[(&str, &str, &str)] = &[
        ("Form_BorderStyle_2", "BorderStyle", "3"),
        ("Form_GridX_2", "GridX", "30"),
        ("Form_GridY_2", "GridY", "40"),
        ("Form_Width_1", "Width", "5000"),
        ("Form_ScrollBars_2", "ScrollBars", "1"),
        ("Form_ItemSuffix_2", "ItemSuffix", "11"),
        ("Form_DatasheetFontHeight_2", "DatasheetFontHeight", "18"),
        ("Form_HelpContextId_2", "HelpContextId", "4321"),
        (
            "Form_DatasheetGridlinesColor_1",
            "DatasheetGridlinesColor",
            "255",
        ),
        ("Form_FilterOnLoad_1", "FilterOnLoad", "-1"),
        ("Form_TimerInterval_2", "TimerInterval", "4321"),
        ("Form_ViewsAllowed_2", "ViewsAllowed", "2"),
        ("Form_DefaultView_2", "DefaultView", "2"),
        ("Form_PictureAlignment_2", "PictureAlignment", "4"),
        (
            "Form_DatasheetGridlinesBehavior_2",
            "DatasheetGridlinesBehavior",
            "1",
        ),
        ("Form_AllowDatasheetView_1", "AllowDatasheetView", "0"),
        ("base_Form", "Right", "14730"),
        ("base_Form", "Bottom", "6608"),
        ("Report_GridX_2", "GridX", "30"),
        ("Report_Width_2", "Width", "7000"),
        ("Report_ItemSuffix_2", "ItemSuffix", "9"),
        ("Report_FilterOnLoad_1", "FilterOnLoad", "-1"),
    ];

    #[test]
    fn named_properties_hold_the_values_access_set() {
        // Each named property holds the value it was set to (see SET_VALUES).
        let path = skip_if_missing!("V2007/propertyIdTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let form = FormObjectType::Form;
        for &(object, name, value) in SET_VALUES {
            let object_type = if object.starts_with("Report_") {
                FormObjectType::Report
            } else {
                form
            };
            let read = read_form_properties_of_type(&mut reader, object, object_type).unwrap();
            let found = read.properties.iter().find(|p| p.name() == Some(name));
            assert!(
                found.is_some_and(|p| saved_forms(&p.value).iter().any(|v| v == value)),
                "{object}: {name}: {:?}",
                found.map(|p| &p.value)
            );
        }

        // The IDs above 0x01C4 are one higher than in Sports.accdb, and are
        // not named: 0x024B is the left of the text box in a layout here.
        let read = read_form_properties_of_type(&mut reader, "TextBox_Left_1", form).unwrap();
        let text_box = read.controls.iter().find(|c| c.name == "txtA").unwrap();
        let left = text_box
            .properties
            .iter()
            .find(|p| p.prop_id == 0x024B)
            .unwrap();
        assert_eq!(left.value.to_string(), "1700");
        assert_eq!(left.name(), None);
    }

    #[test]
    fn form_and_report_match_saveastext_of_access() {
        // Fetched by scripts/fetch-testdata.sh: a form and a report with many
        // controls, and the SaveAsText output of each from Access.
        let path = skip_if_missing!("saveastext/SportsAdmin/Sports.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let keys = ["RecordSource", "Caption", "ControlSource"];
        let value = |properties: &[BlobProperty], key: &str| {
            properties
                .iter()
                .find(|p| p.name() == Some(key))
                .map(|p| p.value.to_string())
        };
        for (name, file, controls, report) in [
            (
                "Statistical Reports",
                "forms/Statistical Reports.bas",
                69,
                false,
            ),
            (
                "Field (High Jump)",
                "reports/Field (High Jump).bas",
                78,
                true,
            ),
        ] {
            let text = std::fs::read_to_string(path.parent().unwrap().join(file)).unwrap();
            let saved = read_saveastext(&text);
            let read = read_form_properties(&mut reader, name).unwrap();
            for key in keys {
                assert_eq!(
                    value(&read.properties, key),
                    saved.properties.get(key).cloned(),
                    "{name}: {key}"
                );
            }
            // Every property jetdb names holds the value Access saved under
            // that name. SaveAsText writes the right and bottom of a form
            // only.
            let mut named = 0;
            let mut check_named =
                |properties: &[BlobProperty], saved: &HashMap<String, String>, at: &str| {
                    for p in properties {
                        let Some(key) = p.name() else { continue };
                        if report && at == "(itself)" && matches!(key, "Right" | "Bottom") {
                            continue;
                        }
                        let saved_value = saved.get(key);
                        assert!(
                            saved_value.is_some_and(|v| saved_forms(&p.value).contains(v)),
                            "{name}: {at}.{key}: {} against {saved_value:?}",
                            p.value
                        );
                        named += 1;
                    }
                };
            check_named(&read.properties, &saved.properties, "(itself)");
            for object in &saved.objects {
                if let Some(control) = read.controls.iter().find(|c| c.name == object.name) {
                    check_named(&control.properties, &object.properties, &object.name);
                }
            }
            assert!(named > controls, "{name}: {named}");
            assert_eq!(read.controls.len(), controls, "{name}");
            assert_eq!(saved.objects.len(), controls, "{name}");
            let mut compared = 0;
            for object in &saved.objects {
                let control = read
                    .controls
                    .iter()
                    .find(|c| c.name == object.name)
                    .unwrap_or_else(|| panic!("{name}: no control {}", object.name));
                assert_eq!(
                    control_type_name(control.type_code),
                    Some(jetdb_type_name(&object.type_name)),
                    "{name}: {}",
                    object.name
                );
                for key in keys {
                    let saved_value = object.properties.get(key).cloned();
                    compared += usize::from(saved_value.is_some());
                    assert_eq!(
                        value(&control.properties, key),
                        saved_value,
                        "{name}: {}.{key}",
                        object.name
                    );
                }
            }
            assert!(compared > 0, "{name}");
        }
    }
}
