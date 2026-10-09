//! DDL (CREATE TABLE / INDEX / FOREIGN KEY) generation for multiple SQL dialects.

pub mod access;
pub mod mysql;
pub mod postgres;
pub mod sqlite;

pub use access::Access;
pub use mysql::Mysql;
pub use postgres::Postgres;
pub use sqlite::Sqlite;

use std::borrow::Cow;
use std::collections::HashMap;

use crate::format::{column_flags, index_flags, index_type, ColumnType, ObjectType};
use crate::{
    CatalogEntry, ColumnDef, FileError, IndexDef, PageReader, Relationship, RelationshipColumn,
    TableDef,
};

/// The names of the dialects, as `jetdb schema --ddl` takes them.
pub const DIALECT_NAMES: [&str; 4] = ["sqlite", "postgres", "mysql", "access"];

/// The dialect named `name`, one of [`DIALECT_NAMES`].
pub fn dialect(name: &str) -> Option<&'static dyn DdlDialect> {
    match name {
        "sqlite" => Some(&Sqlite),
        "postgres" => Some(&Postgres),
        "mysql" => Some(&Mysql),
        "access" => Some(&Access),
        _ => None,
    }
}

/// The definitions of the tables `jetdb schema` shows: the user tables in
/// catalog order, or `table` alone, which may be a system table. Each
/// calculated column has the type of its result (see
/// [`with_calculated_column_types`]), which its values have.
pub fn schema_tables(
    reader: &mut PageReader,
    catalog: &[CatalogEntry],
    table: Option<&str>,
) -> Result<Vec<TableDef>, FileError> {
    let targets: Vec<&CatalogEntry> = match table {
        Some(name) => vec![crate::find_table(catalog, name)?],
        None => catalog
            .iter()
            .filter(|e| e.object_type == ObjectType::Table && !e.is_system_or_hidden())
            .collect(),
    };
    let mut tables = Vec::with_capacity(targets.len());
    for entry in targets {
        let tdef = crate::read_table_def(reader, &entry.name, entry.table_page)?;
        let calculated = crate::calculated_column_types(reader, &tdef);
        tables.push(with_calculated_column_types(&tdef, &calculated));
    }
    Ok(tables)
}

// ---------------------------------------------------------------------------
// DdlDialect trait
// ---------------------------------------------------------------------------

pub trait DdlDialect {
    /// Quote an identifier
    fn quote_id(&self, name: &str) -> String;

    /// Map a column definition to a SQL type string.
    /// When `is_auto` is true, include auto-increment syntax.
    /// A Numeric column with precision 0 has no fixed precision or scale (see
    /// [`with_calculated_column_types`]) and maps to the widest Numeric type.
    fn map_column_type(&self, col: &ColumnDef, is_auto: bool) -> String;

    /// Whether auto-increment columns absorb the PRIMARY KEY constraint
    /// (true only for SQLite)
    fn auto_increment_absorbs_pk(&self) -> bool;

    /// Whether foreign keys should be inlined in CREATE TABLE
    /// (true for SQLite, false for others)
    fn inline_foreign_keys(&self) -> bool;

    /// Whether an auto-increment column must begin an index declared in its
    /// CREATE TABLE (true only for MySQL, whose InnoDB looks up the largest
    /// value through it)
    fn auto_increment_needs_key(&self) -> bool;

    /// Whether the dialect has SQL comments (`--`); Access SQL has none
    fn supports_comments(&self) -> bool;

    /// Whether an index name needs to be unique only within its table (MySQL,
    /// Access), rather than among all the tables and indexes of the schema
    /// (SQLite, PostgreSQL), where [`generate_create_indexes`] names an index
    /// `{table}_{index}_idx`.
    fn index_names_per_table(&self) -> bool;
}

// ---------------------------------------------------------------------------
// Primary key detection
// ---------------------------------------------------------------------------

/// Find the primary key index. `index_type::PRIMARY` is Access's own
/// `Index.Primary` property, not inferred from the index's name (which the
/// user can rename freely) or its UNIQUE/REQUIRED flags (an ordinary,
/// non-primary index can carry either -- e.g. every Attachment/multivalue
/// column gets its own hidden UNIQUE + REQUIRED index) -- see
/// `format::index_type`'s doc comment.
fn find_primary_key(tdef: &TableDef) -> Option<&IndexDef> {
    tdef.indexes
        .iter()
        .find(|idx| idx.index_type == index_type::PRIMARY)
}

/// Check if an index is the hidden index Access keeps on a complex column.
fn is_complex_column_index(tdef: &TableDef, idx: &IndexDef) -> bool {
    idx.columns.iter().all(|ic| {
        tdef.columns
            .iter()
            .any(|c| c.col_num == ic.col_num && c.col_type == ColumnType::ComplexType)
    })
}

/// A copy of `tdef` in which each calculated column has the type of its
/// result, from [`crate::calculated_column_types`], in place of the
/// placeholder type Access declares (most numeric results as Double). This is
/// the type of the values [`crate::read_table_rows`] reads, so a table made
/// from the DDL holds the exported values.
///
/// A calculated column has no fixed precision or scale: each Numeric value
/// carries its own. Its precision and scale are set to 0, which each dialect
/// writes as its widest Numeric type.
pub fn with_calculated_column_types(
    tdef: &TableDef,
    calculated: &HashMap<String, ColumnType>,
) -> TableDef {
    let mut out = tdef.clone();
    for col in &mut out.columns {
        if let Some(&result_type) = calculated.get(&col.name.to_ascii_lowercase()) {
            col.col_type = result_type;
            col.precision = 0;
            col.scale = 0;
        }
    }
    out
}

/// Resolve an index column number to a column name.
fn resolve_col_name(tdef: &TableDef, col_num: u16) -> &str {
    tdef.columns
        .iter()
        .find(|c| c.col_num == col_num)
        .map(|c| c.name.as_str())
        .unwrap_or("?")
}

// ---------------------------------------------------------------------------
// DDL generation (pure functions returning String)
// ---------------------------------------------------------------------------

/// Generate complete DDL for all tables.
pub fn generate_ddl(
    dialect: &dyn DdlDialect,
    tables: &[TableDef],
    relationships: &[Relationship],
    include_indexes: bool,
    include_relations: bool,
) -> String {
    let mut out = String::new();

    // 1. CREATE TABLE statements
    for (i, tdef) in tables.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let table_rels: Vec<&Relationship> = if dialect.inline_foreign_keys() && include_relations {
            relationships
                .iter()
                .filter(|r| r.from_table == tdef.name)
                .collect()
        } else {
            Vec::new()
        };
        out.push_str(&generate_create_table(dialect, tdef, &table_rels));
    }

    // 2. CREATE INDEX statements
    if include_indexes {
        for tdef in tables {
            let idx_sql = generate_create_indexes(dialect, tdef);
            if !idx_sql.is_empty() {
                out.push('\n');
                out.push_str(&idx_sql);
            }
        }
    }

    // 3. ALTER TABLE FOREIGN KEY statements (non-inline dialects only)
    if include_relations && !dialect.inline_foreign_keys() {
        let table_names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        let filtered_rels: Vec<&Relationship> = relationships
            .iter()
            .filter(|r| table_names.contains(&r.from_table.as_str()))
            .collect();
        let fk_sql = generate_foreign_keys(dialect, &filtered_rels);
        if !fk_sql.is_empty() {
            out.push('\n');
            out.push_str(&fk_sql);
        }
    }

    out
}

/// The integer AutoNumber column that is the whole primary key `pk`, if any.
/// A GUID AutoNumber gets a default rather than auto-increment syntax, which
/// cannot stand in for the PRIMARY KEY.
fn sole_auto_increment_column<'a>(tdef: &'a TableDef, pk: &IndexDef) -> Option<&'a ColumnDef> {
    let [only] = pk.columns.as_slice() else {
        return None;
    };
    tdef.columns
        .iter()
        .find(|c| c.col_num == only.col_num && c.is_auto_number() && c.col_type != ColumnType::Guid)
}

