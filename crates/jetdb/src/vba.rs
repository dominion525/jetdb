//! VBA project and module source code extraction.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read as _, Write};

use crate::file::{FileError, PageReader};
use crate::storage::{self, AccessStorage, StorageEntry};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// VBA module type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VbaModuleType {
    /// A standard module.
    Standard,
    /// A class module.
    Class,
    /// The module of a form or report, which VBA calls a document module.
    Document,
}

/// A single VBA module with its source code.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VbaModule {
    pub name: String,
    pub module_type: VbaModuleType,
    pub source: String,
}

/// A VBA project extracted from a database.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VbaProject {
    pub modules: Vec<VbaModule>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read a VBA project from a Jet/ACE database.
///
/// Tries `MSysAccessStorage` (Jet4/ACE) first, then falls back to
/// `MSysAccessObjects` (Jet3/Access 97) for older databases.
pub fn read_vba_project(reader: &mut PageReader) -> Result<VbaProject, FileError> {
    let storage = storage::read_access_storage(reader)?;
    read_vba_project_in(reader, &storage)
}

/// [`read_vba_project`] in the storage already read.
pub fn read_vba_project_in(
    reader: &mut PageReader,
    storage: &AccessStorage,
) -> Result<VbaProject, FileError> {
    // Try MSysAccessStorage first (Jet4/ACE format)
    let entries = &storage.entries;
    if !entries.is_empty() {
        let project = build_cfb_and_extract(entries)?;
        if !project.modules.is_empty() {
            return Ok(project);
        }
    }

    // Fall back to MSysAccessObjects (Jet3/Access 97 format)
    let raw_cfb = storage::read_access_objects_cfb(reader)?;
    if raw_cfb.is_empty() {
        return Ok(VbaProject {
            modules: Vec::new(),
        });
    }
    let cfb_bytes = extract_vba_project_cfb(raw_cfb)?;
    if cfb_bytes.is_empty() {
        return Ok(VbaProject {
            modules: Vec::new(),
        });
    }
    extract_modules_from_cfb(cfb_bytes)
}

/// Extract VBA modules from CFB bytes using the ovba crate.
///
/// ovba panics on some compressed data that is broken, so the dir stream is
/// decompressed here before ovba reads it, and the source of each module is
/// read here rather than by ovba.
fn extract_modules_from_cfb(cfb_bytes: Vec<u8>) -> Result<VbaProject, FileError> {
    let mut cfb = cfb::CompoundFile::open(Cursor::new(cfb_bytes.clone()))
        .map_err(|e| vba_error(format!("failed to open CFB: {e}")))?;
    // Without a dir stream to read, ovba tells what is missing.
    if let Ok(dir) = read_cfb_stream(&mut cfb, "/VBA/dir") {
        decompress(&dir)?;
    }
    let project = ovba::open_project(cfb_bytes.clone()).map_err(|e| vba_error(e.to_string()))?;
    let code_page = project.information.code_page;
    let classes = class_module_names(&cfb_bytes, code_page);

    let mut modules = Vec::new();
    for module in &project.modules {
        let source = module_source(&mut cfb, module, code_page)?;
        modules.push(VbaModule {
            name: module.name.clone(),
            module_type: module_type_of(module, classes.as_ref()),
            source,
        });
    }

    Ok(VbaProject { modules })
}

fn vba_error(reason: String) -> FileError {
    FileError::InvalidVbaProject { reason }
}

/// The source of `module` in `cfb`: its stream from its text offset on,
/// decompressed, in the code page of the project.
fn module_source(
    cfb: &mut cfb::CompoundFile<Cursor<Vec<u8>>>,
    module: &ovba::Module,
    code_page: u16,
) -> Result<String, FileError> {
    let stream = read_cfb_stream(cfb, &format!("/VBA/{}", module.stream_name))?;
    let compressed = stream.get(module.text_offset..).ok_or_else(|| {
        vba_error(format!(
            "the source of module '{}' starts past the end of its stream",
            module.name
        ))
    })?;
    Ok(crate::encoding::decode_code_page(
        &decompress(compressed)?,
        code_page,
    ))
}

