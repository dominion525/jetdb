//! The design of forms and reports saved in the Access 2000 format.
//!
//! Its Blob starts with the version byte `0x13` and holds the design as
//! a tree, unlike the property entries of Access 2002 and later:
//!
//! - 8 bytes of header, then the form or report itself as an entry
//!   without its end: its type (u16, 0x96 for a form, 0x97 for a report)
//!   and its properties, then a list.
//! - A list is `FF` and a count (u16) of entries, or `FE` and one entry.
//! - An entry is a type (u16, the `acControlType` of Access, such as 109 for
//!   a text box or 152 for the detail section), its properties, and `FD`, or a
//!   list of the entries in it: the controls of a section, the pages of a tab
//!   control. The entries at the top are the default controls of the form or
//!   report and its sections.
//! - A property is an ID byte, numbered in each type, and a value whose size
//!   the ID tells: none for 0x00 to 0x2F (a Yes/No set to the value that is
//!   not its default), 1 byte for 0x30 to 0x5F, 2 bytes for 0x60 to 0x7F,
//!   4 bytes for 0x80 to 0xBC, and a length byte and that many bytes for 0xBD
//!   to 0xFC, the length byte 0xFF being followed by a length of 2 bytes. Of
//!   0x80 to 0xBC, only 0x9C to 0xA0 have been seen; 0xBD to 0xBF hold text.

use crate::data::format_guid;
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
        0x80..=0xBC => Some(4),
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
    // The properties of the object follow its type, at 8.
    let mut reader = Reader { data, pos: 10 };
    let object = reader.props()?;
    let entries = reader.list()?;
    if reader.pos != data.len() {
        return None;
    }
    let mut controls = Vec::new();
    for section in entries.iter().filter(|e| is_section(e)) {
        push_with_children(section, &mut controls);
    }
    let scope = if data.get(8) == Some(&0x97) {
        Scope::Report
    } else {
        Scope::Form
    };
    Some((properties(&object, scope), controls))
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
        properties: properties(&entry.props, Scope::Entry(entry.type_code)),
    });
    for child in entry.children.iter().flatten() {
        push_with_children(child, out);
    }
}

fn properties(props: &[(u8, Raw)], scope: Scope) -> Vec<BlobProperty> {
    let table = table(scope);
    props
        .iter()
        .map(|(id, raw)| {
            let tied = table
                .binary_search_by_key(id, |(from, _, _)| *from)
                .ok()
                .map(|i| table[i]);
            let value = tied.and_then(|(_, _, kind)| value_as(raw, kind));
            BlobProperty {
                prop_id: tied.map_or(UNMAPPED | u16::from(*id), |(_, to, _)| to),
                value: value.unwrap_or_else(|| stored_value(*id, raw)),
            }
        })
        .collect()
}

/// The value of `raw`, of the property `id`, as stored.
fn stored_value(id: u8, raw: &Raw) -> BlobValue {
    match raw {
        Raw::Flag => BlobValue::NotDefault,
        Raw::Int(v) => match i16::try_from(*v) {
            Ok(short) if (0x30..=0x7F).contains(&id) => BlobValue::Short(short),
            _ => BlobValue::Long(*v),
        },
        Raw::Bytes(b) => BlobValue::Binary(b.clone()),
    }
}

/// How the value of an ID tied to Access 2002 and later is given: as that
/// format gives it.
#[derive(Clone, Copy)]
enum Kind {
    /// A number, of the size stored.
    Number,
    /// UTF-16LE text.
    Text,
    /// A GUID of 16 bytes.
    Guid,
    /// A double of 8 bytes, such as a date and time.
    Double,
    /// Bytes as they are.
    Bytes,
}

/// A table: `(ID, ID of Access 2002 and later, how the value is given)`,
/// in the order of the IDs.
type Table = &'static [(u8, u16, Kind)];

/// What a table is of: the form or report itself, or an entry type.
#[derive(Clone, Copy)]
enum Scope {
    Form,
    Report,
    Entry(u16),
}

/// The table of `scope`, empty for an entry type without one.
fn table(scope: Scope) -> Table {
    match scope {
        Scope::Form => FORM,
        Scope::Report => REPORT,
        Scope::Entry(type_code) => ENTRY_TABLES
            .iter()
            .find(|(code, _)| *code == type_code)
            .map_or(&[], |(_, table)| table),
    }
}

/// The table of each entry type.
const ENTRY_TABLES: &[(u16, Table)] = &[
    (100, TYPE_100),
    (101, TYPE_101),
    (102, TYPE_102),
    (103, TYPE_103),
    (104, TYPE_104),
    (105, TYPE_105),
    (106, TYPE_106),
    (107, TYPE_107),
    (108, TYPE_108),
    (109, TYPE_109),
    (110, TYPE_110),
    (111, TYPE_111),
    (112, TYPE_112),
    (118, TYPE_118),
    (122, TYPE_122),
    (123, TYPE_123),
    (124, TYPE_124),
    (152, TYPE_152),
    (155, TYPE_155),
    (156, TYPE_156),
];

