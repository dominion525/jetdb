//! The design of forms and reports saved in the Access 2000 format.
//!
//! Its Blob starts with the version byte [`VERSION`] and holds the design as
//! a tree, unlike the property entries of Access 2002 and later:
//!
//! - 12 bytes of header, then the properties of the form or report itself,
//!   then a list.
//! - A list is `FF` and a count (u16) of entries, or `FE` and one entry.
//! - An entry is a type (u16, the `acControlType` of Access, such as 109 for
//!   a text box or 152 for the detail section), its properties, and `FD`, or a
//!   list of the entries in it: the controls of a section, the pages of a tab
//!   control. The entries at the top are the default controls of the form or
//!   report and its sections.
//! - A property is an ID byte, numbered in each type, and a value whose size
//!   the ID tells: none for 0x00 to 0x2F (a Yes/No set to the value that is
//!   not its default), 1 byte for 0x30 to 0x5F, 2 bytes for 0x60 to 0x7F,
//!   4 bytes for 0x80 to 0xBF, and a length byte and that many bytes for 0xC0
//!   to 0xFC, the length byte 0xFF being followed by a length of 2 bytes.

use crate::encoding;
use crate::form::{BlobProperty, BlobValue, ControlProperties};

/// The first byte of the Blob of a form or report in the Access 2000 format.
pub(crate) const VERSION: u8 = 0x13;

/// Added to the ID of this format for a property jetdb gives no ID of Access
/// 2002 and later, so that the IDs stay apart: `0x80DD` is the ID `0xDD`.
pub const UNMAPPED: u16 = 0x8000;

/// The types of sections: the detail section, and the headers and footers
/// of the form or report, of its pages and of its groups.
const SECTIONS: std::ops::RangeInclusive<u16> = 0x98..=0x9E;

/// The ID holding the name of a control.
const CONTROL_NAME: u8 = 0xDC;
/// The ID holding the name of a section.
const SECTION_NAME: u8 = 0xDF;

/// A value as stored, before it is given as a [`BlobValue`].
enum Raw {
    /// An ID without a value.
    Flag,
    /// A value of 1, 2 or 4 bytes.
    Int(i32),
    /// A value of a length.
    Bytes(Vec<u8>),
}

/// The size of the value of the property `id`, `None` for a value of a
/// length.
fn value_size(id: u8) -> Option<usize> {
    match id {
        0x00..=0x2F => Some(0),
        0x30..=0x5F => Some(1),
        0x60..=0x7F => Some(2),
        0x80..=0xBF => Some(4),
        _ => None,
    }
}

