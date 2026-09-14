//! Stored query reading and SQL reconstruction from MSysQueries.

use std::collections::BTreeMap;

use crate::catalog::read_catalog;
use crate::data::{self, Value};
use crate::file::{FileError, PageReader};
use crate::format::ObjectType;
use crate::table;

// ---------------------------------------------------------------------------
// Attribute constants
// ---------------------------------------------------------------------------

const ATTR_TYPE: u8 = 1;
const ATTR_PARAMETER: u8 = 2;
const ATTR_FLAG: u8 = 3;
const ATTR_TABLE: u8 = 5;
const ATTR_COLUMN: u8 = 6;
const ATTR_JOIN: u8 = 7;
const ATTR_WHERE: u8 = 8;
const ATTR_GROUPBY: u8 = 9;
const ATTR_HAVING: u8 = 10;
const ATTR_ORDERBY: u8 = 11;

// SELECT flag bits (from FLAG attribute row)
const SELECT_STAR: i16 = 0x01;
const DISTINCT: i16 = 0x02;
const OWNER_ACCESS: i16 = 0x04;
const DISTINCT_ROW: i16 = 0x08;
const TOP: i16 = 0x10;
const PERCENT: i16 = 0x20;

const UNION_FLAG: i16 = 0x02;
const APPEND_VALUE_FLAG: i16 = -0x8000; // 0x8000 as i16
const CROSSTAB_PIVOT_FLAG: i16 = 0x01;
const CROSSTAB_NORMAL_FLAG: i16 = 0x02;

const UNION_PART1: &str = "X7YZ_____1";
const UNION_PART2: &str = "X7YZ_____2";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Query type, determined by the Flag field of the TYPE attribute row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    Select = 1,
    MakeTable = 2,
    Append = 3,
    Update = 4,
    Delete = 5,
    Crosstab = 6,
    Ddl = 7,
    Passthrough = 8,
    Union = 9,
}

impl QueryType {
    fn from_flag(flag: i16) -> Option<Self> {
        match flag {
            1 => Some(Self::Select),
            2 => Some(Self::MakeTable),
            3 => Some(Self::Append),
            4 => Some(Self::Update),
            5 => Some(Self::Delete),
            6 => Some(Self::Crosstab),
            7 => Some(Self::Ddl),
            8 => Some(Self::Passthrough),
            9 => Some(Self::Union),
            _ => None,
        }
    }
}