/// The value of `raw` given as `kind`; `None` to give it as stored.
fn value_as(raw: &Raw, kind: Kind) -> Option<BlobValue> {
    match (raw, kind) {
        (Raw::Bytes(b), Kind::Text) => encoding::decode_utf16le(b).ok().map(BlobValue::Text),
        (Raw::Bytes(b), Kind::Guid) if b.len() == 16 => Some(BlobValue::Guid(format_guid(b))),
        (Raw::Bytes(b), Kind::Double) => <[u8; 8]>::try_from(b.as_slice())
            .ok()
            .map(|b| BlobValue::Double(f64::from_le_bytes(b))),
        _ => None,
    }
}

// The tables. Each pair was tied by the values the forms and reports of
// testdata/V2000/controlPropertyTestV2000.mdb and propertyIdTestV2000.mdb hold
// against the same objects in V2007/controlPropertyTestV2007.accdb and
// propertyIdTestV2007.accdb (Access for Microsoft 365): the values of an ID
// across the objects are those of one ID of the later format, and of no
// other. Where the SaveAsText output of the files of the 2000 format
// themselves (testdata/V2000/saveastext) names a property another ID holds in
// the later format, the pair follows the name. The other pairs were tied by
// that output alone: the values of an ID of an entry type, or of the form or
// report, are all held by one key, and by no other, and the ID of the later
// format is the one form.rs names after that key. The IDs of the later format
// are those of Access for Microsoft 365, one higher above 0x01C4 than in
// files of earlier Access (see form.rs).

const FORM: Table = &[
    (0x14, 0x013A, Kind::Number),
    (0x1C, 0x0175, Kind::Number),
    (0x30, 0x0093, Kind::Number),
    (0x31, 0x002D, Kind::Number),
    (0x32, 0x0098, Kind::Number),
    (0x33, 0x0092, Kind::Number),
    (0x38, 0x00F4, Kind::Number),
    (0x39, 0x0009, Kind::Number),
    (0x3B, 0x0172, Kind::Number),
    (0x3D, 0x0135, Kind::Number),
    (0x42, 0x013E, Kind::Number),
    (0x61, 0x0029, Kind::Number),
    (0x62, 0x002A, Kind::Number),
    (0x63, 0x0096, Kind::Number),
    (0x64, 0x00A8, Kind::Number),
    (0x65, 0x00A1, Kind::Number),
    (0x66, 0x00A2, Kind::Number),
    (0x67, 0x009E, Kind::Number),
    (0x6A, 0x0081, Kind::Number),
    (0x6B, 0x000C, Kind::Number),
    (0x6D, 0x0183, Kind::Number),
    (0x9C, 0x00DB, Kind::Number),
    (0x9D, 0x006F, Kind::Number),
    (0x9E, 0x0132, Kind::Number),
    (0xA0, 0x013F, Kind::Number),
    (0xBD, 0x0080, Kind::Text),
    (0xBE, 0x007B, Kind::Text),
    (0xBF, 0x007C, Kind::Text),
    (0xC0, 0x007D, Kind::Text),
    (0xC1, 0x010A, Kind::Text),
    (0xC3, 0x00F5, Kind::Text),
    (0xC5, 0x0130, Kind::Text),
    (0xC8, 0x014B, Kind::Text),
    (0xC9, 0x014D, Kind::Text),
    (0xCA, 0x014E, Kind::Text),
    (0xCE, 0x016C, Kind::Double),
    (0xCF, 0x0177, Kind::Text),
    (0xD0, 0x0178, Kind::Guid),
    (0xDC, 0x009C, Kind::Text),
    (0xDD, 0x0011, Kind::Text),
    (0xDE, 0x0057, Kind::Text),
    (0xDF, 0x007A, Kind::Text),
    (0xE0, 0x004F, Kind::Text),
    (0xE1, 0x0055, Kind::Text),
    (0xE2, 0x0056, Kind::Text),
    (0xE3, 0x004D, Kind::Text),
    (0xE4, 0x004E, Kind::Text),
    (0xE5, 0x00F0, Kind::Text),
    (0xE6, 0x00DA, Kind::Text),
    (0xE7, 0x00A0, Kind::Text),
    (0xEA, 0x00BD, Kind::Bytes),
    (0xEB, 0x00BE, Kind::Bytes),
    (0xEC, 0x00BF, Kind::Bytes),
    (0xED, 0x0068, Kind::Text),
    (0xEF, 0x006A, Kind::Text),
    (0xF0, 0x006B, Kind::Text),
    (0xF1, 0x006C, Kind::Text),
    (0xF2, 0x006D, Kind::Text),
    (0xF3, 0x006E, Kind::Text),
    (0xF4, 0x0070, Kind::Text),
    (0xF5, 0x0071, Kind::Text),
    (0xF6, 0x0073, Kind::Text),
    (0xF7, 0x0074, Kind::Text),
    (0xF8, 0x0075, Kind::Text),
    (0xF9, 0x007E, Kind::Text),
    (0xFA, 0x00E0, Kind::Text),
    (0xFB, 0x007F, Kind::Text),
    (0xFC, 0x0077, Kind::Text),
];