/// Whether `col` is written as an AutoNumber of `dialect`. Where the
/// auto-increment syntax is itself a PRIMARY KEY (SQLite), only the column
/// that is the whole primary key can have it, and any other integer
/// AutoNumber column is written as a plain column.
fn writes_auto_number(
    dialect: &dyn DdlDialect,
    col: &ColumnDef,
    auto_pk_col: Option<&ColumnDef>,
) -> bool {
    if !col.is_auto_number() {
        return false;
    }
    if col.col_type == ColumnType::Guid || !dialect.auto_increment_absorbs_pk() {
        return true;
    }
    auto_pk_col.is_some_and(|c| c.col_num == col.col_num)
}

/// `col` with the size of a Text column in characters, as SQL sizes a text
/// type, rather than in the bytes Access stores: two a character in Jet4 and
/// later (see [`TableDef::shown_size`]).
fn in_characters<'a>(tdef: &TableDef, col: &'a ColumnDef) -> Cow<'a, ColumnDef> {
    let size = tdef.shown_size(col);
    if size == col.col_size {
        return Cow::Borrowed(col);
    }
    let mut chars = col.clone();
    chars.col_size = size;
    Cow::Owned(chars)
}

/// The `KEY` lines a dialect that needs an auto-increment column to begin an
/// index (MySQL) adds to CREATE TABLE: one for each integer AutoNumber column
/// that does not begin the primary key `pk`. The indexes of the table come
/// after CREATE TABLE, too late for it. A GUID AutoNumber has a default
/// rather than auto-increment.
fn auto_increment_keys(
    dialect: &dyn DdlDialect,
    tdef: &TableDef,
    pk: Option<&IndexDef>,
    auto_pk_col: Option<&ColumnDef>,
) -> Vec<String> {
    if !dialect.auto_increment_needs_key() {
        return Vec::new();
    }
    let pk_first = pk.and_then(|idx| idx.columns.first()).map(|c| c.col_num);
    tdef.columns
        .iter()
        .filter(|c| {
            writes_auto_number(dialect, c, auto_pk_col)
                && c.col_type != ColumnType::Guid
                && Some(c.col_num) != pk_first
        })
        .map(|c| format!("    KEY ({})", dialect.quote_id(&c.name)))
        .collect()
}

/// Generate CREATE TABLE statement for a single table.
pub fn generate_create_table(
    dialect: &dyn DdlDialect,
    tdef: &TableDef,
    table_rels: &[&Relationship],
) -> String {
    let pk = find_primary_key(tdef);
    let auto_pk_col = pk.and_then(|idx| sole_auto_increment_column(tdef, idx));

    // If dialect absorbs PK and there's an auto-increment PK col, suppress table-level PK
    let suppress_pk = dialect.auto_increment_absorbs_pk() && auto_pk_col.is_some();

    let mut lines: Vec<String> = Vec::new();

    // Column definitions
    for col in &tdef.columns {
        let is_auto = writes_auto_number(dialect, col, auto_pk_col);
        let type_str = dialect.map_column_type(&in_characters(tdef, col), is_auto);
        // An AutoNumber is never NULL, even written as a plain column where
        // Access flags it as nullable.
        let not_null = if (col.flags & column_flags::NULLABLE) == 0 || col.is_auto_number() {
            " NOT NULL"
        } else {
            ""
        };

        // For auto-increment columns, NOT NULL is embedded in the type string
        // returned by map_column_type(), so we skip appending it here.
        if is_auto {
            lines.push(format!("    {} {}", dialect.quote_id(&col.name), type_str));
        } else {
            lines.push(format!(
                "    {} {}{}",
                dialect.quote_id(&col.name),
                type_str,
                not_null
            ));
        }
    }

    // PRIMARY KEY constraint (table-level)
    if !suppress_pk {
        if let Some(pk_idx) = pk {
            let pk_cols: Vec<String> = pk_idx
                .columns
                .iter()
                .map(|c| dialect.quote_id(resolve_col_name(tdef, c.col_num)))
                .collect();
            lines.push(format!("    PRIMARY KEY ({})", pk_cols.join(", ")));
        }
    }
    lines.extend(auto_increment_keys(dialect, tdef, pk, auto_pk_col));

    // Inline foreign keys (SQLite). A relationship without referential
    // integrity is no constraint; it follows the statement as a comment.
    let (table_rels, notes): (Vec<&Relationship>, Vec<&Relationship>) = table_rels
        .iter()
        .copied()
        .partition(|r| r.has_referential_integrity());
    for rel in table_rels {
        let from_cols: Vec<String> = rel
            .columns
            .iter()
            .map(|c| dialect.quote_id(&c.from_column))
            .collect();
        let to_cols: Vec<String> = rel
            .columns
            .iter()
            .map(|c| dialect.quote_id(&c.to_column))
            .collect();
        let mut fk = format!(
            "    FOREIGN KEY ({}) REFERENCES {} ({})",
            from_cols.join(", "),
            dialect.quote_id(&rel.to_table),
            to_cols.join(", ")
        );
        append_cascade_clauses(&mut fk, rel);
        lines.push(fk);
    }

    let mut out = format!(
        "CREATE TABLE {} (\n{}\n);\n",
        dialect.quote_id(&tdef.name),
        lines.join(",\n")
    );
    for rel in notes {
        out.push_str(&no_integrity_note(dialect, rel));
    }
    out
}

