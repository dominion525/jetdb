//! JavaScript bindings of jetdb, built as WebAssembly for the `jetdb-wasm`
//! npm package.
//!
//! [`Database`] holds a database opened from bytes in memory, and each of its
//! methods reads one thing from it the way the matching `jetdb` CLI command
//! does.

use std::io::Cursor;

use jetdb::format::{catalog_flags, JetVersion, ObjectType};
use jetdb::{read_catalog, FileError, PageReader};

/// A database opened from bytes in memory.
pub struct Database {
    reader: PageReader,
}

impl Database {
    /// Open a database from its bytes, with the password for a
    /// password-protected `.accdb`.
    pub fn open(bytes: Vec<u8>, password: Option<&str>) -> Result<Self, FileError> {
        let reader = PageReader::open_reader_with_password(Cursor::new(bytes), password)?;
        Ok(Self { reader })
    }

    /// The database engine version, as the `jetdb ver` command prints it
    /// (`JET3`, `JET4`, `ACE12`, and so on).
    pub fn version(&self) -> &'static str {
        match self.reader.header().version {
            JetVersion::Jet3 => "JET3",
            JetVersion::Jet4 => "JET4",
            JetVersion::Ace12 => "ACE12",
            JetVersion::Ace14 => "ACE14",
            JetVersion::Ace15 => "ACE15",
            JetVersion::Ace16 => "ACE16",
            JetVersion::Ace17 => "ACE17",
        }
    }

    /// The table names, sorted, as the `jetdb tables` command lists them:
    /// user tables, and with `include_system` also system and hidden tables.
    pub fn tables(&mut self, include_system: bool) -> Result<Vec<String>, FileError> {
        let mut names: Vec<String> = read_catalog(&mut self.reader)?
            .into_iter()
            .filter(|e| {
                e.object_type == ObjectType::Table
                    && (include_system
                        || e.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN) == 0)
            })
            .map(|e| e.name)
            .collect();
        names.sort_unstable();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data(relative: &str) -> Option<Vec<u8>> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(relative);
        std::fs::read(path).ok()
    }

    macro_rules! skip_if_missing {
        ($path:expr) => {
            match test_data($path) {
                Some(bytes) => bytes,
                None => {
                    eprintln!("SKIP: test data not found: {}", $path);
                    return;
                }
            }
        };
    }

    #[test]
    fn version_of_each_format() {
        for (file, expected) in [
            ("V1997/testV1997.mdb", "JET3"),
            ("V2003/testV2003.mdb", "JET4"),
            ("V2007/testV2007.accdb", "ACE12"),
            ("V2010/testV2010.accdb", "ACE14"),
        ] {
            let bytes = skip_if_missing!(file);
            assert_eq!(
                Database::open(bytes, None).unwrap().version(),
                expected,
                "{file}"
            );
        }
    }

    #[test]
    fn tables_user_and_system() {
        let bytes = skip_if_missing!("V2003/testV2003.mdb");
        let mut db = Database::open(bytes, None).unwrap();
        let user = db.tables(false).unwrap();
        assert!(user.contains(&"Table1".to_string()), "{user:?}");
        assert!(user.iter().all(|n| !n.starts_with("MSys")), "{user:?}");
        assert!(user.windows(2).all(|w| w[0] <= w[1]), "sorted: {user:?}");
        let all = db.tables(true).unwrap();
        assert!(all.contains(&"MSysObjects".to_string()), "{all:?}");
        assert!(all.len() > user.len());
    }

    #[test]
    fn password_protected() {
        let bytes = skip_if_missing!("db2007-enc.accdb");
        assert!(matches!(
            Database::open(bytes.clone(), None),
            Err(FileError::PasswordRequired)
        ));
        assert!(matches!(
            Database::open(bytes.clone(), Some("wrong")),
            Err(FileError::InvalidPassword)
        ));
        let mut db = Database::open(bytes, Some("Test123")).unwrap();
        assert_eq!(db.tables(false).unwrap(), ["Table1"]);
    }
}