const REPORT: Table = &[
    (0x14, 0x013A, Kind::Number),
    (0x1C, 0x0175, Kind::Number),
    (0x30, 0x0093, Kind::Number),
    (0x3A, 0x011A, Kind::Number),
    (0x3B, 0x0172, Kind::Number),
    (0x3C, 0x011B, Kind::Number),
    (0x3D, 0x0135, Kind::Number),
    (0x61, 0x0029, Kind::Number),
    (0x62, 0x002A, Kind::Number),
    (0x63, 0x0096, Kind::Number),
    (0x67, 0x009E, Kind::Number),
    (0x6A, 0x0081, Kind::Number),
    (0x6B, 0x000C, Kind::Number),
    (0x9C, 0x00DB, Kind::Number),
    (0x9D, 0x006F, Kind::Number),
    (0x9E, 0x0132, Kind::Number),
    (0xBD, 0x0080, Kind::Text),
    (0xC1, 0x010A, Kind::Text),
    (0xC2, 0x0079, Kind::Text),
    (0xC3, 0x00F5, Kind::Text),
    (0xC5, 0x0130, Kind::Text),
    (0xC6, 0x0053, Kind::Text),
    (0xC8, 0x014B, Kind::Text),
    (0xC9, 0x014D, Kind::Text),
    (0xCA, 0x014E, Kind::Text),
    (0xCE, 0x016C, Kind::Double),
    (0xD0, 0x0178, Kind::Guid),
    (0xDC, 0x009C, Kind::Text),
    (0xDD, 0x0011, Kind::Text),
    (0xDE, 0x0057, Kind::Text),
    (0xE3, 0x004D, Kind::Text),
    (0xE4, 0x004E, Kind::Text),
    (0xE5, 0x00F0, Kind::Text),
    (0xE6, 0x00DA, Kind::Text),
    (0xE7, 0x00A0, Kind::Text),
    (0xEA, 0x00BD, Kind::Bytes),
    (0xEB, 0x00BE, Kind::Bytes),
    (0xEC, 0x00BF, Kind::Bytes),
    (0xED, 0x0068, Kind::Text),
    (0xEF, 0x006A, Kind::Text),
    (0xF0, 0x006B, Kind::Text),
    (0xF1, 0x006C, Kind::Text),
    (0xF2, 0x006D, Kind::Text),
    (0xF3, 0x006E, Kind::Text),
    (0xF4, 0x0070, Kind::Text),
    (0xF5, 0x0071, Kind::Text),
    (0xF6, 0x0073, Kind::Text),
    (0xF7, 0x0074, Kind::Text),
    (0xF8, 0x0075, Kind::Text),
    (0xF9, 0x007E, Kind::Text),
    (0xFA, 0x00E0, Kind::Text),
    (0xFB, 0x007F, Kind::Text),
    (0xFC, 0x0077, Kind::Text),
];

const TYPE_100: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x02, 0x0024, Kind::Number),
    (0x03, 0x0163, Kind::Number),
    (0x33, 0x0149, Kind::Number),
    (0x38, 0x00F3, Kind::Number),
    (0x60, 0x0036, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x67, 0x0180, Kind::Number),
    (0x68, 0x0181, Kind::Number),
    (0x69, 0x0182, Kind::Number),
    (0x6A, 0x0184, Kind::Number),
    (0x6B, 0x0185, Kind::Number),
    (0x9E, 0x00CC, Kind::Number),
    (0x9F, 0x00DB, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x0011, Kind::Text),
    (0xDE, 0x0022, Kind::Text),
    (0xDF, 0x007E, Kind::Text),
    (0xE0, 0x00E0, Kind::Text),
    (0xE1, 0x006B, Kind::Text),
    (0xE2, 0x006C, Kind::Text),
    (0xE3, 0x006D, Kind::Text),
    (0xE4, 0x010A, Kind::Text),
    (0xE6, 0x0130, Kind::Text),
    (0xE7, 0x013D, Kind::Text),
    (0xE8, 0x0157, Kind::Text),
    (0xE9, 0x015B, Kind::Text),
    (0xEA, 0x0178, Kind::Guid),
];

const TYPE_101: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x30, 0x0095, Kind::Number),
    (0x31, 0x0004, Kind::Number),
    (0x32, 0x001D, Kind::Number),
    (0x33, 0x0149, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x9D, 0x0008, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x007E, Kind::Text),
    (0xDE, 0x00E0, Kind::Text),
    (0xDF, 0x006B, Kind::Text),
    (0xE0, 0x006C, Kind::Text),
    (0xE1, 0x006D, Kind::Text),
    (0xE2, 0x010A, Kind::Text),
    (0xE4, 0x0178, Kind::Guid),
];

const TYPE_102: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x01, 0x0037, Kind::Number),
    (0x30, 0x0095, Kind::Number),
    (0x31, 0x0149, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x9C, 0x0008, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x010A, Kind::Text),
    (0xDF, 0x0178, Kind::Guid),
];

