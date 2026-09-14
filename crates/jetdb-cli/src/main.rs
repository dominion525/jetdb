mod ddl;
mod export;
mod form;
mod prop;
mod query;
mod vba;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use jetdb::format::{
    catalog_flags, column_flags, index_flags, index_type, ColumnType, JetVersion, ObjectType,
};
use jetdb::{
    read_catalog, read_relationships, read_table_def, relationship_flags, CatalogEntry, ColumnDef,
    IndexColumnOrder, IndexDef, PageReader, Relationship, TableDef,
};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// Read-only tool for Microsoft Access (.mdb / .accdb) databases
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Database password (for password-protected .accdb files)
    #[arg(long = "password", global = true)]
    password: Option<String>,

    /// Print Claude Code Skill installation instructions and exit
    #[arg(long = "help-skill")]
    help_skill: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show database engine version
    Ver(VerArgs),
    /// List table names
    Tables(TablesArgs),
    /// Show table schema (columns, indexes, relationships)
    Schema(SchemaArgs),
    /// Export table data as CSV
    Export(export::ExportArgs),
    /// Manage saved queries (list / show)
    Queries(query::QueryArgs),
    /// Show object properties (LvProp)
    Prop(prop::PropArgs),
    /// Manage VBA modules (list / show)
    Vba(vba::VbaArgs),
    /// Manage forms and reports (list / dump / controls)
    Form(form::FormArgs),
}

#[derive(Args)]
struct VerArgs {
    /// Database file path (.mdb / .accdb)
    file: PathBuf,

    /// Show detailed version info
    #[arg(short, long)]
    long: bool,
}

#[derive(Args)]
struct TablesArgs {
    /// Database file path (.mdb / .accdb)
    file: PathBuf,

    /// Include system tables
    #[arg(short = 's', long = "system")]
    system: bool,

    /// Show object type number
    #[arg(short = 't', long = "show-type", conflicts_with = "show_type_name")]
    show_type: bool,

    /// Show object type name
    #[arg(short = 'T', long = "show-type-name", conflicts_with = "show_type")]
    show_type_name: bool,
}

#[derive(Args)]
struct SchemaArgs {
    /// Database file path (.mdb / .accdb)
    file: PathBuf,

    /// Show only the specified table
    #[arg(short = 'T', long = "table")]
    table_name: Option<String>,

    /// Generate DDL in the specified SQL dialect
    #[arg(long = "ddl", value_enum)]
    ddl_format: Option<ddl::DdlFormat>,

    /// Hide index definitions
    #[arg(long = "no-indexes")]
    no_indexes: bool,

    /// Hide relationship definitions
    #[arg(long = "no-relations")]
    no_relations: bool,
}

// ---------------------------------------------------------------------------
// ver subcommand
// ---------------------------------------------------------------------------

fn cmd_ver(args: VerArgs, password: Option<&str>) -> ExitCode {
    match run_ver(&args, password) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_ver(args: &VerArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let reader = PageReader::open_with_password(&args.file, password)?;
    let version = reader.header().version;

    if args.long {
        println!("{version}");
    } else {
        println!("{}", version_short_name(version));
    }
    Ok(())
}

fn version_short_name(v: JetVersion) -> &'static str {
    match v {
        JetVersion::Jet3 => "JET3",
        JetVersion::Jet4 => "JET4",
        JetVersion::Ace12 => "ACE12",
        JetVersion::Ace14 => "ACE14",
        JetVersion::Ace15 => "ACE15",
        JetVersion::Ace16 => "ACE16",
        JetVersion::Ace17 => "ACE17",
    }
}

// ---------------------------------------------------------------------------
// tables subcommand
// ---------------------------------------------------------------------------