/// Decompress a CompressedContainer of MS-OVBA (2.4.1): a signature byte and
/// chunks of at most 4096 decompressed bytes each.
fn decompress(container: &[u8]) -> Result<Vec<u8>, FileError> {
    let Some((&1, mut rest)) = container.split_first() else {
        return Err(vba_error(
            "compressed container without its signature".into(),
        ));
    };
    if rest.is_empty() {
        return Err(vba_error("compressed container without chunks".into()));
    }
    let mut result = Vec::new();
    while !rest.is_empty() {
        let (chunk, after) = chunk_of(rest)?;
        result.extend(chunk);
        rest = after;
    }
    Ok(result)
}

/// The decompressed data of the chunk at the start of `data`, and the data
/// after it.
fn chunk_of(data: &[u8]) -> Result<(Vec<u8>, &[u8]), FileError> {
    let header = u16::from_le_bytes(le_bytes(data)?);
    // CompressedChunkHeader: 12 bits of the size minus 3, 0b011, and a flag
    if (header >> 12) & 0b111 != 0b011 {
        return Err(vba_error("compressed chunk with a wrong signature".into()));
    }
    let length = (header & 0xfff) as usize + 1;
    let (chunk, after) = data[2..]
        .split_at_checked(length)
        .ok_or_else(|| vba_error("compressed chunk past the end of its container".into()))?;
    if header & 0x8000 == 0 {
        return Ok((chunk.to_vec(), after));
    }
    Ok((decompress_chunk(chunk)?, after))
}

/// The two bytes at the start of `data`.
fn le_bytes(data: &[u8]) -> Result<[u8; 2], FileError> {
    data.first_chunk()
        .copied()
        .ok_or_else(|| vba_error("compressed data cut short".into()))
}

/// The data of a compressed chunk, decompressed: each flag byte tells, for
/// the eight tokens after it, which copy earlier bytes and which are literal.
fn decompress_chunk(mut chunk: &[u8]) -> Result<Vec<u8>, FileError> {
    let mut result = Vec::with_capacity(4096);
    while let Some((&flags, mut tokens)) = chunk.split_first() {
        for bit in 0..8 {
            let Some((&literal, rest)) = tokens.split_first() else {
                break;
            };
            if flags & (1 << bit) == 0 {
                result.push(literal);
                tokens = rest;
                continue;
            }
            copy_token(&mut result, u16::from_le_bytes(le_bytes(tokens)?))?;
            tokens = &tokens[2..];
        }
        chunk = tokens;
    }
    Ok(result)
}

/// Append to `result`, the data of a chunk decompressed so far, the bytes
/// `token`, a CopyToken, copies from earlier in it.
fn copy_token(result: &mut Vec<u8>, token: u16) -> Result<(), FileError> {
    let decompressed = result.len();
    if decompressed > 4096 {
        return Err(vba_error("compressed chunk longer than 4096 bytes".into()));
    }
    // The offset takes the bits that the bytes decompressed so far need,
    // at least 4; the length takes the rest.
    let bit_count = (usize::BITS - decompressed.saturating_sub(1).leading_zeros()).max(4);
    let length_mask = 0xffff_u16 >> bit_count;
    let length = (token & length_mask) as usize + 3;
    let offset = (token >> (16 - bit_count)) as usize + 1;
    let start = decompressed.checked_sub(offset).ok_or_else(|| {
        vba_error("compressed data copies from before the start of its chunk".into())
    })?;
    // The copy may overlap the bytes it appends, so it goes byte by byte.
    for i in start..start + length {
        result.push(result[i]);
    }
    Ok(())
}

/// The type of `module`, with `classes` the names of the class modules the
/// PROJECT stream gives, if it can be read.
fn module_type_of(module: &ovba::Module, classes: Option<&HashSet<String>>) -> VbaModuleType {
    match module.module_type {
        ovba::ModuleType::Procedural => VbaModuleType::Standard,
        // ovba tells only standard modules from the others. The PROJECT
        // stream names the class modules; the others are documents.
        ovba::ModuleType::DocClsDesigner => match classes {
            Some(classes) if !classes.contains(&module.name) => VbaModuleType::Document,
            _ => VbaModuleType::Class,
        },
    }
}

