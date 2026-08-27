//! DDL (CREATE TABLE / INDEX / FOREIGN KEY) generation for multiple SQL dialects.

pub mod access;
pub mod mysql;
pub mod postgres;
pub mod sqlite;

pub use access::Access;
pub use mysql::Mysql;
pub use postgres::Postgres;
pub use sqlite::Sqlite;

use crate::format::{column_flags, index_flags, index_type};
use crate::{ColumnDef, IndexDef, Relationship, TableDef};

// ---------------------------------------------------------------------------
// DdlDialect trait
// ---------------------------------------------------------------------------

pub trait DdlDialect {
    /// Quote an identifier
    fn quote_id(&self, name: &str) -> String;

    /// Map a column definition to a SQL type string.
    /// When `is_auto` is true, include auto-increment syntax.
    fn map_column_type(&self, col: &ColumnDef, is_auto: bool) -> String;

    /// Whether auto-increment columns absorb the PRIMARY KEY constraint
    /// (true only for SQLite)
    fn auto_increment_absorbs_pk(&self) -> bool;

    /// Whether foreign keys should be inlined in CREATE TABLE
    /// (true for SQLite, false for others)
    fn inline_foreign_keys(&self) -> bool;
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
    tdef.indexes.iter().find(|idx| idx.index_type == index_type::PRIMARY)
}

/// Check if a column is auto-increment.
fn is_auto_increment(col: &ColumnDef) -> bool {
    (col.flags & column_flags::AUTO_LONG) != 0 || (col.flags & column_flags::AUTO_UUID) != 0
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

/// Generate CREATE TABLE statement for a single table.
pub fn generate_create_table(
    dialect: &dyn DdlDialect,
    tdef: &TableDef,
    table_rels: &[&Relationship],
) -> String {
    let pk = find_primary_key(tdef);
    let pk_col_nums: Vec<u16> = pk
        .map(|idx| idx.columns.iter().map(|c| c.col_num).collect())
        .unwrap_or_default();

    // Check if there's an auto-increment column in the PK
    let auto_pk_col = pk.and_then(|_| {
        tdef.columns
            .iter()
            .find(|c| pk_col_nums.contains(&c.col_num) && is_auto_increment(c))
    });

    // If dialect absorbs PK and there's an auto-increment PK col, suppress table-level PK
    let suppress_pk = dialect.auto_increment_absorbs_pk() && auto_pk_col.is_some();

    let mut lines: Vec<String> = Vec::new();

    // Column definitions
    for col in &tdef.columns {
        let is_auto = is_auto_increment(col);
        let type_str = dialect.map_column_type(col, is_auto);
        let not_null = if (col.flags & column_flags::NULLABLE) == 0 {
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

    // Inline foreign keys (SQLite)
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

    format!(
        "CREATE TABLE {} (\n{}\n);\n",
        dialect.quote_id(&tdef.name),
        lines.join(",\n")
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
        out.push_str(&format!(
            "CREATE {unique}INDEX {} ON {} ({});\n",
            dialect.quote_id(&idx.name),
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
        let d = mysql();
        let c = col("x", ColumnType::Guid, 0, 0, 0, 0);
        assert_eq!(d.map_column_type(&c, false), "CHAR(36)");
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
        assert_eq!(
            result,
            "CREATE TABLE \"T\" (\n    \"A\" VARCHAR(100) NOT NULL,\n    \"B\" INTEGER\n);\n"
        );
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
            vec![index("idx_B", &[2], 0, index_type::NORMAL, 1)],
        );
        let result = generate_create_indexes(&*d, &tdef);
        assert_eq!(result, "CREATE INDEX \"idx_B\" ON \"T\" (\"B\");\n");
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
                index_type::NORMAL,
                1,
            )],
        );
        let result = generate_create_indexes(&*d, &tdef);
        assert_eq!(result, "CREATE UNIQUE INDEX \"idx_B\" ON \"T\" (\"B\");\n");
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
                    index("idx_pid", &[2], 0, index_type::NORMAL, 1),
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
            result.contains("CREATE INDEX \"idx_pid\""),
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
            vec![index("idx_B", &[2], 0, index_type::NORMAL, 1)],
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
        assert_eq!(d.map_column_type(&c, false), "LONG");
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
    // combination. Regression coverage for `find_primary_key` having
    // relied on the index literally being named "PrimaryKey" (renamable by
    // the user, so unreliable) instead of `index_type::PRIMARY` (Access's
    // own `Index.Primary` property).

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::PathBuf::from(manifest_dir).join("../../testdata").join(relative);
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
        let tdef = crate::table::read_table_def(&mut reader, &entry.name, entry.table_page).unwrap();
        generate_create_table(&Access, &tdef, &[])
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
}