const TYPE_103: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x32, 0x001D, Kind::Number),
    (0x33, 0x0149, Kind::Number),
    (0x36, 0x0059, Kind::Number),
    (0x37, 0x0135, Kind::Number),
    (0x60, 0x0036, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x9D, 0x0008, Kind::Number),
    (0x9E, 0x00DB, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x007E, Kind::Text),
    (0xDE, 0x00E0, Kind::Text),
    (0xE0, 0x006B, Kind::Text),
    (0xE1, 0x006C, Kind::Text),
    (0xE2, 0x006D, Kind::Text),
    (0xE4, 0x010A, Kind::Text),
    (0xE6, 0x0130, Kind::Text),
    (0xE7, 0x013D, Kind::Text),
    (0xE9, 0x0157, Kind::Text),
    (0xEA, 0x015B, Kind::Text),
    (0xEB, 0x0178, Kind::Guid),
];

const TYPE_104: Table = &[
    (0x04, 0x0094, Kind::Number),
    (0x35, 0x00F3, Kind::Number),
    (0x60, 0x0036, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x9C, 0x00DB, Kind::Number),
    (0x9D, 0x00CC, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x0011, Kind::Text),
    (0xDE, 0x0087, Kind::Text),
    (0xDF, 0x007E, Kind::Text),
    (0xE0, 0x00DE, Kind::Text),
    (0xE1, 0x00DF, Kind::Text),
    (0xE2, 0x00E0, Kind::Text),
    (0xE4, 0x0022, Kind::Text),
    (0xE5, 0x0068, Kind::Text),
    (0xE7, 0x006A, Kind::Text),
    (0xE8, 0x006B, Kind::Text),
    (0xE9, 0x006C, Kind::Text),
    (0xEA, 0x006D, Kind::Text),
    (0xEB, 0x0073, Kind::Text),
    (0xEC, 0x0074, Kind::Text),
    (0xEE, 0x010A, Kind::Text),
    (0xF0, 0x0130, Kind::Text),
    (0xF1, 0x013D, Kind::Text),
    (0xF3, 0x0157, Kind::Text),
    (0xF4, 0x015B, Kind::Text),
    (0xF5, 0x0178, Kind::Guid),
];

const TYPE_105: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x35, 0x0149, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x66, 0x00A5, Kind::Number),
    (0x9D, 0x00DB, Kind::Number),
    (0x9E, 0x0008, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x0087, Kind::Text),
    (0xDF, 0x0091, Kind::Text),
    (0xE0, 0x003D, Kind::Text),
    (0xE1, 0x0055, Kind::Text),
    (0xE2, 0x0056, Kind::Text),
    (0xE3, 0x00DE, Kind::Text),
    (0xE4, 0x00DF, Kind::Text),
    (0xE5, 0x00E0, Kind::Text),
    (0xE6, 0x0017, Kind::Text),
    (0xE7, 0x0068, Kind::Text),
    (0xE9, 0x006A, Kind::Text),
    (0xEA, 0x006B, Kind::Text),
    (0xEB, 0x006C, Kind::Text),
    (0xEC, 0x006D, Kind::Text),
    (0xED, 0x0073, Kind::Text),
    (0xEE, 0x0074, Kind::Text),
    (0xEF, 0x007E, Kind::Text),
    (0xF0, 0x010A, Kind::Text),
    (0xF2, 0x0130, Kind::Text),
    (0xF3, 0x013D, Kind::Text),
    (0xF4, 0x0178, Kind::Guid),
];

const TYPE_106: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x35, 0x0149, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x66, 0x00A5, Kind::Number),
    (0x9D, 0x00DB, Kind::Number),
    (0x9E, 0x0008, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x0087, Kind::Text),
    (0xDF, 0x0091, Kind::Text),
    (0xE0, 0x003D, Kind::Text),
    (0xE1, 0x0055, Kind::Text),
    (0xE2, 0x0056, Kind::Text),
    (0xE3, 0x00DE, Kind::Text),
    (0xE4, 0x00DF, Kind::Text),
    (0xE5, 0x00E0, Kind::Text),
    (0xE6, 0x0017, Kind::Text),
    (0xE7, 0x0068, Kind::Text),
    (0xE9, 0x006A, Kind::Text),
    (0xEA, 0x006B, Kind::Text),
    (0xEB, 0x006C, Kind::Text),
    (0xEC, 0x006D, Kind::Text),
    (0xED, 0x0073, Kind::Text),
    (0xEE, 0x0074, Kind::Text),
    (0xEF, 0x007E, Kind::Text),
    (0xF0, 0x010A, Kind::Text),
    (0xF2, 0x0130, Kind::Text),
    (0xF3, 0x013D, Kind::Text),
    (0xF4, 0x0178, Kind::Guid),
];

