//! Names used by the pinned JSON-UI pack, separated by their role.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
};

use serde_json::Value;

use crate::vocabulary::{json_files, read};

type Names = BTreeSet<(&'static str, String)>;
type Bindings = BTreeMap<String, BTreeSet<String>>;

/// Properties whose string value names a data collection.
const COLLECTION_KEYS: [&str; 3] = [
    "collection_name",
    "binding_collection_name",
    "toggle_grid_collection_name",
];

/// Generates sorted, names-only rows from UI definitions and English text keys.
pub fn generate(samples: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let pack = samples.join("resource_pack");
    let mut names = Names::new();
    let mut bindings = Bindings::new();
    let mut collections = BTreeSet::new();
    for path in json_files(&pack.join("ui"))? {
        let document = read(&path)?;
        if let Some(namespace) = document.get("namespace").and_then(Value::as_str)
            && let Some(definitions) = document.as_object()
        {
            for (key, value) in definitions {
                if value.is_object() {
                    let name = control_name(key);
                    names.insert(("reference", format!("{namespace}.{name}")));
                    declared_control(key, value, &mut names);
                }
            }
        }
        collect(&document, &mut names, &mut bindings, &mut collections);
    }
    resolve_collections(&bindings, collections, &mut names);
    for line in fs::read_to_string(pack.join("texts/en_US.lang"))?.lines() {
        if !line.trim_start().starts_with('#')
            && let Some((key, _)) = line.split_once('=')
            && !key.trim().is_empty()
        {
            names.insert(("text", key.trim().to_owned()));
        }
    }
    Ok(names
        .into_iter()
        .map(|(kind, name)| format!("{kind}\t{name}\n"))
        .collect::<String>()
        .into_bytes())
}

/// Collects declarations and expression symbols without treating comments as names.
fn collect(
    value: &Value,
    names: &mut Names,
    bindings: &mut Bindings,
    collections: &mut BTreeSet<String>,
) {
    match value {
        Value::String(text) => symbols(text, names),
        Value::Array(items) => {
            for item in items {
                collect(item, names, bindings, collections);
            }
        }
        Value::Object(properties) => {
            for (key, value) in properties {
                symbols(key, names);
                if let Value::String(name) = value
                    && key.starts_with('$')
                {
                    let variable = variable(key);
                    bindings
                        .entry(variable.to_owned())
                        .or_default()
                        .insert(name.clone());
                }
                match (key.as_str(), value) {
                    ("renderer", Value::String(name)) => insert("renderer", name, names),
                    (key, Value::String(name)) if COLLECTION_KEYS.contains(&key) => {
                        insert("collection", name, names);
                        if name.starts_with('$') {
                            collections.insert(variable(name).to_owned());
                        }
                    }
                    ("controls", Value::Array(controls)) => {
                        for control in controls {
                            if let Some(entries) = control.as_object() {
                                for (key, value) in entries {
                                    declared_control(key, value, names);
                                }
                            }
                        }
                    }
                    ("control_ids", Value::Object(ids)) => {
                        for id in ids.keys() {
                            insert("factory", id, names);
                        }
                    }
                    _ => {}
                }
                collect(value, names, bindings, collections);
            }
        }
        _ => {}
    }
}

/// Follows collection-referenced variables, including defaults and aliases across files.
/// A visited set ends cycles; only identifier literals become resolved collection names.
fn resolve_collections(bindings: &Bindings, mut pending: BTreeSet<String>, names: &mut Names) {
    let mut visited = BTreeSet::new();
    while let Some(reference) = pending.pop_first() {
        if !visited.insert(reference.clone()) {
            continue;
        }
        for name in bindings.get(&reference).into_iter().flatten() {
            if name.starts_with('$') {
                pending.insert(variable(name).to_owned());
            } else if collection_literal(name) {
                insert("collection", name, names);
            }
        }
    }
}

/// Accepts static collection identifiers, excluding bindings and unevaluated expressions.
fn collection_literal(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(identifier_byte)
}

/// Removes a variable's `|default` suffix.
fn variable(text: &str) -> &str {
    text.split_once('|').map_or(text, |(name, _)| name)
}

/// Records a control's local name and, for a factory, its instance name.
fn declared_control(key: &str, value: &Value, names: &mut Names) {
    let name = control_name(key);
    insert("control", name, names);
    if value.get("type").and_then(Value::as_str) == Some("factory") {
        insert("factory", name, names);
    }
}

/// Removes inheritance from a control declaration.
fn control_name(key: &str) -> &str {
    key.split_once('@').map_or(key, |(name, _)| name)
}

/// Records literal names; unresolved variable references are collected separately.
fn insert(kind: &'static str, name: &str, names: &mut Names) {
    if !name.is_empty() && !name.starts_with('$') {
        names.insert((kind, name.to_owned()));
    }
}

/// Extracts complete binding, variable and button tokens, including inside expressions.
fn symbols(text: &str, names: &mut Names) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let boundary = start == 0 || !identifier_byte(bytes[start - 1]);
        let (kind, prefix) = if boundary && matches!(bytes[start], b'#' | b'$') {
            (
                if bytes[start] == b'#' {
                    "binding"
                } else {
                    "variable"
                },
                1,
            )
        } else if boundary && text[start..].starts_with("button.") {
            ("button", "button.".len())
        } else {
            index += text[start..].chars().next().unwrap().len_utf8();
            continue;
        };
        index += prefix;
        if index < bytes.len() && (bytes[index].is_ascii_alphabetic() || bytes[index] == b'_') {
            while index < bytes.len() && identifier_byte(bytes[index]) {
                index += 1;
            }
            names.insert((kind, text[start..index].to_owned()));
        }
    }
}