fn cmd_tables(args: TablesArgs, password: Option<&str>) -> ExitCode {
    match run_tables(&args, password) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_tables(args: &TablesArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let catalog = read_catalog(&mut reader)?;

    let mut entries: Vec<&CatalogEntry> = catalog
        .iter()
        .filter(|e| should_show(e, args.system))
        .collect();
    entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));

    for entry in &entries {
        if args.show_type {
            println!("{}\t{}", entry.object_type as i32, entry.name);
        } else if args.show_type_name {
            println!("{}\t{}", resolve_type_name(entry), entry.name);
        } else {
            println!("{}", entry.name);
        }
    }
    Ok(())
}

fn should_show(entry: &CatalogEntry, include_system: bool) -> bool {
    if entry.object_type != ObjectType::Table {
        return false;
    }
    if include_system {
        return true;
    }
    (entry.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN)) == 0
}

fn resolve_type_name(entry: &CatalogEntry) -> &'static str {
    if entry.object_type == ObjectType::Table
        && (entry.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN)) != 0
    {
        "systable"
    } else {
        object_type_name(entry.object_type)
    }
}

fn object_type_name(t: ObjectType) -> &'static str {
    match t {
        ObjectType::Form => "form",
        ObjectType::Table => "table",
        ObjectType::Macro => "macro",
        ObjectType::SystemTable => "systemtable",
        ObjectType::Report => "report",
        ObjectType::Query => "query",
        ObjectType::LinkedTable => "linkedtable",
        ObjectType::Module => "module",
        ObjectType::Relationship => "relationship",
        ObjectType::DatabaseProperty => "dbproperty",
    }
}

// ---------------------------------------------------------------------------
// schema subcommand
// ---------------------------------------------------------------------------