// ---------------------------------------------------------------------------
// Internal: MSysAccessStorage reading is now in crate::storage
// ---------------------------------------------------------------------------

/// The names of the class modules, from the `Class=` lines of the PROJECT
/// stream, which the project's code page encodes. In Access databases the
/// stream names standard modules with `Module=` and class modules with
/// `Class=`, and does not list the modules of forms and reports. `None`
/// when the stream cannot be read.
fn class_module_names(cfb_bytes: &[u8], code_page: u16) -> Option<HashSet<String>> {
    let mut cfb = cfb::CompoundFile::open(Cursor::new(cfb_bytes)).ok()?;
    let mut data = Vec::new();
    cfb.open_stream("/PROJECT")
        .ok()?
        .read_to_end(&mut data)
        .ok()?;
    let encoding = codepage::to_encoding(code_page).unwrap_or(encoding_rs::WINDOWS_1252);
    let (text, _, _) = encoding.decode(&data);
    Some(
        text.lines()
            .filter_map(|line| line.strip_prefix("Class="))
            .map(|name| name.trim_end().to_string())
            .collect(),
    )
}

/// Where the VBA project is in the compound file of MSysAccessObjects.
const VBA_PROJECT_PREFIX: &str = "/VBA/VBAProject";

/// Extract the VBAProject subtree from a CFB and rebuild it as a root-level CFB.
///
/// In MSysAccessObjects format, the VBA project lives at `/VBA/VBAProject/`
/// within the large CFB. ovba expects VBA entries at the root level
/// (e.g., `/VBA/dir`, `/PROJECT`), so we strip the `/VBA/VBAProject` prefix.
fn extract_vba_project_cfb(cfb_bytes: Vec<u8>) -> Result<Vec<u8>, FileError> {
    let cursor = Cursor::new(cfb_bytes);
    let mut source = cfb::CompoundFile::open(cursor)
        .map_err(|e| vba_error(format!("failed to open CFB: {e}")))?;

    let entries = vba_project_entries(&source);
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    write_vba_project(&mut source, entries)
}

/// A compound file of the `entries` of the VBA project of `source`, at its
/// root.
fn write_vba_project(
    source: &mut cfb::CompoundFile<Cursor<Vec<u8>>>,
    entries: Vec<(String, bool)>,
) -> Result<Vec<u8>, FileError> {
    // The storages first (ensures parent dirs exist)
    let (storages, streams): (Vec<_>, Vec<_>) =
        entries.into_iter().partition(|(_, is_storage)| *is_storage);
    let mut dest = CfbWriter::new()?;
    for (path, _) in &storages {
        dest.storage(path)?;
    }
    for (path, _) in &streams {
        let data = read_cfb_stream(source, &format!("{VBA_PROJECT_PREFIX}{path}"))?;
        dest.stream(path, &data)?;
    }
    dest.finish()
}

/// The paths of the entries under the VBA project of a compound file,
/// relative to it, each with whether it is a storage.
fn vba_project_entries(source: &cfb::CompoundFile<Cursor<Vec<u8>>>) -> Vec<(String, bool)> {
    source
        .walk()
        .filter_map(|e| {
            // Use '/' separators regardless of OS (CFB paths are internal, not filesystem)
            let path = e.path().to_string_lossy().replace('\\', "/");
            let relative = path.strip_prefix(VBA_PROJECT_PREFIX)?;
            (!relative.is_empty()).then(|| (relative.to_string(), e.is_storage()))
        })
        .collect()
}

/// The data of the stream `path` of `source`.
fn read_cfb_stream(
    source: &mut cfb::CompoundFile<Cursor<Vec<u8>>>,
    path: &str,
) -> Result<Vec<u8>, FileError> {
    let mut data = Vec::new();
    source
        .open_stream(path)
        .map_err(|e| vba_error(format!("failed to open stream '{path}': {e}")))?
        .read_to_end(&mut data)
        .map_err(|e| vba_error(format!("failed to read stream '{path}': {e}")))?;
    Ok(data)
}

/// A compound file written in memory, with the errors of a VBA project that
/// cannot be built.
struct CfbWriter {
    cf: cfb::CompoundFile<Cursor<Vec<u8>>>,
}

