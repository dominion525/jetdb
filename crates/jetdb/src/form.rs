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
#[non_exhaustive]
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
#[non_exhaustive]
pub struct FormStream {
    pub name: String,
    pub object_type: FormObjectType,
    pub stream_kind: StreamKind,
    pub data: Vec<u8>,
}

/// A single control entry from TypeInfo.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ControlInfo {
    pub name: String,
    pub type_code: u16,
    pub index: u32,
}

/// Parsed TypeInfo for a form/report.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FormTypeInfo {
    pub form_name: String,
    pub object_type: FormObjectType,
    pub controls: Vec<ControlInfo>,
}

/// Property value extracted from a Blob stream.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
#[non_exhaustive]
pub struct BlobProperty {
    pub prop_id: u16,
    pub value: BlobValue,
}

/// Properties for a single control, extracted from the Blob.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
#[non_exhaustive]
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

/// The folders of forms and of reports in the storage, in the order jetdb
/// looks in them.
const FORM_FOLDERS: [(&str, FormObjectType); 2] = [
    ("Forms", FormObjectType::Form),
    ("Reports", FormObjectType::Report),
];

/// [`list_forms`] in the storage already read.
pub fn list_forms_in(storage: &AccessStorage) -> Result<Vec<FormEntry>, FileError> {
    let entries = &storage.entries;
    let mut result = Vec::new();
    // Collect forms, then reports
    for (folder_name, object_type) in FORM_FOLDERS {
        let Some(folder) = storage::find_top_folder(entries, folder_name) else {
            continue;
        };
        for (name, _storage_num) in storage::folder_mapping(entries, folder)? {
            result.push(FormEntry { name, object_type });
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
    let names = blob_names(&storage.entries, name, object_type);
    let (controls, _) = type_info_named(&stream_data, storage.code_page, &names)?;

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
/// a text value by this table, a number or Yes/No value by
/// [`number_prop_id_name`], and bytes or a GUID by [`bytes_prop_id_name`].
///
/// Each name is the SaveAsText key, and the only one, that holds every value
/// of the ID in the forms and reports of Sports.accdb (ruddj/SportsAdmin) and
/// of the databases made by testdata/form-generators, in both formats.
pub fn prop_id_name(prop_id: u16) -> Option<&'static str> {
    name_in(TEXT_NAMES, prop_id)
}

/// The names of the text values, sorted by ID.
const TEXT_NAMES: &[(u16, &str)] = &[
    (0x0007, "Picture"),
    (0x0011, "Caption"),
    (0x0012, "ColumnWidths"),
    (0x0014, "Name"),
    (0x0016, "EventProcPrefix"),
    (0x0017, "DefaultValue"),
    (0x001B, "ControlSource"),
    (0x0022, "FontName"),
    (0x0026, "Format"),
    (0x0031, "LinkChildFields"),
    (0x0032, "LinkMasterFields"),
    (0x003D, "ValidationText"),
    (0x0048, "InputMask"),
    (0x004C, "OLEClass"),
    (0x004D, "OnOpen"),
    (0x004E, "OnClose"),
    (0x004F, "OnDelete"),
    (0x0050, "OnFormat"),
    (0x0052, "OnPrint"),
    (0x0053, "OnPage"),
    (0x0054, "OnRetreat"),
    (0x0055, "BeforeUpdate"),
    (0x0056, "AfterUpdate"),
    (0x0057, "OnCurrent"),
    (0x005B, "RowSource"),
    (0x005D, "RowSourceType"),
    (0x0068, "OnKeyDown"),
    (0x0069, "OnKeyUp"),
    (0x006A, "OnKeyPress"),
    (0x006B, "OnMouseDown"),
    (0x006C, "OnMouseUp"),
    (0x006D, "OnMouseMove"),
    (0x006E, "OnTimer"),
    (0x0070, "OnActivate"),
    (0x0071, "OnDeactivate"),
    (0x0072, "OnChange"),
    (0x0073, "OnGotFocus"),
    (0x0074, "OnLostFocus"),
    (0x0075, "OnResize"),
    (0x0076, "OnUpdated"),
    (0x0077, "OnError"),
    (0x0078, "OnNotInList"),
    (0x0079, "OnNoData"),
    (0x007A, "BeforeInsert"),
    (0x007B, "AfterInsert"),
    (0x007C, "BeforeDelConfirm"),
    (0x007D, "AfterDelConfirm"),
    (0x007E, "OnClick"),
    (0x007F, "OnLoad"),
    (0x0080, "OnUnload"),
    (0x0084, "SourceObject"),
    (0x0087, "StatusBarText"),
    (0x0091, "ValidationRule"),
    (0x009C, "RecordSource"),
    (0x00A0, "DatasheetFontName"),
    (0x00DA, "HelpFile"),
    (0x00DE, "OnEnter"),
    (0x00DF, "OnExit"),
    (0x00E0, "OnDblClick"),
    (0x00F0, "MenuBar"),
    (0x00F5, "Filter"),
    (0x010A, "Tag"),
    (0x0112, "Class"),
    (0x0130, "ShortcutMenuBar"),
    (0x013D, "ControlTipText"),
    (0x014B, "OrderBy"),
    (0x014D, "OnFilter"),
    (0x014E, "OnApplyFilter"),
    (0x0157, "HyperlinkAddress"),
    (0x015A, "ColumnInfo"),
    (0x015B, "HyperlinkSubAddress"),
    (0x0161, "Toolbar"),
    (0x0167, "FuriganaControl"),
    (0x0168, "PostalAddress"),
    (0x0177, "OnDirty"),
    (0x0194, "OnUndo"),
    (0x01A9, "OnConnect"),
    (0x01AA, "OnDisconnect"),
    (0x01AB, "OnPivotTableChange"),
    (0x01AC, "OnQuery"),
    (0x01AD, "BeforeQuery"),
    (0x01AE, "OnSelectionChange"),
    (0x01AF, "OnCmdBeforeExecute"),
    (0x01B0, "OnCmdChecked"),
    (0x01B1, "OnCmdEnabled"),
    (0x01B2, "OnCmdExecute"),
    (0x01B3, "OnDataSetChange"),
    (0x01B4, "BeforeScreenTip"),
    (0x01B5, "AfterFinalRender"),
    (0x01B6, "AfterRender"),
    (0x01B7, "AfterLayout"),
    (0x01B8, "BeforeRender"),
    (0x01B9, "OnMouseWheel"),
    (0x01BA, "OnViewChange"),
    (0x01BB, "OnDataChange"),
];

/// Return the known name of the property `prop_id` whose value is a number or
/// a Yes/No, with or without a value (NotDefault), or `None`.
///
/// Each name is the SaveAsText key, and the only one, that holds every value
/// of the ID, as for [`prop_id_name`]; a Yes/No without a value is held by a
/// key written as NotDefault in each block that has it. Only IDs up to 0x01C4
/// are named: Access for Microsoft 365 writes the IDs above, such as those of
/// the anchors and the layout, one higher than the Access that made
/// Sports.accdb.
pub fn number_prop_id_name(prop_id: u16) -> Option<&'static str> {
    name_in(NUMBER_NAMES, prop_id)
}

/// The names of the number and Yes/No values, sorted by ID.
const NUMBER_NAMES: &[(u16, &str)] = &[
    (0x0002, "AccessKey"),
    (0x0004, "SpecialEffect"),
    (0x0006, "AutoResize"),
    (0x0008, "BorderColor"),
    (0x0009, "BorderStyle"),
    (0x000A, "BorderWidth"),
    (0x000B, "BorderLineStyle"),
    (0x000C, "Bottom"),
    (0x000D, "BoundColumn"),
    (0x000E, "CanGrow"),
    (0x000F, "RepeatSection"),
    (0x0010, "CanShrink"),
    (0x0013, "RecordSelectors"),
    (0x0019, "Enabled"),
    (0x001C, "BackColor"),
    (0x001D, "BackStyle"),
    (0x001E, "AllowFilters"),
    (0x0021, "FontItalic"),
    (0x0023, "FontSize"),
    (0x0024, "FontUnderline"),
    (0x0025, "FontWeight"),
    (0x0029, "GridX"),
    (0x002A, "GridY"),
    (0x002C, "Height"),
    (0x002D, "AllowUpdating"),
    (0x002E, "Transparent"),
    (0x0036, "Left"),
    (0x0037, "LineSlant"),
    (0x0038, "Locked"),
    (0x003A, "OptionValue"),
    (0x003E, "Modal"),
    (0x0043, "LimitToList"),
    (0x0044, "AutoExpand"),
    (0x0045, "HideDuplicates"),
    (0x0046, "ColumnCount"),
    (0x0047, "DecimalPlaces"),
    (0x004B, "KeepTogether"),
    (0x0059, "SizeMode"),
    (0x005A, "PopUp"),
    (0x005C, "RowSourceTypeInt"),
    (0x005E, "MaxButton"),
    (0x005F, "MinButton"),
    (0x0061, "ControlBox"),
    (0x0065, "AutoActivate"),
    (0x006F, "TimerInterval"),
    (0x0081, "Right"),
    (0x0082, "RunningSum"),
    (0x0083, "ColumnHeads"),
    (0x0088, "TextAlign"),
    (0x008D, "Top"),
    (0x008F, "LayoutForPrint"),
    (0x0090, "FastLaserPrinting"),
    (0x0092, "ViewsAllowed"),
    (0x0093, "DefaultView"),
    (0x0094, "Visible"),
    (0x0095, "DisplayWhen"),
    (0x0096, "Width"),
    (0x0097, "ForceNewPage"),
    (0x0098, "ScrollBars"),
    (0x009A, "ListWidth"),
    (0x009D, "BreakLevel"),
    (0x009E, "ItemSuffix"),
    (0x009F, "OverlapFlags"),
    (0x00A1, "DatasheetFontHeight"),
    (0x00A2, "DatasheetFontWeight"),
    (0x00A5, "ColumnOrder"),
    (0x00A6, "ColumnWidth"),
    (0x00A7, "ColumnHidden"),
    (0x00A8, "RowHeight"),
    (0x00C2, "TabularCharSet"),
    (0x00CC, "ForeColor"),
    (0x00DB, "HelpContextId"),
    (0x00DC, "Default"),
    (0x00DD, "Cancel"),
    (0x00F3, "TextFontFamily"),
    (0x00F4, "TabularFamily"),
    (0x0105, "TabIndex"),
    (0x0106, "TabStop"),
    (0x0116, "AutoCenter"),
    (0x0117, "NavigationButtons"),
    (0x011A, "DateGrouping"),
    (0x0126, "TripleState"),
    (0x012F, "CloseButton"),
    (0x0132, "DatasheetForeColor"),
    (0x0135, "PictureAlignment"),
    (0x0139, "PictureTiling"),
    (0x013A, "DividingLines"),
    (0x013E, "DatasheetGridlinesBehavior"),
    (0x013F, "DatasheetGridlinesColor"),
    (0x0146, "AllowAdditions"),
    (0x0149, "OldBorderStyle"),
    (0x0150, "OrderByOn"),
    (0x0154, "AllowAutoCorrect"),
    (0x0155, "PictureType"),
    (0x015C, "MultiRow"),
    (0x015E, "TabFixedHeight"),
    (0x015F, "TabFixedWidth"),
    (0x0163, "Vertical"),
    (0x0165, "IMEHold"),
    (0x016C, "RecSrcDt"),
    (0x0170, "KeyboardLanguage"),
    (0x0175, "AllowDesignChanges"),
    (0x017B, "IMESentenceMode"),
    (0x0180, "LeftMargin"),
    (0x0181, "TopMargin"),
    (0x0182, "LineSpacing"),
    (0x0183, "SubdatasheetHeight"),
    (0x0184, "RightMargin"),
    (0x0185, "BottomMargin"),
    (0x018C, "IsHyperlink"),
    (0x01A6, "AllowDatasheetView"),
    (0x01BE, "AsianLineBreak"),
    (0x01C2, "GroupTable"),
    (0x01C4, "FilterOnLoad"),
];

/// Return the known name of the property `prop_id` whose value is bytes or a
/// GUID, or `None`: each is the SaveAsText key whose binary value holds the
/// same bytes, as for [`number_prop_id_name`].
pub fn bytes_prop_id_name(prop_id: u16) -> Option<&'static str> {
    name_in(BYTES_NAMES, prop_id)
}

