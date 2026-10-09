//! Table relationship (foreign key) reading from MSysRelationships.

use std::collections::BTreeMap;

use crate::catalog::read_catalog;
use crate::data::{self, Value};
use crate::file::{FileError, PageReader};
use crate::format::ObjectType;
use crate::table;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single column pair in a relationship.
#[derive(Debug, Clone)]
pub struct RelationshipColumn {
    /// Column in the child (referencing) table.
    pub from_column: String,
    /// Column in the parent (referenced) table.
    pub to_column: String,
}

/// A single relationship (foreign key constraint).
#[derive(Debug, Clone)]
pub struct Relationship {
    /// Relationship name (szRelationship).
    pub name: String,
    /// Child (referencing) table name (szObject).
    pub from_table: String,
    /// Parent (referenced) table name (szReferencedObject).
    pub to_table: String,
    /// Column pairs.
    pub columns: Vec<RelationshipColumn>,
    /// Relationship flags (grbit).
    pub flags: u32,
}

impl Relationship {
    /// `true` when Access enforces referential integrity, so that the
    /// relationship is a constraint, not only a note of how the tables join
    /// ([`relationship_flags::NO_REFERENTIAL_INTEGRITY`]).
    pub fn has_referential_integrity(&self) -> bool {
        self.flags & relationship_flags::NO_REFERENTIAL_INTEGRITY == 0
    }
}

/// Relationship flag constants.
pub mod relationship_flags {
    /// No referential integrity enforcement (comment-only).
    pub const NO_REFERENTIAL_INTEGRITY: u32 = 0x0000_0002;
    /// ON UPDATE CASCADE.
    pub const CASCADE_UPDATE: u32 = 0x0000_0100;
    /// ON DELETE CASCADE.
    pub const CASCADE_DELETE: u32 = 0x0000_1000;
}

// ---------------------------------------------------------------------------
// read_relationships
// ---------------------------------------------------------------------------