/// A comment for a relationship without referential integrity, which Access
/// does not enforce and so is no constraint; nothing where the dialect has
/// no comments.
fn no_integrity_note(dialect: &dyn DdlDialect, rel: &Relationship) -> String {
    if !dialect.supports_comments() {
        return String::new();
    }
    let cols = |pick: fn(&RelationshipColumn) -> &str| -> String {
        rel.columns
            .iter()
            .map(|c| dialect.quote_id(pick(c)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "-- Relationship {} from {} ({}) to {} ({}) does not enforce referential integrity.\n",
        dialect.quote_id(&rel.name),
        dialect.quote_id(&rel.from_table),
        cols(|c| &c.from_column),
        dialect.quote_id(&rel.to_table),
        cols(|c| &c.to_column)
    )
}

/// Generate CREATE INDEX statements for a single table.
pub fn generate_create_indexes(dialect: &dyn DdlDialect, tdef: &TableDef) -> String {
    let pk = find_primary_key(tdef);
    let mut out = String::new();

    for idx in &tdef.indexes {
        // Skip FK indexes
        if idx.index_type == index_type::FOREIGN_KEY {
            continue;
        }
        // Skip the hidden indexes on complex columns
        if is_complex_column_index(tdef, idx) {
            continue;
        }
        // Skip the primary key index (already in CREATE TABLE)
        if let Some(pk_idx) = pk {
            if idx.index_num == pk_idx.index_num {
                continue;
            }
        }

        let unique = if (idx.flags & index_flags::UNIQUE) != 0 {
            "UNIQUE "
        } else {
            ""
        };
        let cols: Vec<String> = idx
            .columns
            .iter()
            .map(|c| dialect.quote_id(resolve_col_name(tdef, c.col_num)))
            .collect();
        // Access names an index within its table, so two tables often have
        // an index of the same name, such as "id".
        let name = if dialect.index_names_per_table() {
            idx.name.clone()
        } else {
            format!("{}_{}_idx", tdef.name, idx.name)
        };
        out.push_str(&format!(
            "CREATE {unique}INDEX {} ON {} ({});\n",
            dialect.quote_id(&name),
            dialect.quote_id(&tdef.name),
            cols.join(", ")
        ));
    }

    out
}

/// Generate ALTER TABLE ADD FOREIGN KEY statements.
pub fn generate_foreign_keys(dialect: &dyn DdlDialect, relationships: &[&Relationship]) -> String {
    let mut out = String::new();

    for rel in relationships {
        if !rel.has_referential_integrity() {
            out.push_str(&no_integrity_note(dialect, rel));
            continue;
        }
        let from_cols: Vec<String> = rel
            .columns
            .iter()
            .map(|c| dialect.quote_id(&c.from_column))
            .collect();
        let to_cols: Vec<String> = rel
            .columns
            .iter()
            .map(|c| dialect.quote_id(&c.to_column))
            .collect();

        let mut stmt = format!(
            "ALTER TABLE {} ADD CONSTRAINT {}\n    FOREIGN KEY ({}) REFERENCES {} ({})",
            dialect.quote_id(&rel.from_table),
            dialect.quote_id(&rel.name),
            from_cols.join(", "),
            dialect.quote_id(&rel.to_table),
            to_cols.join(", ")
        );
        append_cascade_clauses(&mut stmt, rel);
        stmt.push_str(";\n");
        out.push_str(&stmt);
    }

    out
}

fn append_cascade_clauses(stmt: &mut String, rel: &Relationship) {
    use crate::relationship::relationship_flags;
    if (rel.flags & relationship_flags::CASCADE_UPDATE) != 0 {
        stmt.push_str("\n    ON UPDATE CASCADE");
    }
    if (rel.flags & relationship_flags::CASCADE_DELETE) != 0 {
        stmt.push_str("\n    ON DELETE CASCADE");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{column_flags, index_flags, index_type, ColumnType};
    use crate::{
        ColumnDef, IndexColumn, IndexColumnOrder, IndexDef, Relationship, RelationshipColumn,
        TableDef,
    };

    // -- Test helpers ---------------------------------------------------------

    fn col(
        name: &str,
        col_type: ColumnType,
        col_size: u16,
        flags: u8,
        precision: u8,
        scale: u8,
    ) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size,
            flags,
            is_fixed: false,
            precision,
            scale,
            is_calculated: false,
            display_index: 0,
        }
    }

    fn col_with_num(
        name: &str,
        col_type: ColumnType,
        col_size: u16,
        flags: u8,
        precision: u8,
        scale: u8,
        col_num: u16,
    ) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            col_type,
            col_num,
            var_col_num: 0,
            fixed_offset: 0,
            col_size,
            flags,
            is_fixed: false,
            precision,
            scale,
            is_calculated: false,
            display_index: col_num,
        }
    }

    fn index(name: &str, col_nums: &[u16], flags: u8, idx_type: u8, index_num: u16) -> IndexDef {
        IndexDef {
            name: name.to_string(),
            index_num,
            index_type: idx_type,
            columns: col_nums
                .iter()
                .map(|&n| IndexColumn {
                    col_num: n,
                    order: IndexColumnOrder::Ascending,
                })
                .collect(),
            flags,
            first_data_page: 0,
            foreign_key: None,
        }
    }

    fn table(name: &str, columns: Vec<ColumnDef>, indexes: Vec<IndexDef>) -> TableDef {
        TableDef {
            name: name.to_string(),
            num_rows: 0,
            num_cols: columns.len() as u16,
            num_var_cols: 0,
            columns,
            indexes,
            data_pages: vec![],
            is_jet3: false,
        }
    }

    fn relationship(
        name: &str,
        from_table: &str,
        to_table: &str,
        col_pairs: &[(&str, &str)],
        flags: u32,
    ) -> Relationship {
        Relationship {
            name: name.to_string(),
            from_table: from_table.to_string(),
            to_table: to_table.to_string(),
            columns: col_pairs
                .iter()
                .map(|(f, t)| RelationshipColumn {
                    from_column: f.to_string(),
                    to_column: t.to_string(),
                })
                .collect(),
            flags,
        }
    }

    fn sqlite() -> Box<dyn DdlDialect> {
        Box::new(Sqlite)
    }
    fn postgres() -> Box<dyn DdlDialect> {
        Box::new(Postgres)
    }
    fn mysql() -> Box<dyn DdlDialect> {
        Box::new(Mysql)
    }
    fn access() -> Box<dyn DdlDialect> {
        Box::new(Access)
    }

    // ========================================================================
    // Type mapping tests
    // ========================================================================

    // -- SQLite ---------------------------------------------------------------

    #[test]
    fn sqlite_map_text() {
        let d = sqlite();
        let c = col("x", ColumnType::Text, 100, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TEXT");
    }

    #[test]
    fn sqlite_map_long() {
        let d = sqlite();
        let c = col("x", ColumnType::Long, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn sqlite_map_long_auto() {
        let d = sqlite();
        let c = col("x", ColumnType::Long, 0, column_flags::AUTO_LONG, 0, 0);
        assert_eq!(
            d.map_column_type(&c, true),
            "INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT"
        );
    }

    #[test]
    fn sqlite_map_money() {
        let d = sqlite();
        let c = col("x", ColumnType::Money, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "NUMERIC");
    }

    #[test]
    fn sqlite_map_timestamp() {
        let d = sqlite();
        let c = col("x", ColumnType::Timestamp, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TEXT");
    }

    #[test]
    fn sqlite_map_binary() {
        let d = sqlite();
        let c = col("x", ColumnType::Binary, 50, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BLOB");
    }

    #[test]
    fn sqlite_map_guid() {
        let d = sqlite();
        let c = col("x", ColumnType::Guid, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TEXT");
    }

    // -- PostgreSQL -----------------------------------------------------------

    #[test]
    fn postgres_map_text() {
        let d = postgres();
        let c = col("x", ColumnType::Text, 100, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "VARCHAR(100)");
    }

    #[test]
    fn postgres_map_boolean() {
        let d = postgres();
        let c = col("x", ColumnType::Boolean, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BOOLEAN");
    }

    #[test]
    fn postgres_map_guid() {
        let d = postgres();
        let c = col("x", ColumnType::Guid, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "UUID");
    }

    #[test]
    fn postgres_map_long_auto() {
        let d = postgres();
        let c = col("x", ColumnType::Long, 0, column_flags::AUTO_LONG, 0, 0);
        assert_eq!(
            d.map_column_type(&c, true),
            "INTEGER NOT NULL GENERATED ALWAYS AS IDENTITY"
        );
    }

    #[test]
    fn postgres_map_timestamp() {
        let d = postgres();
        let c = col("x", ColumnType::Timestamp, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TIMESTAMP WITHOUT TIME ZONE");
    }

    #[test]
    fn postgres_map_money() {
        let d = postgres();
        let c = col("x", ColumnType::Money, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "NUMERIC(19,4)");
    }

    #[test]
    fn postgres_map_ole() {
        let d = postgres();
        let c = col("x", ColumnType::Ole, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BYTEA");
    }

    // -- MySQL ----------------------------------------------------------------

    #[test]
    fn mysql_map_text() {
        let d = mysql();
        let c = col("x", ColumnType::Text, 100, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "VARCHAR(100)");
    }

    #[test]
    fn mysql_map_boolean() {
        let d = mysql();
        let c = col("x", ColumnType::Boolean, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BOOLEAN");
    }

    #[test]
    fn mysql_map_byte() {
        let d = mysql();
        let c = col("x", ColumnType::Byte, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TINYINT UNSIGNED");
    }

    #[test]
    fn mysql_map_long_auto() {
        let d = mysql();
        let c = col("x", ColumnType::Long, 0, column_flags::AUTO_LONG, 0, 0);
        assert_eq!(d.map_column_type(&c, true), "INT NOT NULL AUTO_INCREMENT");
    }

    #[test]
    fn mysql_map_ole() {
        let d = mysql();
        let c = col("x", ColumnType::Ole, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "LONGBLOB");
    }

    #[test]
    fn mysql_map_guid() {
        // A GUID is read and exported with its braces, 38 characters.
        let d = mysql();
        let c = col("x", ColumnType::Guid, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "CHAR(38)");
        assert_eq!(crate::data::format_guid(&[0; 16]).len(), 38);
    }

    // -- Access SQL -----------------------------------------------------------

    #[test]
    fn access_map_long_auto() {
        let d = access();
        let c = col("x", ColumnType::Long, 0, column_flags::AUTO_LONG, 0, 0);
        assert_eq!(d.map_column_type(&c, true), "COUNTER NOT NULL");
    }

    #[test]
    fn access_map_money() {
        let d = access();
        let c = col("x", ColumnType::Money, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "CURRENCY");
    }

    #[test]
    fn access_map_boolean() {
        let d = access();
        let c = col("x", ColumnType::Boolean, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "YESNO");
    }

    #[test]
    fn access_map_text() {
        let d = access();
        let c = col("x", ColumnType::Text, 100, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TEXT(100)");
    }

    #[test]
    fn access_map_memo() {
        let d = access();
        let c = col("x", ColumnType::Memo, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "MEMO");
    }

    // ========================================================================
    // Identifier quoting tests
    // ========================================================================

    #[test]
    fn sqlite_quote_id() {
        let d = sqlite();
        assert_eq!(d.quote_id("Table 1"), "\"Table 1\"");
    }

    #[test]
    fn sqlite_quote_id_escape() {
        let d = sqlite();
        assert_eq!(d.quote_id("col\"x"), "\"col\"\"x\"");
    }

    #[test]
    fn postgres_quote_id_escape() {
        let d = postgres();
        assert_eq!(d.quote_id("col\"x"), "\"col\"\"x\"");
    }

    #[test]
    fn mysql_quote_id() {
        let d = mysql();
        assert_eq!(d.quote_id("Table 1"), "`Table 1`");
    }

    #[test]
    fn mysql_quote_id_escape() {
        let d = mysql();
        assert_eq!(d.quote_id("col`x"), "`col``x`");
    }

    #[test]
    fn access_quote_id() {
        let d = access();
        assert_eq!(d.quote_id("Table 1"), "[Table 1]");
    }

    #[test]
    fn access_quote_id_escape() {
        let d = access();
        assert_eq!(d.quote_id("col]x"), "[col]]x]");
    }

    // ========================================================================
    // generate_create_table tests
    // ========================================================================

    #[test]
    fn create_table_basic_postgres() {
        let d = postgres();
        let tdef = table(
            "T",
            vec![
                col_with_num("A", ColumnType::Text, 100, 0, 0, 0, 1),
                col_with_num("B", ColumnType::Long, 0, column_flags::NULLABLE, 0, 0, 2),
            ],
            vec![],
        );
        let result = generate_create_table(&*d, &tdef, &[]);
        // 100 bytes of Jet4 text are 50 characters.
        assert_eq!(
            result,
            "CREATE TABLE \"T\" (\n    \"A\" VARCHAR(50) NOT NULL,\n    \"B\" INTEGER\n);\n"
        );
    }

    #[test]
    fn text_size_in_characters_by_version() {
        // Jet4 and later store two bytes a character, Jet3 one.
        let mut tdef = table(
            "T",
            vec![col_with_num("A", ColumnType::Text, 510, 0, 0, 0, 1)],
            vec![],
        );
        for (is_jet3, access, postgres_size) in
            [(false, "TEXT(255)", 255), (true, "TEXT(510)", 510)]
        {
            tdef.is_jet3 = is_jet3;
            let ddl = generate_create_table(&Access, &tdef, &[]);
            assert!(
                ddl.contains(&format!("[A] {access} NOT NULL")),
                "got:\n{ddl}"
            );
            let ddl = generate_create_table(&Postgres, &tdef, &[]);
            assert!(
                ddl.contains(&format!("\"A\" VARCHAR({postgres_size}) NOT NULL")),
                "got:\n{ddl}"
            );
            let ddl = generate_create_table(&Mysql, &tdef, &[]);
            assert!(
                ddl.contains(&format!("`A` VARCHAR({postgres_size}) NOT NULL")),
                "got:\n{ddl}"
            );
        }
    }

    #[test]
    fn create_table_pk_postgres() {
        let d = postgres();
        let tdef = table(
            "T",
            vec![col_with_num("id", ColumnType::Long, 0, 0, 0, 0, 1)],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_table(&*d, &tdef, &[]);
        assert!(result.contains("PRIMARY KEY (\"id\")"), "got:\n{result}");
    }

    #[test]
    fn create_table_sqlite_auto_pk() {
        let d = sqlite();
        let tdef = table(
            "T",
            vec![col_with_num(
                "id",
                ColumnType::Long,
                0,
                column_flags::FIXED | column_flags::AUTO_LONG,
                0,
                0,
                1,
            )],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_table(&*d, &tdef, &[]);
        assert!(
            result.contains("INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT"),
            "got:\n{result}"
        );
        // Table-level PRIMARY KEY should be suppressed
        assert!(
            !result.contains("    PRIMARY KEY"),
            "should not have table-level PK, got:\n{result}"
        );
    }

    /// A table with an AutoNumber column "auto" (col 1) and a primary key on
    /// `pk_cols` of the columns "a" (2) and "b" (3).
    fn auto_number_table(pk_cols: &[u16]) -> TableDef {
        table(
            "T",
            vec![
                // Flagged as nullable, as Access flags some AutoNumber columns.
                col_with_num(
                    "auto",
                    ColumnType::Long,
                    4,
                    column_flags::FIXED | column_flags::NULLABLE | column_flags::AUTO_LONG,
                    0,
                    0,
                    1,
                ),
                col_with_num("a", ColumnType::Long, 4, column_flags::FIXED, 0, 0, 2),
                col_with_num("b", ColumnType::Long, 4, column_flags::FIXED, 0, 0, 3),
            ],
            vec![index(
                "PrimaryKey",
                pk_cols,
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        )
    }

    #[test]
    fn create_table_sqlite_auto_number_outside_the_primary_key() {
        // SQLite has auto-increment only as INTEGER PRIMARY KEY, so an
        // AutoNumber column that is not the whole primary key is a plain
        // column, and the real primary key stays.
        for (pk_cols, pk) in [(&[2][..], "(\"a\")"), (&[1, 2][..], "(\"auto\", \"a\")")] {
            let result = generate_create_table(&*sqlite(), &auto_number_table(pk_cols), &[]);
            assert!(
                result.contains("    \"auto\" INTEGER NOT NULL,"),
                "got:\n{result}"
            );
            assert!(!result.contains("AUTOINCREMENT"), "got:\n{result}");
            assert!(
                result.contains(&format!("    PRIMARY KEY {pk}")),
                "got:\n{result}"
            );
        }
        // Other dialects keep auto-increment on a column outside the key.
        let result = generate_create_table(&*postgres(), &auto_number_table(&[2]), &[]);
        assert!(
            result.contains("\"auto\" INTEGER NOT NULL GENERATED ALWAYS AS IDENTITY"),
            "got:\n{result}"
        );
    }

    #[test]
    fn create_table_mysql_auto_increment_begins_an_index() {
        // InnoDB needs an AUTO_INCREMENT column to begin an index, and the
        // indexes come after CREATE TABLE.
        for (pk_cols, key) in [
            (&[1][..], false),    // the AutoNumber is the primary key
            (&[1, 2][..], false), // it begins the primary key
            (&[2][..], true),     // the primary key is another column
            (&[2, 1][..], true),  // it is in the primary key, but not first
        ] {
            let result = generate_create_table(&*mysql(), &auto_number_table(pk_cols), &[]);
            assert!(
                result.contains("`auto` INT NOT NULL AUTO_INCREMENT"),
                "got:\n{result}"
            );
            assert_eq!(
                result.contains("    KEY (`auto`)"),
                key,
                "{pk_cols:?}:\n{result}"
            );
        }
        // The other dialects need no KEY.
        let result = generate_create_table(&*postgres(), &auto_number_table(&[2]), &[]);
        assert!(!result.contains("\n    KEY ("), "got:\n{result}");
    }

    #[test]
    fn guid_auto_number_has_a_default_in_each_dialect() {
        let c = col("x", ColumnType::Guid, 16, column_flags::AUTO_UUID, 0, 0);
        assert_eq!(
            postgres().map_column_type(&c, true),
            "UUID NOT NULL DEFAULT gen_random_uuid()"
        );
        assert_eq!(
            mysql().map_column_type(&c, true),
            "CHAR(38) NOT NULL DEFAULT (CONCAT('{', UPPER(UUID()), '}'))"
        );
        assert_eq!(
            sqlite().map_column_type(&c, true),
            "TEXT NOT NULL DEFAULT ('{' || upper(hex(randomblob(4))) || '-' \
             || upper(hex(randomblob(2))) || '-' || upper(hex(randomblob(2))) || '-' \
             || upper(hex(randomblob(2))) || '-' || upper(hex(randomblob(6))) || '}')"
        );
        // The SQL view of Access rejects DEFAULT without ANSI-92 syntax.
        assert_eq!(
            access().map_column_type(&c, true),
            "UNIQUEIDENTIFIER NOT NULL"
        );
    }

    #[test]
    fn create_table_sqlite_guid_auto_pk_keeps_the_primary_key() {
        // A GUID AutoNumber gets a default, not PRIMARY KEY AUTOINCREMENT, so
        // the table-level PRIMARY KEY must stay.
        let tdef = table(
            "T",
            vec![col_with_num(
                "id",
                ColumnType::Guid,
                16,
                column_flags::FIXED | column_flags::AUTO_UUID,
                0,
                0,
                1,
            )],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_table(&*sqlite(), &tdef, &[]);
        assert!(
            result.contains("\"id\" TEXT NOT NULL DEFAULT ("),
            "got:\n{result}"
        );
        assert!(
            result.contains("    PRIMARY KEY (\"id\")"),
            "got:\n{result}"
        );
    }

    #[test]
    fn create_table_mysql_auto_increment() {
        let d = mysql();
        let tdef = table(
            "T",
            vec![col_with_num(
                "id",
                ColumnType::Long,
                0,
                column_flags::FIXED | column_flags::AUTO_LONG,
                0,
                0,
                1,
            )],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_table(&*d, &tdef, &[]);
        assert!(
            result.contains("INT NOT NULL AUTO_INCREMENT"),
            "got:\n{result}"
        );
        assert!(result.contains("PRIMARY KEY (`id`)"), "got:\n{result}");
    }

    #[test]
    fn create_table_access_counter() {
        let d = access();
        let tdef = table(
            "T",
            vec![col_with_num(
                "id",
                ColumnType::Long,
                0,
                column_flags::FIXED | column_flags::AUTO_LONG,
                0,
                0,
                1,
            )],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_table(&*d, &tdef, &[]);
        assert!(result.contains("COUNTER NOT NULL"), "got:\n{result}");
        assert!(result.contains("PRIMARY KEY ([id])"), "got:\n{result}");
    }

    // ========================================================================
    // generate_create_indexes tests
    // ========================================================================

    #[test]
    fn create_index_basic() {
        let d = postgres();
        let tdef = table(
            "T",
            vec![col_with_num("B", ColumnType::Long, 0, 0, 0, 0, 2)],
            vec![index("idx_B", &[2], 0, index_type::ORDINARY, 1)],
        );
        let result = generate_create_indexes(&*d, &tdef);
        assert_eq!(result, "CREATE INDEX \"T_idx_B_idx\" ON \"T\" (\"B\");\n");
    }

    #[test]
    fn index_names_of_two_tables() {
        // Access names indexes within their table: both tables have "id".
        let tables = ["T1", "T2"].map(|name| {
            table(
                name,
                vec![col_with_num("id", ColumnType::Long, 0, 0, 0, 0, 1)],
                vec![index("id", &[1], 0, index_type::ORDINARY, 1)],
            )
        });
        let names = |dialect: &dyn DdlDialect| -> Vec<String> {
            tables
                .iter()
                .map(|t| generate_create_indexes(dialect, t))
                .collect()
        };
        assert_eq!(
            names(&*sqlite()),
            [
                "CREATE INDEX \"T1_id_idx\" ON \"T1\" (\"id\");\n",
                "CREATE INDEX \"T2_id_idx\" ON \"T2\" (\"id\");\n"
            ]
        );
        assert_eq!(
            names(&*postgres()),
            [
                "CREATE INDEX \"T1_id_idx\" ON \"T1\" (\"id\");\n",
                "CREATE INDEX \"T2_id_idx\" ON \"T2\" (\"id\");\n"
            ]
        );
        assert_eq!(
            names(&*mysql()),
            [
                "CREATE INDEX `id` ON `T1` (`id`);\n",
                "CREATE INDEX `id` ON `T2` (`id`);\n"
            ]
        );
        assert_eq!(
            names(&*access()),
            [
                "CREATE INDEX [id] ON [T1] ([id]);\n",
                "CREATE INDEX [id] ON [T2] ([id]);\n"
            ]
        );
    }

    #[test]
    fn create_index_unique() {
        let d = postgres();
        let tdef = table(
            "T",
            vec![col_with_num("B", ColumnType::Long, 0, 0, 0, 0, 2)],
            vec![index(
                "idx_B",
                &[2],
                index_flags::UNIQUE,
                index_type::ORDINARY,
                1,
            )],
        );
        let result = generate_create_indexes(&*d, &tdef);
        assert_eq!(
            result,
            "CREATE UNIQUE INDEX \"T_idx_B_idx\" ON \"T\" (\"B\");\n"
        );
    }

    #[test]
    fn create_index_skip_pk() {
        let d = postgres();
        let tdef = table(
            "T",
            vec![col_with_num("id", ColumnType::Long, 0, 0, 0, 0, 1)],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_indexes(&*d, &tdef);
        assert_eq!(result, "", "PK index should be skipped");
    }

    #[test]
    fn create_index_skip_fk() {
        let d = postgres();
        let tdef = table(
            "T",
            vec![col_with_num("fk_id", ColumnType::Long, 0, 0, 0, 0, 1)],
            vec![index("fk_idx", &[1], 0, index_type::FOREIGN_KEY, 0)],
        );
        let result = generate_create_indexes(&*d, &tdef);
        assert_eq!(result, "", "FK index should be skipped");
    }

    // ========================================================================
    // generate_foreign_keys tests
    // ========================================================================

    #[test]
    fn foreign_key_postgres() {
        let d = postgres();
        let rels = [relationship(
            "fk_child_parent",
            "Child",
            "Parent",
            &[("parent_id", "id")],
            0,
        )];
        let refs: Vec<&Relationship> = rels.iter().collect();
        let result = generate_foreign_keys(&*d, &refs);
        assert!(
            result.contains("ALTER TABLE \"Child\" ADD CONSTRAINT \"fk_child_parent\""),
            "got:\n{result}"
        );
        assert!(
            result.contains("FOREIGN KEY (\"parent_id\") REFERENCES \"Parent\" (\"id\")"),
            "got:\n{result}"
        );
    }

    #[test]
    fn relationship_without_integrity_is_a_comment() {
        // Access does not enforce it, so it is no constraint.
        let rel = relationship(
            "Table1Table2",
            "Table2",
            "Table1",
            &[("Field1", "Field1")],
            crate::relationship_flags::NO_REFERENTIAL_INTEGRITY,
        );
        let note = "-- Relationship \"Table1Table2\" from \"Table2\" (\"Field1\") to \"Table1\" \
                    (\"Field1\") does not enforce referential integrity.\n";
        assert_eq!(generate_foreign_keys(&*postgres(), &[&rel]), note);
        assert_eq!(
            generate_foreign_keys(&*mysql(), &[&rel]),
            "-- Relationship `Table1Table2` from `Table2` (`Field1`) to `Table1` (`Field1`) \
             does not enforce referential integrity.\n"
        );
        // Access SQL has no comments.
        assert_eq!(generate_foreign_keys(&*access(), &[&rel]), "");

        // SQLite writes foreign keys in CREATE TABLE; the note follows it.
        let tdef = table(
            "Table2",
            vec![col_with_num("Field1", ColumnType::Long, 4, 0, 0, 0, 1)],
            vec![],
        );
        let enforced = relationship("Enforced", "Table2", "Table3", &[("Field1", "id")], 0);
        let sql = generate_create_table(&*sqlite(), &tdef, &[&rel, &enforced]);
        assert_eq!(
            sql,
            format!(
                "CREATE TABLE \"Table2\" (\n    \"Field1\" INTEGER NOT NULL,\n    \
                 FOREIGN KEY (\"Field1\") REFERENCES \"Table3\" (\"id\")\n);\n{note}"
            )
        );
    }

    #[test]
    fn foreign_key_cascade() {
        use crate::relationship::relationship_flags;
        let d = postgres();
        let rels = [relationship(
            "fk1",
            "Child",
            "Parent",
            &[("pid", "id")],
            relationship_flags::CASCADE_UPDATE | relationship_flags::CASCADE_DELETE,
        )];
        let refs: Vec<&Relationship> = rels.iter().collect();
        let result = generate_foreign_keys(&*d, &refs);
        assert!(result.contains("ON UPDATE CASCADE"), "got:\n{result}");
        assert!(result.contains("ON DELETE CASCADE"), "got:\n{result}");
    }

    #[test]
    fn foreign_key_no_cascade() {
        let d = postgres();
        let rels = [relationship("fk1", "Child", "Parent", &[("pid", "id")], 0)];
        let refs: Vec<&Relationship> = rels.iter().collect();
        let result = generate_foreign_keys(&*d, &refs);
        assert!(!result.contains("ON UPDATE"), "got:\n{result}");
        assert!(!result.contains("ON DELETE"), "got:\n{result}");
    }

    // ========================================================================
    // SQLite inline FK test
    // ========================================================================

    #[test]
    fn create_table_sqlite_inline_fk() {
        let d = sqlite();
        let tdef = table(
            "Child",
            vec![
                col_with_num(
                    "id",
                    ColumnType::Long,
                    0,
                    column_flags::FIXED | column_flags::AUTO_LONG,
                    0,
                    0,
                    1,
                ),
                col_with_num(
                    "parent_id",
                    ColumnType::Long,
                    0,
                    column_flags::NULLABLE,
                    0,
                    0,
                    2,
                ),
            ],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let rel = relationship("fk1", "Child", "Parent", &[("parent_id", "id")], 0);
        let result = generate_create_table(&*d, &tdef, &[&rel]);
        assert!(
            result.contains("FOREIGN KEY (\"parent_id\") REFERENCES \"Parent\" (\"id\")"),
            "got:\n{result}"
        );
    }

    // ========================================================================
    // generate_ddl combined tests
    // ========================================================================

    #[test]
    fn generate_ddl_full_postgres() {
        let d = postgres();
        let tables = vec![
            table(
                "Parent",
                vec![col_with_num("id", ColumnType::Long, 0, 0, 0, 0, 1)],
                vec![index(
                    "PrimaryKey",
                    &[1],
                    index_flags::UNIQUE | index_flags::REQUIRED,
                    index_type::PRIMARY,
                    0,
                )],
            ),
            table(
                "Child",
                vec![
                    col_with_num("id", ColumnType::Long, 0, 0, 0, 0, 1),
                    col_with_num("pid", ColumnType::Long, 0, column_flags::NULLABLE, 0, 0, 2),
                ],
                vec![
                    index(
                        "PrimaryKey",
                        &[1],
                        index_flags::UNIQUE | index_flags::REQUIRED,
                        index_type::PRIMARY,
                        0,
                    ),
                    index("idx_pid", &[2], 0, index_type::ORDINARY, 1),
                ],
            ),
        ];
        let rels = [relationship("fk1", "Child", "Parent", &[("pid", "id")], 0)];
        let result = generate_ddl(&*d, &tables, &rels, true, true);

        // Should contain CREATE TABLE for both
        assert!(result.contains("CREATE TABLE \"Parent\""), "got:\n{result}");
        assert!(result.contains("CREATE TABLE \"Child\""), "got:\n{result}");
        // Should contain CREATE INDEX
        assert!(
            result.contains("CREATE INDEX \"Child_idx_pid_idx\""),
            "got:\n{result}"
        );
        // Should contain ALTER TABLE FK
        assert!(
            result.contains("ALTER TABLE \"Child\" ADD CONSTRAINT \"fk1\""),
            "got:\n{result}"
        );
    }

    #[test]
    fn generate_ddl_no_indexes() {
        let d = postgres();
        let tables = vec![table(
            "T",
            vec![col_with_num("B", ColumnType::Long, 0, 0, 0, 0, 2)],
            vec![index("idx_B", &[2], 0, index_type::ORDINARY, 1)],
        )];
        let result = generate_ddl(&*d, &tables, &[], false, true);
        assert!(!result.contains("CREATE INDEX"), "got:\n{result}");
    }

    #[test]
    fn generate_ddl_no_relations() {
        let d = postgres();
        let tables = vec![table(
            "T",
            vec![col_with_num("id", ColumnType::Long, 0, 0, 0, 0, 1)],
            vec![],
        )];
        let rels = vec![relationship("fk1", "T", "Other", &[("id", "id")], 0)];
        let result = generate_ddl(&*d, &tables, &rels, true, false);
        assert!(!result.contains("ALTER TABLE"), "got:\n{result}");
    }

    // ========================================================================
    // FK filtering by table set
    // ========================================================================

    #[test]
    fn generate_ddl_fk_filtered_by_table_set() {
        let d = postgres();
        // Only Table1 is in the output set
        let tables = vec![table(
            "Table1",
            vec![
                col_with_num("id", ColumnType::Long, 0, 0, 0, 0, 1),
                col_with_num(
                    "fk_col",
                    ColumnType::Long,
                    0,
                    column_flags::NULLABLE,
                    0,
                    0,
                    2,
                ),
            ],
            vec![],
        )];
        // Two relationships: one from Table1, one from Table2 (not in table set)
        let rels = vec![
            relationship("fk_t1", "Table1", "Parent", &[("fk_col", "id")], 0),
            relationship("fk_t2", "Table2", "Parent", &[("fk_col", "id")], 0),
        ];
        let result = generate_ddl(&*d, &tables, &rels, true, true);
        // Should include FK for Table1
        assert!(
            result.contains("ALTER TABLE \"Table1\""),
            "should include FK for Table1, got:\n{result}"
        );
        // Should NOT include FK for Table2 (not in table set)
        assert!(
            !result.contains("ALTER TABLE \"Table2\""),
            "should not include FK for Table2, got:\n{result}"
        );
    }

    // ========================================================================
    // Composite PK / FK tests
    // ========================================================================

    #[test]
    fn create_table_composite_pk() {
        let d = postgres();
        let tdef = table(
            "T",
            vec![
                col_with_num("a", ColumnType::Long, 0, 0, 0, 0, 1),
                col_with_num("b", ColumnType::Long, 0, 0, 0, 0, 2),
            ],
            vec![index(
                "PrimaryKey",
                &[1, 2],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_table(&*d, &tdef, &[]);
        assert!(
            result.contains("PRIMARY KEY (\"a\", \"b\")"),
            "got:\n{result}"
        );
    }

    // -- Access additional type mappings --------------------------------------

    #[test]
    fn access_map_byte() {
        let d = access();
        let c = col("x", ColumnType::Byte, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BYTE");
    }

    #[test]
    fn access_map_int() {
        let d = access();
        let c = col("x", ColumnType::Int, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "SHORT");
    }

    #[test]
    fn access_map_long() {
        let d = access();
        let c = col("x", ColumnType::Long, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "LONG");
    }

    #[test]
    fn access_map_float() {
        let d = access();
        let c = col("x", ColumnType::Float, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "SINGLE");
    }

    #[test]
    fn access_map_double() {
        let d = access();
        let c = col("x", ColumnType::Double, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DOUBLE");
    }

    #[test]
    fn access_map_timestamp() {
        let d = access();
        let c = col("x", ColumnType::Timestamp, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DATETIME");
    }

    #[test]
    fn access_map_binary() {
        let d = access();
        let c = col("x", ColumnType::Binary, 50, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BINARY(50)");
    }

    #[test]
    fn access_map_ole() {
        let d = access();
        let c = col("x", ColumnType::Ole, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "OLEOBJECT");
    }

    #[test]
    fn access_map_guid() {
        let d = access();
        let c = col("x", ColumnType::Guid, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "UNIQUEIDENTIFIER");
    }

    #[test]
    fn access_map_numeric() {
        let d = access();
        let c = col("x", ColumnType::Numeric, 0, 0, 10, 2);
        assert_eq!(d.map_column_type(&c, false), "DECIMAL(10,2)");
    }

    #[test]
    fn access_map_complex_type() {
        let d = access();
        let c = col("x", ColumnType::ComplexType, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "LONG");
    }

    #[test]
    fn access_map_bigint() {
        let d = access();
        let c = col("x", ColumnType::BigInt, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BIGINT");
    }

    #[test]
    fn access_map_unknown() {
        let d = access();
        let c = col("x", ColumnType::Unknown(0xFF), 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BINARY");
    }

    #[test]
    fn access_map_datetime_extended() {
        let d = access();
        let c = col("x", ColumnType::DateTimeExtended, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DATETIME");
    }

    // -- MySQL additional type mappings ---------------------------------------

    #[test]
    fn mysql_map_int() {
        let d = mysql();
        let c = col("x", ColumnType::Int, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "SMALLINT");
    }

    #[test]
    fn mysql_map_long() {
        let d = mysql();
        let c = col("x", ColumnType::Long, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INT");
    }

    #[test]
    fn mysql_map_money() {
        let d = mysql();
        let c = col("x", ColumnType::Money, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DECIMAL(19,4)");
    }

    #[test]
    fn mysql_map_float() {
        let d = mysql();
        let c = col("x", ColumnType::Float, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "FLOAT");
    }

    #[test]
    fn mysql_map_double() {
        let d = mysql();
        let c = col("x", ColumnType::Double, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DOUBLE");
    }

    #[test]
    fn mysql_map_timestamp() {
        let d = mysql();
        let c = col("x", ColumnType::Timestamp, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DATETIME");
    }

    #[test]
    fn mysql_map_binary() {
        let d = mysql();
        let c = col("x", ColumnType::Binary, 50, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "VARBINARY(50)");
    }

    #[test]
    fn mysql_map_memo() {
        let d = mysql();
        let c = col("x", ColumnType::Memo, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "LONGTEXT");
    }

    #[test]
    fn mysql_map_numeric() {
        let d = mysql();
        let c = col("x", ColumnType::Numeric, 0, 0, 10, 2);
        assert_eq!(d.map_column_type(&c, false), "DECIMAL(10,2)");
    }

    #[test]
    fn mysql_map_complex_type() {
        let d = mysql();
        let c = col("x", ColumnType::ComplexType, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INT");
    }

    #[test]
    fn mysql_map_bigint() {
        let d = mysql();
        let c = col("x", ColumnType::BigInt, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BIGINT");
    }

    #[test]
    fn mysql_map_unknown() {
        let d = mysql();
        let c = col("x", ColumnType::Unknown(0xFF), 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "LONGBLOB");
    }

    #[test]
    fn mysql_map_datetime_extended() {
        let d = mysql();
        let c = col("x", ColumnType::DateTimeExtended, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DATETIME(6)");
    }

    // -- PostgreSQL additional type mappings -----------------------------------

    #[test]
    fn postgres_map_byte() {
        let d = postgres();
        let c = col("x", ColumnType::Byte, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "SMALLINT");
    }

    #[test]
    fn postgres_map_int() {
        let d = postgres();
        let c = col("x", ColumnType::Int, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "SMALLINT");
    }

    #[test]
    fn postgres_map_long() {
        let d = postgres();
        let c = col("x", ColumnType::Long, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn postgres_map_float() {
        let d = postgres();
        let c = col("x", ColumnType::Float, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "REAL");
    }

    #[test]
    fn postgres_map_double() {
        let d = postgres();
        let c = col("x", ColumnType::Double, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "DOUBLE PRECISION");
    }

    #[test]
    fn postgres_map_binary() {
        let d = postgres();
        let c = col("x", ColumnType::Binary, 50, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BYTEA");
    }

    #[test]
    fn postgres_map_memo() {
        let d = postgres();
        let c = col("x", ColumnType::Memo, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TEXT");
    }

    #[test]
    fn postgres_map_numeric() {
        let d = postgres();
        let c = col("x", ColumnType::Numeric, 0, 0, 10, 2);
        assert_eq!(d.map_column_type(&c, false), "NUMERIC(10,2)");
    }

    #[test]
    fn postgres_map_complex_type() {
        let d = postgres();
        let c = col("x", ColumnType::ComplexType, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn postgres_map_bigint() {
        let d = postgres();
        let c = col("x", ColumnType::BigInt, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BIGINT");
    }

    #[test]
    fn postgres_map_unknown() {
        let d = postgres();
        let c = col("x", ColumnType::Unknown(0xFF), 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BYTEA");
    }

    #[test]
    fn postgres_map_datetime_extended() {
        let d = postgres();
        let c = col("x", ColumnType::DateTimeExtended, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TIMESTAMP WITHOUT TIME ZONE");
    }

    // -- SQLite additional type mappings --------------------------------------

    #[test]
    fn sqlite_map_boolean() {
        let d = sqlite();
        let c = col("x", ColumnType::Boolean, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn sqlite_map_byte() {
        let d = sqlite();
        let c = col("x", ColumnType::Byte, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn sqlite_map_int() {
        let d = sqlite();
        let c = col("x", ColumnType::Int, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn sqlite_map_float() {
        let d = sqlite();
        let c = col("x", ColumnType::Float, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "REAL");
    }

    #[test]
    fn sqlite_map_double() {
        let d = sqlite();
        let c = col("x", ColumnType::Double, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "REAL");
    }

    #[test]
    fn sqlite_map_memo() {
        let d = sqlite();
        let c = col("x", ColumnType::Memo, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TEXT");
    }

    #[test]
    fn sqlite_map_ole() {
        let d = sqlite();
        let c = col("x", ColumnType::Ole, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BLOB");
    }

    #[test]
    fn sqlite_map_numeric() {
        let d = sqlite();
        let c = col("x", ColumnType::Numeric, 0, 0, 10, 2);
        assert_eq!(d.map_column_type(&c, false), "NUMERIC");
    }

    #[test]
    fn sqlite_map_complex_type() {
        let d = sqlite();
        let c = col("x", ColumnType::ComplexType, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn sqlite_map_bigint() {
        let d = sqlite();
        let c = col("x", ColumnType::BigInt, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "INTEGER");
    }

    #[test]
    fn sqlite_map_unknown() {
        let d = sqlite();
        let c = col("x", ColumnType::Unknown(0xFF), 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "BLOB");
    }

    #[test]
    fn sqlite_map_datetime_extended() {
        let d = sqlite();
        let c = col("x", ColumnType::DateTimeExtended, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "TEXT");
    }

    // ========================================================================
    // Composite PK / FK tests (continued)
    // ========================================================================

    #[test]
    fn japanese_identifiers_sqlite() {
        let d = sqlite();
        let tdef = table(
            "jp_テーブル2",
            vec![
                col_with_num(
                    "ID",
                    ColumnType::Long,
                    0,
                    column_flags::FIXED | column_flags::AUTO_LONG,
                    0,
                    0,
                    1,
                ),
                col_with_num(
                    "商品名",
                    ColumnType::Text,
                    255,
                    column_flags::NULLABLE,
                    0,
                    0,
                    2,
                ),
                col_with_num("単価", ColumnType::Long, 0, column_flags::NULLABLE, 0, 0, 3),
                col_with_num("個数", ColumnType::Long, 0, column_flags::NULLABLE, 0, 0, 4),
            ],
            vec![index(
                "PrimaryKey",
                &[1],
                index_flags::UNIQUE | index_flags::REQUIRED,
                index_type::PRIMARY,
                0,
            )],
        );
        let result = generate_create_table(&*d, &tdef, &[]);
        assert!(
            result.contains("\"jp_テーブル2\""),
            "should quote Japanese table name, got:\n{result}"
        );
        assert!(
            result.contains("\"商品名\""),
            "should quote Japanese column name 商品名, got:\n{result}"
        );
        assert!(
            result.contains("\"単価\""),
            "should quote Japanese column name 単価, got:\n{result}"
        );
        assert!(
            result.contains("\"個数\""),
            "should quote Japanese column name 個数, got:\n{result}"
        );
    }

    #[test]
    fn foreign_key_multi_column() {
        let d = postgres();
        let rels = [relationship(
            "fk_composite",
            "Child",
            "Parent",
            &[("a", "x"), ("b", "y")],
            0,
        )];
        let refs: Vec<&Relationship> = rels.iter().collect();
        let result = generate_foreign_keys(&*d, &refs);
        assert!(
            result.contains("FOREIGN KEY (\"a\", \"b\") REFERENCES \"Parent\" (\"x\", \"y\")"),
            "got:\n{result}"
        );
    }

    // -- find_primary_key: real samples ----------------------------------------
    //
    // primaryKeyTestV2007.accdb, covering a
    // default-named PK, a renamed PK, a renamed multi-column PK, and a PK
    // coexisting with other indexes carrying every UNIQUE/IGNORE_NULLS
    // combination. These pin `find_primary_key` to `index_type::PRIMARY`
    // (Access's own `Index.Primary` property) rather than to the index name
    // or its UNIQUE/REQUIRED flags.

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

    fn create_table_ddl(path: &std::path::Path, table_name: &str) -> String {
        let mut reader = crate::file::PageReader::open(path).unwrap();
        let entries = crate::catalog::read_catalog(&mut reader).unwrap();
        let entry = entries
            .iter()
            .find(|e| e.name == table_name && e.object_type == crate::format::ObjectType::Table)
            .unwrap_or_else(|| panic!("table '{table_name}' not found"));
        let tdef =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        generate_create_table(&Access, &tdef, &[])
    }

    #[test]
    fn complex_columns_are_plain_integers_without_hidden_indexes() {
        // Table1's version history, multi-value, and attachment columns hold
        // the ID of their values in a hidden table. Access numbers those IDs
        // and gives each such column a hidden unique index; neither makes the
        // column an auto-increment column or an index to recreate.
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        let mut reader = crate::file::PageReader::open(&path).unwrap();
        let entry = crate::catalog::read_catalog(&mut reader)
            .unwrap()
            .into_iter()
            .find(|e| e.name == "Table1")
            .unwrap();
        let tdef =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let dialects: [(&dyn DdlDialect, &str); 4] = [
            (&Access, "COUNTER"),
            (&Postgres, "IDENTITY"),
            (&Mysql, "AUTO_INCREMENT"),
            (&Sqlite, "AUTOINCREMENT"),
        ];
        for (dialect, auto_keyword) in dialects {
            let ddl = generate_ddl(dialect, std::slice::from_ref(&tdef), &[], true, false);
            assert!(!ddl.contains(auto_keyword), "got:\n{ddl}");
            assert!(!ddl.contains("CREATE UNIQUE INDEX"), "got:\n{ddl}");
            assert_eq!(ddl.matches("PRIMARY KEY").count(), 1, "got:\n{ddl}");
            for column in ["multi-value-data", "attach-data"] {
                assert!(ddl.contains(column), "{column} should stay, got:\n{ddl}");
            }
        }
    }

    #[test]
    fn access_bigint_column_from_a_database() {
        // Access creates a Large Number column from BIGINT in CREATE TABLE.
        let path = skip_if_missing!("V2016/bigIntTestV2016.accdb");
        let ddl = create_table_ddl(&path, "BigIntTable");
        assert!(ddl.contains("[Big] BIGINT"), "got:\n{ddl}");
    }

    #[test]
    fn numeric_without_a_fixed_precision_in_each_dialect() {
        let c = col("x", ColumnType::Numeric, 17, 0, 0, 0);
        assert_eq!(access().map_column_type(&c, false), "DECIMAL(28,10)");
        assert_eq!(postgres().map_column_type(&c, false), "NUMERIC");
        assert_eq!(mysql().map_column_type(&c, false), "DECIMAL(65,30)");
        assert_eq!(sqlite().map_column_type(&c, false), "NUMERIC");
    }

    #[test]
    fn dialect_of_each_name() {
        for name in DIALECT_NAMES {
            let dialect = dialect(name).unwrap();
            let quoted = dialect.quote_id("a");
            let expected = match name {
                "access" => "[a]",
                "mysql" => "`a`",
                _ => "\"a\"",
            };
            assert_eq!(quoted, expected, "{name}");
        }
        assert!(dialect("sqlite").unwrap().inline_foreign_keys());
        assert!(dialect("oracle").is_none());
        assert!(dialect("SQLite").is_none());
    }

    #[test]
    fn schema_tables_user_tables_or_one_table() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = crate::file::PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();

        let all = schema_tables(&mut reader, &catalog, None).unwrap();
        let user: Vec<&str> = catalog
            .iter()
            .filter(|e| e.object_type == ObjectType::Table && !e.is_system_or_hidden())
            .map(|e| e.name.as_str())
            .collect();
        let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, user);
        assert!(names.contains(&"Table1"), "{names:?}");

        let one = schema_tables(&mut reader, &catalog, Some("Table1")).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, "Table1");
        let system = schema_tables(&mut reader, &catalog, Some("MSysObjects")).unwrap();
        assert_eq!(system[0].name, "MSysObjects");
        assert!(matches!(
            schema_tables(&mut reader, &catalog, Some("NoSuchTable")),
            Err(FileError::TableNotFound { .. })
        ));
    }

    #[test]
    fn schema_tables_have_calculated_column_types() {
        let path = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut reader = crate::file::PageReader::open(&path).unwrap();
        let catalog = crate::catalog::read_catalog(&mut reader).unwrap();
        let tables = schema_tables(&mut reader, &catalog, Some("Table1")).unwrap();
        let column = |name: &str| {
            tables[0]
                .columns
                .iter()
                .find(|c| c.name == name)
                .unwrap()
                .clone()
        };
        assert_eq!(column("MonthlySalary").col_type, ColumnType::Money);
        assert_eq!(column("IsRich").col_type, ColumnType::Boolean);
        let decimal = column("DecimalTest");
        assert_eq!(decimal.col_type, ColumnType::Numeric);
        assert_eq!((decimal.precision, decimal.scale), (0, 0));
    }

    #[test]
    fn calculated_columns_have_the_type_of_their_result() {
        // Table1 of calcFieldTestV2010.accdb declares MonthlySalary (Currency
        // result) and FloatTest (Single result) as Double, IsRich (Yes/No
        // result) as Int, and DecimalTest (Decimal result) as Numeric(0,0).
        let path = skip_if_missing!("V2010/calcFieldTestV2010.accdb");
        let mut reader = crate::file::PageReader::open(&path).unwrap();
        let entry = crate::catalog::read_catalog(&mut reader)
            .unwrap()
            .into_iter()
            .find(|e| e.name == "Table1")
            .unwrap();
        let tdef =
            crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        let calculated = crate::calculated_column_types(&mut reader, &tdef);
        let tdef = with_calculated_column_types(&tdef, &calculated);

        let ddl = generate_create_table(&Access, &tdef, &[]);
        for expected in [
            "[MonthlySalary] CURRENCY",
            "[FloatTest] SINGLE",
            "[IsRich] YESNO",
            "[DecimalTest] DECIMAL(28,10)",
            "[Popularity] DECIMAL(18,6)",
        ] {
            assert!(ddl.contains(expected), "{expected} in:\n{ddl}");
        }
        let ddl = generate_create_table(&Postgres, &tdef, &[]);
        assert!(ddl.contains("\"DecimalTest\" NUMERIC,"), "got:\n{ddl}");
    }

    #[test]
    fn find_primary_key_default_name() {
        let path = skip_if_missing!("V2007/primaryKeyTestV2007.accdb");
        let ddl = create_table_ddl(&path, "t1defaultPK");
        assert!(ddl.contains("PRIMARY KEY ([ID])"), "got:\n{ddl}");
    }

    #[test]
    fn find_primary_key_renamed() {
        // The index itself is named "MyKey", not "PrimaryKey" -- this is
        // exactly the case a name-based check gets wrong.
        let path = skip_if_missing!("V2007/primaryKeyTestV2007.accdb");
        let ddl = create_table_ddl(&path, "t2renamedPK");
        assert!(ddl.contains("PRIMARY KEY ([ID])"), "got:\n{ddl}");
    }

    #[test]
    fn find_primary_key_ignores_other_unique_required_indexes() {
        // 4 other indexes here span every UNIQUE/IGNORE_NULLS combination
        // (including plain UNIQUE with no REQUIRED) -- none of them should
        // be picked over the real, "PrimaryKey"-named PK.
        let path = skip_if_missing!("V2007/primaryKeyTestV2007.accdb");
        let ddl = create_table_ddl(&path, "t3defaultPKandOtherIndexes");
        assert!(ddl.contains("PRIMARY KEY ([ID])"), "got:\n{ddl}");
    }

    #[test]
    fn find_primary_key_renamed_multi_column() {
        let path = skip_if_missing!("V2007/primaryKeyTestV2007.accdb");
        let ddl = create_table_ddl(&path, "t4renamedPKmulticol");
        assert!(ddl.contains("PRIMARY KEY ([ID1], [ID2])"), "got:\n{ddl}");
    }

    #[test]
    fn columns_in_design_order() {
        // Table1's fields were created as ID, A, C, and then B was inserted
        // between A and C in Design View.
        for file in [
            "V2007/columnOrderTestV2007.accdb",
            "V1997/columnOrderTestV1997.mdb",
        ] {
            let path = skip_if_missing!(file);
            let ddl = create_table_ddl(&path, "Table1");
            let positions: Vec<usize> = ["[ID]", "[A]", "[B]", "[C]"]
                .iter()
                .map(|name| ddl.find(name).unwrap_or_else(|| panic!("{file}: {name}")))
                .collect();
            assert!(positions.is_sorted(), "{file}: got:\n{ddl}");
        }
    }

    #[test]
    fn find_primary_key_ignores_complex_column_indexes() {
        // Table1's attachment and multi-value columns each have a hidden
        // UNIQUE + REQUIRED index listed before the primary key.
        let path = skip_if_missing!("V2007/complexDataTestV2007.accdb");
        let ddl = create_table_ddl(&path, "Table1");
        assert!(ddl.contains("PRIMARY KEY ([id])"), "got:\n{ddl}");
    }
}