fn cmd_schema(args: SchemaArgs, password: Option<&str>) -> ExitCode {
    match run_schema(&args, password) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_schema(args: &SchemaArgs, password: Option<&str>) -> Result<(), jetdb::FileError> {
    let mut reader = PageReader::open_with_password(&args.file, password)?;
    let catalog = read_catalog(&mut reader)?;

    // Collect target tables
    let targets: Vec<&CatalogEntry> = if let Some(ref name) = args.table_name {
        let entry = catalog
            .iter()
            .find(|e| e.object_type == ObjectType::Table && e.name == *name)
            .ok_or(jetdb::FileError::TableNotFound { name: name.clone() })?;
        vec![entry]
    } else {
        catalog
            .iter()
            .filter(|e| {
                e.object_type == ObjectType::Table
                    && (e.flags & (catalog_flags::SYSTEM | catalog_flags::HIDDEN)) == 0
            })
            .collect()
    };

    // Read relationships once if needed
    let need_relations = !args.no_relations;
    let relationships = if need_relations {
        match read_relationships(&mut reader) {
            Ok(rels) => rels,
            Err(e) => {
                log::warn!("failed to read relationships: {e}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Read all target table definitions
    let mut tables: Vec<TableDef> = Vec::new();
    for entry in &targets {
        let tdef = read_table_def(&mut reader, &entry.name, entry.table_page)?;
        tables.push(tdef);
    }

    // DDL mode or human-readable mode
    if let Some(format) = args.ddl_format {
        let dialect = ddl::create_dialect(format);
        let output = jetdb::ddl::generate_ddl(
            &*dialect,
            &tables,
            &relationships,
            !args.no_indexes,
            need_relations,
        );
        print!("{output}");
    } else {
        let mut first = true;
        for tdef in &tables {
            if !first {
                println!();
            }
            first = false;
            print_table_schema(tdef, &relationships, args);
        }
    }

    Ok(())
}

fn format_col_type(col: &ColumnDef) -> String {
    match col.col_type {
        ColumnType::Text => format!("Text({})", col.col_size),
        ColumnType::Binary => format!("Binary({})", col.col_size),
        ColumnType::Numeric => format!("Numeric({},{})", col.precision, col.scale),
        ColumnType::Memo => "Memo".to_string(),
        ColumnType::Ole => "Ole".to_string(),
        other => format!("{other}"),
    }
}

fn format_col_attrs(col: &ColumnDef) -> String {
    let mut attrs = Vec::new();
    if (col.flags & column_flags::NULLABLE) == 0 {
        attrs.push("NOT NULL");
    }
    if (col.flags & column_flags::AUTO_LONG) != 0 || (col.flags & column_flags::AUTO_UUID) != 0 {
        attrs.push("AUTO");
    }
    attrs.join("  ")
}

fn print_table_schema(tdef: &TableDef, rels: &[Relationship], args: &SchemaArgs) {
    println!("Table: {}", tdef.name);
    println!();

    // -- Columns --
    println!("  Columns:");

    // Pre-compute type strings to avoid double formatting
    let col_types: Vec<String> = tdef.columns.iter().map(format_col_type).collect();
    let name_width = tdef.columns.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let type_width = col_types.iter().map(|s| s.len()).max().unwrap_or(0);

    for (col, col_type) in tdef.columns.iter().zip(&col_types) {
        let attrs = format_col_attrs(col);
        if attrs.is_empty() {
            println!("    {:<nw$}  {col_type}", col.name, nw = name_width);
        } else {
            println!(
                "    {:<nw$}  {:<tw$}  {attrs}",
                col.name,
                col_type,
                nw = name_width,
                tw = type_width,
            );
        }
    }

    // -- Indexes --
    if !args.no_indexes {
        let visible_indexes: Vec<&IndexDef> = tdef
            .indexes
            .iter()
            .filter(|idx| idx.index_type != index_type::FOREIGN_KEY)
            .collect();

        if !visible_indexes.is_empty() {
            println!();
            println!("  Indexes:");

            let idx_name_width = visible_indexes
                .iter()
                .map(|i| i.name.len())
                .max()
                .unwrap_or(0);
            let cols_strs: Vec<String> = visible_indexes
                .iter()
                .map(|idx| format_index_columns(idx, tdef))
                .collect();
            let cols_width = cols_strs.iter().map(|s| s.len()).max().unwrap_or(0);

            for (idx, cols_str) in visible_indexes.iter().zip(&cols_strs) {
                let flags_str = format_index_flags(idx);
                if flags_str.is_empty() {
                    println!("    {:<w$}  {cols_str}", idx.name, w = idx_name_width);
                } else {
                    println!(
                        "    {:<w$}  {:<cw$}  {flags_str}",
                        idx.name,
                        cols_str,
                        w = idx_name_width,
                        cw = cols_width,
                    );
                }
            }
        }
    }

    // -- Relationships --
    if !args.no_relations {
        let table_rels: Vec<&Relationship> = rels
            .iter()
            .filter(|r| r.from_table == tdef.name || r.to_table == tdef.name)
            .collect();

        if !table_rels.is_empty() {
            println!();
            println!("  Relationships:");

            let rel_name_width = table_rels.iter().map(|r| r.name.len()).max().unwrap_or(0);

            for rel in &table_rels {
                let mapping = format_relationship_mapping(rel);
                let flags_str = format_relationship_flags(rel);
                if flags_str.is_empty() {
                    println!("    {:<w$}  {mapping}", rel.name, w = rel_name_width);
                } else {
                    println!(
                        "    {:<w$}  {mapping}  {flags_str}",
                        rel.name,
                        w = rel_name_width,
                    );
                }
            }
        }
    }
}

fn format_index_columns(idx: &IndexDef, tdef: &TableDef) -> String {
    let parts: Vec<String> = idx
        .columns
        .iter()
        .map(|ic| {
            let col_name = tdef
                .columns
                .iter()
                .find(|c| c.col_num == ic.col_num)
                .map(|c| c.name.as_str())
                .unwrap_or("?");
            let order = match ic.order {
                IndexColumnOrder::Ascending => "ASC",
                IndexColumnOrder::Descending => "DESC",
            };
            format!("{col_name} {order}")
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

fn format_index_flags(idx: &IndexDef) -> String {
    let mut flags = Vec::new();
    if (idx.flags & index_flags::UNIQUE) != 0 {
        flags.push("UNIQUE");
    }
    if (idx.flags & index_flags::IGNORE_NULLS) != 0 {
        flags.push("IGNORE_NULLS");
    }
    if (idx.flags & index_flags::REQUIRED) != 0 {
        flags.push("REQUIRED");
    }
    flags.join(" ")
}

fn format_relationship_mapping(rel: &Relationship) -> String {
    let from_cols: Vec<&str> = rel.columns.iter().map(|c| c.from_column.as_str()).collect();
    let to_cols: Vec<&str> = rel.columns.iter().map(|c| c.to_column.as_str()).collect();

    if from_cols.len() == 1 {
        format!(
            "{}.{} -> {}.{}",
            rel.from_table, from_cols[0], rel.to_table, to_cols[0]
        )
    } else {
        let from = from_cols
            .iter()
            .map(|c| format!("{}.{c}", rel.from_table))
            .collect::<Vec<_>>()
            .join(",");
        let to = to_cols
            .iter()
            .map(|c| format!("{}.{c}", rel.to_table))
            .collect::<Vec<_>>()
            .join(",");
        format!("{from} -> {to}")
    }
}

fn format_relationship_flags(rel: &Relationship) -> String {
    let mut flags = Vec::new();
    if (rel.flags & relationship_flags::CASCADE_UPDATE) != 0 {
        flags.push("CASCADE UPDATE");
    }
    if (rel.flags & relationship_flags::CASCADE_DELETE) != 0 {
        flags.push("CASCADE DELETE");
    }
    flags.join("  ")
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Warn)
        .parse_default_env()
        .format(|buf, record| {
            use std::io::Write;
            match record.level() {
                log::Level::Error => writeln!(buf, "jetdb: {}", record.args()),
                log::Level::Warn => writeln!(buf, "jetdb: warning: {}", record.args()),
                _ => writeln!(buf, "jetdb: {}: {}", record.level(), record.args()),
            }
        })
        .init();

    let cli = Cli::parse();

    if cli.help_skill {
        println!(
            "The jetdb Claude Code Skill has moved to the agent-skills repository.

Install via Claude Code Plugin Marketplace:
  /plugin marketplace add dominion525/agent-skills
  /plugin install jetdb-cli@dominion525-skills

Install via Vercel skills CLI:
  npx skills add dominion525/agent-skills --skill jetdb-cli -a claude-code

Details: https://github.com/dominion525/agent-skills"
        );
        return ExitCode::SUCCESS;
    }

    let Some(command) = cli.command else {
        // No subcommand and no --help-skill: show help
        use clap::CommandFactory;
        Cli::command().print_help().ok();
        eprintln!();
        return ExitCode::FAILURE;
    };

    let password = cli.password;
    match command {
        Commands::Ver(args) => cmd_ver(args, password.as_deref()),
        Commands::Tables(args) => cmd_tables(args, password.as_deref()),
        Commands::Schema(args) => cmd_schema(args, password.as_deref()),
        Commands::Export(args) => export::cmd_export(args, password.as_deref()),
        Commands::Queries(args) => query::cmd_queries(args, password.as_deref()),
        Commands::Prop(args) => prop::cmd_prop(args, password.as_deref()),
        Commands::Vba(args) => vba::cmd_vba(args, password.as_deref()),
        Commands::Form(args) => form::cmd_form(args, password.as_deref()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_all_versions() {
        assert_eq!(version_short_name(JetVersion::Jet3), "JET3");
        assert_eq!(version_short_name(JetVersion::Jet4), "JET4");
        assert_eq!(version_short_name(JetVersion::Ace12), "ACE12");
        assert_eq!(version_short_name(JetVersion::Ace14), "ACE14");
        assert_eq!(version_short_name(JetVersion::Ace15), "ACE15");
        assert_eq!(version_short_name(JetVersion::Ace16), "ACE16");
        assert_eq!(version_short_name(JetVersion::Ace17), "ACE17");
    }

    // -- tables helpers -------------------------------------------------------

    fn entry(name: &str, object_type: ObjectType, flags: u32) -> CatalogEntry {
        CatalogEntry {
            name: name.to_string(),
            object_type,
            table_page: 100,
            flags,
        }
    }

    // should_show tests

    #[test]
    fn should_show_user_table() {
        let e = entry("Users", ObjectType::Table, 0);
        assert!(should_show(&e, false));
    }

    #[test]
    fn should_show_excludes_system_by_default() {
        let e = entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM);
        assert!(!should_show(&e, false));
    }

    #[test]
    fn should_show_includes_system_with_flag() {
        let e = entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM);
        assert!(should_show(&e, true));
    }

    #[test]
    fn should_show_excludes_hidden_by_default() {
        let e = entry("Hidden", ObjectType::Table, catalog_flags::HIDDEN);
        assert!(!should_show(&e, false));
    }

    #[test]
    fn should_show_includes_hidden_with_flag() {
        let e = entry("Hidden", ObjectType::Table, catalog_flags::HIDDEN);
        assert!(should_show(&e, true));
    }

    #[test]
    fn should_show_excludes_non_table() {
        let e = entry("MyQuery", ObjectType::Query, 0);
        assert!(!should_show(&e, false));
        assert!(!should_show(&e, true));
    }

    // resolve_type_name tests

    #[test]
    fn resolve_type_name_normal_table() {
        let e = entry("Users", ObjectType::Table, 0);
        assert_eq!(resolve_type_name(&e), "table");
    }

    #[test]
    fn resolve_type_name_system_table() {
        let e = entry("MSysObjects", ObjectType::Table, catalog_flags::SYSTEM);
        assert_eq!(resolve_type_name(&e), "systable");
    }

    #[test]
    fn resolve_type_name_hidden_table() {
        let e = entry("Hidden", ObjectType::Table, catalog_flags::HIDDEN);
        assert_eq!(resolve_type_name(&e), "systable");
    }

    // object_type_name tests

    #[test]
    fn object_type_name_all_variants() {
        assert_eq!(object_type_name(ObjectType::Form), "form");
        assert_eq!(object_type_name(ObjectType::Table), "table");
        assert_eq!(object_type_name(ObjectType::Macro), "macro");
        assert_eq!(object_type_name(ObjectType::SystemTable), "systemtable");
        assert_eq!(object_type_name(ObjectType::Report), "report");
        assert_eq!(object_type_name(ObjectType::Query), "query");
        assert_eq!(object_type_name(ObjectType::LinkedTable), "linkedtable");
        assert_eq!(object_type_name(ObjectType::Module), "module");
        assert_eq!(object_type_name(ObjectType::Relationship), "relationship");
        assert_eq!(object_type_name(ObjectType::DatabaseProperty), "dbproperty");
    }

    // -- schema helpers -------------------------------------------------------

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
        }
    }

    // -- format_col_type tests ------------------------------------------------

    #[test]
    fn format_col_type_text() {
        let c = col("x", ColumnType::Text, 100, 0, 0, 0);
        assert_eq!(format_col_type(&c), "Text(100)");
    }

    #[test]
    fn format_col_type_binary() {
        let c = col("x", ColumnType::Binary, 50, 0, 0, 0);
        assert_eq!(format_col_type(&c), "Binary(50)");
    }

    #[test]
    fn format_col_type_numeric() {
        let c = col("x", ColumnType::Numeric, 0, 0, 18, 2);
        assert_eq!(format_col_type(&c), "Numeric(18,2)");
    }

    #[test]
    fn format_col_type_memo() {
        let c = col("x", ColumnType::Memo, 0, 0, 0, 0);
        assert_eq!(format_col_type(&c), "Memo");
    }

    #[test]
    fn format_col_type_ole() {
        let c = col("x", ColumnType::Ole, 0, 0, 0, 0);
        assert_eq!(format_col_type(&c), "Ole");
    }

    #[test]
    fn format_col_type_long() {
        let c = col("x", ColumnType::Long, 0, 0, 0, 0);
        assert_eq!(format_col_type(&c), "Long");
    }

    #[test]
    fn format_col_type_double() {
        let c = col("x", ColumnType::Double, 0, 0, 0, 0);
        assert_eq!(format_col_type(&c), "Double");
    }

    #[test]
    fn format_col_type_timestamp() {
        let c = col("x", ColumnType::Timestamp, 0, 0, 0, 0);
        assert_eq!(format_col_type(&c), "Timestamp");
    }

    // -- format_col_attrs tests -----------------------------------------------

    #[test]
    fn format_col_attrs_not_null() {
        // FIXED only (NULLABLE bit not set) → "NOT NULL"
        let c = col("x", ColumnType::Long, 0, column_flags::FIXED, 0, 0);
        assert_eq!(format_col_attrs(&c), "NOT NULL");
    }

    #[test]
    fn format_col_attrs_auto_long() {
        let c = col(
            "x",
            ColumnType::Long,
            0,
            column_flags::NULLABLE | column_flags::AUTO_LONG,
            0,
            0,
        );
        assert!(format_col_attrs(&c).contains("AUTO"));
    }

    #[test]
    fn format_col_attrs_auto_uuid() {
        let c = col(
            "x",
            ColumnType::Long,
            0,
            column_flags::NULLABLE | column_flags::AUTO_UUID,
            0,
            0,
        );
        assert!(format_col_attrs(&c).contains("AUTO"));
    }

    #[test]
    fn format_col_attrs_nullable() {
        let c = col("x", ColumnType::Long, 0, column_flags::NULLABLE, 0, 0);
        assert_eq!(format_col_attrs(&c), "");
    }

    #[test]
    fn format_col_attrs_not_null_auto() {
        let c = col(
            "x",
            ColumnType::Long,
            0,
            column_flags::FIXED | column_flags::AUTO_LONG,
            0,
            0,
        );
        assert_eq!(format_col_attrs(&c), "NOT NULL  AUTO");
    }

    // -- format_index_flags tests ---------------------------------------------

    #[test]
    fn format_index_flags_empty() {
        let idx = IndexDef {
            name: "idx".to_string(),
            index_num: 0,
            index_type: 1,
            columns: vec![],
            flags: 0,
            first_data_page: 0,
            foreign_key: None,
        };
        assert_eq!(format_index_flags(&idx), "");
    }

    #[test]
    fn format_index_flags_unique() {
        let idx = IndexDef {
            name: "idx".to_string(),
            index_num: 0,
            index_type: 1,
            columns: vec![],
            flags: index_flags::UNIQUE,
            first_data_page: 0,
            foreign_key: None,
        };
        assert_eq!(format_index_flags(&idx), "UNIQUE");
    }

    #[test]
    fn format_index_flags_all() {
        let idx = IndexDef {
            name: "idx".to_string(),
            index_num: 0,
            index_type: 1,
            columns: vec![],
            flags: index_flags::UNIQUE | index_flags::IGNORE_NULLS | index_flags::REQUIRED,
            first_data_page: 0,
            foreign_key: None,
        };
        assert_eq!(format_index_flags(&idx), "UNIQUE IGNORE_NULLS REQUIRED");
    }

    // -- format_relationship_flags tests --------------------------------------

    #[test]
    fn format_relationship_flags_empty() {
        let rel = Relationship {
            name: "r".to_string(),
            from_table: "A".to_string(),
            to_table: "B".to_string(),
            columns: vec![],
            flags: 0,
        };
        assert_eq!(format_relationship_flags(&rel), "");
    }

    #[test]
    fn format_relationship_flags_cascade() {
        let rel = Relationship {
            name: "r".to_string(),
            from_table: "A".to_string(),
            to_table: "B".to_string(),
            columns: vec![],
            flags: relationship_flags::CASCADE_UPDATE | relationship_flags::CASCADE_DELETE,
        };
        assert_eq!(
            format_relationship_flags(&rel),
            "CASCADE UPDATE  CASCADE DELETE"
        );
    }
}