const TYPE_107: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x30, 0x0095, Kind::Number),
    (0x31, 0x0004, Kind::Number),
    (0x33, 0x0149, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x66, 0x00A5, Kind::Number),
    (0x9D, 0x0008, Kind::Number),
    (0x9E, 0x00DB, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x0087, Kind::Text),
    (0xDF, 0x0091, Kind::Text),
    (0xE0, 0x003D, Kind::Text),
    (0xE1, 0x0055, Kind::Text),
    (0xE2, 0x0056, Kind::Text),
    (0xE3, 0x00DE, Kind::Text),
    (0xE4, 0x00DF, Kind::Text),
    (0xE5, 0x00E0, Kind::Text),
    (0xE6, 0x0017, Kind::Text),
    (0xEA, 0x006B, Kind::Text),
    (0xEB, 0x006C, Kind::Text),
    (0xEC, 0x006D, Kind::Text),
    (0xEF, 0x007E, Kind::Text),
    (0xF0, 0x010A, Kind::Text),
    (0xF2, 0x0130, Kind::Text),
    (0xF3, 0x013D, Kind::Text),
    (0xF4, 0x0178, Kind::Guid),
];

const TYPE_108: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x30, 0x0059, Kind::Number),
    (0x32, 0x0004, Kind::Number),
    (0x33, 0x0149, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x66, 0x00A5, Kind::Number),
    (0x6A, 0x0065, Kind::Number),
    (0x9C, 0x0008, Kind::Number),
    (0x9D, 0x00DB, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x0087, Kind::Text),
    (0xDF, 0x00DE, Kind::Text),
    (0xE0, 0x00DF, Kind::Text),
    (0xE1, 0x00E0, Kind::Text),
    (0xE2, 0x0068, Kind::Text),
    (0xE4, 0x006A, Kind::Text),
    (0xE5, 0x006B, Kind::Text),
    (0xE6, 0x006C, Kind::Text),
    (0xE7, 0x006D, Kind::Text),
    (0xE8, 0x0073, Kind::Text),
    (0xE9, 0x0074, Kind::Text),
    (0xEA, 0x007E, Kind::Text),
    (0xEB, 0x0076, Kind::Text),
    (0xEF, 0x010A, Kind::Text),
    (0xF0, 0x0055, Kind::Text),
    (0xF1, 0x0056, Kind::Text),
    (0xF3, 0x0130, Kind::Text),
    (0xF4, 0x013D, Kind::Text),
    (0xF5, 0x0178, Kind::Guid),
];

const TYPE_109: Table = &[
    (0x01, 0x0019, Kind::Number),
    (0x0A, 0x0024, Kind::Number),
    (0x0F, 0x0165, Kind::Number),
    (0x10, 0x0154, Kind::Number),
    (0x35, 0x0149, Kind::Number),
    (0x37, 0x009F, Kind::Number),
    (0x3C, 0x00F3, Kind::Number),
    (0x40, 0x0170, Kind::Number),
    (0x60, 0x0036, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x65, 0x00A6, Kind::Number),
    (0x66, 0x00A5, Kind::Number),
    (0x6D, 0x0180, Kind::Number),
    (0x6E, 0x0181, Kind::Number),
    (0x6F, 0x0182, Kind::Number),
    (0x70, 0x0184, Kind::Number),
    (0x71, 0x0185, Kind::Number),
    (0x9D, 0x0008, Kind::Number),
    (0x9E, 0x00DB, Kind::Number),
    (0x9F, 0x00CC, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x0026, Kind::Text),
    (0xDF, 0x0087, Kind::Text),
    (0xE0, 0x0091, Kind::Text),
    (0xE1, 0x003D, Kind::Text),
    (0xE2, 0x0055, Kind::Text),
    (0xE3, 0x0056, Kind::Text),
    (0xE4, 0x00DE, Kind::Text),
    (0xE5, 0x00DF, Kind::Text),
    (0xE6, 0x00E0, Kind::Text),
    (0xE7, 0x0017, Kind::Text),
    (0xE8, 0x0022, Kind::Text),
    (0xE9, 0x0048, Kind::Text),
    (0xEA, 0x0068, Kind::Text),
    (0xEC, 0x006A, Kind::Text),
    (0xED, 0x006B, Kind::Text),
    (0xEE, 0x006C, Kind::Text),
    (0xEF, 0x006D, Kind::Text),
    (0xF0, 0x0073, Kind::Text),
    (0xF1, 0x0074, Kind::Text),
    (0xF2, 0x007E, Kind::Text),
    (0xF3, 0x0072, Kind::Text),
    (0xF4, 0x010A, Kind::Text),
    (0xF6, 0x0167, Kind::Text),
    (0xF7, 0x0130, Kind::Text),
    (0xF8, 0x013D, Kind::Text),
    (0xFA, 0x0178, Kind::Guid),
    (0xFB, 0x0168, Kind::Text),
];

