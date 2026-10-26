use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use jetdb::format::ObjectType;
use jetdb::{
    read_catalog, read_object_properties, read_object_properties_of_type, CatalogEntry,
    ObjectProperties, PageReader, PropMapType, PropertyMap, Value,
};

use crate::object_type_name;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct PropArgs {
    /// Database file path (.mdb / .accdb)
    pub file: PathBuf,
    /// Object name (table, query, etc.)
    pub object_name: String,

    /// Object type, needed when objects of different types share the name
    #[arg(short = 't', long = "type", value_parser = clap::builder::PossibleValuesParser::new(ObjectType::ALL.map(object_type_name)))]
    pub object_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Command entry point
// ---------------------------------------------------------------------------

pub fn cmd_prop(args: PropArgs, password: Option<&str>) -> ExitCode {
    crate::exit_code(run_prop(&args, password))
}

fn run_prop(args: &PropArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let object_type = object_type_of(&mut reader, args)?;
    // Without a type, the object may be of a type the catalog leaves out:
    // read it by its name alone.
    let props = match object_type {
        Some(t) => read_object_properties_of_type(&mut reader, &args.object_name, t),
        None => read_object_properties(&mut reader, &args.object_name),
    }
    .map_err(|e| e.to_string())?;
    print_properties(&props, object_type);
    Ok(())
}

/// The type of the object of `prop`: the one `--type` names, or the only
/// one of that name in the catalog, or `None` when there is none.
fn object_type_of(reader: &mut PageReader, args: &PropArgs) -> Result<Option<ObjectType>, String> {
    match &args.object_type {
        Some(name) => Ok(Some(object_type_named(name))),
        None => {
            let catalog = read_catalog(reader).map_err(|e| e.to_string())?;
            only_type_of(&catalog, &args.object_name)
        }
    }
}

/// The object type of a name `--type` takes.
fn object_type_named(name: &str) -> ObjectType {
    ObjectType::ALL
        .into_iter()
        .find(|&t| object_type_name(t) == name)
        .expect("clap accepts only the type names")
}

/// Prints the properties of an object of `object_type`, nothing when it has
/// none. An object of a type the catalog leaves out has none given.
fn print_properties(props: &ObjectProperties, object_type: Option<ObjectType>) {
    if props.maps.is_empty() {
        return;
    }

    println!("Object: {}", props.object_name);

    // The object's own properties first, then the others in the order they
    // are stored: the maps of the columns can come before or after them.
    let mut maps: Vec<&PropertyMap> = props
        .maps
        .iter()
        .filter(|m| !m.properties.is_empty())
        .collect();
    maps.sort_by_key(|m| m.map_type != PropMapType::Default);
    for map in maps {
        println!();
        println!("  {}", map_heading(map, object_type));
        print_map(map);
    }
}

/// The heading of the properties of `map`, of an object of `object_type`.
fn map_heading(map: &PropertyMap, object_type: Option<ObjectType>) -> String {
    match map.map_type {
        PropMapType::Default => match object_type {
            Some(t) => format!("{t} Properties:"),
            None => "Object Properties:".to_string(),
        },
        PropMapType::Column => format!("Column: {}", map.name),
        PropMapType::Additional if map.name.is_empty() => "Additional Properties:".to_string(),
        PropMapType::Additional => format!("Additional: {}", map.name),
        _ => format!("{:?}: {}", map.map_type, map.name),
    }
}

/// Print the properties of `map`, their names aligned.
fn print_map(map: &PropertyMap) {
    // Calculate name width for alignment
    let name_width = map
        .properties
        .iter()
        .map(|p| unicode_width::UnicodeWidthStr::width(p.name.as_str()))
        .max()
        .unwrap_or(0);

    for prop in &map.properties {
        let display_value = format_value(&prop.value);
        println!(
            "    {}  {display_value}",
            crate::padded(&prop.name, name_width)
        );
    }
}

/// The type of the object named `name` in `catalog`, `None` when there is
/// none, or an error naming the types when objects of several types share
/// the name. Rows of the same type are one type.
fn only_type_of(catalog: &[CatalogEntry], name: &str) -> Result<Option<ObjectType>, String> {
    let mut types: Vec<ObjectType> = Vec::new();
    for e in catalog.iter().filter(|e| e.name == name) {
        if !types.contains(&e.object_type) {
            types.push(e.object_type);
        }
    }
    match types.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(*only)),
        several => {
            let names: Vec<&str> = several.iter().map(|&t| object_type_name(t)).collect();
            Err(format!(
                "objects of several types are named {name}: {}; choose one with --type",
                names.join(", ")
            ))
        }
    }
}

