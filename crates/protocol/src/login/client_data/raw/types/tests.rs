use super::*;
use crate::login::client_data::raw::{RawClientData, schema_generated};

/// The compiled descriptor must identify the actual checked-in Go replacement and checksum.
#[test]
fn generated_schema_matches_the_active_go_source() {
    use schema_generated::{SOURCE_IMPORT_MODULE, SOURCE_MODULE, SOURCE_SUM, SOURCE_VERSION};
    let module = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../core/go.mod"));
    let replacements = replacements(module);
    let selected: Vec<_> = replacements
        .into_iter()
        .filter(|row| row[0] == SOURCE_IMPORT_MODULE)
        .collect();
    assert_eq!(
        selected,
        [[SOURCE_IMPORT_MODULE, SOURCE_MODULE, SOURCE_VERSION]]
    );
    let sums = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../core/go.sum"));
    assert_eq!(
        sums.lines()
            .filter(|line| line
                .split_whitespace()
                .eq([SOURCE_MODULE, SOURCE_VERSION, SOURCE_SUM]))
            .count(),
        1,
        "generated client-data source checksum differs from core/go.sum"
    );
}

/// Parses only identified versioned replacements; unfamiliar syntax requires an explicit review.
fn replacements(module: &str) -> Vec<[&str; 3]> {
    let mut rows = Vec::new();
    let mut block = false;
    for line in module.lines() {
        let line = line.split_once("//").map_or(line, |(code, _)| code);
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        let declaration = if block {
            if fields == [")"] {
                block = false;
                continue;
            }
            fields.as_slice()
        } else {
            if fields[0] != "replace" {
                continue;
            }
            if fields[1..] == ["("] {
                block = true;
                continue;
            }
            &fields[1..]
        };
        assert_eq!(declaration.len(), 4, "unsupported Go replacement: {line}");
        assert_eq!(declaration[1], "=>", "unsupported Go replacement: {line}");
        assert!(
            !declaration
                .iter()
                .any(|field| field.contains(['"', '`', '(', ')'])),
            "unsupported Go replacement: {line}"
        );
        rows.push([declaration[0], declaration[2], declaration[3]]);
    }
    assert!(!block, "unterminated Go replacement block");
    rows
}

/// Every generated known field reaches the real raw owner, with no second field-name catalog.
#[test]
fn generated_field_kinds_are_enforced_at_the_raw_boundary() {
    let records = std::iter::once((Record::Client, None)).chain(
        Record::Client.fields().iter().filter_map(|field| {
            if let Kind::Slice(record) = field.kind {
                Some((record, Some(field.name)))
            } else {
                None
            }
        }),
    );
    for (record, parent) in records {
        for field in record.fields() {
            let valid = match field.kind {
                Kind::String => r#""typed""#,
                Kind::Integer(bits) => {
                    assert_eq!(
                        u32::from(bits),
                        i64::BITS,
                        "review changed Go integer width"
                    );
                    "-0"
                }
                Kind::Bool | Kind::OptionalBool => "true",
                Kind::Float64 => "1.5",
                Kind::Slice(_) | Kind::Strings(_) => "[]",
            };
            for value in [valid, "null"] {
                let raw = object(field.name, value, parent);
                assert!(
                    RawClientData::from_raw(&raw).is_ok(),
                    "rejected valid {record:?}.{}: {value}",
                    field.name
                );
            }
            let raw = object(field.name, "{}", parent);
            assert!(
                RawClientData::from_raw(&raw).is_err(),
                "accepted object for {record:?}.{}",
                field.name
            );
        }
    }
}

/// Wraps one generated child field through its actual root record-slice field when needed.
fn object(name: &str, value: &str, parent: Option<&str>) -> Box<RawValue> {
    let entry = format!("{{{}:{value}}}", serde_json::to_string(name).unwrap());
    let object = match parent {
        Some(name) => format!("{{{}:[{entry}]}}", serde_json::to_string(name).unwrap()),
        None => entry,
    };
    RawValue::from_string(object).unwrap()
}
