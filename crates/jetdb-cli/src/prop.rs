use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use jetdb::format::ObjectType;
use jetdb::{
    read_catalog, read_object_properties_of_type, ObjectProperties, PageReader, PropMapType, Value,
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
    match run_prop(&args, password) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_prop(args: &PropArgs, password: Option<&str>) -> Result<(), String> {
    let mut reader =
        PageReader::open_with_password(&args.file, password).map_err(|e| e.to_string())?;
    let object_type = match &args.object_type {
        Some(name) => Some(object_type_named(name)),
        None => only_type_of(&mut reader, &args.object_name)?,
    };
    let Some(object_type) = object_type else {
        // No object has the name: nothing to print.
        return Ok(());
    };
    let props = read_object_properties_of_type(&mut reader, &args.object_name, object_type)
        .map_err(|e| e.to_string())?;
    print_properties(&props, object_type);
    Ok(())
}

/// The object type of a name `--type` takes.
fn object_type_named(name: &str) -> ObjectType {
    ObjectType::ALL
        .into_iter()
        .find(|&t| object_type_name(t) == name)
        .expect("clap accepts only the type names")
}

/// Prints the properties of an object of `object_type`, nothing when it has
/// none.
fn print_properties(props: &ObjectProperties, object_type: ObjectType) {
    if props.maps.is_empty() {
        return;
    }

    println!("Object: {}", props.object_name);

    for map in &props.maps {
        if map.properties.is_empty() {
            continue;
        }

        println!();
        match map.map_type {
            PropMapType::Default => {
                println!("  {object_type} Properties:");
            }
            PropMapType::Column => {
                println!("  Column: {}", map.name);
            }
            PropMapType::Additional => {
                if map.name.is_empty() {
                    println!("  Additional Properties:");
                } else {
                    println!("  Additional: {}", map.name);
                }
            }
        }

        // Calculate name width for alignment
        let name_width = map
            .properties
            .iter()
            .map(|p| p.name.len())
            .max()
            .unwrap_or(0);

        for prop in &map.properties {
            let display_value = format_value(&prop.value);
            println!(
                "    {:<width$}  {display_value}",
                prop.name,
                width = name_width
            );
        }
    }
}

/// The type of the object named `name`, `None` when there is none, or an
/// error naming the types when objects of several types share the name.
fn only_type_of(reader: &mut PageReader, name: &str) -> Result<Option<ObjectType>, String> {
    let catalog = read_catalog(reader).map_err(|e| e.to_string())?;
    let types: Vec<ObjectType> = catalog
        .iter()
        .filter(|e| e.name == name)
        .map(|e| e.object_type)
        .collect();
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
        Value::Text(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Binary(b) => format!("({} bytes)", b.len()),
        Value::Money(s) => s.clone(),
        Value::Numeric(s) => s.clone(),
        Value::Timestamp(ts) => jetdb::timestamp::format_default(*ts),
        Value::Guid(s) => s.clone(),
        Value::DateTimeExtended(s) => s.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
