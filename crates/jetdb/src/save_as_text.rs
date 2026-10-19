//! Access's SaveAsText output of forms and reports, read for the tests that
//! check the values jetdb reads from a Blob against it.

use std::collections::{HashMap, HashSet};

use crate::form::{BlobProperty, BlobValue, FormObjectType, FormProperties};

/// A value of a SaveAsText line: the text of a quoted value, unescaped and
/// joined with its continuation lines, the bytes of a binary value
/// (`Key = Begin`, lines of `0x...`, `End`), or the bare text of another.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Saved {
    Quoted(String),
    Bytes(Vec<u8>),
    Bare(String),
}

/// The values of a block of a SaveAsText file, by key.
pub(crate) type Block = HashMap<String, Vec<Saved>>;

/// The blocks of a SaveAsText file by the name of their control, "" for the
/// form or report itself. Blocks without a name, such as those of the
/// default controls, are left out.
pub(crate) fn read(text: &str) -> HashMap<String, Block> {
    let mut reader = Reader::default();
    for line in text.trim_start_matches('\u{feff}').lines() {
        reader.line(line.trim());
    }
    reader.found
}

#[derive(Default)]
struct Reader {
    found: HashMap<String, Block>,
    /// The blocks being read, each with its name once its Name line is read.
    blocks: Vec<(Option<String>, Block)>,
    /// The key of the binary value being read.
    binary: Option<String>,
    /// The key of the quoted value a continuation line adds to.
    last: Option<String>,
}

impl Reader {
    fn line(&mut self, line: &str) {
        if let Some(key) = &self.binary {
            if line == "End" {
                self.binary = None;
            } else if let Some(Saved::Bytes(bytes)) = self.last_value(key.clone()) {
                bytes.extend(hex_bytes(line));
            }
            return;
        }
        if !line.starts_with('"') {
            self.last = None;
        }
        if let Some(key) = binary_start(line) {
            self.push(key.to_string(), Saved::Bytes(Vec::new()));
            self.binary = Some(key.to_string());
        } else if line == "Begin" || line.starts_with("Begin ") {
            self.blocks.push((None, Block::new()));
        } else if line == "End" {
            self.close();
        } else {
            self.property(line);
        }
    }

    fn last_value(&mut self, key: String) -> Option<&mut Saved> {
        let (_, block) = self.blocks.last_mut()?;
        block.get_mut(&key)?.last_mut()
    }

    fn push(&mut self, key: String, saved: Saved) {
        if let Some((name, block)) = self.blocks.last_mut() {
            if let (Saved::Quoted(text), "Name", None) = (&saved, key.as_str(), &name) {
                *name = Some(text.clone());
            }
            block.entry(key).or_default().push(saved);
        }
    }

    fn close(&mut self) {
        let Some((name, block)) = self.blocks.pop() else {
            return;
        };
        let name = if self.blocks.is_empty() {
            Some(String::new())
        } else {
            name
        };
        if let Some(name) = name {
            let found = self.found.entry(name).or_default();
            for (key, values) in block {
                found.entry(key).or_default().extend(values);
            }
        }
    }

    fn property(&mut self, line: &str) {
        if let (Some(key), Some(more)) = (self.last.clone(), quoted(line)) {
            if let Some(Saved::Quoted(text)) = self.last_value(key) {
                text.push_str(&more);
            }
            return;
        }
        let Some((key, value)) = line.split_once('=') else {
            return;
        };
        let (key, value) = (key.trim().to_string(), value.trim());
        match quoted(value) {
            Some(text) => {
                self.last = Some(key.clone());
                self.push(key, Saved::Quoted(text));
            }
            None => self.push(key, Saved::Bare(value.to_string())),
        }
    }
}

/// The key of the binary value `line` starts: `Key = Begin`, or
/// `Key =Begin`.
fn binary_start(line: &str) -> Option<&str> {
    let (key, rest) = line.split_once('=')?;
    (rest.trim() == "Begin").then(|| key.trim())
}

