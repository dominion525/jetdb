//! MSysAccessStorage table reading — shared infrastructure for VBA, form/report,
//! and macro extraction.

use std::collections::HashSet;

use crate::catalog;
use crate::data::{self, Value};
use crate::encoding;
use crate::file::{FileError, PageReader};
use crate::table;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single entry from the MSysAccessStorage table.
#[derive(Debug, Clone)]
pub(crate) struct StorageEntry {
    pub id: i32,
    pub parent_id: i32,
    pub name: String,
    pub entry_type: i32,
    pub data: Vec<u8>,
}

/// The storage where Access keeps the designs of forms and reports, macros
/// and the VBA project: the `MSysAccessStorage` table, or `MSysAccessObjects`
/// in Access 97. [`read_access_storage`] reads it once for the `_in`
/// functions, such as [`crate::list_forms_in`], which would otherwise each
/// read the whole table again.
#[derive(Debug, Clone)]
pub struct AccessStorage {
    pub(crate) entries: Vec<StorageEntry>,
    /// The code page in the header of the database, in which jetdb reads the
    /// names in the TypeInfo of forms and reports.
    pub(crate) code_page: u16,
}

/// Read the storage of forms, reports, macros and VBA (see [`AccessStorage`]).
pub fn read_access_storage(reader: &mut PageReader) -> Result<AccessStorage, FileError> {
    Ok(AccessStorage {
        entries: read_storage_entries(reader)?,
        code_page: reader.header().code_page,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read all entries from the MSysAccessStorage system table.
///
/// Databases in the Access 2000 format have no MSysAccessStorage; the same
/// tree is a compound file stored in MSysAccessObjects, which is converted
/// into entries of the same shape.
pub(crate) fn read_storage_entries(
    reader: &mut PageReader,
) -> Result<Vec<StorageEntry>, FileError> {
    // Find MSysAccessStorage in the catalog
    let catalog = catalog::read_catalog(reader)?;
    let entry = match catalog.iter().find(|e| e.name == "MSysAccessStorage") {
        Some(e) => e,
        None => {
            let cfb_bytes = read_access_objects_cfb(reader)?;
            if cfb_bytes.is_empty() {
                return Ok(Vec::new());
            }
            return entries_from_cfb(cfb_bytes);
        }
    };

    let tdef = table::read_table_def(reader, &entry.name, entry.table_page)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysAccessStorage");

    // Locate column indices
    let (mut id_idx, mut parent_id_idx, mut name_idx, mut type_idx, mut lv_idx) =
        (None, None, None, None, None);
    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "Id" => id_idx = Some(i),
            "ParentId" => parent_id_idx = Some(i),
            "Name" => name_idx = Some(i),
            "Type" => type_idx = Some(i),
            "Lv" => lv_idx = Some(i),
            _ => {}
        }
    }

    let id_idx = id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Id column",
    })?;
    let parent_id_idx = parent_id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing ParentId column",
    })?;
    let name_idx = name_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Name column",
    })?;
    let type_idx = type_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Type column",
    })?;
    let lv_idx = lv_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessStorage missing Lv column",
    })?;

    let mut entries = Vec::new();
    for row in &result.rows {
        let id = match row.get(id_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let parent_id = match row.get(parent_id_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let name = match row.get(name_idx) {
            Some(Value::Text(s)) => s.clone(),
            _ => continue,
        };
        let entry_type = match row.get(type_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let data = match row.get(lv_idx) {
            Some(Value::Binary(b)) => b.clone(),
            _ => Vec::new(),
        };

        entries.push(StorageEntry {
            id,
            parent_id,
            name,
            entry_type,
            data,
        });
    }

    Ok(entries)
}

/// Read the MSysAccessObjects table and reconstruct the full CFB.
///
/// In MSysAccessObjects format, all database objects (forms, reports, VBA,
/// etc.) are stored in a single large OLE2/CFB file split across multiple
/// rows in MSysAccessObjects. Row 0 is a metadata/directory entry; rows 1+
/// contain the CFB data that should be concatenated in ID order.
pub(crate) fn read_access_objects_cfb(reader: &mut PageReader) -> Result<Vec<u8>, FileError> {
    let catalog = catalog::read_catalog(reader)?;
    let entry = match catalog.iter().find(|e| e.name == "MSysAccessObjects") {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let tdef = table::read_table_def(reader, &entry.name, entry.table_page)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysAccessObjects");

    // Locate column indices
    let (mut data_idx, mut id_idx) = (None, None);
    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "Data" => data_idx = Some(i),
            "ID" => id_idx = Some(i),
            _ => {}
        }
    }

    let data_idx = data_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessObjects missing Data column",
    })?;
    let id_idx = id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysAccessObjects missing ID column",
    })?;

    // Collect rows with their IDs
    let mut rows: Vec<(i32, Vec<u8>)> = Vec::new();
    for row in &result.rows {
        let id = match row.get(id_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let data = match row.get(data_idx) {
            Some(Value::Binary(b)) => b.clone(),
            _ => continue,
        };
        rows.push((id, data));
    }

    // Sort by ID, skip row 0 (metadata), concatenate
    rows.sort_by_key(|(id, _)| *id);

    let mut cfb_bytes = Vec::new();
    for (id, data) in &rows {
        if *id == 0 {
            continue;
        }
        cfb_bytes.extend_from_slice(data);
    }

    if cfb_bytes.len() < 4 || cfb_bytes[..4] != [0xD0, 0xCF, 0x11, 0xE0] {
        return Ok(Vec::new());
    }

    Ok(cfb_bytes)
}