impl CfbWriter {
    fn new() -> Result<Self, FileError> {
        let cf = cfb::CompoundFile::create(Cursor::new(Vec::new()))
            .map_err(|e| vba_error(format!("failed to create CFB: {e}")))?;
        Ok(Self { cf })
    }

    /// Create the storage `path`, and the storages above it.
    fn storage(&mut self, path: &str) -> Result<(), FileError> {
        self.cf
            .create_storage_all(path)
            .map_err(|e| vba_error(format!("failed to create storage '{path}': {e}")))
    }

    /// Create the stream `path` with `data`.
    fn stream(&mut self, path: &str, data: &[u8]) -> Result<(), FileError> {
        let mut stream = self
            .cf
            .create_stream(path)
            .map_err(|e| vba_error(format!("failed to create stream '{path}': {e}")))?;
        stream
            .write_all(data)
            .map_err(|e| vba_error(format!("failed to write stream '{path}': {e}")))
    }

    /// The bytes of the compound file.
    fn finish(mut self) -> Result<Vec<u8>, FileError> {
        self.cf
            .flush()
            .map_err(|e| vba_error(format!("failed to flush CFB: {e}")))?;
        Ok(self.cf.into_inner().into_inner())
    }
}

// ---------------------------------------------------------------------------
// Internal: VBA subtree filtering
// ---------------------------------------------------------------------------

/// Find the VBAProject root entry and collect all its children.
///
/// The MSysAccessStorage tree for VBA typically looks like:
/// ```text
/// MSysAccessStorage_ROOT (id=1)
///   VBA (id=8)                    ← outer VBA storage
///     VBAProject (id=17)          ← CFB root equivalent
///       VBA (id=18)               ← inner VBA storage
///         dir, _VBA_PROJECT, module streams...
///       PROJECT, PROJECTwm
///     AcessVBAData                ← not needed
/// ```
///
/// Returns the VBAProject entry ID and all its descendant entries.
fn find_vba_project_entries(entries: &[StorageEntry]) -> Option<(i32, Vec<&StorageEntry>)> {
    // Find the "VBAProject" storage entry
    let vba_project = entries
        .iter()
        .find(|e| e.name == "VBAProject" && storage::is_storage(e))?;

    let mut children = Vec::new();
    let mut visited = HashSet::new();
    storage::collect_children(entries, vba_project.id, &mut children, &mut visited);
    Some((vba_project.id, children))
}

// ---------------------------------------------------------------------------
// Internal: CFB reconstruction
// ---------------------------------------------------------------------------

/// Build an in-memory OLE2/CFB from MSysAccessStorage entries and extract modules.
fn build_cfb_and_extract(entries: &[StorageEntry]) -> Result<VbaProject, FileError> {
    let Some((vba_project_id, vba_entries)) =
        find_vba_project_entries(entries).filter(|(_, vba_entries)| !vba_entries.is_empty())
    else {
        return Ok(VbaProject {
            modules: Vec::new(),
        });
    };

    extract_modules_from_cfb(write_vba_entries(entries, vba_project_id, vba_entries)?)
}

/// A compound file of the `vba_entries` under the VBAProject entry
/// `vba_project_id`, at its root; an entry whose path cannot be built from
/// `entries` is left out.
fn write_vba_entries(
    entries: &[StorageEntry],
    vba_project_id: i32,
    vba_entries: Vec<&StorageEntry>,
) -> Result<Vec<u8>, FileError> {
    // Build an ID-to-entry map for path construction
    let id_map: HashMap<i32, &StorageEntry> = entries.iter().map(|e| (e.id, e)).collect();

    // Create storages first, then streams (ensures parent dirs exist)
    let (storages, streams): (Vec<_>, Vec<_>) = vba_entries
        .into_iter()
        .partition(|e| storage::is_storage(e));
    let mut cf = CfbWriter::new()?;
    for (path, _) in entry_paths(storages, vba_project_id, &id_map) {
        cf.storage(&path)?;
    }
    for (path, entry) in entry_paths(streams, vba_project_id, &id_map) {
        cf.stream(&path, &entry.data)?;
    }
    cf.finish()
}