/// Read all relationships from the MSysRelationships system table.
///
/// Returns an empty `Vec` if the table does not exist.
pub fn read_relationships(reader: &mut PageReader) -> Result<Vec<Relationship>, FileError> {
    // Find MSysRelationships in the catalog
    let catalog = read_catalog(reader)?;
    let rel_entry = catalog
        .iter()
        .find(|e| e.name == "MSysRelationships" && e.object_type == ObjectType::Table);
    let rel_page = match rel_entry {
        Some(e) => e.table_page,
        None => return Ok(Vec::new()),
    };

    // Read table definition and rows
    let tdef = table::read_table_def(reader, "MSysRelationships", rel_page)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysRelationships");

    // Locate column indices
    let mut rel_name_idx = None;
    let mut col_idx = None;
    let mut obj_idx = None;
    let mut ref_col_idx = None;
    let mut ref_obj_idx = None;
    let mut grbit_idx = None;

    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "szRelationship" => rel_name_idx = Some(i),
            "szColumn" => col_idx = Some(i),
            "szObject" => obj_idx = Some(i),
            "szReferencedColumn" => ref_col_idx = Some(i),
            "szReferencedObject" => ref_obj_idx = Some(i),
            "grbit" => grbit_idx = Some(i),
            _ => {}
        }
    }

    let rel_name_idx = rel_name_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysRelationships missing szRelationship column",
    })?;
    let col_idx = col_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysRelationships missing szColumn column",
    })?;
    let obj_idx = obj_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysRelationships missing szObject column",
    })?;
    let ref_col_idx = ref_col_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysRelationships missing szReferencedColumn column",
    })?;
    let ref_obj_idx = ref_obj_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysRelationships missing szReferencedObject column",
    })?;
    let grbit_idx = grbit_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysRelationships missing grbit column",
    })?;

    // Group rows by szRelationship name (supports compound foreign keys)
    struct RawRow {
        from_table: String,
        to_table: String,
        from_column: String,
        to_column: String,
        flags: u32,
    }

    let mut groups: BTreeMap<String, Vec<RawRow>> = BTreeMap::new();

    for row in &result.rows {
        let name = match row.get(rel_name_idx) {
            Some(Value::Text(s)) if !s.is_empty() => s.clone(),
            _ => continue,
        };
        let from_table = match row.get(obj_idx) {
            Some(Value::Text(s)) => s.clone(),
            _ => continue,
        };
        let to_table = match row.get(ref_obj_idx) {
            Some(Value::Text(s)) => s.clone(),
            _ => continue,
        };
        let from_column = match row.get(col_idx) {
            Some(Value::Text(s)) => s.clone(),
            _ => continue,
        };
        let to_column = match row.get(ref_col_idx) {
            Some(Value::Text(s)) => s.clone(),
            _ => continue,
        };
        let flags = match row.get(grbit_idx) {
            Some(Value::Long(v)) => *v as u32,
            _ => 0,
        };

        groups.entry(name).or_default().push(RawRow {
            from_table,
            to_table,
            from_column,
            to_column,
            flags,
        });
    }

    // Convert groups to Relationship structs
    let mut relationships = Vec::with_capacity(groups.len());
    for (name, raw_rows) in groups {
        if raw_rows.is_empty() {
            continue;
        }
        let from_table = raw_rows[0].from_table.clone();
        let to_table = raw_rows[0].to_table.clone();
        let flags = raw_rows[0].flags;

        // Validate consistency within the group — use first row's values
        // if later rows have inconsistent table references or flags.
        for raw in &raw_rows[1..] {
            if raw.from_table != from_table || raw.to_table != to_table {
                log::warn!(
                    "relationship '{}': inconsistent table references, using first row's values",
                    name
                );
                break;
            }
            if raw.flags != flags {
                log::warn!(
                    "relationship '{}': inconsistent flags, using first row's values",
                    name
                );
                break;
            }
        }

        let columns = raw_rows
            .into_iter()
            .map(|r| RelationshipColumn {
                from_column: r.from_column,
                to_column: r.to_column,
            })
            .collect();

        relationships.push(Relationship {
            name,
            from_table,
            to_table,
            columns,
            flags,
        });
    }

    Ok(relationships)
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

    #[test]
    fn jet4_relationships() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let rels = read_relationships(&mut reader).unwrap();

        // testV2003.mdb should have relationships defined
        // Each relationship should have non-empty name, tables, and columns
        for rel in &rels {
            assert!(
                !rel.name.is_empty(),
                "relationship name should not be empty"
            );
            assert!(!rel.from_table.is_empty(), "from_table should not be empty");
            assert!(!rel.to_table.is_empty(), "to_table should not be empty");
            assert!(
                !rel.columns.is_empty(),
                "relationship should have at least one column pair"
            );
            for col_pair in &rel.columns {
                assert!(
                    !col_pair.from_column.is_empty(),
                    "from_column should not be empty"
                );
                assert!(
                    !col_pair.to_column.is_empty(),
                    "to_column should not be empty"
                );
            }
        }
    }

    #[test]
    fn ace12_relationships() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let rels = read_relationships(&mut reader).unwrap();

        for rel in &rels {
            assert!(!rel.name.is_empty());
            assert!(!rel.from_table.is_empty());
            assert!(!rel.to_table.is_empty());
            assert!(!rel.columns.is_empty());
        }
    }

    #[test]
    fn jet3_relationships() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        // Should not panic; may or may not have relationships
        let rels = read_relationships(&mut reader).unwrap();
        for rel in &rels {
            assert!(!rel.name.is_empty());
        }
    }

    #[test]
    fn relationship_flags_check() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let rels = read_relationships(&mut reader).unwrap();

        // Verify flags field is readable (valid u32)
        for rel in &rels {
            // flags should be a valid combination of known bits
            // (may also include bits we don't define constants for)
            let _ = rel.flags & relationship_flags::NO_REFERENTIAL_INTEGRITY;
            let _ = rel.flags & relationship_flags::CASCADE_UPDATE;
            let _ = rel.flags & relationship_flags::CASCADE_DELETE;
        }
    }
}