/// Format a property value for display.
fn format_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
        Value::Byte(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::Long(v) => v.to_string(),
        Value::BigInt(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::Double(v) => v.to_string(),
        Value::Text(s) => crate::quoted(s),
        Value::Binary(b) => format!("({} bytes)", b.len()),
        Value::Money(s) => s.clone(),
        Value::Numeric(s) => s.clone(),
        Value::Timestamp(ts) => jetdb::timestamp::format_default(*ts),
        Value::Guid(s) => s.clone(),
        Value::DateTimeExtended(s) => s.clone(),
        _ => format!("{value:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_type_of_rows_of_the_same_type() {
        let entry = |object_type| CatalogEntry {
            name: "Customers".to_string(),
            object_type,
            table_page: 0,
            flags: 0,
            lv_prop: None,
            lv_extra: None,
        };
        let twice = [entry(ObjectType::Table), entry(ObjectType::Table)];
        assert_eq!(
            only_type_of(&twice, "Customers"),
            Ok(Some(ObjectType::Table))
        );
        assert_eq!(only_type_of(&twice, "Orders"), Ok(None));
        let several = [
            entry(ObjectType::Table),
            entry(ObjectType::Form),
            entry(ObjectType::Table),
        ];
        assert_eq!(
            only_type_of(&several, "Customers"),
            Err(
                "objects of several types are named Customers: table, form; choose one with --type"
                    .to_string()
            )
        );
    }

    #[test]
    fn format_value_bool_true() {
        assert_eq!(format_value(&Value::Bool(true)), "yes");
    }

    #[test]
    fn format_value_bool_false() {
        assert_eq!(format_value(&Value::Bool(false)), "no");
    }

    #[test]
    fn format_value_text_quoted() {
        assert_eq!(format_value(&Value::Text("hello".to_string())), "\"hello\"");
    }

    #[test]
    fn format_value_binary_size() {
        assert_eq!(format_value(&Value::Binary(vec![0; 42])), "(42 bytes)");
    }

    #[test]
    fn format_value_long() {
        assert_eq!(format_value(&Value::Long(123)), "123");
    }

    #[test]
    fn format_value_null() {
        assert_eq!(format_value(&Value::Null), "null");
    }

    #[test]
    fn format_value_guid() {
        let guid = "{04030201-0605-0807-090A-0B0C0D0E0F10}".to_string();
        assert_eq!(format_value(&Value::Guid(guid.clone())), guid);
    }

    #[test]
    fn format_value_timestamp_date_only() {
        assert_eq!(format_value(&Value::Timestamp(37623.0)), "2003-01-02");
    }

    #[test]
    fn format_value_timestamp_datetime() {
        assert_eq!(
            format_value(&Value::Timestamp(37623.5)),
            "2003-01-02 12:00:00"
        );
    }

    #[test]
    fn format_value_text_with_quotes() {
        assert_eq!(
            format_value(&Value::Text("say \"hello\"".to_string())),
            "\"say \\\"hello\\\"\""
        );
    }

    #[test]
    fn format_value_text_with_backslash() {
        assert_eq!(format_value(&Value::Text("a\\b".to_string())), "\"a\\\\b\"");
    }

    #[test]
    fn format_value_text_with_line_breaks() {
        assert_eq!(
            format_value(&Value::Text("a\r\nb\tc\u{1}".to_string())),
            "\"a\\r\\nb\\tc\\u{1}\""
        );
    }

    #[test]
    fn format_value_bigint() {
        assert_eq!(format_value(&Value::BigInt(42)), "42");
        assert_eq!(format_value(&Value::BigInt(-1)), "-1");
    }

    #[test]
    fn format_value_float() {
        assert_eq!(format_value(&Value::Float(1.5)), "1.5");
    }

    #[test]
    fn format_value_double() {
        assert_eq!(format_value(&Value::Double(3.125)), "3.125");
    }

    #[test]
    fn format_value_byte() {
        assert_eq!(format_value(&Value::Byte(255)), "255");
    }

    #[test]
    fn format_value_int() {
        assert_eq!(format_value(&Value::Int(-42)), "-42");
    }

    #[test]
    fn format_value_money() {
        assert_eq!(format_value(&Value::Money("1.0000".to_string())), "1.0000");
    }

    #[test]
    fn format_value_numeric() {
        assert_eq!(
            format_value(&Value::Numeric("123.45".to_string())),
            "123.45"
        );
    }
}