/// The bytes of a line of a binary value, such as `0xa2050000 ,`.
fn hex_bytes(line: &str) -> Vec<u8> {
    let hex = line.trim_end_matches(',').trim().trim_start_matches("0x");
    (0..hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

/// The GUID `bytes` holds, as jetdb writes one: its first three fields
/// little-endian, in lower case. Written here apart from the library, so that
/// the GUIDs jetdb reads are checked against SaveAsText on their own.
fn guid(bytes: &[u8]) -> Option<String> {
    let b: [u8; 16] = bytes.try_into().ok()?;
    let hex = |range: &[u8]| range.iter().map(|x| format!("{x:02x}")).collect::<String>();
    Some(format!(
        "{{{:08x}-{:04x}-{:04x}-{}-{}}}",
        u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        u16::from_le_bytes([b[4], b[5]]),
        u16::from_le_bytes([b[6], b[7]]),
        hex(&b[8..10]),
        hex(&b[10..16]),
    ))
}

/// The text of the quoted value `value`, unescaped: `\"`, `\\`, and three
/// octal digits for a byte, such as `\015\012` for a line break.
fn quoted(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut text = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        let rest = chars.as_str();
        match rest.get(..3).and_then(|d| u8::from_str_radix(d, 8).ok()) {
            Some(byte) => {
                text.push(char::from(byte));
                chars = rest[3..].chars();
            }
            None => text.extend(chars.next()),
        }
    }
    Some(text)
}

/// Whether `saved` is `value` as SaveAsText writes it. A Yes value is -1,
/// 1 or 255, as Access writes different properties; a double can be written
/// as its 8 bytes, as the date and time `RecSrcDt`.
pub(crate) fn holds(saved: &Saved, value: &BlobValue) -> bool {
    match saved {
        Saved::Quoted(text) => matches!(value, BlobValue::Text(t) if t == text),
        Saved::Bytes(bytes) => match value {
            BlobValue::Binary(b) => b == bytes,
            BlobValue::Guid(g) => guid(bytes).is_some_and(|text| text == *g),
            BlobValue::Double(v) => *bytes == v.to_le_bytes(),
            _ => false,
        },
        Saved::Bare(bare) => holds_bare(bare, value),
    }
}

fn holds_bare(bare: &str, value: &BlobValue) -> bool {
    let integer = bare.parse::<i64>().ok();
    match *value {
        BlobValue::NotDefault => bare == "NotDefault",
        BlobValue::Short(v) => integer == Some(i64::from(v)),
        BlobValue::Long(v) => integer == Some(i64::from(v)),
        BlobValue::Bool(true) => matches!(integer, Some(-1 | 1 | 255)),
        BlobValue::Bool(false) => integer == Some(0),
        BlobValue::Double(v) => bare
            .parse::<f64>()
            .is_ok_and(|n| (n - v).abs() <= 1e-6 * v.abs().max(1.0)),
        _ => false,
    }
}

/// The kind of a value an ID is kept by: an ID holds text and a number as
/// different properties where one does not apply (see
/// `crate::form::prop_id_name`), but a number whatever its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Text,
    Number,
    NotDefault,
    Bytes,
}

fn kind(value: &BlobValue) -> Kind {
    match value {
        BlobValue::Text(_) => Kind::Text,
        BlobValue::NotDefault => Kind::NotDefault,
        BlobValue::Short(_) | BlobValue::Long(_) | BlobValue::Bool(_) | BlobValue::Double(_) => {
            Kind::Number
        }
        BlobValue::Binary(_) | BlobValue::Guid(_) => Kind::Bytes,
    }
}

/// Whether the value of `p` in `block` is one SaveAsText does not write:
///
/// - 0x0000, which no key holds;
/// - 0x0159, which no key holds, and which a combo box with a row source of
///   a table can have: the SQL of its row source with the names of its table
///   and columns;
/// - a property of `unwritten`, when the block has no key of its name.
fn left_out(p: &BlobProperty, block: &Block, unwritten: &[(u16, &str)]) -> bool {
    matches!(p.prop_id, 0x0000 | 0x0159)
        || unwritten
            .iter()
            .any(|(id, key)| *id == p.prop_id && !block.contains_key(*key))
}

