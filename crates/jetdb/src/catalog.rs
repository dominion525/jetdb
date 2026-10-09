//! MSysObjects system catalog reading and table discovery.

use crate::data::{self, Value};
use crate::file::{FileError, PageReader};
use crate::format::{catalog_flags, ObjectType, CATALOG_PAGE};
use crate::table;

// ---------------------------------------------------------------------------
// CatalogEntry
// ---------------------------------------------------------------------------

/// A single entry from the MSysObjects system catalog.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub name: String,
    pub object_type: ObjectType,
    /// TDEF page number, extracted from the lower 24 bits of MSysObjects.Id.
    pub table_page: u32,
    /// MSysObjects.Flags value (0 if the column is absent).
    pub flags: u32,
    /// MSysObjects.LvProp, the properties of the object, which
    /// [`crate::entry_properties`] reads; `None` when it has none.
    pub lv_prop: Option<Vec<u8>>,
}

impl CatalogEntry {
    /// `true` for a system object, such as the `MSys*` tables.
    pub fn is_system(&self) -> bool {
        self.flags & catalog_flags::SYSTEM != 0
    }

    /// `true` for an object Access keeps from users: a system object or one
    /// marked hidden. The `jetdb tables` command and [`table_names`] leave
    /// these out unless asked for them.
    pub fn is_system_or_hidden(&self) -> bool {
        self.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN) != 0
    }
}

// ---------------------------------------------------------------------------
// read_catalog
// ---------------------------------------------------------------------------

/// Read the MSysObjects system catalog and return all entries.
pub fn read_catalog(reader: &mut PageReader) -> Result<Vec<CatalogEntry>, FileError> {
    let tdef = table::read_table_def(reader, "MSysObjects", CATALOG_PAGE)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysObjects");

    // Locate required column indices in a single pass
    let (mut id_idx, mut name_idx, mut type_idx, mut flags_idx, mut lvprop_idx) =
        (None, None, None, None, None);
    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "Id" => id_idx = Some(i),
            "Name" => name_idx = Some(i),
            "Type" => type_idx = Some(i),
            "Flags" => flags_idx = Some(i),
            "LvProp" => lvprop_idx = Some(i),
            _ => {}
        }
    }
    let id_idx = id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysObjects missing Id column",
    })?;
    let name_idx = name_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysObjects missing Name column",
    })?;
    let type_idx = type_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysObjects missing Type column",
    })?;

    let mut entries = Vec::new();

    for row in &result.rows {
        // Name must be non-null Text
        let name = match row.get(name_idx) {
            Some(Value::Text(s)) if !s.is_empty() => s.clone(),
            _ => continue,
        };

        // Type must be convertible to ObjectType
        let object_type = match row.get(type_idx) {
            Some(Value::Int(v)) => match ObjectType::try_from(*v as i32) {
                Ok(ot) => ot,
                Err(_) => continue,
            },
            _ => continue,
        };

        // Id → table_page (lower 24 bits)
        let table_page = match row.get(id_idx) {
            Some(Value::Long(id)) => (*id & 0x00FF_FFFF) as u32,
            _ => continue,
        };

        // Flags (optional column)
        // Flags は i32 だが、ビットフラグとして解釈するため
        // u32 にキャスト (ビットパターン保持: -2147483648i32 → 0x80000000u32)
        let flags = match flags_idx.and_then(|i| row.get(i)) {
            Some(Value::Long(f)) => *f as u32,
            _ => 0,
        };

        let lv_prop = match lvprop_idx.and_then(|i| row.get(i)) {
            Some(Value::Binary(b)) => Some(b.clone()),
            _ => None,
        };

        entries.push(CatalogEntry {
            name,
            object_type,
            table_page,
            flags,
            lv_prop,
        });
    }

    Ok(entries)
}

// ---------------------------------------------------------------------------
// table_names
// ---------------------------------------------------------------------------

/// Find the table named `name` in `catalog`, system and hidden tables
/// included, or return [`FileError::TableNotFound`].
pub fn find_table<'a>(
    catalog: &'a [CatalogEntry],
    name: &str,
) -> Result<&'a CatalogEntry, FileError> {
    catalog
        .iter()
        .find(|e| e.object_type == ObjectType::Table && e.name == name)
        .ok_or_else(|| FileError::TableNotFound {
            name: name.to_string(),
        })
}