/// Converts a compound file into entries shaped like MSysAccessStorage rows:
/// the root storage is id 1 (its own parent), and every other storage or
/// stream gets the next id with its parent's id.
fn entries_from_cfb(cfb_bytes: Vec<u8>) -> Result<Vec<StorageEntry>, FileError> {
    use std::collections::HashMap;
    use std::io::Read;

    const INVALID: FileError = FileError::InvalidFormData {
        reason: "invalid compound file in MSysAccessObjects",
    };
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(cfb_bytes)).map_err(|_| INVALID)?;
    let walked: Vec<(std::path::PathBuf, bool)> = cf
        .walk()
        .map(|e| (e.path().to_path_buf(), e.is_storage()))
        .collect();

    let mut ids: HashMap<std::path::PathBuf, i32> = HashMap::new();
    let mut entries = Vec::new();
    for (path, is_storage) in walked {
        let id = entries.len() as i32 + 1;
        let (parent_id, name) = match path.parent() {
            None => (id, "MSysAccessStorage_ROOT".to_string()),
            Some(parent) => (
                *ids.get(parent).ok_or(INVALID)?,
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
        };
        let mut data = Vec::new();
        if !is_storage {
            cf.open_stream(&path)
                .and_then(|mut s| s.read_to_end(&mut data))
                .map_err(|_| INVALID)?;
        }
        ids.insert(path, id);
        entries.push(StorageEntry {
            id,
            parent_id,
            name,
            entry_type: if is_storage { 1 } else { 2 },
            data,
        });
    }
    Ok(entries)
}

/// Check if a storage entry is a storage (directory) vs stream (file).
///
/// Type values: 1 = storage, 2 = stream (observed in Access databases).
pub(crate) fn is_storage(entry: &StorageEntry) -> bool {
    entry.entry_type == 1
}

/// Recursively collect children of a given parent ID.
pub(crate) fn collect_children<'a>(
    entries: &'a [StorageEntry],
    parent_id: i32,
    result: &mut Vec<&'a StorageEntry>,
    visited: &mut HashSet<i32>,
) {
    for entry in entries {
        if entry.parent_id == parent_id && visited.insert(entry.id) {
            result.push(entry);
            collect_children(entries, entry.id, result, visited);
        }
    }
}

/// Find the root entry ID (MSysAccessStorage_ROOT).
///
/// The root has `parent_id == id` (self-referencing) or is simply id=1.
pub(crate) fn find_root_id(entries: &[StorageEntry]) -> i32 {
    entries
        .iter()
        .find(|e| e.parent_id == e.id && is_storage(e))
        .map(|e| e.id)
        .unwrap_or(1)
}

/// Find the DirData stream entry under a folder.
///
/// The name may be prefixed with a control character (e.g., "\x03DirData").
pub(crate) fn find_dir_data(entries: &[StorageEntry], folder_id: i32) -> Option<&StorageEntry> {
    entries.iter().find(|e| {
        e.parent_id == folder_id
            && !is_storage(e)
            && (e.name == "DirData" || e.name.ends_with("DirData"))
    })
}

// ---------------------------------------------------------------------------
// Internal: DirData parser
// ---------------------------------------------------------------------------