/// What a property is kept by: its ID, the kind of its value, and for an ID
/// of the Access 2000 format without a pair (the ID plus 0x8000), which is
/// numbered in each type, the type of its object or control.
pub(crate) type Key = (u16, Kind, u16);

/// A value checked against its block: the keys of the block that hold it.
pub(crate) struct Observation {
    pub(crate) key: Key,
    pub(crate) found: HashSet<String>,
    pub(crate) name: Option<&'static str>,
}

/// How the SaveAsText output of a file is checked.
#[derive(Default)]
pub(crate) struct Rules {
    /// The properties, with their keys, its SaveAsText output may not write:
    /// an older Access does not write some that Access for Microsoft 365
    /// does. Such a value is left out where its block has no key of the name.
    pub(crate) unwritten: &'static [(u16, &'static str)],
    /// Whether the output is that of the database before it was saved in the
    /// Access 2000 format, as the generators export each object in the
    /// .accdb they make. The values of IDs without a pair, which the Access
    /// 2000 format keeps in its own way, are then not checked: one of them
    /// can be held by a key of the output only by chance. The object may have
    /// fewer NotDefault values than the output: the Blob reads to its end, so
    /// what it does not give is not in it as a NotDefault value.
    pub(crate) converted: bool,
}

/// What the forms and reports of a file gave.
#[derive(Default)]
pub(crate) struct Checked {
    pub(crate) rules: Rules,
    /// The values of IDs without a pair of a converted file, not checked.
    pub(crate) unpaired: usize,
    pub(crate) observations: Vec<Observation>,
    /// The values no key of their block holds.
    pub(crate) unmatched: Vec<String>,
    /// The blocks that differ from what jetdb reads otherwise: a control
    /// without a block or a block without a control, another number of
    /// NotDefault values, or an ID given twice with different values.
    pub(crate) problems: Vec<String>,
    /// How many values are of an ID their section, control or object has
    /// before: Access writes some properties twice in a Blob, with the same
    /// value.
    pub(crate) repeated: usize,
}

impl Checked {
    /// Checks the values of `read` against `saved`, its SaveAsText blocks,
    /// except those of the controls `skipped`.
    pub(crate) fn object(
        &mut self,
        saved: &HashMap<String, Block>,
        read: &FormProperties,
        skipped: &[&str],
    ) {
        let mut names = HashSet::new();
        for control in &read.controls {
            if !names.insert(control.name.as_str()) {
                self.problems.push(format!(
                    "{}: a control given twice: {}",
                    read.form_name, control.name
                ));
            }
        }
        for name in saved
            .keys()
            .filter(|n| !n.is_empty() && !names.contains(n.as_str()))
        {
            self.problems.push(format!(
                "{}: a block without a control: {name}",
                read.form_name
            ));
        }
        let report = read.object_type == FormObjectType::Report;
        let object_scope = 0x100 | u16::from(report);
        self.group(
            &read.form_name,
            saved.get(""),
            &read.properties,
            object_scope,
        );
        for control in read
            .controls
            .iter()
            .filter(|c| !skipped.contains(&c.name.as_str()))
        {
            let at = format!("{}/{}", read.form_name, control.name);
            let scope = control.type_code & 0xFF;
            self.group(&at, saved.get(&control.name), &control.properties, scope);
        }
    }

    fn group(&mut self, at: &str, block: Option<&Block>, properties: &[BlobProperty], scope: u16) {
        let Some(block) = block else {
            self.problems.push(format!("{at}: no block"));
            return;
        };
        self.count_not_default(at, block, properties);
        let mut seen: HashMap<u16, &BlobValue> = HashMap::new();
        for p in properties {
            if let Some(first) = seen.insert(p.prop_id, &p.value) {
                self.repeated += 1;
                // BlobValue is compared by its Debug output, having no PartialEq.
                if format!("{first:?}") != format!("{:?}", p.value) {
                    self.problems.push(format!(
                        "{at} {:#06x}: {first:?}, then {:?}",
                        p.prop_id, p.value
                    ));
                }
            }
            if !left_out(p, block, self.rules.unwritten) {
                self.value(at, block, p, scope);
            }
        }
    }