/// Each of `entries` whose path under the VBAProject entry `vba_project_id`
/// can be built, with the path.
fn entry_paths<'a>(
    entries: Vec<&'a StorageEntry>,
    vba_project_id: i32,
    id_map: &HashMap<i32, &StorageEntry>,
) -> Vec<(String, &'a StorageEntry)> {
    entries
        .into_iter()
        .filter_map(|entry| {
            Some((
                storage::build_entry_path(entry, vba_project_id, id_map)?,
                entry,
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

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
    fn no_vba_project() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("should succeed");
        assert!(
            project.modules.is_empty(),
            "expected empty modules for database without VBA project"
        );
    }

    /// Helper to verify VBA project modules match expected names and types.
    fn assert_vba_modules(project: &VbaProject, expected: &[(&str, VbaModuleType)]) {
        assert_eq!(project.modules.len(), expected.len());
        let mut names: Vec<&str> = project.modules.iter().map(|m| m.name.as_str()).collect();
        names.sort();
        let mut expected_names: Vec<&str> = expected.iter().map(|(n, _)| *n).collect();
        expected_names.sort();
        assert_eq!(names, expected_names);

        for (name, expected_type) in expected {
            let module = project.modules.iter().find(|m| m.name == *name).unwrap();
            assert_eq!(
                module.module_type, *expected_type,
                "type mismatch for {name}"
            );
            assert!(!module.source.is_empty(), "empty source for {name}");
        }
    }

    const EXPECTED_MODULES: &[(&str, VbaModuleType)] = &[
        ("Module1", VbaModuleType::Standard),
        ("Class1", VbaModuleType::Class),
        ("Form_Form1", VbaModuleType::Document),
    ];

    /// The source Access held for each module when vba-japanese-generator.bas
    /// made it, from tblVbaExpected, by module name.
    fn expected_sources(reader: &mut PageReader) -> Vec<(String, String)> {
        let catalog = catalog::read_catalog(reader).unwrap();
        let entry = catalog.iter().find(|e| e.name == "tblVbaExpected").unwrap();
        let table = crate::table::read_table_def(reader, &entry.name, entry.table_page).unwrap();
        let column = |name: &str| table.columns.iter().position(|c| c.name == name).unwrap();
        let (name, source) = (column("ModuleName"), column("Source"));
        crate::data::read_table_rows(reader, &table)
            .unwrap()
            .rows
            .into_iter()
            .map(|row| match (&row[name], &row[source]) {
                (crate::data::Value::Text(n), crate::data::Value::Text(s)) => {
                    (n.clone(), s.clone())
                }
                other => panic!("unexpected row {other:?}"),
            })
            .collect()
    }

    #[test]
    fn japanese_module_names_and_source() {
        // Made by testdata/vba-generators/vba-japanese-generator.bas: the
        // names, comments, identifiers and strings are Japanese, with
        // characters whose code page 932 mapping differs between converters.
        for file in [
            "vbaJapaneseV2000.mdb",
            "vbaJapaneseV2003.mdb",
            "vbaJapaneseV2007.accdb",
        ] {
            let path = skip_if_missing!(file);
            let mut reader = PageReader::open(&path).unwrap();
            let expected = expected_sources(&mut reader);
            assert_eq!(expected.len(), 2, "{file}");
            let project = read_vba_project(&mut reader).unwrap();
            for (name, source) in &expected {
                let module = project
                    .modules
                    .iter()
                    .find(|m| &m.name == name)
                    .unwrap_or_else(|| panic!("{file}: no module {name}"));
                // Access keeps the Attribute lines out of the source it shows.
                let read: Vec<&str> = module
                    .source
                    .lines()
                    .filter(|line| !line.starts_with("Attribute "))
                    .collect();
                let shown: Vec<&str> = source.lines().collect();
                assert_eq!(read, shown, "{file}: {name}");
            }
            let module_type = |name: &str| {
                project
                    .modules
                    .iter()
                    .find(|m| m.name == name)
                    .map(|m| m.module_type)
            };
            assert_eq!(
                module_type("日本語モジュール"),
                Some(VbaModuleType::Standard),
                "{file}"
            );
            assert_eq!(
                module_type("顧客クラス"),
                Some(VbaModuleType::Class),
                "{file}"
            );
        }
    }

    #[test]
    fn vba_v2003() {
        let path = skip_if_missing!("vbaV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("failed to read VBA project");
        assert_vba_modules(&project, EXPECTED_MODULES);
    }

    #[test]
    fn vba_v2007() {
        let path = skip_if_missing!("vbaV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("failed to read VBA project");
        assert_vba_modules(&project, EXPECTED_MODULES);
    }

    #[test]
    fn storage_entries_v2003() {
        let path = skip_if_missing!("vbaV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let entries = storage::read_storage_entries(&mut reader).unwrap();
        assert!(!entries.is_empty());
    }

    #[test]
    fn vba_v2000_catalog() {
        let path = skip_if_missing!("vbaV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let catalog = catalog::read_catalog(&mut reader).unwrap();
        assert!(!catalog.is_empty(), "catalog should not be empty");
    }

    #[test]
    fn vba_v2000() {
        // No MSysAccessStorage in this database: the storage entries come from
        // the compound file in MSysAccessObjects.
        let path = skip_if_missing!("vbaV2000.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let project = read_vba_project(&mut reader).expect("failed to read VBA project");
        assert_vba_modules(&project, EXPECTED_MODULES);
    }

    // -- build_entry_path unit tests ------------------------------------------

    fn make_entry(id: i32, parent_id: i32, name: &str) -> StorageEntry {
        StorageEntry {
            id,
            parent_id,
            name: name.to_string(),
            entry_type: 0,
            data: Vec::new(),
        }
    }

    #[test]
    fn build_entry_path_normal() {
        // VBAProject(id=1) -> VBA(id=2) -> dir(id=3)
        let vba = make_entry(2, 1, "VBA");
        let dir = make_entry(3, 2, "dir");
        let id_map: HashMap<i32, &StorageEntry> = [(2, &vba)].into_iter().collect();
        assert_eq!(
            storage::build_entry_path(&dir, 1, &id_map),
            Some("/VBA/dir".to_string())
        );
    }

    #[test]
    fn build_entry_path_circular() {
        // A(id=2, parent=3) -> B(id=3, parent=2) — cycle
        let a = make_entry(2, 3, "A");
        let b = make_entry(3, 2, "B");
        let id_map: HashMap<i32, &StorageEntry> = [(2, &a), (3, &b)].into_iter().collect();
        assert_eq!(storage::build_entry_path(&a, 1, &id_map), None);
    }

    #[test]
    fn build_entry_path_missing_parent() {
        // Entry with parent_id=99 which doesn't exist in the map
        let entry = make_entry(2, 99, "orphan");
        let id_map: HashMap<i32, &StorageEntry> = HashMap::new();
        assert_eq!(storage::build_entry_path(&entry, 1, &id_map), None);
    }

    // The examples of MS-OVBA 3.2.
    #[test]
    fn decompress_the_examples_of_the_specification() {
        let no_compression = [
            0x01, 0x19, 0xB0, 0x00, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x00, 0x69,
            0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70, 0x00, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76,
            0x2E,
        ];
        assert_eq!(
            decompress(&no_compression).unwrap(),
            b"abcdefghijklmnopqrstuv."
        );
        let normal = [
            0x01, 0x2F, 0xB0, 0x00, 0x23, 0x61, 0x61, 0x61, 0x62, 0x63, 0x64, 0x65, 0x82, 0x66,
            0x00, 0x70, 0x61, 0x67, 0x68, 0x69, 0x6A, 0x01, 0x38, 0x08, 0x61, 0x6B, 0x6C, 0x00,
            0x30, 0x6D, 0x6E, 0x6F, 0x70, 0x06, 0x71, 0x02, 0x70, 0x04, 0x10, 0x72, 0x73, 0x74,
            0x75, 0x76, 0x10, 0x77, 0x78, 0x79, 0x7A, 0x00, 0x3C,
        ];
        assert_eq!(
            decompress(&normal).unwrap(),
            b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa"
        );
    }

    #[test]
    fn decompress_a_copy_that_overlaps_what_it_appends() {
        // "a", then a CopyToken of offset 1 and length 14.
        let data = [0x01, 0x03, 0xB0, 0x02, 0x61, 0x0B, 0x00];
        assert_eq!(decompress(&data).unwrap(), b"aaaaaaaaaaaaaaa");
    }

    #[test]
    fn decompress_an_uncompressed_chunk() {
        // The flag bit is clear: the chunk holds its bytes as they are.
        assert_eq!(decompress(&[0x01, 0x02, 0x30, 1, 2, 3]).unwrap(), [1, 2, 3]);
    }

    #[test]
    fn decompress_broken_data_is_an_error() {
        for (data, reason) in [
            (&[][..], "without its signature"),
            (&[0x02, 0x02, 0xB0][..], "without its signature"),
            (&[0x01][..], "without chunks"),
            (&[0x01, 0x02][..], "cut short"),
            (&[0x01, 0x02, 0xC0, 0x00][..], "wrong signature"),
            // A chunk of 26 bytes, of which 1 is there.
            (
                &[0x01, 0x19, 0xB0, 0x61][..],
                "past the end of its container",
            ),
            // A CopyToken first, with nothing before it to copy.
            (
                &[0x01, 0x02, 0xB0, 0x01, 0x00, 0x00][..],
                "before the start",
            ),
            // A CopyToken of one byte.
            (&[0x01, 0x01, 0xB0, 0x01, 0x00][..], "cut short"),
        ] {
            let error = decompress(data).unwrap_err().to_string();
            assert!(error.contains(reason), "{data:02X?}: {error}");
        }
    }

    #[test]
    fn decompress_a_chunk_longer_than_4096_bytes_is_an_error() {
        // "a", a CopyToken of offset 1 and length 4098, then another
        // CopyToken: a chunk decompresses to 4096 bytes at most, and the
        // offset of a token past that has no bits.
        let data = [0x01, 0x05, 0xB0, 0x06, b'a', 0xFF, 0x0F, 0x00, 0x00];
        let error = decompress(&data).unwrap_err().to_string();
        assert!(error.contains("longer than 4096"), "{error}");
    }

    fn module_of_stream(text_offset: usize) -> ovba::Module {
        ovba::Module {
            name: "Module1".to_string(),
            stream_name: "Module1".to_string(),
            doc_string: String::new(),
            text_offset,
            help_context: 0,
            module_type: ovba::ModuleType::Procedural,
            read_only: false,
            private: false,
        }
    }

    fn cfb_of(streams: &[(&str, &[u8])]) -> cfb::CompoundFile<Cursor<Vec<u8>>> {
        let mut writer = CfbWriter::new().unwrap();
        writer.storage("/VBA").unwrap();
        for (path, data) in streams {
            writer.stream(path, data).unwrap();
        }
        cfb::CompoundFile::open(Cursor::new(writer.finish().unwrap())).unwrap()
    }

    #[test]
    fn module_source_from_its_text_offset() {
        // Two bytes of the performance cache, then "abc" in an uncompressed chunk.
        let stream = [0xFF, 0xFF, 0x01, 0x02, 0x30, b'a', b'b', b'c'];
        let mut cfb = cfb_of(&[("/VBA/Module1", &stream)]);
        assert_eq!(
            module_source(&mut cfb, &module_of_stream(2), 1252).unwrap(),
            "abc"
        );
    }

    #[test]
    fn module_source_past_the_end_of_its_stream_is_an_error() {
        let mut cfb = cfb_of(&[("/VBA/Module1", &[0x01, 0x02, 0x30, b'a', b'b', b'c'])]);
        let error = module_source(&mut cfb, &module_of_stream(7), 1252)
            .unwrap_err()
            .to_string();
        assert!(error.contains("past the end of its stream"), "{error}");
    }

    #[test]
    fn broken_dir_stream_is_an_error_before_ovba_reads_it() {
        let mut writer = CfbWriter::new().unwrap();
        writer.storage("/VBA").unwrap();
        writer
            .stream("/VBA/dir", &[0x01, 0x02, 0xB0, 0x01, 0x00, 0x00])
            .unwrap();
        let error = extract_modules_from_cfb(writer.finish().unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("before the start"), "{error}");
    }
}