const TYPE_110: Table = &[
    (0x01, 0x0094, Kind::Number),
    (0x37, 0x00F3, Kind::Number),
    (0x3D, 0x0149, Kind::Number),
    (0x60, 0x0046, Kind::Number),
    (0x62, 0x008D, Kind::Number),
    (0x63, 0x0096, Kind::Number),
    (0x64, 0x002C, Kind::Number),
    (0x67, 0x00A5, Kind::Number),
    (0x9C, 0x000D, Kind::Number),
    (0x9E, 0x00DB, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x005D, Kind::Text),
    (0xDF, 0x005B, Kind::Text),
    (0xE1, 0x0087, Kind::Text),
    (0xE2, 0x0091, Kind::Text),
    (0xE3, 0x003D, Kind::Text),
    (0xE4, 0x0055, Kind::Text),
    (0xE5, 0x0056, Kind::Text),
    (0xE6, 0x00DE, Kind::Text),
    (0xE7, 0x00DF, Kind::Text),
    (0xE8, 0x00E0, Kind::Text),
    (0xE9, 0x0017, Kind::Text),
    (0xEA, 0x0022, Kind::Text),
    (0xEB, 0x0068, Kind::Text),
    (0xED, 0x006A, Kind::Text),
    (0xEE, 0x006B, Kind::Text),
    (0xEF, 0x006C, Kind::Text),
    (0xF0, 0x006D, Kind::Text),
    (0xF1, 0x0073, Kind::Text),
    (0xF2, 0x0074, Kind::Text),
    (0xF3, 0x007E, Kind::Text),
    (0xF4, 0x010A, Kind::Text),
    (0xF6, 0x0130, Kind::Text),
    (0xF7, 0x013D, Kind::Text),
    (0xF8, 0x0178, Kind::Guid),
];

const TYPE_111: Table = &[
    (0x0B, 0x0044, Kind::Number),
    (0x33, 0x0149, Kind::Number),
    (0x3A, 0x00F3, Kind::Number),
    (0x3E, 0x0170, Kind::Number),
    (0x60, 0x0046, Kind::Number),
    (0x64, 0x008D, Kind::Number),
    (0x65, 0x0096, Kind::Number),
    (0x66, 0x002C, Kind::Number),
    (0x68, 0x00A6, Kind::Number),
    (0x69, 0x00A5, Kind::Number),
    (0x9C, 0x000D, Kind::Number),
    (0x9F, 0x00DB, Kind::Number),
    (0xBE, 0x0178, Kind::Guid),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x005D, Kind::Text),
    (0xDF, 0x005B, Kind::Text),
    (0xE1, 0x0087, Kind::Text),
    (0xE2, 0x0091, Kind::Text),
    (0xE3, 0x003D, Kind::Text),
    (0xE4, 0x0055, Kind::Text),
    (0xE5, 0x0056, Kind::Text),
    (0xE6, 0x00DE, Kind::Text),
    (0xE7, 0x00DF, Kind::Text),
    (0xE8, 0x00E0, Kind::Text),
    (0xE9, 0x0017, Kind::Text),
    (0xEA, 0x0022, Kind::Text),
    (0xEB, 0x0068, Kind::Text),
    (0xED, 0x006A, Kind::Text),
    (0xEE, 0x006B, Kind::Text),
    (0xEF, 0x006C, Kind::Text),
    (0xF0, 0x006D, Kind::Text),
    (0xF1, 0x0073, Kind::Text),
    (0xF2, 0x0074, Kind::Text),
    (0xF3, 0x007E, Kind::Text),
    (0xF4, 0x0072, Kind::Text),
    (0xF5, 0x010A, Kind::Text),
    (0xF6, 0x0078, Kind::Text),
    (0xF8, 0x0130, Kind::Text),
    (0xF9, 0x013D, Kind::Text),
    (0xFA, 0x0026, Kind::Text),
    (0xFB, 0x0048, Kind::Text),
];

const TYPE_112: Table = &[
    (0x01, 0x0019, Kind::Number),
    (0x30, 0x0095, Kind::Number),
    (0x34, 0x0149, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x9C, 0x0008, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x0084, Kind::Text),
    (0xDE, 0x0031, Kind::Text),
    (0xDF, 0x0032, Kind::Text),
    (0xE0, 0x0087, Kind::Text),
    (0xE1, 0x00DE, Kind::Text),
    (0xE2, 0x00DF, Kind::Text),
    (0xE3, 0x010A, Kind::Text),
    (0xE5, 0x0178, Kind::Guid),
];

const TYPE_118: Table = &[
    (0x61, 0x008D, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x010A, Kind::Text),
    (0xDF, 0x0178, Kind::Guid),
];

const TYPE_122: Table = &[
    (0x09, 0x0106, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x65, 0x00A6, Kind::Number),
    (0x66, 0x00A5, Kind::Number),
    (0x9D, 0x00DB, Kind::Number),
    (0x9E, 0x00CC, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x001B, Kind::Text),
    (0xDE, 0x0087, Kind::Text),
    (0xDF, 0x0091, Kind::Text),
    (0xE0, 0x003D, Kind::Text),
    (0xE1, 0x0055, Kind::Text),
    (0xE2, 0x0056, Kind::Text),
    (0xE3, 0x00DE, Kind::Text),
    (0xE4, 0x00DF, Kind::Text),
    (0xE5, 0x00E0, Kind::Text),
    (0xE6, 0x0017, Kind::Text),
    (0xE7, 0x0011, Kind::Text),
    (0xE9, 0x0022, Kind::Text),
    (0xEA, 0x0068, Kind::Text),
    (0xEC, 0x006A, Kind::Text),
    (0xED, 0x006B, Kind::Text),
    (0xEE, 0x006C, Kind::Text),
    (0xEF, 0x006D, Kind::Text),
    (0xF0, 0x0073, Kind::Text),
    (0xF1, 0x0074, Kind::Text),
    (0xF2, 0x007E, Kind::Text),
    (0xF4, 0x010A, Kind::Text),
    (0xF6, 0x0130, Kind::Text),
    (0xF7, 0x013D, Kind::Text),
    (0xF9, 0x0178, Kind::Guid),
];