    fn count_not_default(&mut self, at: &str, block: &Block, properties: &[BlobProperty]) {
        let read = properties
            .iter()
            .filter(|p| matches!(p.value, BlobValue::NotDefault))
            .count();
        let written = block
            .values()
            .flatten()
            .filter(|s| matches!(s, Saved::Bare(b) if b == "NotDefault"))
            .count();
        if read != written && !(self.rules.converted && read < written) {
            self.problems.push(format!(
                "{at}: {read} NotDefault values, {written} in SaveAsText"
            ));
        }
    }

    fn value(&mut self, at: &str, block: &Block, p: &BlobProperty, scope: u16) {
        if self.rules.converted && p.prop_id >= 0x8000 {
            self.unpaired += 1;
            return;
        }
        let found: HashSet<String> = block
            .iter()
            .filter(|(_, saved)| saved.iter().any(|s| holds(s, &p.value)))
            .map(|(key, _)| key.clone())
            .collect();
        if found.is_empty() {
            self.unmatched
                .push(format!("{at} {:#06x} {:?}", p.prop_id, p.value));
            return;
        }
        let scope = if p.prop_id >= 0x8000 { scope } else { 0 };
        self.observations.push(Observation {
            key: (p.prop_id, kind(&p.value), scope),
            found,
            name: p.name(),
        });
    }
}

/// The keys that hold every value of each property, over the observations
/// added.
#[derive(Default)]
pub(crate) struct Agreement {
    keys: HashMap<Key, (HashSet<String>, Option<&'static str>)>,
}

impl Agreement {
    pub(crate) fn add<'a>(&mut self, observations: impl IntoIterator<Item = &'a Observation>) {
        for o in observations {
            let (keys, _) = self
                .keys
                .entry(o.key)
                .or_insert_with(|| (o.found.clone(), o.name));
            keys.retain(|k| o.found.contains(k));
        }
    }

    /// The properties no key holds every value of.
    pub(crate) fn disagreements(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .keys
            .iter()
            .filter(|(_, (keys, _))| keys.is_empty())
            .map(|(key, _)| format!("{key:x?}: no key holds every value"))
            .collect();
        out.sort();
        out
    }

    /// The named properties not held by their name alone.
    pub(crate) fn misnamed(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .keys
            .iter()
            .filter_map(|(key, (keys, name))| {
                let name = (*name)?;
                (keys.len() != 1 || !keys.contains(name))
                    .then(|| format!("{key:x?} {name}: held by {keys:?}"))
            })
            .collect();
        out.sort();
        out
    }

    /// The keys held alone by more than one ID with a pair, of the same
    /// kind of value.
    pub(crate) fn shared(&self) -> Vec<String> {
        let mut by_key: HashMap<(&str, Kind), Vec<u16>> = HashMap::new();
        for ((id, kind, scope), (keys, _)) in &self.keys {
            if let (0, [key]) = (*scope, keys.iter().collect::<Vec<_>>().as_slice()) {
                by_key.entry((key.as_str(), *kind)).or_default().push(*id);
            }
        }
        let mut out: Vec<String> = by_key
            .into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(key, ids)| format!("{key:?}: {ids:x?}"))
            .collect();
        out.sort();
        out
    }

    /// Whether `key` alone holds every value of the ID `id` with a pair.
    pub(crate) fn holds(&self, id: u16, key: &str) -> bool {
        self.keys
            .iter()
            .any(|((i, _, _), (keys, _))| *i == id && keys.len() == 1 && keys.contains(key))
    }

    /// How many named properties there are.
    pub(crate) fn named(&self) -> usize {
        self.keys
            .values()
            .filter(|(_, name)| name.is_some())
            .count()
    }
}