/// Parse DirData binary into (name, storage_number) pairs.
///
/// Format:
/// - 4-byte header (zeros)
/// - Entries: `[0x04] [len:u8] [UTF-16LE name] [storage_index:u16LE] [0x0000]`
///
/// `len` is normally reliable, but can be too short when names contain characters
/// whose UTF-16LE low byte is 0x00 (e.g., U+4E00 '一' → 00 4E). We use `len` as
/// the primary boundary but fall back to scanning if the payload doesn't end with
/// a null terminator.
pub(crate) fn parse_dir_data(data: &[u8]) -> Result<Vec<(String, String)>, FileError> {
    if data.len() < 4 {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let mut pos = 4; // skip header

    while pos + 1 < data.len() {
        if data[pos] != 0x04 {
            break;
        }
        let declared_len = data[pos + 1] as usize;
        pos += 2; // skip marker and len byte

        if declared_len < 4 || pos + declared_len > data.len() {
            break;
        }

        // Try declared_len first: check if payload ends with 0x0000
        let payload_end = pos + declared_len;
        let ends_with_null =
            payload_end >= 2 && data[payload_end - 2] == 0x00 && data[payload_end - 1] == 0x00;

        let actual_end = if ends_with_null {
            payload_end
        } else {
            // declared_len is wrong; scan forward for the null terminator.
            // Look for a u16-aligned 0x0000 that is followed by 0x04 or EOF.
            let mut scan = pos + declared_len;
            loop {
                if scan + 1 >= data.len() {
                    break scan + 1; // end of data
                }
                let val = u16::from_le_bytes([data[scan], data[scan + 1]]);
                if val == 0x0000 {
                    break scan + 2; // past the null terminator
                }
                scan += 2;
            }
        };

        // Payload layout: [name UTF-16LE] [storage_index u16LE] [0x0000]
        // Last 4 bytes: storage_index(2) + null(2)
        if actual_end < pos + 4 {
            pos = actual_end;
            continue;
        }

        let name_bytes = &data[pos..actual_end - 4];
        let storage_index = u16::from_le_bytes([data[actual_end - 4], data[actual_end - 3]]);

        if name_bytes.is_empty() {
            pos = actual_end;
            continue;
        }

        let name =
            encoding::decode_utf16le(name_bytes).map_err(|_| FileError::InvalidFormData {
                reason: "invalid UTF-16LE in DirData name",
            })?;

        entries.push((name, storage_index.to_string()));
        pos = actual_end;
    }

    Ok(entries)
}

/// Build the CFB path for a storage entry relative to a given root.
///
/// Returns `None` if the parent chain is broken (circular reference or
/// missing parent), logging a warning so the caller can skip the entry.
pub(crate) fn build_entry_path(
    entry: &StorageEntry,
    root_id: i32,
    id_map: &std::collections::HashMap<i32, &StorageEntry>,
) -> Option<String> {
    let mut parts = vec![entry.name.clone()];
    let mut current_parent = entry.parent_id;
    let mut visited = HashSet::new();

    // Walk up the tree, stopping at the root (which is the CFB root)
    while current_parent != root_id {
        if !visited.insert(current_parent) {
            log::warn!(
                "skipping entry '{}': circular reference in parent chain",
                entry.name
            );
            return None;
        }
        match id_map.get(&current_parent) {
            Some(parent) => {
                parts.push(parent.name.clone());
                current_parent = parent.parent_id;
            }
            None => {
                log::warn!(
                    "skipping entry '{}': missing parent id {}",
                    entry.name,
                    current_parent
                );
                return None;
            }
        }
    }

    parts.reverse();
    Some(format!("/{}", parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `.mdb` and `.accdb` file under `dir`.
    fn databases(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(databases(&path));
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("mdb" | "accdb")
            ) {
                out.push(path);
            }
        }
        out
    }

    fn debug(value: &dyn std::fmt::Debug) -> String {
        format!("{value:?}")
    }

    /// Compares the forms and reports, their properties and their embedded
    /// macros, as the functions given `storage` read them and as the others
    /// do, and gives how many forms and reports were compared.
    fn compare_forms(reader: &mut PageReader, storage: &AccessStorage, at: &str) -> usize {
        assert_eq!(
            debug(&crate::list_forms(reader).ok()),
            debug(&crate::list_forms_in(storage).ok()),
            "{at}"
        );
        let forms = crate::list_forms(reader).unwrap_or_default();
        for form in &forms {
            let (name, t) = (&form.name, form.object_type);
            assert_eq!(
                debug(&crate::read_form_properties_of_type(reader, name, t).ok()),
                debug(&crate::read_form_properties_in(storage, name, Some(t)).ok()),
                "{at}: {name}"
            );
            assert_eq!(
                debug(&crate::read_embedded_macros_of_type(reader, name, t).ok()),
                debug(&crate::read_embedded_macros_in(storage, name, Some(t)).ok()),
                "{at}: {name}"
            );
        }
        forms.len()
    }

    /// Compares the macros in the same way, and gives how many were compared.
    fn compare_macros(reader: &mut PageReader, storage: &AccessStorage, at: &str) -> usize {
        let names = crate::list_macros(reader).ok();
        assert_eq!(
            debug(&names),
            debug(&crate::list_macros_in(reader, storage).ok()),
            "{at}"
        );
        let names = names.unwrap_or_default();
        for entry in &names {
            assert_eq!(
                debug(&crate::read_macro(reader, &entry.name).ok()),
                debug(&crate::read_macro_in(reader, storage, &entry.name).ok()),
                "{at}: {}",
                entry.name
            );
        }
        names.len()
    }

    /// Compares the VBA project in the same way, and gives how many modules
    /// it has.
    fn compare_vba(reader: &mut PageReader, storage: &AccessStorage, at: &str) -> usize {
        let project = crate::read_vba_project(reader).ok();
        assert_eq!(
            debug(&project),
            debug(&crate::read_vba_project_in(reader, storage).ok()),
            "{at}"
        );
        project.map_or(0, |p| p.modules.len())
    }

    #[test]
    fn the_in_functions_read_what_the_others_read() {
        // For every test database that opens without a password, the
        // functions given the storage already read give what the others give.
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
        let (mut forms, mut macros, mut modules) = (0, 0, 0);
        for path in databases(&root) {
            let Ok(mut reader) = PageReader::open(&path) else {
                continue;
            };
            let Ok(storage) = read_access_storage(&mut reader) else {
                continue;
            };
            let at = path.display().to_string();
            forms += compare_forms(&mut reader, &storage, &at);
            macros += compare_macros(&mut reader, &storage, &at);
            modules += compare_vba(&mut reader, &storage, &at);
        }
        // Forms and reports, macros and modules were all compared.
        assert!(
            forms > 0 && macros > 0 && modules > 0,
            "{forms} {macros} {modules}"
        );
    }

    /// DirData of the entries `(marker, declared length, payload)`, after its
    /// header of four zeros.
    fn dir_data(entries: &[(u8, u8, &[u8])]) -> Vec<u8> {
        let mut data = vec![0u8; 4];
        for (marker, len, payload) in entries {
            data.extend_from_slice(&[*marker, *len]);
            data.extend_from_slice(payload);
        }
        data
    }

    /// The payload of an entry: `name` in UTF-16LE, `index` and a null.
    fn dir_data_payload(name: &str, index: u16) -> Vec<u8> {
        let mut payload: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        payload.extend_from_slice(&index.to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload
    }

    #[test]
    fn parse_dir_data_entries() {
        let form = dir_data_payload("Form1", 3);
        let report = dir_data_payload("Report1", 12);
        let data = dir_data(&[
            (4, form.len() as u8, &form),
            (4, report.len() as u8, &report),
        ]);
        assert_eq!(
            parse_dir_data(&data).unwrap(),
            [
                ("Form1".to_string(), "3".to_string()),
                ("Report1".to_string(), "12".to_string())
            ]
        );
        assert_eq!(parse_dir_data(&[0, 0, 0]).unwrap(), []);
    }

    #[test]
    fn parse_dir_data_with_a_length_too_short() {
        // 一 is 00 4E in UTF-16LE. With a declared length of 4, the payload
        // seems to end at the index, which is not a null, so the null that
        // follows is looked for.
        let payload = dir_data_payload("一", 5);
        let data = dir_data(&[(4, 4, &payload)]);
        assert_eq!(
            parse_dir_data(&data).unwrap(),
            [("一".to_string(), "5".to_string())]
        );
    }

    #[test]
    fn parse_dir_data_passes_over_an_empty_name_and_stops_at_another_marker() {
        let empty = dir_data_payload("", 1);
        let form = dir_data_payload("Form1", 3);
        let data = dir_data(&[(4, 4, &empty), (4, form.len() as u8, &form), (5, 0, &[])]);
        assert_eq!(
            parse_dir_data(&data).unwrap(),
            [("Form1".to_string(), "3".to_string())]
        );
    }

    #[test]
    fn parse_dir_data_with_a_name_not_utf16() {
        // A lone high surrogate.
        let payload = [0x00, 0xD8, 0x01, 0x00, 0x00, 0x00];
        let data = dir_data(&[(4, 6, &payload)]);
        assert!(matches!(
            parse_dir_data(&data),
            Err(FileError::InvalidFormData { reason }) if reason == "invalid UTF-16LE in DirData name"
        ));
    }

    #[test]
    fn parse_dir_data_with_a_length_to_the_end_without_a_null() {
        // The declared length reaches the end of the data, whose last two
        // bytes are not a null. The end is then taken to be one past the
        // data, so the name is read as the three bytes before the index,
        // which are not UTF-16LE.
        let payload = [0x41, 0x00, 0x07, 0x00, 0x01, 0x00];
        let data = dir_data(&[(4, 6, &payload)]);
        assert!(matches!(
            parse_dir_data(&data),
            Err(FileError::InvalidFormData { reason }) if reason == "invalid UTF-16LE in DirData name"
        ));
    }
}