/// An entry of a list: a control, a section, or a default control.
struct Entry {
    type_code: u16,
    props: Vec<(u8, Raw)>,
    children: Option<Vec<Entry>>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        let b = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        let b = self.bytes(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    /// The properties up to the next `FD`, `FE` or `FF`, or the end.
    fn props(&mut self) -> Option<Vec<(u8, Raw)>> {
        let mut props = Vec::new();
        while let Some(&id) = self.data.get(self.pos) {
            if id >= 0xFD {
                break;
            }
            self.pos += 1;
            let raw = self.value(id)?;
            props.push((id, raw));
        }
        Some(props)
    }

    /// The value of the property `id`, whose size the ID tells.
    fn value(&mut self, id: u8) -> Option<Raw> {
        let Some(size) = value_size(id) else {
            let len = self.length()?;
            return Some(Raw::Bytes(self.bytes(len)?.to_vec()));
        };
        Some(match *self.bytes(size)? {
            [] => Raw::Flag,
            [b] => Raw::Int(i32::from(b)),
            [b0, b1] => Raw::Int(i32::from(i16::from_le_bytes([b0, b1]))),
            [b0, b1, b2, b3] => Raw::Int(i32::from_le_bytes([b0, b1, b2, b3])),
            _ => return None,
        })
    }

    /// The length of a value: a byte, or 0xFF and 2 bytes.
    fn length(&mut self) -> Option<usize> {
        match self.byte()? {
            0xFF => self.u16().map(usize::from),
            n => Some(usize::from(n)),
        }
    }

    fn list(&mut self) -> Option<Vec<Entry>> {
        let count = match self.byte()? {
            0xFF => self.u16()?,
            0xFE => 1,
            _ => return None,
        };
        // The count comes from the file: grow as entries are read.
        let mut entries = Vec::new();
        for _ in 0..count {
            entries.push(self.entry()?);
        }
        Some(entries)
    }

    fn entry(&mut self) -> Option<Entry> {
        let type_code = self.u16()?;
        let props = self.props()?;
        let children = if self.data.get(self.pos) == Some(&0xFD) {
            self.pos += 1;
            None
        } else {
            Some(self.list()?)
        };
        Some(Entry {
            type_code,
            props,
            children,
        })
    }
}

/// The properties of the form or report itself and of its sections and
/// controls, from a Blob of the Access 2000 format; `None` when the bytes do
/// not fit the format.
pub(crate) fn read(data: &[u8]) -> Option<(Vec<BlobProperty>, Vec<ControlProperties>)> {
    let mut reader = Reader { data, pos: 12 };
    let object = reader.props()?;
    let entries = reader.list()?;
    if reader.pos != data.len() {
        return None;
    }
    let mut controls = Vec::new();
    for section in entries.iter().filter(|e| is_section(e)) {
        push_with_children(section, &mut controls);
    }
    Some((properties(&object), controls))
}

/// Whether the entry at the top of the tree is a section rather than a
/// default control: of a section type, or holding controls.
fn is_section(entry: &Entry) -> bool {
    SECTIONS.contains(&entry.type_code) || entry.children.is_some()
}

/// Adds `entry`, and the entries in it, to `out`.
fn push_with_children(entry: &Entry, out: &mut Vec<ControlProperties>) {
    let name_id = if SECTIONS.contains(&entry.type_code) {
        SECTION_NAME
    } else {
        CONTROL_NAME
    };
    let name = entry
        .props
        .iter()
        .find(|(id, _)| *id == name_id)
        .and_then(|(_, raw)| match raw {
            Raw::Bytes(b) => encoding::decode_utf16le(b).ok(),
            _ => None,
        })
        .unwrap_or_else(|| format!("Control_{}", out.len()));
    out.push(ControlProperties {
        name,
        type_code: entry.type_code,
        properties: properties(&entry.props),
    });
    for child in entry.children.iter().flatten() {
        push_with_children(child, out);
    }
}

fn properties(props: &[(u8, Raw)]) -> Vec<BlobProperty> {
    props
        .iter()
        .map(|(id, raw)| BlobProperty {
            prop_id: UNMAPPED | u16::from(*id),
            value: match raw {
                Raw::Flag => BlobValue::NotDefault,
                Raw::Int(v) => match i16::try_from(*v) {
                    Ok(short) if (0x30..=0x7F).contains(id) => BlobValue::Short(short),
                    _ => BlobValue::Long(*v),
                },
                Raw::Bytes(b) => BlobValue::Binary(b.clone()),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::{
        list_forms, read_form_properties_of_type, read_form_stream_of_type, FormObjectType,
        StreamKind,
    };
    use crate::PageReader;

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::PathBuf::from(manifest_dir)
            .join("../../testdata")
            .join(relative);
        path.exists().then_some(path)
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

    /// A Blob with the header, the properties of the object, and `rest`.
    fn blob(object: &[u8], rest: &[u8]) -> Vec<u8> {
        let mut data = vec![VERSION, 0, VERSION, 0, 0, 0, 0, 0, 0x96, 0, 0, 0];
        data.extend_from_slice(object);
        data.extend_from_slice(rest);
        data
    }

    /// The bytes of the property `id` holding the text `s`.
    fn text_prop(id: u8, s: &str) -> Vec<u8> {
        let units: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
        [vec![id, units.len() as u8], units].concat()
    }

    #[test]
    fn reads_a_tree_of_sections_and_controls() {
        // The object: 0x30 = 2, then one section (FE) of type 152 named "D",
        // holding two controls (FF 02 00): a text box named "t" with 0x60 =
        // 1500 and a flag 0x05, and a label with 0x9E = 70000.
        let mut rest = vec![0xFE, 152, 0];
        rest.extend(text_prop(0xDF, "D"));
        rest.extend([0xFF, 2, 0, 109, 0, 0x05, 0x60, 0xDC, 0x05]);
        rest.extend(text_prop(0xDC, "t"));
        rest.extend([0xFD, 100, 0, 0x9E]);
        rest.extend(70000i32.to_le_bytes());
        rest.push(0xFD);
        let (object, controls) = read(&blob(&[0x30, 2], &rest)).unwrap();
        assert_eq!(object.len(), 1);
        assert_eq!(object[0].prop_id, 0x8030);
        assert!(matches!(object[0].value, BlobValue::Short(2)));
        let names: Vec<(&str, u16)> = controls
            .iter()
            .map(|c| (c.name.as_str(), c.type_code))
            .collect();
        assert_eq!(names, [("D", 152), ("t", 109), ("Control_2", 100)]);
        let text_box = &controls[1].properties;
        assert!(matches!(text_box[0].value, BlobValue::NotDefault));
        assert_eq!(text_box[1].prop_id, 0x8060);
        assert!(matches!(text_box[1].value, BlobValue::Short(1500)));
        assert!(matches!(
            controls[2].properties[0].value,
            BlobValue::Long(70000)
        ));
    }

    #[test]
    fn a_length_of_ff_is_followed_by_two_bytes() {
        // 0xEB with 300 bytes, then an empty list.
        let mut object = vec![0xEB, 0xFF, 0x2C, 0x01];
        object.extend([7; 300]);
        let (props, controls) = read(&blob(&object, &[0xFF, 0, 0])).unwrap();
        assert!(matches!(&props[0].value, BlobValue::Binary(b) if b.len() == 300));
        assert!(controls.is_empty());
    }

    #[test]
    fn bytes_that_do_not_fit_are_none() {
        // No list after the properties; a list longer than the data; a byte
        // left after the list.
        assert!(read(&blob(&[0x30, 2], &[])).is_none());
        assert!(read(&blob(&[], &[0xFF, 5, 0, 109, 0, 0xFD])).is_none());
        assert!(read(&blob(&[], &[0xFF, 0, 0, 0x00])).is_none());
    }

    /// The names of the forms and reports of the Access 2000 format in
    /// `file`, with their Blobs.
    fn blobs(file: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        let mut reader = PageReader::open(file).unwrap();
        let mut out = Vec::new();
        for entry in list_forms(&mut reader).unwrap() {
            let stream = read_form_stream_of_type(
                &mut reader,
                &entry.name,
                entry.object_type,
                StreamKind::Blob,
            )
            .unwrap();
            if stream.data.first() == Some(&VERSION) {
                out.push((entry.name, stream.data));
            }
        }
        out
    }

    #[test]
    fn every_form_and_report_reads_to_the_end() {
        for (file, count) in [
            ("V2000/sameNameTestV2000.mdb", 2),
            ("V2000/macroGeneratedTestV2000.mdb", 2),
            ("V2000/macroTestV2000.mdb", 1),
            ("vbaV2000.mdb", 1),
            ("V2000/propertyIdTestV2000.mdb", 106),
            ("V2000/controlPropertyTestV2000.mdb", 96),
        ] {
            let path = skip_if_missing!(file);
            let found = blobs(&path);
            assert_eq!(found.len(), count, "{file}");
            for (name, data) in found {
                assert!(read(&data).is_some(), "{file}: {name}");
            }
        }
    }

    #[test]
    fn a_form_of_the_access_2000_format() {
        // Made by testdata/form-generators/same-name-generator.bas, then
        // saved in the Access 2000 format: the form Shared, with a caption, a
        // record source and a button of its own.
        let path = skip_if_missing!("V2000/sameNameTestV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let read =
            read_form_properties_of_type(&mut reader, "Shared", FormObjectType::Form).unwrap();
        let controls: Vec<(&str, u16)> = read
            .controls
            .iter()
            .map(|c| (c.name.as_str(), c.type_code))
            .collect();
        assert_eq!(controls, [("詳細", 152), ("btnFormOnly", 104)]);
        let text = |id: u16| {
            read.properties
                .iter()
                .find(|p| p.prop_id == id)
                .and_then(|p| match &p.value {
                    BlobValue::Binary(b) => encoding::decode_utf16le(b).ok(),
                    _ => None,
                })
        };
        assert_eq!(text(0x80DC).as_deref(), Some("tblShared"));
        assert_eq!(text(0x80DD).as_deref(), Some("Shared form"));
    }

    #[test]
    fn controls_of_each_type() {
        // Made by testdata/form-generators/control-property-generator.bas,
        // then saved in the Access 2000 format: each form and report has the
        // detail section and a control of the type in its name.
        let path = skip_if_missing!("V2000/controlPropertyTestV2000.mdb");
        let types = [
            ("Label", 100),
            ("Rectangle", 101),
            ("Line", 102),
            ("Image", 103),
            ("CommandButton", 104),
            ("OptionButton", 105),
            ("CheckBox", 106),
            ("OptionGroup", 107),
            ("BoundObjectFrame", 108),
            ("TextBox", 109),
            ("ListBox", 110),
            ("ComboBox", 111),
            ("Subform", 112),
            ("PageBreak", 118),
            ("ToggleButton", 122),
            ("TabControl", 123),
        ];
        let mut reader = PageReader::open(&path).unwrap();
        for entry in list_forms(&mut reader).unwrap() {
            let read =
                read_form_properties_of_type(&mut reader, &entry.name, entry.object_type).unwrap();
            let type_name = &entry.name[2..entry.name.len() - 2];
            let (_, code) = types.iter().find(|(t, _)| *t == type_name).unwrap();
            let types: Vec<u16> = read.controls.iter().map(|c| c.type_code).collect();
            assert!(types.contains(code), "{}: {types:?}", entry.name);
            assert!(read
                .controls
                .iter()
                .any(|c| c.name == "詳細" && c.type_code == 152));
        }
    }

    #[test]
    fn values_of_a_text_box() {
        // Made by testdata/form-generators/property-id-generator.bas, then
        // saved in the Access 2000 format: the text box txtA moved to a left
        // of 1700.
        let path = skip_if_missing!("V2000/propertyIdTestV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let read =
            read_form_properties_of_type(&mut reader, "TextBox_Left_1", FormObjectType::Form)
                .unwrap();
        let text_box = read.controls.iter().find(|c| c.name == "txtA").unwrap();
        assert_eq!(text_box.type_code, 109);
        let left = text_box
            .properties
            .iter()
            .find(|p| p.prop_id == 0x8060)
            .unwrap();
        assert!(matches!(left.value, BlobValue::Short(1700)));
    }
}