/// Return the names of user-visible tables in the database.
///
/// Filters out system objects (`MSys*`) and hidden tables based on the
/// `Flags` column.
pub fn table_names(reader: &mut PageReader) -> Result<Vec<String>, FileError> {
    let catalog = read_catalog(reader)?;
    let names = catalog
        .into_iter()
        .filter(|e| e.object_type == ObjectType::Table && !e.is_system_or_hidden())
        .map(|e| e.name)
        .collect();
    Ok(names)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::PageReader;

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

    // -- read_catalog tests ---------------------------------------------------

    fn assert_catalog(path: &std::path::Path) {
        let mut reader = PageReader::open(path).unwrap();
        let catalog = read_catalog(&mut reader).unwrap();

        assert!(!catalog.is_empty(), "catalog should not be empty");

        // All entries must have a non-empty name
        for entry in &catalog {
            assert!(!entry.name.is_empty(), "entry name should not be empty");
        }

        // MSysObjects itself should be in the catalog (Type=Table with system flag)
        let msysobjects = catalog
            .iter()
            .find(|e| e.name == "MSysObjects")
            .expect("MSysObjects should be in the catalog");
        assert_eq!(msysobjects.object_type, ObjectType::Table);
        assert_ne!(
            msysobjects.flags & catalog_flags::SYSTEM,
            0,
            "MSysObjects should have the system flag set"
        );
    }

    #[test]
    fn jet3_read_catalog() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        assert_catalog(&path);
    }

    #[test]
    fn jet4_read_catalog() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        assert_catalog(&path);
    }

    #[test]
    fn ace12_read_catalog() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        assert_catalog(&path);
    }

    #[test]
    fn ace14_read_catalog() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        assert_catalog(&path);
    }

    #[test]
    fn ace17_read_catalog() {
        let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
        assert_catalog(&path);
    }

    #[test]
    fn object_types_of_real_catalogs() {
        // Every `Type` value in these catalogs is known, and each object gets
        // the type of the DAO container it belongs to.
        let cases: &[(&str, &[(&str, ObjectType)])] = &[
            (
                "V1997/nwind.mdb",
                &[
                    ("Tables", ObjectType::Container),
                    ("MSysDb", ObjectType::Database),
                    ("MSysObjects", ObjectType::Table),
                    ("Categories", ObjectType::Table),
                    ("AccessLayout", ObjectType::DatabaseProperty),
                    ("Admin", ObjectType::UserInfo),
                    ("Customer Labels Dialog", ObjectType::Form),
                    ("Customer Labels Dialog", ObjectType::Macro),
                    ("Alphabetical List of Products", ObjectType::Report),
                    ("Alphabetical List of Products", ObjectType::Query),
                    ("Northwind Help Function", ObjectType::Module),
                    ("CategoriesProducts", ObjectType::Relationship),
                ],
            ),
            (
                // Ordrar is linked over ODBC (its Connect column holds a DSN).
                "V2007/odbcLinkerTestV2007.accdb",
                &[("Ordrar", ObjectType::LinkedOdbcTable)],
            ),
        ];
        for (file, expected) in cases {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let catalog = read_catalog(&mut reader).unwrap();
            for (name, object_type) in *expected {
                assert!(
                    catalog
                        .iter()
                        .any(|e| e.name == *name && e.object_type == *object_type),
                    "{file}: {name} should be {object_type}"
                );
            }
        }
    }

    // -- table_names tests ----------------------------------------------------

    fn assert_table_names(path: &std::path::Path) {
        let mut reader = PageReader::open(path).unwrap();
        let names = table_names(&mut reader).unwrap();

        assert!(!names.is_empty(), "should have at least one user table");

        // No system tables should be present
        for name in &names {
            assert!(
                !name.starts_with("MSys"),
                "system table {name} should not appear in table_names"
            );
        }
    }

    #[test]
    fn jet3_table_names() {
        let path = skip_if_missing!("V1997/testV1997.mdb");
        assert_table_names(&path);
    }

    #[test]
    fn jet4_table_names() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        assert_table_names(&path);
    }

    #[test]
    fn ace12_table_names() {
        let path = skip_if_missing!("V2007/testV2007.accdb");
        assert_table_names(&path);
    }

    #[test]
    fn ace14_table_names() {
        let path = skip_if_missing!("V2010/testV2010.accdb");
        assert_table_names(&path);
    }

    #[test]
    fn ace17_table_names() {
        let path = skip_if_missing!("V2019/extDateTestV2019.accdb");
        assert_table_names(&path);
    }

    // -- flag filtering unit tests --------------------------------------------

    /// Apply the same filter logic as `table_names` to a pre-built catalog.
    fn filter_user_tables(catalog: Vec<CatalogEntry>) -> Vec<String> {
        catalog
            .into_iter()
            .filter(|e| e.object_type == ObjectType::Table && !e.is_system_or_hidden())
            .map(|e| e.name)
            .collect()
    }

    fn entry(name: &str, object_type: ObjectType, flags: u32) -> CatalogEntry {
        CatalogEntry {
            name: name.to_string(),
            object_type,
            table_page: 100,
            flags,
            lv_prop: None,
        }
    }

    #[test]
    fn find_table_by_name() {
        let catalog = vec![
            entry("Table1", ObjectType::Table, 0),
            entry("Query1", ObjectType::Query, 0),
            entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM),
        ];
        assert_eq!(find_table(&catalog, "Table1").unwrap().name, "Table1");
        assert_eq!(
            find_table(&catalog, "MSysObjects").unwrap().name,
            "MSysObjects"
        );
        // A query of that name is not a table.
        assert!(matches!(
            find_table(&catalog, "Query1"),
            Err(FileError::TableNotFound { name }) if name == "Query1"
        ));
        assert!(matches!(
            find_table(&catalog, "NoSuchTable"),
            Err(FileError::TableNotFound { name }) if name == "NoSuchTable"
        ));
    }

    #[test]
    fn system_and_hidden_flags() {
        let user = entry("Table1", ObjectType::Table, 0);
        let system = entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM);
        let hidden = entry("Hidden", ObjectType::Table, catalog_flags::HIDDEN);
        assert!(!user.is_system() && !user.is_system_or_hidden());
        assert!(system.is_system() && system.is_system_or_hidden());
        assert!(!hidden.is_system() && hidden.is_system_or_hidden());
    }

    #[test]
    fn filter_excludes_system_flag() {
        let catalog = vec![
            entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM),
            entry("Users", ObjectType::Table, 0),
        ];
        let names = filter_user_tables(catalog);
        assert_eq!(names, vec!["Users"]);
    }

    #[test]
    fn filter_excludes_hidden_flag() {
        let catalog = vec![
            entry(
                "MSysNavPaneGroups",
                ObjectType::Table,
                catalog_flags::HIDDEN,
            ),
            entry("Orders", ObjectType::Table, 0),
        ];
        let names = filter_user_tables(catalog);
        assert_eq!(names, vec!["Orders"]);
    }

    #[test]
    fn filter_excludes_system_and_hidden() {
        let catalog = vec![entry(
            "Internal",
            ObjectType::Table,
            catalog_flags::SYSTEM | catalog_flags::HIDDEN,
        )];
        let names = filter_user_tables(catalog);
        assert!(names.is_empty());
    }

    #[test]
    fn filter_includes_normal_table() {
        let catalog = vec![entry("Products", ObjectType::Table, 0)];
        let names = filter_user_tables(catalog);
        assert_eq!(names, vec!["Products"]);
    }

    #[test]
    fn filter_excludes_non_table_types() {
        let catalog = vec![
            entry("MyQuery", ObjectType::Query, 0),
            entry("MyForm", ObjectType::Form, 0),
            entry("MyMacro", ObjectType::Macro, 0),
            entry("MyReport", ObjectType::Report, 0),
            entry("Users", ObjectType::Table, 0),
        ];
        let names = filter_user_tables(catalog);
        assert_eq!(names, vec!["Users"]);
    }

    #[test]
    fn filter_empty_catalog() {
        let names = filter_user_tables(vec![]);
        assert!(names.is_empty());
    }

    #[test]
    fn filter_mixed_flags_and_types() {
        let catalog = vec![
            entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM),
            entry("MSysACEs", ObjectType::Table, catalog_flags::SYSTEM),
            entry(
                "MSysNavPaneGroups",
                ObjectType::Table,
                catalog_flags::HIDDEN | 0x08,
            ),
            entry("SavedQuery", ObjectType::Query, 0),
            entry("Employees", ObjectType::Table, 0),
            entry("Departments", ObjectType::Table, 0),
        ];
        let names = filter_user_tables(catalog);
        assert_eq!(names, vec!["Employees", "Departments"]);
    }

    #[test]
    fn japanese_table_name() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = read_catalog(&mut reader).unwrap();
        let jp_table = catalog.iter().find(|e| e.name == "jp_テーブル2");
        assert!(
            jp_table.is_some(),
            "catalog should contain jp_テーブル2, found: {:?}",
            catalog.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        assert_eq!(jp_table.unwrap().object_type, ObjectType::Table);
    }
}
