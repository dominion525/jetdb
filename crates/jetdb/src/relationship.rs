//! Table relationship (foreign key) reading from MSysRelationships.

use std::collections::BTreeMap;

use crate::catalog::{read_catalog, CatalogEntry};
use crate::data::{self, Value};
use crate::file::{FileError, PageReader};
use crate::format::ObjectType;
use crate::table::{self, required_column, ColumnDef};

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
    let catalog = read_catalog(reader)?;
    read_relationships_in(reader, &catalog)
}

/// [`read_relationships`] with the catalog already read, which it searches
/// for MSysRelationships rather than read it again.
pub fn read_relationships_in(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
) -> Result<Vec<Relationship>, FileError> {
    let Some(rel_entry) = catalog
        .iter()
        .find(|e| e.name == "MSysRelationships" && e.object_type == ObjectType::Table)
    else {
        return Ok(Vec::new());
    };

    // Read table definition and rows
    let tdef = table::read_table_def(reader, "MSysRelationships", rel_entry.table_page)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysRelationships");
    let columns = RelationshipColumns::of(&tdef.columns)?;

    Ok(group_relationship_rows(&result.rows, &columns)
        .into_iter()
        .map(|(name, rows)| relationship_of_rows(name, rows))
        .collect())
}

/// The indexes of the columns of MSysRelationships.
struct RelationshipColumns {
    name: usize,
    column: usize,
    object: usize,
    referenced_column: usize,
    referenced_object: usize,
    grbit: usize,
}

impl RelationshipColumns {
    fn of(columns: &[ColumnDef]) -> Result<Self, FileError> {
        let required = |name, missing| required_column(columns, name, missing);
        Ok(Self {
            name: required(
                "szRelationship",
                "MSysRelationships missing szRelationship column",
            )?,
            column: required("szColumn", "MSysRelationships missing szColumn column")?,
            object: required("szObject", "MSysRelationships missing szObject column")?,
            referenced_column: required(
                "szReferencedColumn",
                "MSysRelationships missing szReferencedColumn column",
            )?,
            referenced_object: required(
                "szReferencedObject",
                "MSysRelationships missing szReferencedObject column",
            )?,
            grbit: required("grbit", "MSysRelationships missing grbit column")?,
        })
    }
}

/// A row of MSysRelationships: one pair of columns of a relationship.
struct RelationshipRow {
    from_table: String,
    to_table: String,
    from_column: String,
    to_column: String,
    flags: u32,
}

/// The name of the relationship of `row` and its pair of columns, or `None`
/// when the row lacks the name, a table or a column.
fn relationship_row(
    row: &[Value],
    columns: &RelationshipColumns,
) -> Option<(String, RelationshipRow)> {
    let text = |i: usize| row.get(i).and_then(Value::as_text).map(str::to_string);
    let name = text(columns.name).filter(|s| !s.is_empty())?;
    let relationship_row = RelationshipRow {
        from_table: text(columns.object)?,
        to_table: text(columns.referenced_object)?,
        from_column: text(columns.column)?,
        to_column: text(columns.referenced_column)?,
        flags: row
            .get(columns.grbit)
            .and_then(Value::as_long)
            .map_or(0, |v| v as u32),
    };
    Some((name, relationship_row))
}

/// The rows of MSysRelationships grouped by the name of their relationship
/// (supports compound foreign keys), in the order of the names.
fn group_relationship_rows(
    rows: &[Vec<Value>],
    columns: &RelationshipColumns,
) -> BTreeMap<String, Vec<RelationshipRow>> {
    let mut groups: BTreeMap<String, Vec<RelationshipRow>> = BTreeMap::new();
    for (name, row) in rows.iter().filter_map(|row| relationship_row(row, columns)) {
        groups.entry(name).or_default().push(row);
    }
    groups
}

/// The relationship `name` of its rows, with the tables and flags of the
/// first row, which it keeps when a later row has others.
fn relationship_of_rows(name: String, rows: Vec<RelationshipRow>) -> Relationship {
    let from_table = rows[0].from_table.clone();
    let to_table = rows[0].to_table.clone();
    let flags = rows[0].flags;

    // Validate consistency within the group — use first row's values
    // if later rows have inconsistent table references or flags.
    for raw in &rows[1..] {
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

    let columns = rows
        .into_iter()
        .map(|r| RelationshipColumn {
            from_column: r.from_column,
            to_column: r.to_column,
        })
        .collect();

    Relationship {
        name,
        from_table,
        to_table,
        columns,
        flags,
    }
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
    fn relationships_in_a_catalog_already_read() {
        let path = skip_if_missing!("V1997/nwind.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = read_catalog(&mut reader).unwrap();
        let read = read_relationships(&mut reader).unwrap();
        assert_eq!(read.len(), 7);
        assert_eq!(
            format!(
                "{:?}",
                read_relationships_in(&mut reader, &catalog).unwrap()
            ),
            format!("{read:?}")
        );
    }

    /// A relationship as names, the column pairs, and its flags of
    /// referential integrity and cascades.
    type Summary<'a> = (&'a str, &'a str, &'a str, Vec<(&'a str, &'a str)>, u32);

    fn summary(r: &Relationship) -> Summary<'_> {
        let kinds = relationship_flags::NO_REFERENTIAL_INTEGRITY
            | relationship_flags::CASCADE_UPDATE
            | relationship_flags::CASCADE_DELETE;
        let columns = r
            .columns
            .iter()
            .map(|c| (c.from_column.as_str(), c.to_column.as_str()))
            .collect();
        let tables = (r.from_table.as_str(), r.to_table.as_str());
        (
            r.name.as_str(),
            tables.0,
            tables.1,
            columns,
            r.flags & kinds,
        )
    }

    #[test]
    fn relationships_of_each_version() {
        // indexTest of every version has the same two relationships with
        // referential integrity, from Table1 to Table2 and to Table3. Newer
        // Access adds some of its own system tables, which are left out.
        let expected = [
            (
                "Table2Table1",
                "Table1",
                "Table2",
                vec![("otherfk1", "id")],
                relationship_flags::CASCADE_DELETE,
            ),
            (
                "Table3Table1",
                "Table1",
                "Table3",
                vec![("otherfk2", "id")],
                relationship_flags::CASCADE_UPDATE,
            ),
        ];
        for file in [
            "V1997/indexTestV1997.mdb",
            "V2000/indexTestV2000.mdb",
            "V2003/indexTestV2003.mdb",
            "V2007/indexTestV2007.accdb",
            "V2010/indexTestV2010.accdb",
        ] {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let mut rels = read_relationships(&mut reader).unwrap();
            rels.retain(|r| !r.from_table.starts_with("MSys"));
            rels.sort_by(|a, b| a.name.cmp(&b.name));
            let read: Vec<Summary> = rels.iter().map(summary).collect();
            assert_eq!(read, expected, "{file}");
        }
    }
}