const TYPE_123: Table = &[
    (0x04, 0x0106, Kind::Number),
    (0x05, 0x015C, Kind::Number),
    (0x30, 0x0095, Kind::Number),
    (0x34, 0x00F3, Kind::Number),
    (0x35, 0x001D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x67, 0x015F, Kind::Number),
    (0x68, 0x015E, Kind::Number),
    (0x9C, 0x00DB, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x0087, Kind::Text),
    (0xDE, 0x00E0, Kind::Text),
    (0xDF, 0x0022, Kind::Text),
    (0xE0, 0x0068, Kind::Text),
    (0xE2, 0x006A, Kind::Text),
    (0xE3, 0x006B, Kind::Text),
    (0xE4, 0x006C, Kind::Text),
    (0xE5, 0x006D, Kind::Text),
    (0xE6, 0x007E, Kind::Text),
    (0xE7, 0x0072, Kind::Text),
    (0xE8, 0x010A, Kind::Text),
    (0xEA, 0x0130, Kind::Text),
    (0xEB, 0x0178, Kind::Guid),
];

const TYPE_124: Table = &[
    (0x00, 0x0094, Kind::Number),
    (0x31, 0x009F, Kind::Number),
    (0x33, 0x0155, Kind::Number),
    (0x60, 0x0036, Kind::Number),
    (0x61, 0x008D, Kind::Number),
    (0x62, 0x0096, Kind::Number),
    (0x63, 0x002C, Kind::Number),
    (0x9D, 0x00DB, Kind::Number),
    (0xDC, 0x0014, Kind::Text),
    (0xDD, 0x0087, Kind::Text),
    (0xDE, 0x00E0, Kind::Text),
    (0xDF, 0x006B, Kind::Text),
    (0xE0, 0x006C, Kind::Text),
    (0xE1, 0x006D, Kind::Text),
    (0xE2, 0x007E, Kind::Text),
    (0xE3, 0x010A, Kind::Text),
    (0xE5, 0x0130, Kind::Text),
    (0xE6, 0x013D, Kind::Text),
    (0xE8, 0x0011, Kind::Text),
    (0xEA, 0x0178, Kind::Guid),
];

const TYPE_152: Table = &[
    (0x00, 0x004B, Kind::Number),
    (0x03, 0x0010, Kind::Number),
    (0x60, 0x002C, Kind::Number),
    (0x9C, 0x001C, Kind::Number),
    (0xDC, 0x0050, Kind::Text),
    (0xDD, 0x0052, Kind::Text),
    (0xDE, 0x0054, Kind::Text),
    (0xDF, 0x0014, Kind::Text),
    (0xE0, 0x010A, Kind::Text),
    (0xE1, 0x007E, Kind::Text),
    (0xE2, 0x00E0, Kind::Text),
    (0xE3, 0x006B, Kind::Text),
    (0xE4, 0x006C, Kind::Text),
    (0xE5, 0x006D, Kind::Text),
    (0xE7, 0x0178, Kind::Guid),
];

const TYPE_155: Table = &[(0xDF, 0x0014, Kind::Text), (0xE7, 0x0178, Kind::Guid)];

