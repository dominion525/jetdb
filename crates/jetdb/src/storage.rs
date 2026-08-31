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
pub(crate) struct StorageEntry {
    pub id: i32,
    pub parent_id: i32,
    pub name: String,
    pub entry_type: i32,
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read all entries from the MSysAccessStorage system table.
pub(crate) fn read_storage_entries(
    reader: &mut PageReader,
) -> Result<Vec<StorageEntry>, FileError> {
    // Find MSysAccessStorage in the catalog
    let catalog = catalog::read_catalog(reader)?;
    let entry = match catalog.iter().find(|e| e.name == "MSysAccessStorage") {
        Some(e) => e,
        None => return Ok(Vec::new()),
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