/// The names of the bytes and GUID values, sorted by ID.
const BYTES_NAMES: &[(u16, &str)] = &[
    (0x0063, "ObjectPalette"),
    (0x0064, "PaintPalette"),
    (0x00AB, "OleData"),
    (0x00BC, "PictureData"),
    (0x00BD, "PrtMip"),
    (0x00BE, "PrtDevMode"),
    (0x00BF, "PrtDevNames"),
    (0x0178, "GUID"),
    (0x0179, "NameMap"),
];

/// The name of `prop_id` in `table`, sorted by ID.
fn name_in(table: &[(u16, &'static str)], prop_id: u16) -> Option<&'static str> {
    table
        .binary_search_by_key(&prop_id, |&(id, _)| id)
        .ok()
        .map(|i| table[i].1)
}

/// Return the known control type name for a given TypeInfo type_code, or `None`.
/// The control types jetdb knows by their codes, sorted by code: the codes of
/// TypeInfo for form controls (0x06xx to 0x24xx), report controls (0x1Bxx)
/// and sections (0x18xx, 0x19xx, 0x1Fxx), and the types in the Blob, of the
/// Access 2000 format and of the tree of later formats (0x0064 to 0x009E),
/// which are the acControlType of Access. 0x0099 and 0x009A are the header
/// and footer of a form or of a report, so not named here.
const CONTROL_TYPE_NAMES: &[(u16, &str)] = &[
    (0x0064, "Label"),
    (0x0065, "Rectangle"),
    (0x0066, "Line"),
    (0x0067, "Image"),
    (0x0068, "CommandButton"),
    (0x0069, "OptionButton"),
    (0x006A, "CheckBox"),
    (0x006B, "OptionGroup"),
    (0x006C, "BoundObjectFrame"),
    (0x006D, "TextBox"),
    (0x006E, "ListBox"),
    (0x006F, "ComboBox"),
    (0x0070, "SubForm"),
    (0x0072, "ObjectFrame"),
    (0x0076, "PageBreak"),
    (0x0077, "CustomControl"),
    (0x007A, "ToggleButton"),
    (0x007B, "TabControl"),
    (0x007C, "Page"),
    (0x0098, "Detail"),
    (0x009B, "PageHeader"),
    (0x009C, "PageFooter"),
    (0x009D, "GroupHeader"),
    (0x009E, "GroupFooter"),
    (0x066A, "CheckBox"),
    (0x0A7A, "ToggleButton"),
    (0x0B68, "CommandButton"),
    (0x0C64, "Label"),
    (0x0D64, "Label"),
    (0x0E65, "Rectangle"),
    (0x0F67, "Image"),
    (0x126D, "TextBox"),
    (0x136F, "ComboBox"),
    (0x1470, "SubForm"),
    (0x1666, "Line"),
    (0x1898, "Detail"),
    (0x1899, "FormHeader"),
    (0x189A, "FormFooter"),
    (0x1998, "Detail"),
    (0x1999, "ReportHeader"),
    (0x199A, "ReportFooter"),
    (0x199D, "GroupHeader"),
    (0x199E, "GroupFooter"),
    (0x1B64, "Label"),
    (0x1B65, "Rectangle"),
    (0x1B66, "Line"),
    (0x1B67, "Image"),
    (0x1B68, "CommandButton"),
    (0x1B6A, "CheckBox"),
    (0x1B6D, "TextBox"),
    (0x1B6F, "ComboBox"),
    (0x1B70, "SubReport"),
    (0x1F9B, "PageHeader"),
    (0x1F9C, "PageFooter"),
    (0x247F, "EmptyCell"),
];

/// Return the known control type name for a given TypeInfo type_code, or `None`.
pub fn control_type_name(type_code: u16) -> Option<&'static str> {
    name_in(CONTROL_TYPE_NAMES, type_code)
}

/// The known control type name for `type_code`, or the code as `0x....` for
/// a type jetdb does not know, as `jetdb form controls` shows it.
pub fn control_type_label(type_code: u16) -> String {
    control_type_name(type_code).map_or_else(|| format!("0x{type_code:04X}"), str::to_string)
}

impl BlobProperty {
    /// Return the known property name, or `None` for unknown IDs: a text
    /// value is named by [`prop_id_name`], a number or Yes/No value by
    /// [`number_prop_id_name`], and bytes or a GUID by [`bytes_prop_id_name`].
    pub fn name(&self) -> Option<&'static str> {
        match self.value {
            BlobValue::Text(_) => prop_id_name(self.prop_id),
            BlobValue::Bool(_)
            | BlobValue::NotDefault
            | BlobValue::Short(_)
            | BlobValue::Long(_)
            | BlobValue::Double(_) => number_prop_id_name(self.prop_id),
            BlobValue::Binary(_) | BlobValue::Guid(_) => bytes_prop_id_name(self.prop_id),
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
        let (properties, controls) = crate::form2000::read(&blob_data).unwrap_or_else(|| {
            log::warn!(
                "{name}: the properties and controls in the Access 2000 format could not be read"
            );
            Default::default()
        });
        return Ok(FormProperties {
            form_name: name.to_string(),
            object_type,
            properties,
            controls,
        });
    }

    let (form_props, control_prop_groups) = parse_blob(&blob_data)?;
    let names: Vec<String> = control_prop_groups
        .iter()
        .filter_map(|(_, props)| name_in_blob(props))
        .collect();

    let type_info_controls = type_info_controls(storage, name, object_type, &names);
    Ok(FormProperties {
        form_name: name.to_string(),
        object_type,
        properties: form_props,
        controls: merge_controls(control_prop_groups, &type_info_controls),
    })
}

/// The controls of the TypeInfo of the form or report `name`, named by the
/// `names` in its Blob; none when it has no TypeInfo that can be read. It is
/// that of the object the Blob is of, even when a report shares the name of a
/// form.
fn type_info_controls(
    storage: &AccessStorage,
    name: &str,
    object_type: FormObjectType,
    names: &[String],
) -> Vec<ControlInfo> {
    match find_stream(
        &storage.entries,
        name,
        Some(object_type),
        StreamKind::TypeInfo,
    ) {
        Ok((_, ti_data)) => type_info_named(&ti_data, storage.code_page, names)
            .map(|(controls, _)| controls)
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// The controls of the Blob, each named by its Name property and typed by
/// the control of the same name in TypeInfo, or else by its type in the Blob.
/// Not by index: TypeInfo can list the controls in another order.
fn merge_controls(
    control_prop_groups: Vec<BlobControl>,
    type_info_controls: &[ControlInfo],
) -> Vec<ControlProperties> {
    control_prop_groups
        .into_iter()
        .enumerate()
        .map(|(i, (blob_type, props))| {
            let blob_name = name_in_blob(&props).unwrap_or_else(|| format!("Control_{i}"));
            let type_code = type_info_by_name(type_info_controls, &blob_name)
                .map_or(blob_type, |c| c.type_code);
            ControlProperties {
                name: blob_name,
                type_code,
                properties: props,
            }
        })
        .collect()
}

/// The name of a control of the Blob, its Name property (0x14).
fn name_in_blob(props: &[BlobProperty]) -> Option<String> {
    props
        .iter()
        .find(|p| p.prop_id == 0x0014)
        .and_then(|p| match &p.value {
            BlobValue::Text(s) => Some(s.clone()),
            _ => None,
        })
}

/// The names of the controls of the Blob of the object `name` of
/// `object_type`, empty when it has none that can be read.
fn blob_names(
    entries: &[storage::StorageEntry],
    name: &str,
    object_type: FormObjectType,
) -> Vec<String> {
    let Ok((_, blob)) = find_stream(entries, name, Some(object_type), StreamKind::Blob) else {
        return Vec::new();
    };
    if blob.first() == Some(&crate::form2000::VERSION) {
        let controls = crate::form2000::read(&blob).map_or_else(Vec::new, |(_, c)| c);
        return controls.into_iter().map(|c| c.name).collect();
    }
    parse_blob(&blob).map_or_else(
        |_| Vec::new(),
        |(_, groups)| groups.iter().filter_map(|(_, p)| name_in_blob(p)).collect(),
    )
}

/// The code pages of Windows a name of TypeInfo can be written in, after
/// that of the header of the database.
const ANSI_CODE_PAGES: [u16; 14] = [
    874, 932, 936, 949, 950, 1250, 1251, 1252, 1253, 1254, 1255, 1256, 1257, 1258,
];

/// The controls of the TypeInfo `data`, and the code page their names are
/// read in. The names are bytes in the code page of the Access that saved
/// the object, which the file does not tell: the Russian names of
/// Strings.mdb are in 1251, while its header has 1252. They are read in the
/// code page, of `code_page` (that of the header) and then
/// [`ANSI_CODE_PAGES`], in which the most names that are not ASCII are among
/// `names`, those of the Blob, regardless of case; in `code_page` when none
/// is.
fn type_info_named(
    data: &[u8],
    code_page: u16,
    names: &[String],
) -> Result<(Vec<ControlInfo>, u16), FileError> {
    let names: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let matched = |controls: &[ControlInfo]| {
        controls
            .iter()
            .filter(|c| !c.name.is_ascii() && names.contains(&c.name.to_lowercase()))
            .count()
    };
    let mut best = (parse_type_info(data, code_page)?, code_page);
    let mut most = matched(&best.0);
    for cp in ANSI_CODE_PAGES {
        let controls = parse_type_info(data, cp)?;
        let n = matched(&controls);
        if n > most {
            most = n;
            best = (controls, cp);
        }
    }
    Ok(best)
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
    // Try Forms first, then Reports
    for (folder_name, obj_type) in FORM_FOLDERS {
        if object_type.is_some_and(|t| t != obj_type) {
            continue;
        }
        let Some(folder) = storage::find_top_folder(entries, folder_name) else {
            continue;
        };
        let stream_name = stream_kind.storage_name();
        if let Some(stream) = storage::object_stream(entries, folder, name, stream_name)? {
            return Ok((obj_type, stream.data.clone()));
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

/// The type of an ActiveX control (`acCustomControl`), in the low byte of
/// its type code.
const ACTIVEX: u16 = 0x77;

/// The bytes the entry of an ActiveX control has after its names: the class
/// ID and the ID of the type library of the control (16 bytes each), and 4
/// bytes. The test data has the spin button of Microsoft Forms 2.0 only.
const ACTIVEX_TAIL: usize = 36;

/// Parse TypeInfo binary into a list of control entries.
///
/// Format:
/// - Header (32 bytes): magic(u32) + field1(u32) + field2(i32) + count(u32) + GUID(16)
/// - Entries: ctrl_type(u16) + padding(u16) + index(u32) + name(NUL) + align(0x00)
///   (see [`ACTIVEX_TAIL`] for the bytes an ActiveX control has after them)
///
/// The names are bytes in a code page, read in `code_page` (see
/// [`type_info_named`]).
fn parse_type_info(data: &[u8], code_page: u16) -> Result<Vec<ControlInfo>, FileError> {
    let entry_count = type_info_count(data)?;
    let mut controls = Vec::with_capacity(entry_count);
    let mut pos = 32; // skip header
    for _ in 0..entry_count {
        let Some((entry, next)) = type_info_entry(data, pos, code_page) else {
            break;
        };
        pos = next;
        // 0x1EFF: internal metadata entries, not real controls
        if entry.type_code != 0x1EFF {
            controls.push(entry);
        }
    }
    Ok(controls)
}

/// The number of entries the header of the TypeInfo `data` gives, after
/// checking its size and magic number.
fn type_info_count(data: &[u8]) -> Result<usize, FileError> {
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
    Ok(entry_count)
}

/// The entry of TypeInfo at `pos`, and where the next one starts; `None`
/// when the data has no room for one.
fn type_info_entry(data: &[u8], pos: usize, code_page: u16) -> Option<(ControlInfo, usize)> {
    let head = data.get(pos..pos.checked_add(8)?)?;
    let type_code = u16::from_le_bytes([head[0], head[1]]);
    // skip padding u16 at 2..4
    let index = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);

    // Each entry has two NUL-terminated strings:
    //   1. EventProcPrefix (or Name if no special characters)
    //   2. RealName (actual name when it contains parentheses, else empty)
    let (epp_bytes, next) = nul_terminated(data, pos + 8);
    let (real_bytes, mut next) = nul_terminated(data, next);
    if type_code & 0xFF == ACTIVEX {
        next = (next + ACTIVEX_TAIL).min(data.len());
    }

    // Use RealName if present, otherwise EventProcPrefix
    let name_bytes = if real_bytes.is_empty() {
        epp_bytes
    } else {
        real_bytes
    };
    let name = encoding::decode_code_page(name_bytes, code_page);
    let entry = ControlInfo {
        name,
        type_code,
        index,
    };
    Some((entry, next))
}

/// The bytes of `data` from `start` up to a NUL, or to its end, and where
/// the reading goes on after the NUL.
fn nul_terminated(data: &[u8], start: usize) -> (&[u8], usize) {
    let rest = &data[start..];
    let len = rest.iter().position(|&b| b == 0x00).unwrap_or(rest.len());
    (&rest[..len], (start + len + 1).min(data.len()))
}

// ---------------------------------------------------------------------------
// Internal: Blob parser
// ---------------------------------------------------------------------------

/// The properties of a section or control: its type (0 when it is not
/// known), and its properties.
type BlobControl = (u16, Vec<BlobProperty>);

/// Parse the Blob binary into the properties of the form or report itself
/// and of its sections and controls.
///
/// The Blob keeps the design as a tree (see [`parse_nodes`]): the group of
/// entries of the form or report itself (see [`parse_entry`]), then its
/// default controls and its sections, each holding its controls. When the
/// bytes do not read to the end as such a tree, each control is found by its
/// Name entry instead, and only its properties from the name on are read.
fn parse_blob(data: &[u8]) -> Result<(Vec<BlobProperty>, Vec<BlobControl>), FileError> {
    if data.len() < 14 {
        return Ok((Vec::new(), Vec::new()));
    }

    // Skip 8-byte blob header + 6-byte section preamble.
    let (all_props, pos, end) = parse_group(data, 14, data.len());

    if let Some(controls) = end.and_then(|end| controls_of_tree(data, pos, end)) {
        return Ok((all_props, controls));
    }
    let controls = scan_control_sections(data, pos)
        .into_iter()
        .map(|props| (0, props))
        .collect();
    Ok((all_props, controls))
}

/// The types of sections: the detail section, and the headers and footers
/// of the form or report, of its pages and of its groups.
const SECTIONS: std::ops::RangeInclusive<u16> = 0x98..=0x9E;

/// A node of the tree a Blob keeps: a default control, a section or a
/// control, with the nodes in it.
struct BlobNode {
    /// The `acControlType` of Access, such as 109 for a text box.
    type_code: u16,
    properties: Vec<BlobProperty>,
    children: Vec<BlobNode>,
}

/// The sections and controls of the tree from `pos`, which follows a group
/// that ended with `end`, in the order of the Blob: each section followed by
/// its controls. `None` when the bytes do not read to the end as a tree.
fn controls_of_tree(data: &[u8], mut pos: usize, end: u32) -> Option<Vec<BlobControl>> {
    let nodes = parse_nodes(data, &mut pos, end)?;
    if pos != data.len() {
        return None;
    }
    let mut controls = Vec::new();
    // The nodes at the top are the default controls and the sections.
    for section in nodes
        .into_iter()
        .filter(|n| SECTIONS.contains(&n.type_code) || !n.children.is_empty())
    {
        push_with_children(section, &mut controls);
    }
    Some(controls)
}

/// Adds `node`, and the nodes in it, to `out`.
fn push_with_children(node: BlobNode, out: &mut Vec<BlobControl>) {
    out.push((node.type_code, node.properties));
    for child in node.children {
        push_with_children(child, out);
    }
}

/// The nodes that follow a group that ended with `end`, read from `pos`: none
/// for 253, one for 254, and a count (u16) of them for 255. Each node is its
/// type (u16), 4 bytes jetdb does not use, a group of entries, and the nodes
/// that follow it: the controls of a section, the pages of a tab control.
fn parse_nodes(data: &[u8], pos: &mut usize, end: u32) -> Option<Vec<BlobNode>> {
    let count = match end {
        253 => 0,
        254 => 1,
        _ => {
            let b = data.get(*pos..*pos + 2)?;
            *pos += 2;
            u16::from_le_bytes([b[0], b[1]])
        }
    };
    // The count comes from the file: grow as nodes are read.
    let mut nodes = Vec::new();
    for _ in 0..count {
        nodes.push(parse_node(data, pos)?);
    }
    Some(nodes)
}

fn parse_node(data: &[u8], pos: &mut usize) -> Option<BlobNode> {
    let b = data.get(*pos..*pos + 6)?;
    let type_code = u16::from_le_bytes([b[0], b[1]]);
    let (properties, next, end) = parse_group(data, *pos + 6, data.len());
    *pos = next;
    let children = parse_nodes(data, pos, end?)?;
    Some(BlobNode {
        type_code,
        properties,
        children,
    })
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

/// The trailers that end a group of entries, which tell the nodes that
/// follow it (see [`parse_nodes`]): 253 none, 254 one, 255 a count of them.
const GROUP_END: std::ops::RangeInclusive<u32> = 253..=255;

/// The entries of the group from `start`, which ends by `end`: through the
/// entry whose trailer ends the group (see [`GROUP_END`]), or up to the
/// bytes that are not an entry. Returns them, where the reading stopped, and
/// the trailer that ended the group, `None` when it did not end.
fn parse_group(data: &[u8], start: usize, end: usize) -> (Vec<BlobProperty>, usize, Option<u32>) {
    let mut props = Vec::new();
    let mut pos = start;
    while let Some((property, next, last)) = parse_entry(data, pos, end) {
        props.push(property);
        pos = next;
        if last.is_some() {
            return (props, pos, last);
        }
    }
    (props, pos, None)
}

/// The property entry at `pos`, which ends by `end`, where the next entry
/// starts, and its trailer when it ends its group; `None` when there is no
/// entry there.
///
/// An entry is reverse-engineered as `prop_id(u16) + type(u32) + B(u32) +
/// C(u32)`, then C bytes of value and a 4-byte trailer. The type tells how
/// to read the value (see [`entry_value`]); B is not used. The trailer grows
/// from one entry to the next, and is one of [`GROUP_END`] for the last entry
/// of a group.
fn parse_entry(data: &[u8], pos: usize, end: usize) -> Option<(BlobProperty, usize, Option<u32>)> {
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
    let last = GROUP_END.contains(&trailer).then_some(trailer);
    Some((property, data_end + 4, last))
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
        0x09 if bytes.len() == 16 => BlobValue::Guid(crate::data::format_guid(bytes)),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_as_text;
    use std::collections::HashMap;

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
    fn type_info_reads_on_past_an_activex_control() {
        // The form Hyphenate of Strings.mdb has spin buttons of Microsoft
        // Forms 2.0, whose entries have 36 bytes after their names: each
        // entry after them has the type of the control of its name in the
        // tree of the Blob.
        let path = skip_if_missing!("saveastext/Strings/Strings.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let info =
            read_form_type_info_of_type(&mut reader, "Hyphenate", FormObjectType::Form).unwrap();
        let read =
            read_form_properties_of_type(&mut reader, "Hyphenate", FormObjectType::Form).unwrap();
        let tree: HashMap<&str, u16> = read
            .controls
            .iter()
            .map(|c| (c.name.as_str(), c.type_code))
            .collect();
        let ascii: Vec<&ControlInfo> = info.controls.iter().filter(|c| c.name.is_ascii()).collect();
        assert!(ascii.len() >= 20, "{}", ascii.len());
        for c in ascii {
            assert_eq!(tree.get(c.name.as_str()), Some(&c.type_code), "{}", c.name);
        }
        // As many ActiveX controls as the tree has, past which it reads.
        let activex = |code: u16| code & 0xFF == ACTIVEX;
        let in_tree = read
            .controls
            .iter()
            .filter(|c| activex(c.type_code))
            .count();
        assert!(in_tree > 0);
        let listed = info
            .controls
            .iter()
            .filter(|c| activex(c.type_code))
            .count();
        assert_eq!(listed, in_tree);
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

        // The header says 1252, and the Blob names the control in another
        // case: 1251 is the code page in which the name is among the names.
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (controls, code_page) =
            type_info_named(&type_info(&russian), 1252, &names(&["ЗАГОЛОВОК"])).unwrap();
        assert_eq!((controls[0].name.as_str(), code_page), ("Заголовок", 1251));
        // No name among them, or no name but ASCII: the code page of the header.
        let (_, code_page) =
            type_info_named(&type_info(&russian), 1252, &names(&["Detail"])).unwrap();
        assert_eq!(code_page, 1252);
        let (_, code_page) = type_info_named(&type_info(b"Detail"), 1252, &[]).unwrap();
        assert_eq!(code_page, 1252);
    }

    #[test]
    fn type_info_names_are_those_of_the_blob_in_another_code_page() {
        // The header of Strings.mdb has 1252, and its forms name controls in
        // Russian, in 1251.
        let path = skip_if_missing!("saveastext/Strings/Strings.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let mut russian = 0;
        for entry in list_forms(&mut reader).unwrap() {
            let ti =
                read_form_type_info_of_type(&mut reader, &entry.name, entry.object_type).unwrap();
            let read =
                read_form_properties_of_type(&mut reader, &entry.name, entry.object_type).unwrap();
            for c in &ti.controls {
                let name = c.name.to_lowercase();
                assert!(
                    read.controls.iter().any(|b| b.name.to_lowercase() == name),
                    "{} {}",
                    entry.name,
                    c.name
                );
                russian += usize::from(!c.name.is_ascii());
            }
        }
        assert!(russian >= 100, "{russian}");
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

    #[test]
    fn every_blob_of_access_2002_and_later_reads_to_the_end_as_a_tree() {
        for (file, count) in [
            ("vbaV2003.mdb", 1),
            ("formPropTest.accdb", 4),
            ("jetdb-sample.accdb", 1),
            ("V2003/macroGeneratedTestV2003.mdb", 2),
            ("V2007/sameNameTestV2007.accdb", 2),
            ("V2007/propertyIdTestV2007.accdb", 106),
            ("V2007/controlPropertyTestV2007.accdb", 96),
            ("V2010/macroGeneratedTestV2010.accdb", 2),
            ("saveastext/SportsAdmin/Sports.accdb", 116),
            ("saveastext/Strings/Strings.mdb", 13),
        ] {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let mut found = 0;
            for entry in list_forms(&mut reader).unwrap() {
                let data = read_form_stream_of_type(
                    &mut reader,
                    &entry.name,
                    entry.object_type,
                    StreamKind::Blob,
                )
                .unwrap()
                .data;
                if data.first() == Some(&crate::form2000::VERSION) {
                    continue;
                }
                found += 1;
                let (_, pos, end) = parse_group(&data, 14, data.len());
                let tree = end.and_then(|end| controls_of_tree(&data, pos, end));
                assert!(tree.is_some(), "{file}: {}", entry.name);
            }
            assert_eq!(found, count, "{file}");
        }
    }

    /// The combo boxes of Sports.accdb whose block in its SaveAsText output
    /// also holds the lines of the control after them: in the files of
    /// ruddj/SportsAdmin, the `End` of the combo box and the `Begin` of the
    /// control are missing. Neither control has a block of its own to be
    /// checked against.
    const SPORTS_BROKEN: [(&str, &str, &str); 3] = [
        ("EnterCompetitors", "FinalStatus", "HeatComplete"),
        ("EventRecordsSF", "Field4", "Date"),
        ("House Summary", "HT_Code", "Text15"),
    ];

    /// The properties the SaveAsText output of Sports.accdb does not write,
    /// with the keys Access for Microsoft 365 writes them under: the right
    /// and bottom of a report, the GUIDs, and the printer settings. The IDs
    /// above 0x01C4 are those of the Access that made Sports.accdb, one
    /// lower than those of Access for Microsoft 365.
    const SPORTS_UNWRITTEN: [(u16, &str); 7] = [
        (0x0081, "Right"),
        (0x000C, "Bottom"),
        (0x0178, "GUID"),
        (0x00BE, "PrtDevMode"),
        (0x00BF, "PrtDevNames"),
        (0x02A6, "PrtDevModeW"),
        (0x02A7, "PrtDevNamesW"),
    ];

    /// SaveAsText outputs by whether they are of a report, and by name.
    type SaveAsTextFiles = HashMap<(bool, String), String>;

    /// The SaveAsText output of the forms and reports read by `reader`, in
    /// the files `forms/<name>.<extension>` and `reports/<name>.<extension>`
    /// of `dir`, read by `decode`.
    fn save_as_text_files(
        reader: &mut PageReader,
        dir: &std::path::Path,
        extension: &str,
        decode: fn(Vec<u8>) -> String,
    ) -> SaveAsTextFiles {
        let mut texts = HashMap::new();
        for entry in list_forms(reader).unwrap() {
            let report = entry.object_type == FormObjectType::Report;
            let file = dir
                .join(if report { "reports" } else { "forms" })
                .join(format!("{}.{extension}", entry.name));
            let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{file:?}: {e}"));
            texts.insert((report, entry.name), decode(bytes));
        }
        texts
    }

    /// The SaveAsText output a database made by a generator of
    /// testdata/form-generators keeps in its table tblGenExport.
    fn generated_save_as_text(reader: &mut PageReader) -> SaveAsTextFiles {
        let catalog = crate::catalog::read_catalog(reader).unwrap();
        let entry = crate::catalog::find_table(&catalog, "tblGenExport").unwrap();
        let tdef = crate::table::read_table_def(reader, &entry.name, entry.table_page).unwrap();
        let column = |name: &str| tdef.columns.iter().position(|c| c.name == name).unwrap();
        let (kind, name, content) = (column("Kind"), column("ObjName"), column("Content"));
        let text = |v: &crate::data::Value| match v {
            crate::data::Value::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        };
        let rows = crate::data::read_table_rows(reader, &tdef).unwrap().rows;
        rows.iter()
            .map(|row| {
                (
                    (text(&row[kind]) == "report", text(&row[name])),
                    text(&row[content]),
                )
            })
            .collect()
    }

    /// Checks the forms and reports read by `reader` against `texts`, their
    /// SaveAsText output, leaving out the controls of `broken`, after
    /// checking that each pair of them has one block.
    fn check_save_as_text(
        reader: &mut PageReader,
        texts: &SaveAsTextFiles,
        broken: &[(&str, &str, &str)],
        unwritten: &'static [(u16, &'static str)],
    ) -> save_as_text::Checked {
        let mut checked = save_as_text::Checked {
            unwritten,
            ..Default::default()
        };
        for entry in list_forms(reader).unwrap() {
            let report = entry.object_type == FormObjectType::Report;
            let saved = save_as_text::read(&texts[&(report, entry.name.clone())]);
            let mut skipped = Vec::new();
            for (_, combo, next) in broken.iter().filter(|(o, ..)| *o == entry.name) {
                assert!(!saved.contains_key(*next), "{next}");
                assert_eq!(saved[*combo]["Name"].len(), 2, "{combo}");
                skipped.extend([*combo, *next]);
            }
            let read =
                read_form_properties_of_type(reader, &entry.name, entry.object_type).unwrap();
            checked.object(&saved, &read, &skipped);
        }
        checked
    }

    #[test]
    fn every_value_is_one_save_as_text_writes() {
        // Each value jetdb reads of each form and report, of its sections and
        // of its controls is one Access's SaveAsText output writes in the
        // block of the same object, section or control; the blocks and the
        // controls are the same, each control given once, with as many
        // NotDefault values; an ID given twice in a block has the same value;
        // and over the files, every value of a property is under one key,
        // which no other ID holds alone, and a named property is under its
        // name and under no other key. The ID of a property is the same in
        // each file through 0x01C4, and above it in the files of one version
        // of Access: Sports.accdb, and the four made by Access for Microsoft
        // 365. The values SaveAsText does not write are left out (see
        // save_as_text).
        //
        // What this does not show: which property an ID without a name is,
        // where its values fit several keys, as NotDefault values do; jetdb
        // gives such an ID by its number.
        let files = [
            ("saveastext/SportsAdmin/Sports.accdb", 28328),
            ("V2007/controlPropertyTestV2007.accdb", 22074),
            ("V2007/propertyIdTestV2007.accdb", 9217),
            ("V2000/controlPropertyTestV2000.mdb", 13322),
            ("V2000/propertyIdTestV2000.mdb", 5200),
        ];
        let mut all = Vec::new();
        for (file, at_least) in files {
            let path = skip_if_missing!(file);
            let observations = checked_values(file, &path);
            assert!(observations.len() >= at_least, "{file}");
            all.push(observations);
        }
        check_agreement(&all);
    }

    /// The values of the forms and reports of `file`, at `path`, after
    /// checking each against its SaveAsText output: the files of
    /// Sports.accdb, testdata/V2000/saveastext for the databases of the Access
    /// 2000 format, which Access writes in Shift_JIS, and the table
    /// tblGenExport for the others.
    fn checked_values(file: &str, path: &std::path::Path) -> Vec<save_as_text::Observation> {
        let mut reader = PageReader::open(path).unwrap();
        let dir = path.parent().unwrap();
        let (texts, broken, unwritten): (_, &[_], &'static [_]) = if file.starts_with("saveastext/")
        {
            let utf8 = |bytes| String::from_utf8(bytes).unwrap();
            let texts = save_as_text_files(&mut reader, dir, "bas", utf8);
            (texts, &SPORTS_BROKEN, &SPORTS_UNWRITTEN)
        } else if file.starts_with("V2000/") {
            let dir = dir.join("saveastext").join(path.file_stem().unwrap());
            let shift_jis = |bytes: Vec<u8>| crate::encoding::decode_code_page(&bytes, 932);
            (
                save_as_text_files(&mut reader, &dir, "txt", shift_jis),
                &[],
                &[],
            )
        } else {
            (generated_save_as_text(&mut reader), &[], &[])
        };
        let checked = check_save_as_text(&mut reader, &texts, broken, unwritten);
        assert_eq!(checked.unmatched, Vec::<String>::new(), "{file}");
        assert_eq!(checked.problems, Vec::<String>::new(), "{file}");
        checked.observations
    }

    /// Checks the values of the files, those of Sports.accdb first in
    /// `all`, against each other: in each version of Access, and through
    /// 0x01C4 over them all.
    fn check_agreement(all: &[Vec<save_as_text::Observation>]) {
        let (sports, later) = all.split_first().unwrap();
        let mut by_version = [
            save_as_text::Agreement::default(),
            save_as_text::Agreement::default(),
        ];
        by_version[0].add(sports);
        by_version[1].add(later.iter().flatten());
        let mut low = save_as_text::Agreement::default();
        low.add(
            all.iter()
                .flatten()
                .filter(|o| o.key.0 <= 0x01C4 && o.key.2 == 0),
        );
        for agreement in by_version.iter().chain([&low]) {
            assert_eq!(agreement.disagreements(), Vec::<String>::new());
            assert_eq!(agreement.shared(), Vec::<String>::new());
        }
        assert_eq!(low.misnamed(), Vec::<String>::new());
        // Access for Microsoft 365 writes what the SaveAsText output of
        // Sports.accdb does not, under the keys left out for it.
        for (id, key) in SPORTS_UNWRITTEN {
            let id = if id > 0x01C4 { id + 1 } else { id };
            assert!(by_version[1].holds(id, key), "{id:#06x} {key}");
        }
        assert!(low.named() >= 217, "{}", low.named());
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
    fn controls_of_an_empty_type_info_have_the_types_of_the_tree() {
        // Made by VBA, the report Shared has an empty TypeInfo; its sections
        // and controls have the types in the tree of its Blob.
        let path = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let read =
            read_form_properties_of_type(&mut reader, "Shared", FormObjectType::Report).unwrap();
        let controls: Vec<(&str, u16)> = read
            .controls
            .iter()
            .map(|c| (c.name.as_str(), c.type_code))
            .collect();
        assert_eq!(
            controls,
            [
                ("ページヘッダーセクション", 0x9B),
                ("詳細", 0x98),
                ("lblReportOnly", 0x64),
                ("ページフッターセクション", 0x9C),
            ]
        );
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
            BlobValue::Guid(s) => assert_eq!(s, "{8D64A650-62E7-4903-9733-0D8CE84978BF}"),
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
        // A Yes/No without a value, a double, and bytes are named too.
        assert_eq!(
            property(0x013A, BlobValue::NotDefault).name(),
            Some("DividingLines")
        );
        assert_eq!(
            property(0x016C, BlobValue::Double(36450.5)).name(),
            Some("RecSrcDt")
        );
        assert_eq!(
            property(0x0178, BlobValue::Guid(String::new())).name(),
            Some("GUID")
        );
        assert_eq!(
            property(0x0036, BlobValue::Text("x".to_string())).name(),
            None
        );
    }

    #[test]
    fn name_tables_are_sorted_by_id_without_repeats() {
        // name_in looks names up by binary search.
        for table in [TEXT_NAMES, NUMBER_NAMES, BYTES_NAMES, CONTROL_TYPE_NAMES] {
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
        }
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
    fn control_type_name_types_in_the_blob() {
        // The low byte of the type in TypeInfo: an ActiveX control
        // (Forms.SpinButton.1 in Strings.mdb), and the sections of pages and
        // groups. The header and footer of a form or report are not named.
        for (code, expected) in [
            (0x0072, Some("ObjectFrame")),
            (0x0077, Some("CustomControl")),
            (0x007C, Some("Page")),
            (0x0098, Some("Detail")),
            (0x009B, Some("PageHeader")),
            (0x009C, Some("PageFooter")),
            (0x009D, Some("GroupHeader")),
            (0x009E, Some("GroupFooter")),
            (0x0099, None),
            (0x009A, None),
        ] {
            assert_eq!(control_type_name(code), expected, "0x{code:04X}");
        }
    }

    #[test]
    fn control_type_label_names_or_numbers() {
        assert_eq!(control_type_label(0x126D), "TextBox");
        assert_eq!(control_type_label(0x0099), "0x0099");
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
        let mut reader = SaveAsTextReader {
            saved: Saved {
                properties: HashMap::new(),
                objects: Vec::new(),
            },
            blocks: Vec::new(),
            last_key: None,
        };
        for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
            reader.read_line(line);
        }
        reader.saved
    }

    /// SaveAsText output read so far, line by line.
    struct SaveAsTextReader {
        saved: Saved,
        /// The open blocks: `Begin <Type>`, or `<Key> = Begin` for binary data.
        blocks: Vec<(String, HashMap<String, String>)>,
        /// The key of the last string, which a quoted line of its own goes on.
        last_key: Option<String>,
    }

    impl SaveAsTextReader {
        /// The properties of the innermost open block: those of the form or
        /// report itself in the outermost, and none outside any block.
        fn properties(&mut self) -> Option<&mut HashMap<String, String>> {
            match self.blocks.len() {
                0 => None,
                1 => Some(&mut self.saved.properties),
                _ => self.blocks.last_mut().map(|(_, p)| p),
            }
        }

        fn read_line(&mut self, line: &str) {
            if let (Some(key), Some(more)) = (self.last_key.clone(), saved_string(line)) {
                if let Some(properties) = self.properties() {
                    properties.get_mut(&key).unwrap().push_str(&more);
                    return;
                }
            }
            self.last_key = None;
            if line == "End" {
                self.end_block();
            } else if line == "Begin" || line.starts_with("Begin ") {
                let type_name = line["Begin".len()..].trim().to_string();
                self.blocks.push((type_name, HashMap::new()));
            } else if line.ends_with(" = Begin") {
                self.blocks.push((String::new(), HashMap::new()));
            } else if let Some((key, value)) = line.split_once(" =") {
                self.property(key, value);
            }
        }

        /// Close the innermost block, which is an object when it has a name.
        fn end_block(&mut self) {
            if let Some((type_name, properties)) = self.blocks.pop() {
                if let Some(name) = properties.get("Name").cloned() {
                    self.saved.objects.push(SavedObject {
                        name,
                        type_name,
                        properties,
                    });
                }
            }
        }

        /// Keep the property `key` of the innermost block, whose value is a
        /// string that later lines may go on, or a value as it is written.
        fn property(&mut self, key: &str, value: &str) {
            let string = saved_string(value);
            let Some(properties) = self.properties() else {
                return;
            };
            match string {
                Some(string) => {
                    properties.insert(key.to_string(), string);
                    self.last_key = Some(key.to_string());
                }
                None => {
                    properties.insert(key.to_string(), value.trim().to_string());
                }
            }
        }
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

    /// The Caption of a form or report.
    fn caption(p: &FormProperties) -> Option<String> {
        p.properties
            .iter()
            .find(|x| x.name() == Some("Caption"))
            .map(|x| x.value.to_string())
    }

    fn has_control(p: &FormProperties, name: &str) -> bool {
        p.controls.iter().any(|c| c.name == name)
    }

    #[test]
    fn form_and_report_of_the_same_name() {
        // Made by testdata/form-generators/same-name-generator.bas: a form and
        // a report both named Shared, each with a caption and a control of
        // its own.
        let path = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();

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
    }

    #[test]
    fn streams_of_a_form_and_report_of_the_same_name() {
        let path = skip_if_missing!("V2007/sameNameTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let mut stream = |object_type, kind| {
            read_form_stream_of_type(&mut reader, "Shared", object_type, kind).unwrap()
        };

        // The TypeInfo of the report. (Objects made with CreateForm and
        // CreateReport have an empty TypeInfo, so only which object it is of
        // is checked; the Blob below shows the two apart.)
        let report_type_info = stream(FormObjectType::Report, StreamKind::TypeInfo);
        assert_eq!(report_type_info.object_type, FormObjectType::Report);

        let form_blob = stream(FormObjectType::Form, StreamKind::Blob);
        let report_blob = stream(FormObjectType::Report, StreamKind::Blob);
        assert_eq!(report_blob.object_type, FormObjectType::Report);
        assert_ne!(form_blob.data, report_blob.data);
    }

    #[test]
    fn a_type_that_the_name_does_not_have_is_not_found() {
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

    /// The properties compared one by one with SaveAsText.
    const SAVED_KEYS: [&str; 3] = ["RecordSource", "Caption", "ControlSource"];

    fn property_value(properties: &[BlobProperty], key: &str) -> Option<String> {
        properties
            .iter()
            .find(|p| p.name() == Some(key))
            .map(|p| p.value.to_string())
    }

    /// Checks that every property jetdb names holds the value Access saved
    /// under that name, and gives how many were checked. SaveAsText writes the
    /// right and bottom of a form only, so `report` passes them over.
    fn check_named(
        properties: &[BlobProperty],
        saved: &HashMap<String, String>,
        at: &str,
        report: bool,
    ) -> usize {
        let mut named = 0;
        for p in properties {
            // Values SaveAsText writes on a line of their own; those in its
            // binary blocks are checked by every_value_is_one_save_as_text_writes.
            if matches!(
                p.value,
                BlobValue::Binary(_) | BlobValue::Guid(_) | BlobValue::Double(_)
            ) {
                continue;
            }
            let Some(key) = p.name() else { continue };
            if report && matches!(key, "Right" | "Bottom") {
                continue;
            }
            let saved_value = saved.get(key);
            assert!(
                saved_value.is_some_and(|v| saved_forms(&p.value).contains(v)),
                "{at}.{key}: {} against {saved_value:?}",
                p.value
            );
            named += 1;
        }
        named
    }

    /// Checks the type and the SAVED_KEYS of each control against the object
    /// of the same name in SaveAsText, and gives how many values were compared.
    fn check_controls(read: &FormProperties, saved: &Saved, name: &str) -> usize {
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
            for key in SAVED_KEYS {
                let saved_value = object.properties.get(key).cloned();
                compared += usize::from(saved_value.is_some());
                assert_eq!(
                    property_value(&control.properties, key),
                    saved_value,
                    "{name}: {}.{key}",
                    object.name
                );
            }
        }
        compared
    }

    /// Checks the form or report `name` of Sports.accdb, with `controls`
    /// controls, against `file`, its SaveAsText output from Access. Fetched by
    /// scripts/fetch-testdata.sh.
    fn assert_matches_saveastext(name: &str, file: &str, controls: usize, report: bool) {
        let path = skip_if_missing!("saveastext/SportsAdmin/Sports.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let text = std::fs::read_to_string(path.parent().unwrap().join(file)).unwrap();
        let saved = read_saveastext(&text);
        let read = read_form_properties(&mut reader, name).unwrap();
        for key in SAVED_KEYS {
            assert_eq!(
                property_value(&read.properties, key),
                saved.properties.get(key).cloned(),
                "{name}: {key}"
            );
        }

        let mut named = check_named(&read.properties, &saved.properties, name, report);
        for object in &saved.objects {
            if let Some(control) = read.controls.iter().find(|c| c.name == object.name) {
                let at = format!("{name}: {}", object.name);
                named += check_named(&control.properties, &object.properties, &at, false);
            }
        }
        assert!(named > controls, "{name}: {named}");
        assert_eq!(read.controls.len(), controls, "{name}");
        assert_eq!(saved.objects.len(), controls, "{name}");
        assert!(check_controls(&read, &saved, name) > 0, "{name}");
    }

    #[test]
    fn form_matches_saveastext_of_access() {
        assert_matches_saveastext(
            "Statistical Reports",
            "forms/Statistical Reports.bas",
            69,
            false,
        );
    }

    #[test]
    fn report_matches_saveastext_of_access() {
        assert_matches_saveastext(
            "Field (High Jump)",
            "reports/Field (High Jump).bas",
            78,
            true,
        );
    }

    /// The warnings logged while the tests run, of every thread.
    static WARNINGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    struct WarningLogger;

    impl log::Log for WarningLogger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                WARNINGS.lock().unwrap().push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    /// Logs the warnings to [`WARNINGS`].
    fn log_warnings() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            log::set_logger(&WarningLogger).unwrap();
            log::set_max_level(log::LevelFilter::Warn);
        });
    }

    #[test]
    fn a_blob_of_the_access_2000_format_that_cannot_be_read_is_warned() {
        // The Blob of the form Shared, cut after its header: neither its
        // properties nor its controls can be read.
        let path = skip_if_missing!("V2000/sameNameTestV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let mut storage = storage::read_access_storage(&mut reader).unwrap();
        let folder = storage::find_top_folder(&storage.entries, "Forms").unwrap();
        let blob_id = storage::object_stream(&storage.entries, folder, "Shared", "Blob")
            .unwrap()
            .unwrap()
            .id;
        let blob = storage
            .entries
            .iter_mut()
            .find(|e| e.id == blob_id)
            .unwrap();
        assert_eq!(blob.data.first(), Some(&crate::form2000::VERSION));
        blob.data.truncate(10);

        log_warnings();
        let read = read_form_properties_in(&storage, "Shared", Some(FormObjectType::Form)).unwrap();
        assert!(read.properties.is_empty());
        assert!(read.controls.is_empty());
        let warning =
            "Shared: the properties and controls in the Access 2000 format could not be read";
        assert!(WARNINGS.lock().unwrap().iter().any(|w| w == warning));
    }
}