const TYPE_156: Table = &[(0xDF, 0x0014, Kind::Text), (0xE7, 0x0178, Kind::Guid)];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::{
        list_forms, read_form_properties_of_type, read_form_stream_of_type, FormObjectType,
        StreamKind,
    };
    use crate::PageReader;
    use std::collections::HashMap;

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
        let mut data = vec![VERSION, 0, VERSION, 0, 0, 0, 0, 0, 0x96, 0];
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
        assert_eq!(object[0].prop_id, 0x0093);
        assert!(matches!(object[0].value, BlobValue::Short(2)));
        let names: Vec<(&str, u16)> = controls
            .iter()
            .map(|c| (c.name.as_str(), c.type_code))
            .collect();
        assert_eq!(names, [("D", 152), ("t", 109), ("Control_2", 100)]);
        let text_box = &controls[1].properties;
        assert!(matches!(text_box[0].value, BlobValue::NotDefault));
        assert_eq!(text_box[1].prop_id, 0x0036);
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
    fn the_properties_of_the_object_start_after_its_type() {
        // The form itself is an entry without its end: its type at 8, then
        // its properties. In its SaveAsText output (testdata/V2000/saveastext),
        // base_Form has DividingLines and AllowDesignChanges = NotDefault, and
        // Form_AllowLayoutView_1 DividingLines alone, followed by the 1-byte
        // DefaultView.
        let path = skip_if_missing!("V2000/propertyIdTestV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        for (name, not_default) in [
            ("base_Form", &[0x013A, 0x0175][..]),
            ("Form_AllowLayoutView_1", &[0x013A][..]),
        ] {
            let read =
                read_form_properties_of_type(&mut reader, name, FormObjectType::Form).unwrap();
            let ids: Vec<u16> = read.properties.iter().map(|p| p.prop_id).collect();
            assert_eq!(ids[..not_default.len()], *not_default, "{name}");
            let next = &read.properties[not_default.len()];
            assert_eq!(next.prop_id, 0x0093, "{name}");
            assert!(matches!(next.value, BlobValue::Short(0)), "{name}");
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
        // Given the IDs and names of Access 2002 and later.
        let named = |name: &str| {
            read.properties
                .iter()
                .find(|p| p.name() == Some(name))
                .map(|p| p.value.to_string())
        };
        assert_eq!(named("RecordSource").as_deref(), Some("tblShared"));
        assert_eq!(named("Caption").as_deref(), Some("Shared form"));
    }

    /// The properties of each node of each form and report of `file`: the
    /// object itself (""), and its sections and controls by type and name.
    fn nodes(file: &std::path::Path) -> HashMap<(String, String), Vec<BlobProperty>> {
        let mut reader = PageReader::open(file).unwrap();
        let mut out = HashMap::new();
        for entry in list_forms(&mut reader).unwrap() {
            let read =
                read_form_properties_of_type(&mut reader, &entry.name, entry.object_type).unwrap();
            let object = format!("{:?} {}", entry.object_type, entry.name);
            out.insert((object.clone(), String::new()), read.properties);
            for c in read.controls {
                let node = format!("{} {}", c.type_code & 0xFF, c.name);
                out.insert((object.clone(), node), c.properties);
            }
        }
        out
    }

    /// A value as both formats give it: a number by its value.
    fn comparable(value: &BlobValue) -> String {
        match value {
            BlobValue::Short(v) => v.to_string(),
            BlobValue::Long(v) => v.to_string(),
            other => format!("{other:?}"),
        }
    }

    /// The ID whose value the ID `id` of Access 2002 and later holds in a
    /// file saved in the Access 2000 format: Access writes the value of
    /// DatasheetGridlinesColor12 (0x0247) as DatasheetGridlinesColor (0x013F).
    fn holding(id: u16) -> u16 {
        if id == 0x013F {
            0x0247
        } else {
            id
        }
    }

    /// The pairs whose values differ between the files of the two formats,
    /// by the scope of their table (the object type, or the entry type) and
    /// the ID of Access 2002 and later. Their values are checked against the
    /// SaveAsText output of the files of the 2000 format instead.
    const DIFFERING: &[(&str, u16)] = &[
        ("Form", 0x000C),
        ("Form", 0x0081),
        ("Form", 0x0093),
        ("Form", 0x00BE),
        ("Form", 0x00BF),
        ("Form", 0x0132),
        ("Report", 0x000C),
        ("Report", 0x0081),
        ("Report", 0x0093),
        ("Report", 0x00BE),
        ("Report", 0x00BF),
        ("123", 0x002C),
        ("123", 0x0096),
        ("124", 0x002C),
        ("124", 0x0036),
        ("124", 0x008D),
        ("124", 0x0096),
        ("124", 0x009F),
    ];

    /// Whether the value of `id` at the node `key` of `nodes` is of a pair
    /// in `DIFFERING`.
    fn differs((object, node): &(String, String), id: u16) -> bool {
        let scope = if node.is_empty() { object } else { node };
        let scope = scope.split(' ').next().unwrap_or("");
        DIFFERING.contains(&(scope, id))
    }

    #[test]
    fn tied_ids_hold_the_values_of_access_2002_and_later() {
        // The same forms and reports in both formats: each property given an
        // ID of Access 2002 and later holds the value that ID holds there, or
        // the one `holding` gives, but for the pairs `differs` gives.
        for (file2000, file2007, at_least) in [
            (
                "V2000/controlPropertyTestV2000.mdb",
                "V2007/controlPropertyTestV2007.accdb",
                5000,
            ),
            (
                "V2000/propertyIdTestV2000.mdb",
                "V2007/propertyIdTestV2007.accdb",
                1000,
            ),
        ] {
            let path2000 = skip_if_missing!(file2000);
            let path2007 = skip_if_missing!(file2007);
            let later = nodes(&path2007);
            let mut checked = 0;
            for (key, properties) in nodes(&path2000) {
                let other = later.get(&key).unwrap_or_else(|| panic!("{key:?}"));
                for p in properties
                    .iter()
                    .filter(|p| p.prop_id < UNMAPPED && !differs(&key, p.prop_id))
                {
                    let there = other.iter().find(|q| q.prop_id == holding(p.prop_id));
                    assert_eq!(
                        there.map(|q| comparable(&q.value)),
                        Some(comparable(&p.value)),
                        "{key:?} {:#06x}",
                        p.prop_id
                    );
                    checked += 1;
                }
            }
            assert!(checked >= at_least, "{file2000}: {checked}");
        }
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
            .find(|p| p.prop_id == 0x0036)
            .unwrap();
        assert!(matches!(left.value, BlobValue::Short(1700)));
    }
}