/// Tests the ASCII characters permitted in symbolic UI names.
fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn ui_vocabulary_has_the_pinned_projection_fingerprint() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let bytes =
            fs::read(root.join("crates/json-ui/data/vanilla-ui-vocabulary-1.26.50.txt")).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            "e3e48bf66a8e23d5efa40cbcb4ee63b2b42322ee61e204b437b017eb542446e6"
        );
    }

    #[test]
    fn ui_vocabulary_regenerates_from_the_pinned_archive() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = assets::vanilla_source();
        let paths = source.local_paths(&root).unwrap();
        if !paths.archive.is_file() {
            eprintln!(
                "skipping UI vocabulary regeneration: missing pinned archive {}",
                paths.archive.display()
            );
            return;
        }
        assert_eq!(
            assets::vanilla_pack::sha256_file(&paths.archive).unwrap(),
            source.sha256.as_ref()
        );
        let scratch = tempfile::tempdir().unwrap();
        let isolated = source.local_paths(scratch.path()).unwrap();
        fs::create_dir_all(isolated.archive.parent().unwrap()).unwrap();
        fs::copy(&paths.archive, &isolated.archive).unwrap();
        assets::vanilla_pack::unpack(
            &isolated,
            &assets::vanilla_pack::UnpackLimits::PINNED,
            &|| false,
        )
        .unwrap();
        assert_eq!(
            generate(&isolated.cache).unwrap(),
            fs::read(root.join("crates/json-ui/data/vanilla-ui-vocabulary-1.26.50.txt")).unwrap()
        );
    }

    #[test]
    fn collection_names_assigned_through_variables_are_collections() {
        let document = serde_json::json!({
            "grid": { "collection_name": "$grid_name", "$grid_name|default": "$item_name" },
            "cell": { "$item_name": "trade_tiers" },
            "label": { "$text_name": "not_a_collection" },
        });
        let mut names = Names::new();
        let mut bindings = Bindings::new();
        let mut collections = BTreeSet::new();
        collect(&document, &mut names, &mut bindings, &mut collections);
        names.retain(|(kind, _)| *kind == "collection");
        resolve_collections(&bindings, collections, &mut names);
        assert_eq!(
            names,
            BTreeSet::from([("collection", "trade_tiers".to_owned())])
        );
    }

    #[test]
    fn expression_tokens_preserve_names_and_ignore_default_suffixes() {
        let mut names = Names::new();
        symbols(
            "($desktop_screen and (#form_button_text = 'é'))",
            &mut names,
        );
        symbols("$desktop_screen|default", &mut names);
        symbols("button.menu_cancel", &mut names);
        assert_eq!(
            names,
            BTreeSet::from([
                ("variable", "$desktop_screen".to_owned()),
                ("binding", "#form_button_text".to_owned()),
                ("button", "button.menu_cancel".to_owned()),
            ])
        );
    }

    #[test]
    fn collection_variables_resolve_across_files_without_admitting_unrelated_strings() {
        let scratch = tempfile::tempdir().unwrap();
        let pack = scratch.path().join("resource_pack");
        fs::create_dir_all(pack.join("ui")).unwrap();
        fs::create_dir_all(pack.join("texts")).unwrap();
        fs::write(pack.join("texts/en_US.lang"), "").unwrap();
        fs::write(
            pack.join("ui/definitions.json"),
            r##"{
                "$rows|default": "outer_rows",
                "$alias": "$rows",
                "$unused": "unrelated_text",
                "$cycle_a": "$cycle_b",
                "$cycle_b": "$cycle_a",
                "$dynamic_alias": "$dynamic",
                "$dynamic": "#dynamic_rows",
                "$expression_alias": "$expression",
                "$expression": "($prefix + '_rows')",
                "$numeric_start": "123_rows",
                "$punctuated": "row.names"
            }"##,
        )
        .unwrap();
        fs::write(
            pack.join("ui/screen.json"),
            r##"{
                "collection_name": "$alias",
                "binding_collection_name": "$cycle_a",
                "controls": [
                    {"dynamic": {"collection_name": "$dynamic_alias"}},
                    {"expression": {"collection_name": "$expression_alias"}},
                    {"numeric_start": {"collection_name": "$numeric_start"}},
                    {"punctuated": {"collection_name": "$punctuated"}},
                    {"direct": {"collection_name": "#direct_rows"}},
                    {"direct_expression": {
                        "collection_name": "($prefix + '_direct_rows')"
                    }}
                ]
            }"##,
        )
        .unwrap();
        let output = String::from_utf8(generate(scratch.path()).unwrap()).unwrap();
        assert_eq!(
            output
                .lines()
                .filter(|line| line.starts_with("collection\t"))
                .collect::<Vec<_>>(),
            [
                "collection\t#direct_rows",
                "collection\t($prefix + '_direct_rows')",
                "collection\touter_rows",
            ]
        );
    }
}