/// A parsed query definition from MSysQueries.
#[derive(Debug, Clone)]
pub struct QueryDef {
    pub name: String,
    pub query_type: QueryType,
    rows: Vec<QueryRow>,
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct QueryRow {
    attribute: u8,
    expression: Option<String>,
    name1: Option<String>,
    name2: Option<String>,
    flag: Option<i16>,
    extra: Option<i32>,
}

// ---------------------------------------------------------------------------
// Value extraction helpers
// ---------------------------------------------------------------------------

fn get_long(row: &[Value], idx: usize) -> Option<i32> {
    match row.get(idx) {
        Some(Value::Long(v)) => Some(*v),
        _ => None,
    }
}

fn get_byte(row: &[Value], idx: usize) -> Option<u8> {
    match row.get(idx) {
        Some(Value::Byte(v)) => Some(*v),
        _ => None,
    }
}

fn get_binary(row: &[Value], idx: usize) -> Option<&[u8]> {
    match row.get(idx) {
        Some(Value::Binary(b)) => Some(b),
        _ => None,
    }
}

fn get_text(row: &[Value], idx: usize) -> Option<&str> {
    match row.get(idx) {
        Some(Value::Text(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

fn get_int(row: &[Value], idx: usize) -> Option<i16> {
    match row.get(idx) {
        Some(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Query column resolution
// ---------------------------------------------------------------------------

struct QueryColumnIndexes {
    object_id: usize,
    attribute: usize,
    order: Option<usize>,
    name1: Option<usize>,
    name2: Option<usize>,
    expression: Option<usize>,
    flag: Option<usize>,
    extra: Option<usize>,
}

fn resolve_query_columns(columns: &[table::ColumnDef]) -> Result<QueryColumnIndexes, FileError> {
    let mut object_id = None;
    let mut attribute = None;
    let mut order = None;
    let mut name1 = None;
    let mut name2 = None;
    let mut expression = None;
    let mut flag = None;
    let mut extra = None;

    for (i, col) in columns.iter().enumerate() {
        match col.name.as_str() {
            "ObjectId" => object_id = Some(i),
            "Attribute" => attribute = Some(i),
            "Order" => order = Some(i),
            "Name1" => name1 = Some(i),
            "Name2" => name2 = Some(i),
            "Expression" => expression = Some(i),
            "Flag" => flag = Some(i),
            "LvExtra" => extra = Some(i),
            _ => {}
        }
    }

    let object_id = object_id.ok_or(FileError::InvalidTableDef {
        reason: "MSysQueries missing ObjectId column",
    })?;
    let attribute = attribute.ok_or(FileError::InvalidTableDef {
        reason: "MSysQueries missing Attribute column",
    })?;

    Ok(QueryColumnIndexes {
        object_id,
        attribute,
        order,
        name1,
        name2,
        expression,
        flag,
        extra,
    })
}

// ---------------------------------------------------------------------------
// RawRow — intermediate representation for grouping
// ---------------------------------------------------------------------------

struct RawRow {
    attribute: u8,
    order: Vec<u8>,
    name1: Option<String>,
    name2: Option<String>,
    expression: Option<String>,
    flag: Option<i16>,
    extra: Option<i32>,
}

// ---------------------------------------------------------------------------
// build_query_defs
// ---------------------------------------------------------------------------

fn build_query_defs(
    groups: BTreeMap<i32, Vec<RawRow>>,
    query_name_map: &BTreeMap<u32, String>,
) -> Vec<QueryDef> {
    let mut queries = Vec::new();
    for (object_id, mut raw_rows) in groups {
        raw_rows.sort_by(|a, b| a.order.cmp(&b.order));

        let page_key = (object_id as u32) & 0x00FF_FFFF;
        let name = match query_name_map.get(&page_key) {
            Some(n) => n.clone(),
            None => continue,
        };

        let type_row = raw_rows.iter().find(|r| r.attribute == ATTR_TYPE);
        let query_type = match type_row.and_then(|r| r.flag).and_then(QueryType::from_flag) {
            Some(qt) => qt,
            // Embedded queries (~sq_ prefix) lack ATTR_TYPE; skip them.
            None if name.starts_with("~sq_") => continue,
            None => QueryType::Select,
        };

        let rows: Vec<QueryRow> = raw_rows
            .into_iter()
            .map(|r| QueryRow {
                attribute: r.attribute,
                expression: r.expression,
                name1: r.name1,
                name2: r.name2,
                flag: r.flag,
                extra: r.extra,
            })
            .collect();

        queries.push(QueryDef {
            name,
            query_type,
            rows,
        });
    }
    queries
}

// ---------------------------------------------------------------------------
// read_queries
// ---------------------------------------------------------------------------

/// Read all query definitions from the MSysQueries system table.
///
/// Returns an empty `Vec` if the table does not exist.
pub fn read_queries(reader: &mut PageReader) -> Result<Vec<QueryDef>, FileError> {
    let catalog = read_catalog(reader)?;

    let queries_entry = catalog.iter().find(|e| {
        e.name == "MSysQueries"
            && matches!(e.object_type, ObjectType::Table | ObjectType::SystemTable)
    });
    let queries_page = match queries_entry {
        Some(e) => e.table_page,
        None => return Ok(Vec::new()),
    };

    let tdef = table::read_table_def(reader, "MSysQueries", queries_page)?;
    let result = data::read_table_rows(reader, &tdef)?;
    result.warn_skipped("MSysQueries");

    let ci = resolve_query_columns(&tdef.columns)?;

    // Group rows by ObjectId
    let mut groups: BTreeMap<i32, Vec<RawRow>> = BTreeMap::new();

    for row in &result.rows {
        let object_id = match get_long(row, ci.object_id) {
            Some(v) => v,
            None => continue,
        };
        let attribute = match get_byte(row, ci.attribute) {
            Some(v) => v,
            None => continue,
        };
        let order = ci
            .order
            .and_then(|i| get_binary(row, i))
            .map(|b| b.to_vec())
            .unwrap_or_default();
        let name1 = ci
            .name1
            .and_then(|i| get_text(row, i))
            .map(|s| s.to_string());
        let name2 = ci
            .name2
            .and_then(|i| get_text(row, i))
            .map(|s| s.to_string());
        let expression = ci.expression.and_then(|i| get_text(row, i)).and_then(|s| {
            let trimmed = s.trim_end_matches('\0');
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let flag = ci.flag.and_then(|i| get_int(row, i));
        let extra = ci.extra.and_then(|i| get_long(row, i));

        groups.entry(object_id).or_default().push(RawRow {
            attribute,
            order,
            name1,
            name2,
            expression,
            flag,
            extra,
        });
    }

    // Build query name map from catalog
    let query_name_map: BTreeMap<u32, String> = catalog
        .iter()
        .filter(|e| e.object_type == ObjectType::Query)
        .map(|e| (e.table_page, e.name.clone()))
        .collect();

    Ok(build_query_defs(groups, &query_name_map))
}

// ---------------------------------------------------------------------------
// query_to_sql — SQL restoration
// ---------------------------------------------------------------------------

/// Restore the SQL string for a query definition.
pub fn query_to_sql(qdef: &QueryDef) -> String {
    let mut builder = String::new();
    let supports_standard = !matches!(qdef.query_type, QueryType::Passthrough | QueryType::Ddl);

    if supports_standard {
        let params = format_parameters(&qdef.rows);
        if !params.is_empty() {
            builder.push_str("PARAMETERS ");
            builder.push_str(&params);
            builder.push_str(";\n");
        }
    }

    match qdef.query_type {
        QueryType::Select => sql_select(&mut builder, &qdef.rows),
        QueryType::Delete => sql_delete(&mut builder, &qdef.rows),
        QueryType::Update => sql_update(&mut builder, &qdef.rows),
        QueryType::Append => sql_append(&mut builder, &qdef.rows),
        QueryType::MakeTable => sql_make_table(&mut builder, &qdef.rows),
        QueryType::Crosstab => sql_crosstab(&mut builder, &qdef.rows),
        QueryType::Union => sql_union(&mut builder, &qdef.rows),
        QueryType::Passthrough => sql_passthrough(&mut builder, &qdef.rows),
        QueryType::Ddl => sql_ddl(&mut builder, &qdef.rows),
    }

    if supports_standard {
        if has_flag(&qdef.rows, OWNER_ACCESS) {
            builder.push_str("\nWITH OWNERACCESS OPTION");
        }
        builder.push(';');
    }

    builder
}

// ---------------------------------------------------------------------------
// Row access helpers
// ---------------------------------------------------------------------------

fn rows_by_attr(rows: &[QueryRow], attr: u8) -> Vec<&QueryRow> {
    rows.iter().filter(|r| r.attribute == attr).collect()
}

fn flag_row(rows: &[QueryRow]) -> Option<&QueryRow> {
    rows.iter().find(|r| r.attribute == ATTR_FLAG)
}

fn has_flag(rows: &[QueryRow], mask: i16) -> bool {
    flag_row(rows)
        .and_then(|r| r.flag)
        .map(|f| (f & mask) != 0)
        .unwrap_or(false)
}

fn type_row(rows: &[QueryRow]) -> Option<&QueryRow> {
    rows.iter().find(|r| r.attribute == ATTR_TYPE)
}

fn where_expr(rows: &[QueryRow]) -> Option<&str> {
    rows.iter()
        .find(|r| r.attribute == ATTR_WHERE)
        .and_then(|r| r.expression.as_deref())
}

fn having_expr(rows: &[QueryRow]) -> Option<&str> {
    rows.iter()
        .find(|r| r.attribute == ATTR_HAVING)
        .and_then(|r| r.expression.as_deref())
}

// ---------------------------------------------------------------------------
// Identifier quoting
// ---------------------------------------------------------------------------

fn needs_quoting(s: &str) -> bool {
    s.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_')
}

fn is_quoted(s: &str) -> bool {
    s.len() >= 2 && s.starts_with('[') && s.ends_with(']')
}

fn to_quoted_expr(s: &str) -> String {
    if is_quoted(s) {
        s.to_string()
    } else {
        format!("[{}]", s.replace(']', "]]"))
    }
}

/// Quote an expression, splitting on '.' for identifiers.
fn to_optional_quoted(full_expr: &str, is_identifier: bool) -> String {
    if is_identifier {
        full_expr
            .split('.')
            .map(|part| {
                if needs_quoting(part) {
                    to_quoted_expr(part)
                } else {
                    part.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    } else if needs_quoting(full_expr) {
        to_quoted_expr(full_expr)
    } else {
        full_expr.to_string()
    }
}

fn to_alias(alias: Option<&str>) -> String {
    match alias {
        Some(a) => format!(" AS {}", to_optional_quoted(a, false)),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// PARAMETERS clause
// ---------------------------------------------------------------------------

fn param_type_name(flag: i16) -> Option<&'static str> {
    match flag {
        0 => Some("Value"),
        1 => Some("Bit"),
        10 => Some("Text"),
        2 => Some("Byte"),
        3 => Some("Short"),
        4 => Some("Long"),
        5 => Some("Currency"),
        6 => Some("IEEESingle"),
        7 => Some("IEEEDouble"),
        8 => Some("DateTime"),
        9 => Some("Binary"),
        11 => Some("LongBinary"),
        15 => Some("Guid"),
        _ => None,
    }
}

fn format_parameters(rows: &[QueryRow]) -> String {
    let param_rows = rows_by_attr(rows, ATTR_PARAMETER);
    let parts: Vec<String> = param_rows
        .iter()
        .filter_map(|r| {
            let name1 = r.name1.as_deref()?;
            let flag = r.flag?;
            let type_name = param_type_name(flag)?;
            let mut s = format!("{name1} {type_name}");
            // TEXT type with extra size
            if flag == 10 {
                if let Some(extra) = r.extra {
                    if extra > 0 {
                        s.push_str(&format!("({extra})"));
                    }
                }
            }
            Some(s)
        })
        .collect();
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// SELECT type modifier (DISTINCT, TOP, etc.)
// ---------------------------------------------------------------------------

fn get_select_type(rows: &[QueryRow]) -> String {
    if has_flag(rows, DISTINCT) {
        return "DISTINCT".to_string();
    }
    if has_flag(rows, DISTINCT_ROW) {
        return "DISTINCTROW".to_string();
    }
    if has_flag(rows, TOP) {
        let n = flag_row(rows)
            .and_then(|r| r.name1.as_deref())
            .unwrap_or("");
        let mut s = format!("TOP {n}");
        if has_flag(rows, PERCENT) {
            s.push_str(" PERCENT");
        }
        return s;
    }
    String::new()
}

// ---------------------------------------------------------------------------
// SELECT columns
// ---------------------------------------------------------------------------

fn get_select_columns(rows: &[QueryRow], filter: impl Fn(&QueryRow) -> bool) -> String {
    let column_rows = rows_by_attr(rows, ATTR_COLUMN);
    let mut parts: Vec<String> = column_rows
        .iter()
        .filter(|r| filter(r))
        .filter_map(|r| {
            let expr = r.expression.as_deref()?;
            let mut s = expr.to_string();
            s.push_str(&to_alias(r.name1.as_deref()));
            Some(s)
        })
        .collect();
    if has_flag(rows, SELECT_STAR) {
        parts.push("*".to_string());
    }
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// GROUP BY
// ---------------------------------------------------------------------------

fn get_groupings(rows: &[QueryRow], filter: impl Fn(&QueryRow) -> bool) -> String {
    let gb_rows = rows_by_attr(rows, ATTR_GROUPBY);
    let parts: Vec<&str> = gb_rows
        .iter()
        .filter(|r| filter(r))
        .filter_map(|r| r.expression.as_deref())
        .collect();
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// ORDER BY
// ---------------------------------------------------------------------------

fn get_orderings(rows: &[QueryRow]) -> String {
    let ob_rows = rows_by_attr(rows, ATTR_ORDERBY);
    let parts: Vec<String> = ob_rows
        .iter()
        .filter_map(|r| {
            let expr = r.expression.as_deref()?;
            let mut s = expr.to_string();
            if r.name1
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("D"))
                .unwrap_or(false)
            {
                s.push_str(" DESC");
            }
            Some(s)
        })
        .collect();
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// FROM clause + JOIN building
// ---------------------------------------------------------------------------

enum TableSource {
    Simple {
        name: String,
        expr: String,
    },
    Join {
        from: Box<TableSource>,
        to: Box<TableSource>,
        join_type: i16,
        on_conditions: Vec<String>,
    },
}

impl TableSource {
    fn contains_table(&self, table: &str) -> bool {
        match self {
            TableSource::Simple { name, .. } => name.eq_ignore_ascii_case(table),
            TableSource::Join { from, to, .. } => {
                from.contains_table(table) || to.contains_table(table)
            }
        }
    }

    fn same_join(&mut self, jtype: i16, on: &str) -> bool {
        match self {
            TableSource::Join {
                join_type,
                on_conditions,
                ..
            } => {
                if *join_type == jtype {
                    // AND conditions are added in reverse order
                    on_conditions.insert(0, on.to_string());
                    true
                } else {
                    false
                }
            }
            TableSource::Simple { .. } => false,
        }
    }

    fn to_sql(&self, is_top_level: bool) -> String {
        match self {
            TableSource::Simple { expr, .. } => expr.clone(),
            TableSource::Join {
                from,
                to,
                join_type,
                on_conditions,
            } => {
                let join_str = match join_type {
                    1 => " INNER JOIN ",
                    2 => " LEFT JOIN ",
                    3 => " RIGHT JOIN ",
                    _ => " JOIN ",
                };

                let mut sb = String::new();
                if !is_top_level {
                    sb.push('(');
                }

                sb.push_str(&from.to_sql(false));
                sb.push_str(join_str);
                sb.push_str(&to.to_sql(false));
                sb.push_str(" ON ");

                let multi = on_conditions.len() > 1;
                if multi {
                    sb.push('(');
                }
                sb.push_str(&on_conditions.join(") AND ("));
                if multi {
                    sb.push(')');
                }

                if !is_top_level {
                    sb.push(')');
                }

                sb
            }
        }
    }
}

fn build_from_tables(rows: &[QueryRow]) -> Vec<String> {
    let table_rows = rows_by_attr(rows, ATTR_TABLE);
    let join_rows = rows_by_attr(rows, ATTR_JOIN);

    let mut sources: Vec<TableSource> = Vec::new();
    for trow in &table_rows {
        let mut expr = String::new();
        if let Some(ref e) = trow.expression {
            expr.push_str(&to_quoted_expr(e));
            expr.push('.');
        }
        if let Some(ref n1) = trow.name1 {
            expr.push_str(&to_optional_quoted(n1, true));
        }
        if let Some(ref n2) = trow.name2 {
            expr.push_str(&to_alias(Some(n2)));
        }
        let key = trow
            .name2
            .as_deref()
            .or(trow.name1.as_deref())
            .unwrap_or("")
            .to_string();
        sources.push(TableSource::Simple { name: key, expr });
    }

    for jrow in &join_rows {
        let from_table = match jrow.name1.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let to_table = match jrow.name2.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let on_expr = match jrow.expression.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let jtype = jrow.flag.unwrap_or(1);

        // Find from and to in existing sources
        let mut from_idx = None;
        let mut to_idx = None;
        let mut same_source = false;

        for (i, ts) in sources.iter().enumerate() {
            if from_idx.is_none() && ts.contains_table(from_table) {
                from_idx = Some(i);
                if to_idx.is_none() && ts.contains_table(to_table) {
                    to_idx = from_idx;
                    same_source = true;
                    break;
                }
            } else if to_idx.is_none() && ts.contains_table(to_table) {
                to_idx = Some(i);
            }
            if from_idx.is_some() && to_idx.is_some() {
                break;
            }
        }

        if same_source {
            if let Some(idx) = from_idx {
                if sources[idx].same_join(jtype, on_expr) {
                    continue;
                }
                // Inconsistent join types — skip
                continue;
            }
        }

        // Extract sources, removing from list (higher index first)
        let from_ts;
        let to_ts;

        match (from_idx, to_idx) {
            (Some(fi), Some(ti)) => {
                if fi > ti {
                    from_ts = sources.remove(fi);
                    to_ts = sources.remove(ti);
                } else {
                    to_ts = sources.remove(ti);
                    from_ts = sources.remove(fi);
                }
            }
            (Some(fi), None) => {
                from_ts = sources.remove(fi);
                to_ts = TableSource::Simple {
                    name: to_table.to_string(),
                    expr: to_optional_quoted(to_table, true),
                };
            }
            (None, Some(ti)) => {
                from_ts = TableSource::Simple {
                    name: from_table.to_string(),
                    expr: to_optional_quoted(from_table, true),
                };
                to_ts = sources.remove(ti);
            }
            (None, None) => {
                from_ts = TableSource::Simple {
                    name: from_table.to_string(),
                    expr: to_optional_quoted(from_table, true),
                };
                to_ts = TableSource::Simple {
                    name: to_table.to_string(),
                    expr: to_optional_quoted(to_table, true),
                };
            }
        }

        sources.push(TableSource::Join {
            from: Box::new(from_ts),
            to: Box::new(to_ts),
            join_type: jtype,
            on_conditions: vec![on_expr.to_string()],
        });
    }

    sources.iter().map(|ts| ts.to_sql(true)).collect()
}

// ---------------------------------------------------------------------------
// Core SELECT-like SQL generation
// ---------------------------------------------------------------------------

fn append_select_body(
    builder: &mut String,
    rows: &[QueryRow],
    use_prefix: bool,
    into_target: Option<&str>,
    col_filter: &dyn Fn(&QueryRow) -> bool,
    gb_filter: &dyn Fn(&QueryRow) -> bool,
) {
    if use_prefix {
        builder.push_str("SELECT ");
        let sel_type = get_select_type(rows);
        if !sel_type.is_empty() {
            builder.push_str(&sel_type);
            builder.push(' ');
        }
    }

    builder.push_str(&get_select_columns(rows, col_filter));

    if let Some(target) = into_target {
        builder.push_str(" INTO ");
        builder.push_str(&to_optional_quoted(target, true));
    }

    let from = build_from_tables(rows);
    if !from.is_empty() {
        builder.push_str("\nFROM ");
        builder.push_str(&from.join(", "));
    }

    if let Some(w) = where_expr(rows) {
        builder.push_str("\nWHERE ");
        builder.push_str(w);
    }

    let gb = get_groupings(rows, gb_filter);
    if !gb.is_empty() {
        builder.push_str("\nGROUP BY ");
        builder.push_str(&gb);
    }

    if let Some(h) = having_expr(rows) {
        builder.push_str("\nHAVING ");
        builder.push_str(h);
    }

    let ord = get_orderings(rows);
    if !ord.is_empty() {
        builder.push_str("\nORDER BY ");
        builder.push_str(&ord);
    }
}

// ---------------------------------------------------------------------------
// Query-type-specific SQL
// ---------------------------------------------------------------------------

fn sql_select(builder: &mut String, rows: &[QueryRow]) {
    append_select_body(builder, rows, true, None, &|_| true, &|_| true);
}

fn sql_delete(builder: &mut String, rows: &[QueryRow]) {
    builder.push_str("DELETE ");
    append_select_body(builder, rows, false, None, &|_| true, &|_| true);
}

fn sql_update(builder: &mut String, rows: &[QueryRow]) {
    builder.push_str("UPDATE ");
    let from = build_from_tables(rows);
    builder.push_str(&from.join(", "));

    let column_rows = rows_by_attr(rows, ATTR_COLUMN);
    let set_parts: Vec<String> = column_rows
        .iter()
        .filter_map(|r| {
            let name2 = r.name2.as_deref()?;
            let expr = r.expression.as_deref()?;
            Some(format!("{} = {}", to_optional_quoted(name2, true), expr))
        })
        .collect();
    if !set_parts.is_empty() {
        builder.push_str("\nSET ");
        builder.push_str(&set_parts.join(", "));
    }

    if let Some(w) = where_expr(rows) {
        builder.push_str("\nWHERE ");
        builder.push_str(w);
    }
}

fn sql_append(builder: &mut String, rows: &[QueryRow]) {
    let target = type_row(rows)
        .and_then(|r| r.name1.as_deref())
        .unwrap_or("");

    builder.push_str("INSERT INTO ");
    builder.push_str(&to_optional_quoted(target, true));

    // Target columns: column rows with name2 set (that are NOT value rows)
    let all_col_rows = rows_by_attr(rows, ATTR_COLUMN);
    let target_cols: Vec<String> = all_col_rows
        .iter()
        .filter(|r| r.name2.is_some())
        .filter_map(|r| Some(to_optional_quoted(r.name2.as_deref()?, true)))
        .collect();
    if !target_cols.is_empty() {
        builder.push_str(" (");
        builder.push_str(&target_cols.join(", "));
        builder.push(')');
    }

    // Check for VALUES (rows with APPEND_VALUE_FLAG)
    let value_rows: Vec<&&QueryRow> = all_col_rows
        .iter()
        .filter(|r| {
            r.flag
                .map(|f| (f & APPEND_VALUE_FLAG) != 0)
                .unwrap_or(false)
        })
        .collect();

    builder.push('\n');
    if !value_rows.is_empty() {
        let values: Vec<&str> = value_rows
            .iter()
            .filter_map(|r| r.expression.as_deref())
            .collect();
        builder.push_str("VALUES (");
        builder.push_str(&values.join(", "));
        builder.push(')');
    } else {
        // SELECT ... FROM ...
        let not_value = |r: &QueryRow| {
            !r.flag
                .map(|f| (f & APPEND_VALUE_FLAG) != 0)
                .unwrap_or(false)
        };
        append_select_body(builder, rows, true, None, &not_value, &|_| true);
    }
}

fn sql_make_table(builder: &mut String, rows: &[QueryRow]) {
    let target = type_row(rows)
        .and_then(|r| r.name1.as_deref())
        .unwrap_or("");
    append_select_body(builder, rows, true, Some(target), &|_| true, &|_| true);
}

fn sql_crosstab(builder: &mut String, rows: &[QueryRow]) {
    // TRANSFORM expression: column rows without PIVOT or NORMAL flag
    let all_col_rows = rows_by_attr(rows, ATTR_COLUMN);
    let transform_row = all_col_rows.iter().find(|r| {
        let f = r.flag.unwrap_or(0);
        (f & (CROSSTAB_PIVOT_FLAG | CROSSTAB_NORMAL_FLAG)) == 0
    });
    if let Some(trow) = transform_row {
        if let Some(ref expr) = trow.expression {
            builder.push_str("TRANSFORM ");
            builder.push_str(expr);
            builder.push_str(&to_alias(trow.name1.as_deref()));
            builder.push('\n');
        }
    }

    // SELECT body with NORMAL columns and NORMAL groupby
    let normal_col = |r: &QueryRow| {
        r.flag
            .map(|f| (f & CROSSTAB_NORMAL_FLAG) != 0)
            .unwrap_or(false)
    };
    let normal_gb = |r: &QueryRow| {
        r.flag
            .map(|f| (f & CROSSTAB_NORMAL_FLAG) != 0)
            .unwrap_or(false)
    };
    append_select_body(builder, rows, true, None, &normal_col, &normal_gb);

    // PIVOT expression: column row with PIVOT flag
    let pivot_row = all_col_rows.iter().find(|r| {
        r.flag
            .map(|f| (f & CROSSTAB_PIVOT_FLAG) != 0)
            .unwrap_or(false)
    });
    if let Some(prow) = pivot_row {
        if let Some(ref expr) = prow.expression {
            builder.push_str("\nPIVOT ");
            builder.push_str(expr);
        }
    }
}

fn sql_union(builder: &mut String, rows: &[QueryRow]) {
    let table_rows = rows_by_attr(rows, ATTR_TABLE);

    let part1 = table_rows
        .iter()
        .find(|r| r.name2.as_deref() == Some(UNION_PART1))
        .and_then(|r| r.expression.as_deref());
    let part2 = table_rows
        .iter()
        .find(|r| r.name2.as_deref() == Some(UNION_PART2))
        .and_then(|r| r.expression.as_deref());

    if let Some(p1) = part1 {
        let cleaned = clean_union_string(p1);
        builder.push_str(&cleaned);
    }

    builder.push_str("\nUNION ");

    // UNION_FLAG set means regular UNION; unset means UNION ALL
    if !has_flag(rows, UNION_FLAG) {
        builder.push_str("ALL ");
    }

    if let Some(p2) = part2 {
        let cleaned = clean_union_string(p2);
        builder.push_str(&cleaned);
    }

    let ord = get_orderings(rows);
    if !ord.is_empty() {
        builder.push_str("\nORDER BY ");
        builder.push_str(&ord);
    }
}

fn clean_union_string(s: &str) -> String {
    let trimmed = s.trim();
    let mut result = String::with_capacity(trimmed.len());
    let mut prev_newline = false;
    for c in trimmed.chars() {
        if c == '\r' || c == '\n' {
            if !prev_newline {
                result.push('\n');
                prev_newline = true;
            }
            // 連続する改行は無視（圧縮）
        } else {
            result.push(c);
            prev_newline = false;
        }
    }
    result
}

fn sql_passthrough(builder: &mut String, rows: &[QueryRow]) {
    if let Some(expr) = type_row(rows).and_then(|r| r.expression.as_deref()) {
        builder.push_str(expr);
    }
}

fn sql_ddl(builder: &mut String, rows: &[QueryRow]) {
    if let Some(expr) = type_row(rows).and_then(|r| r.expression.as_deref()) {
        builder.push_str(expr);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Unit tests --

    #[test]
    fn query_type_from_flag() {
        assert_eq!(QueryType::from_flag(1), Some(QueryType::Select));
        assert_eq!(QueryType::from_flag(2), Some(QueryType::MakeTable));
        assert_eq!(QueryType::from_flag(3), Some(QueryType::Append));
        assert_eq!(QueryType::from_flag(4), Some(QueryType::Update));
        assert_eq!(QueryType::from_flag(5), Some(QueryType::Delete));
        assert_eq!(QueryType::from_flag(6), Some(QueryType::Crosstab));
        assert_eq!(QueryType::from_flag(7), Some(QueryType::Ddl));
        assert_eq!(QueryType::from_flag(8), Some(QueryType::Passthrough));
        assert_eq!(QueryType::from_flag(9), Some(QueryType::Union));
        assert_eq!(QueryType::from_flag(0), None);
        assert_eq!(QueryType::from_flag(99), None);
    }

    #[test]
    fn join_type_str() {
        // INNER JOIN
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert_eq!(ts.to_sql(true), "T1 INNER JOIN T2 ON T1.id = T2.id");

        // LEFT JOIN
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 2,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert_eq!(ts.to_sql(true), "T1 LEFT JOIN T2 ON T1.id = T2.id");

        // RIGHT JOIN
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 3,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert_eq!(ts.to_sql(true), "T1 RIGHT JOIN T2 ON T1.id = T2.id");
    }

    #[test]
    fn join_multi_on() {
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.a = T2.a".to_string(), "T1.b = T2.b".to_string()],
        };
        assert_eq!(
            ts.to_sql(true),
            "T1 INNER JOIN T2 ON (T1.a = T2.a) AND (T1.b = T2.b)"
        );
    }

    #[test]
    fn join_nested_parens() {
        let inner = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        let outer = TableSource::Join {
            from: Box::new(inner),
            to: Box::new(TableSource::Simple {
                name: "T3".to_string(),
                expr: "T3".to_string(),
            }),
            join_type: 2,
            on_conditions: vec!["T1.id = T3.id".to_string()],
        };
        assert_eq!(
            outer.to_sql(true),
            "(T1 INNER JOIN T2 ON T1.id = T2.id) LEFT JOIN T3 ON T1.id = T3.id"
        );
    }

    #[test]
    fn quoting_simple() {
        assert_eq!(to_optional_quoted("Table1", true), "Table1");
        assert_eq!(to_optional_quoted("col1", true), "col1");
    }

    #[test]
    fn quoting_with_space() {
        assert_eq!(
            to_optional_quoted("Another Table", false),
            "[Another Table]"
        );
    }

    #[test]
    fn quoting_dotted_identifier() {
        assert_eq!(to_optional_quoted("Table1.col1", true), "Table1.col1");
    }

    #[test]
    fn quoting_already_quoted() {
        assert_eq!(to_optional_quoted("[Table1]", true), "[Table1]");
    }

    #[test]
    fn quoting_bracket_escape() {
        assert_eq!(to_quoted_expr("col]x"), "[col]]x]");
    }

    // -- Integration tests with real .mdb files --

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
    fn read_queries_count() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        assert_eq!(queries.len(), 9, "should have 9 queries");
    }

    #[test]
    fn read_queries_names() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let names: Vec<&str> = queries.iter().map(|q| q.name.as_str()).collect();
        assert!(names.contains(&"SelectQuery"));
        assert!(names.contains(&"DeleteQuery"));
        assert!(names.contains(&"UpdateQuery"));
        assert!(names.contains(&"AppendQuery"));
        assert!(names.contains(&"MakeTableQuery"));
        assert!(names.contains(&"CrosstabQuery"));
        assert!(names.contains(&"UnionQuery"));
        assert!(names.contains(&"PassthroughQuery"));
        assert!(names.contains(&"DataDefinitionQuery"));
    }

    #[test]
    fn read_queries_types() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        for q in &queries {
            let expected = match q.name.as_str() {
                "SelectQuery" => QueryType::Select,
                "DeleteQuery" => QueryType::Delete,
                "UpdateQuery" => QueryType::Update,
                "AppendQuery" => QueryType::Append,
                "MakeTableQuery" => QueryType::MakeTable,
                "CrosstabQuery" => QueryType::Crosstab,
                "UnionQuery" => QueryType::Union,
                "PassthroughQuery" => QueryType::Passthrough,
                "DataDefinitionQuery" => QueryType::Ddl,
                other => panic!("unexpected query name: {other}"),
            };
            assert_eq!(q.query_type, expected, "type mismatch for {}", q.name);
        }
    }

    fn find_query<'a>(queries: &'a [QueryDef], name: &str) -> &'a QueryDef {
        queries.iter().find(|q| q.name == name).unwrap()
    }

    #[test]
    fn sql_delete_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "DeleteQuery");
        let sql = query_to_sql(q);
        // Expected:
        // DELETE Table1.col1, Table1.col2, Table1.col3
        // FROM Table1
        // WHERE (((Table1.col1)>"blah"));
        assert!(
            sql.starts_with("DELETE "),
            "should start with DELETE: {sql}"
        );
        assert!(
            sql.contains("Table1.col1"),
            "should contain Table1.col1: {sql}"
        );
        assert!(
            sql.contains("FROM Table1"),
            "should contain FROM Table1: {sql}"
        );
        assert!(
            sql.contains("WHERE (((Table1.col1)>\"blah\"))"),
            "should contain WHERE clause: {sql}"
        );
        assert!(sql.ends_with(';'), "should end with semicolon: {sql}");
    }

    #[test]
    fn sql_select_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "SelectQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.starts_with("SELECT DISTINCT "),
            "should start with SELECT DISTINCT: {sql}"
        );
        assert!(sql.contains("Table1.*"), "should contain Table1.*: {sql}");
        assert!(
            sql.contains("Table2.col1"),
            "should contain Table2.col1: {sql}"
        );
        assert!(
            sql.contains("LEFT JOIN Table3 ON Table1.col1 = Table3.col1"),
            "should contain LEFT JOIN: {sql}"
        );
        assert!(
            sql.contains("INNER JOIN Table2"),
            "should contain INNER JOIN: {sql}"
        );
        assert!(
            sql.contains("ORDER BY Table2.col1"),
            "should contain ORDER BY: {sql}"
        );
    }

    #[test]
    fn sql_append_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "AppendQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.starts_with("INSERT INTO Table3"),
            "should start with INSERT INTO Table3: {sql}"
        );
        assert!(
            sql.contains("(col2, col2, col3)"),
            "should contain target columns: {sql}"
        );
        assert!(sql.contains("SELECT "), "should contain SELECT: {sql}");
        assert!(
            sql.contains("INNER JOIN Table2 ON [Table1].[col1]=[Table2].[col1]"),
            "should contain JOIN: {sql}"
        );
    }

    #[test]
    fn sql_update_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "UpdateQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.contains("PARAMETERS User Name Text;"),
            "should contain PARAMETERS: {sql}"
        );
        assert!(
            sql.contains("UPDATE Table1"),
            "should contain UPDATE Table1: {sql}"
        );
        assert!(sql.contains("SET "), "should contain SET: {sql}");
        assert!(
            sql.contains("Table1.col1 = \"foo\""),
            "should contain col1 assignment: {sql}"
        );
    }

    #[test]
    fn sql_make_table_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "MakeTableQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.contains("INTO Table4"),
            "should contain INTO Table4: {sql}"
        );
        assert!(sql.contains("SELECT "), "should contain SELECT: {sql}");
        assert!(
            sql.contains("Max(Table2.col1) AS MaxOfcol1"),
            "should contain aggregate: {sql}"
        );
        assert!(
            sql.contains("GROUP BY Table2.col2, Table3.col2"),
            "should contain GROUP BY: {sql}"
        );
    }

    #[test]
    fn sql_crosstab_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "CrosstabQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.starts_with("TRANSFORM "),
            "should start with TRANSFORM: {sql}"
        );
        assert!(
            sql.contains("Count([Table2].[col2]) AS CountOfcol2"),
            "should contain TRANSFORM expression: {sql}"
        );
        assert!(
            sql.contains("PIVOT [Table1].[col1]"),
            "should contain PIVOT: {sql}"
        );
    }

    #[test]
    fn no_queries_returns_empty() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        assert!(queries.is_empty(), "testV2003.mdb should have no queries");
    }

    // -- resolve_query_columns tests ------------------------------------------

    fn make_col(name: &str) -> table::ColumnDef {
        table::ColumnDef {
            name: name.to_string(),
            col_type: crate::format::ColumnType::Long,
            col_num: 0,
            var_col_num: 0,
            fixed_offset: 0,
            col_size: 4,
            flags: 0,
            is_fixed: true,
            precision: 0,
            scale: 0,
            is_calculated: false,
        }
    }

    #[test]
    fn resolve_all_columns_present() {
        let columns = vec![
            make_col("ObjectId"),
            make_col("Attribute"),
            make_col("Order"),
            make_col("Name1"),
            make_col("Name2"),
            make_col("Expression"),
            make_col("Flag"),
            make_col("LvExtra"),
        ];
        let ci = resolve_query_columns(&columns).unwrap();
        assert_eq!(ci.object_id, 0);
        assert_eq!(ci.attribute, 1);
        assert_eq!(ci.order, Some(2));
        assert_eq!(ci.name1, Some(3));
        assert_eq!(ci.name2, Some(4));
        assert_eq!(ci.expression, Some(5));
        assert_eq!(ci.flag, Some(6));
        assert_eq!(ci.extra, Some(7));
    }

    #[test]
    fn resolve_missing_object_id() {
        let columns = vec![make_col("Attribute")];
        assert!(resolve_query_columns(&columns).is_err());
    }

    #[test]
    fn resolve_missing_attribute() {
        let columns = vec![make_col("ObjectId")];
        assert!(resolve_query_columns(&columns).is_err());
    }

    #[test]
    fn resolve_optional_columns_missing() {
        let columns = vec![make_col("ObjectId"), make_col("Attribute")];
        let ci = resolve_query_columns(&columns).unwrap();
        assert_eq!(ci.object_id, 0);
        assert_eq!(ci.attribute, 1);
        assert_eq!(ci.order, None);
        assert_eq!(ci.name1, None);
        assert_eq!(ci.expression, None);
    }

    #[test]
    fn resolve_unknown_column_ignored() {
        let columns = vec![
            make_col("ObjectId"),
            make_col("Attribute"),
            make_col("UnknownCol"),
        ];
        let ci = resolve_query_columns(&columns).unwrap();
        assert_eq!(ci.object_id, 0);
        assert_eq!(ci.attribute, 1);
        assert_eq!(ci.order, None);
    }

    // -- param_type_name all flag values -------------------------------------

    #[test]
    fn param_type_name_all_flags() {
        assert_eq!(param_type_name(0), Some("Value"));
        assert_eq!(param_type_name(1), Some("Bit"));
        assert_eq!(param_type_name(2), Some("Byte"));
        assert_eq!(param_type_name(3), Some("Short"));
        assert_eq!(param_type_name(4), Some("Long"));
        assert_eq!(param_type_name(5), Some("Currency"));
        assert_eq!(param_type_name(6), Some("IEEESingle"));
        assert_eq!(param_type_name(7), Some("IEEEDouble"));
        assert_eq!(param_type_name(8), Some("DateTime"));
        assert_eq!(param_type_name(9), Some("Binary"));
        assert_eq!(param_type_name(10), Some("Text"));
        assert_eq!(param_type_name(11), Some("LongBinary"));
        assert_eq!(param_type_name(15), Some("Guid"));
        assert_eq!(param_type_name(-1), None);
        assert_eq!(param_type_name(99), None);
        assert_eq!(param_type_name(12), None);
        assert_eq!(param_type_name(13), None);
        assert_eq!(param_type_name(14), None);
    }

    // -- clean_union_string --------------------------------------------------

    #[test]
    fn clean_union_string_basic() {
        let result = clean_union_string("  SELECT * FROM T1  ");
        assert_eq!(result, "SELECT * FROM T1");
    }

    #[test]
    fn clean_union_string_collapses_newlines() {
        let input = "SELECT *\n\n\nFROM T1\r\n\r\nWHERE 1=1";
        let result = clean_union_string(input);
        assert_eq!(result, "SELECT *\nFROM T1\nWHERE 1=1");
    }

    #[test]
    fn clean_union_string_cr_only() {
        let result = clean_union_string("A\r\r\rB");
        assert_eq!(result, "A\nB");
    }

    // -- get_byte / get_binary fallback paths ---------------------------------

    #[test]
    fn get_byte_wrong_type() {
        let row = vec![Value::Long(42)];
        assert_eq!(get_byte(&row, 0), None);
    }

    #[test]
    fn get_binary_wrong_type() {
        let row = vec![Value::Long(42)];
        assert_eq!(get_binary(&row, 0), None);
    }

    #[test]
    fn get_text_empty_string() {
        let row = vec![Value::Text(String::new())];
        assert_eq!(get_text(&row, 0), None);
    }

    #[test]
    fn get_int_wrong_type() {
        let row = vec![Value::Long(42)];
        assert_eq!(get_int(&row, 0), None);
    }

    #[test]
    fn get_long_wrong_type() {
        let row = vec![Value::Text("hello".into())];
        assert_eq!(get_long(&row, 0), None);
    }

    // -- get_select_type tests ------------------------------------------------

    #[test]
    fn get_select_type_distinct_row() {
        let rows = vec![QueryRow {
            attribute: ATTR_FLAG,
            expression: None,
            name1: None,
            name2: None,
            flag: Some(DISTINCT_ROW),
            extra: None,
        }];
        assert_eq!(get_select_type(&rows), "DISTINCTROW");
    }

    #[test]
    fn get_select_type_top() {
        let rows = vec![QueryRow {
            attribute: ATTR_FLAG,
            expression: None,
            name1: Some("10".to_string()),
            name2: None,
            flag: Some(TOP),
            extra: None,
        }];
        assert_eq!(get_select_type(&rows), "TOP 10");
    }

    #[test]
    fn get_select_type_top_percent() {
        let rows = vec![QueryRow {
            attribute: ATTR_FLAG,
            expression: None,
            name1: Some("25".to_string()),
            name2: None,
            flag: Some(TOP | PERCENT),
            extra: None,
        }];
        assert_eq!(get_select_type(&rows), "TOP 25 PERCENT");
    }

    #[test]
    fn get_select_type_none() {
        let rows = vec![QueryRow {
            attribute: ATTR_FLAG,
            expression: None,
            name1: None,
            name2: None,
            flag: Some(0),
            extra: None,
        }];
        assert_eq!(get_select_type(&rows), "");
    }

    // -- format_parameters tests ----------------------------------------------

    #[test]
    fn format_parameters_text_with_size() {
        let rows = vec![QueryRow {
            attribute: ATTR_PARAMETER,
            expression: None,
            name1: Some("Param1".to_string()),
            name2: None,
            flag: Some(10), // Text
            extra: Some(255),
        }];
        let result = format_parameters(&rows);
        assert_eq!(result, "Param1 Text(255)");
    }

    #[test]
    fn format_parameters_text_no_size() {
        let rows = vec![QueryRow {
            attribute: ATTR_PARAMETER,
            expression: None,
            name1: Some("Param1".to_string()),
            name2: None,
            flag: Some(10), // Text
            extra: Some(0), // zero size → not appended
        }];
        let result = format_parameters(&rows);
        assert_eq!(result, "Param1 Text");
    }

    #[test]
    fn format_parameters_multiple() {
        let rows = vec![
            QueryRow {
                attribute: ATTR_PARAMETER,
                expression: None,
                name1: Some("P1".to_string()),
                name2: None,
                flag: Some(4), // Long
                extra: None,
            },
            QueryRow {
                attribute: ATTR_PARAMETER,
                expression: None,
                name1: Some("P2".to_string()),
                name2: None,
                flag: Some(8), // DateTime
                extra: None,
            },
        ];
        let result = format_parameters(&rows);
        assert_eq!(result, "P1 Long, P2 DateTime");
    }

    // -- get_orderings DESC test -----------------------------------------------

    #[test]
    fn get_orderings_desc() {
        let rows = vec![QueryRow {
            attribute: ATTR_ORDERBY,
            expression: Some("Table1.col1".to_string()),
            name1: Some("D".to_string()),
            name2: None,
            flag: None,
            extra: None,
        }];
        assert_eq!(get_orderings(&rows), "Table1.col1 DESC");
    }

    #[test]
    fn get_orderings_asc() {
        let rows = vec![QueryRow {
            attribute: ATTR_ORDERBY,
            expression: Some("Table1.col1".to_string()),
            name1: None,
            name2: None,
            flag: None,
            extra: None,
        }];
        assert_eq!(get_orderings(&rows), "Table1.col1");
    }

    // -- same_join tests -------------------------------------------------------

    #[test]
    fn same_join_matching_type() {
        let mut ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.a = T2.a".to_string()],
        };
        assert!(ts.same_join(1, "T1.b = T2.b"));
        // New condition should be inserted at front
        if let TableSource::Join { on_conditions, .. } = &ts {
            assert_eq!(on_conditions[0], "T1.b = T2.b");
            assert_eq!(on_conditions[1], "T1.a = T2.a");
        }
    }

    #[test]
    fn same_join_different_type() {
        let mut ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.a = T2.a".to_string()],
        };
        assert!(!ts.same_join(2, "T1.b = T2.b")); // different join type
    }

    #[test]
    fn same_join_on_simple() {
        let mut ts = TableSource::Simple {
            name: "T1".to_string(),
            expr: "T1".to_string(),
        };
        assert!(!ts.same_join(1, "T1.a = T2.a")); // Simple → always false
    }

    // -- unknown JOIN type ---------------------------------------------------

    #[test]
    fn join_unknown_type() {
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 99,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert_eq!(ts.to_sql(true), "T1 JOIN T2 ON T1.id = T2.id");
    }

    // -- sql_append VALUES branch ---------------------------------------------

    #[test]
    fn sql_append_values() {
        let rows = vec![
            QueryRow {
                attribute: ATTR_TYPE,
                expression: None,
                name1: Some("TargetTable".to_string()),
                name2: None,
                flag: Some(3), // Append
                extra: None,
            },
            QueryRow {
                attribute: ATTR_COLUMN,
                expression: Some("42".to_string()),
                name1: None,
                name2: Some("col1".to_string()),
                flag: Some(APPEND_VALUE_FLAG),
                extra: None,
            },
            QueryRow {
                attribute: ATTR_COLUMN,
                expression: Some("'hello'".to_string()),
                name1: None,
                name2: Some("col2".to_string()),
                flag: Some(APPEND_VALUE_FLAG),
                extra: None,
            },
        ];
        let mut builder = String::new();
        sql_append(&mut builder, &rows);
        assert!(builder.contains("INSERT INTO TargetTable"));
        assert!(builder.contains("(col1, col2)"));
        assert!(builder.contains("VALUES (42, 'hello')"));
    }

    // -- query_to_sql OWNER_ACCESS -------------------------------------------

    #[test]
    fn query_to_sql_owner_access() {
        let qdef = QueryDef {
            name: "TestQuery".to_string(),
            query_type: QueryType::Select,
            rows: vec![
                QueryRow {
                    attribute: ATTR_TYPE,
                    expression: None,
                    name1: None,
                    name2: None,
                    flag: Some(1),
                    extra: None,
                },
                QueryRow {
                    attribute: ATTR_FLAG,
                    expression: None,
                    name1: None,
                    name2: None,
                    flag: Some(SELECT_STAR | OWNER_ACCESS),
                    extra: None,
                },
                QueryRow {
                    attribute: ATTR_TABLE,
                    expression: None,
                    name1: Some("Table1".to_string()),
                    name2: None,
                    flag: None,
                    extra: None,
                },
            ],
        };
        let sql = query_to_sql(&qdef);
        assert!(
            sql.contains("WITH OWNERACCESS OPTION"),
            "should contain OWNER_ACCESS: {sql}"
        );
        assert!(sql.contains("*"), "should contain SELECT *: {sql}");
    }

    // -- sql_union integration test ------------------------------------------

    #[test]
    fn sql_union_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "UnionQuery");
        let sql = query_to_sql(q);
        assert!(sql.contains("UNION"), "should contain UNION: {sql}");
    }

    // -- sql_passthrough integration test ------------------------------------

    #[test]
    fn sql_passthrough_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "PassthroughQuery");
        let sql = query_to_sql(q);
        // Passthrough queries don't have semicolons added by query_to_sql
        assert!(!sql.is_empty(), "passthrough query should produce SQL");
    }

    // -- sql_ddl integration test --------------------------------------------

    #[test]
    fn sql_ddl_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "DataDefinitionQuery");
        let sql = query_to_sql(q);
        assert!(!sql.is_empty(), "DDL query should produce SQL");
    }

    // -- build_from_tables with external DB ref and alias ----------------------

    #[test]
    fn build_from_tables_with_alias() {
        let rows = vec![QueryRow {
            attribute: ATTR_TABLE,
            expression: None,
            name1: Some("Table1".to_string()),
            name2: Some("T1".to_string()),
            flag: None,
            extra: None,
        }];
        let result = build_from_tables(&rows);
        assert_eq!(result, vec!["Table1 AS T1"]);
    }

    #[test]
    fn build_from_tables_with_external_db() {
        let rows = vec![QueryRow {
            attribute: ATTR_TABLE,
            expression: Some("C:\\path\\to\\db.mdb".to_string()),
            name1: Some("Table1".to_string()),
            name2: None,
            flag: None,
            extra: None,
        }];
        let result = build_from_tables(&rows);
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("[C:\\path\\to\\db.mdb]"));
        assert!(result[0].contains("Table1"));
    }

    #[test]
    fn build_from_tables_join_missing_name() {
        // JOIN row with missing name1/name2 should be skipped
        let rows = vec![
            QueryRow {
                attribute: ATTR_TABLE,
                expression: None,
                name1: Some("T1".to_string()),
                name2: None,
                flag: None,
                extra: None,
            },
            QueryRow {
                attribute: ATTR_JOIN,
                expression: Some("T1.id = T2.id".to_string()),
                name1: None, // missing from_table
                name2: Some("T2".to_string()),
                flag: Some(1),
                extra: None,
            },
        ];
        let result = build_from_tables(&rows);
        // JOIN is skipped due to missing name1, only T1 remains
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "T1");
    }

    #[test]
    fn build_from_tables_join_both_not_found() {
        // JOIN where neither table is in existing sources
        let rows = vec![QueryRow {
            attribute: ATTR_JOIN,
            expression: Some("A.id = B.id".to_string()),
            name1: Some("A".to_string()),
            name2: Some("B".to_string()),
            flag: Some(1),
            extra: None,
        }];
        let result = build_from_tables(&rows);
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("INNER JOIN"));
    }

    // -- contains_table -------------------------------------------------------

    #[test]
    fn contains_table_simple() {
        let ts = TableSource::Simple {
            name: "Table1".to_string(),
            expr: "Table1".to_string(),
        };
        assert!(ts.contains_table("Table1"));
        assert!(ts.contains_table("table1")); // case insensitive
        assert!(!ts.contains_table("Table2"));
    }

    #[test]
    fn contains_table_nested_join() {
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert!(ts.contains_table("T1"));
        assert!(ts.contains_table("T2"));
        assert!(!ts.contains_table("T3"));
    }

    // -- to_alias / quoting edge cases ----------------------------------------

    #[test]
    fn to_alias_none() {
        assert_eq!(to_alias(None), "");
    }

    #[test]
    fn to_alias_some() {
        assert_eq!(to_alias(Some("Alias1")), " AS Alias1");
    }

    #[test]
    fn is_quoted_true() {
        assert!(is_quoted("[Table1]"));
    }

    #[test]
    fn is_quoted_false() {
        assert!(!is_quoted("Table1"));
        assert!(!is_quoted("[T"));
        assert!(!is_quoted("T]"));
        assert!(!is_quoted(""));
    }

    #[test]
    fn needs_quoting_simple() {
        assert!(!needs_quoting("Table1"));
        assert!(needs_quoting("My Table"));
        assert!(needs_quoting("col-1"));
    }

    // -- sql_select with no columns and no tables ---------------------------

    #[test]
    fn sql_select_empty() {
        let rows = vec![QueryRow {
            attribute: ATTR_FLAG,
            expression: None,
            name1: None,
            name2: None,
            flag: Some(0),
            extra: None,
        }];
        let mut builder = String::new();
        sql_select(&mut builder, &rows);
        assert!(builder.starts_with("SELECT "));
    }

    #[test]
    fn read_queries_form_prop_test() {
        let path = skip_if_missing!("formPropTest.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        // 3 queries in catalog, but ~sq_fF_Table1 is an embedded query (skipped)
        assert_eq!(
            queries.len(),
            2,
            "formPropTest.accdb should have 2 user queries"
        );
        for q in &queries {
            assert!(
                !q.name.starts_with("~sq_"),
                "embedded query should be filtered: {}",
                q.name
            );
            // Both lack ATTR_TYPE, so they default to Select
            assert_eq!(
                q.query_type,
                QueryType::Select,
                "query '{}' should default to Select",
                q.name
            );
        }

        // Verify Japanese query name and SQL content
        let jp_query = queries.iter().find(|q| q.name == "jp_クエリ_02");
        assert!(
            jp_query.is_some(),
            "should contain jp_クエリ_02, found: {:?}",
            queries.iter().map(|q| &q.name).collect::<Vec<_>>()
        );
        let sql = query_to_sql(jp_query.unwrap());
        assert!(
            sql.contains("jp_テーブル2"),
            "jp_クエリ_02 SQL should reference jp_テーブル2, got: {sql}"
        );
    }

    #[test]
    fn read_queries_jet3() {
        let path = skip_if_missing!("V1997/queryTestV1997.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        assert!(
            queries.len() >= 9,
            "queryTestV1997.mdb should have at least 9 queries, got {}",
            queries.len()
        );
        let names: Vec<&str> = queries.iter().map(|q| q.name.as_str()).collect();
        assert!(names.contains(&"SelectQuery"), "should contain SelectQuery");
        assert!(names.contains(&"UnionQuery"), "should contain UnionQuery");
        for q in &queries {
            let expected = match q.name.as_str() {
                "SelectQuery" => QueryType::Select,
                "DeleteQuery" => QueryType::Delete,
                "UpdateQuery" => QueryType::Update,
                "AppendQuery" => QueryType::Append,
                "MakeTableQuery" => QueryType::MakeTable,
                "CrosstabQuery" => QueryType::Crosstab,
                "UnionQuery" => QueryType::Union,
                "PassthroughQuery" => QueryType::Passthrough,
                "DataDefinitionQuery" => QueryType::Ddl,
                other => panic!("unexpected query name: {other}"),
            };
            assert_eq!(q.query_type, expected, "type mismatch for {}", q.name);
        }
    }

    #[test]
    fn read_queries_ace14() {
        let path = skip_if_missing!("V2010/queryTestV2010.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        assert!(
            queries.len() >= 9,
            "queryTestV2010.accdb should have at least 9 queries, got {}",
            queries.len()
        );
        let names: Vec<&str> = queries.iter().map(|q| q.name.as_str()).collect();
        assert!(names.contains(&"SelectQuery"), "should contain SelectQuery");
        assert!(names.contains(&"UnionQuery"), "should contain UnionQuery");
        for q in &queries {
            let expected = match q.name.as_str() {
                "SelectQuery" => QueryType::Select,
                "DeleteQuery" => QueryType::Delete,
                "UpdateQuery" => QueryType::Update,
                "AppendQuery" => QueryType::Append,
                "MakeTableQuery" => QueryType::MakeTable,
                "CrosstabQuery" => QueryType::Crosstab,
                "UnionQuery" => QueryType::Union,
                "PassthroughQuery" => QueryType::Passthrough,
                "DataDefinitionQuery" => QueryType::Ddl,
                other => panic!("unexpected query name: {other}"),
            };
            assert_eq!(q.query_type, expected, "type mismatch for {}", q.name);
        }
    }

    #[test]
    fn read_queries_v2007() {
        let path = skip_if_missing!("V2007/queryTestV2007.accdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        assert!(
            queries.len() >= 9,
            "queryTestV2007.accdb should have at least 9 queries, got {}",
            queries.len()
        );
        for q in &queries {
            let expected = match q.name.as_str() {
                "SelectQuery" => QueryType::Select,
                "DeleteQuery" => QueryType::Delete,
                "UpdateQuery" => QueryType::Update,
                "AppendQuery" => QueryType::Append,
                "MakeTableQuery" => QueryType::MakeTable,
                "CrosstabQuery" => QueryType::Crosstab,
                "UnionQuery" => QueryType::Union,
                "PassthroughQuery" => QueryType::Passthrough,
                "DataDefinitionQuery" => QueryType::Ddl,
                other => panic!("unexpected query name: {other}"),
            };
            assert_eq!(q.query_type, expected, "type mismatch for {}", q.name);
        }
    }
}
