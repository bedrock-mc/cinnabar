//! Names used by the pinned JSON-UI pack, separated by their role.

use std::{collections::BTreeSet, error::Error, fs, path::Path};

use serde_json::Value;

use crate::vocabulary::{json_files, read};

type Names = BTreeSet<(&'static str, String)>;

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
    let mut flow = CollectionFlow::default();
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
        collect(&document, &mut names);
        flow.visit(&document);
    }
    flow.insert_into(&mut names);
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
fn collect(value: &Value, names: &mut Names) {
    match value {
        Value::String(text) => symbols(text, names),
        Value::Array(items) => {
            for item in items {
                collect(item, names);
            }
        }
        Value::Object(properties) => {
            for (key, value) in properties {
                symbols(key, names);
                match (key.as_str(), value) {
                    ("renderer", Value::String(name)) => insert("renderer", name, names),
                    (key, Value::String(name)) if COLLECTION_KEYS.contains(&key) => {
                        insert("collection", name, names);
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
                collect(value, names);
            }
        }
        _ => {}
    }
}

/// Collection names that reach a collection property only through variables, which pack
/// scopes resolve by name; the flow is tracked pack-wide.
#[derive(Default)]
struct CollectionFlow {
    /// Variables read by a collection property, directly or through other variables.
    sinks: BTreeSet<String>,
    /// Every `"$variable": "value"` assignment.
    assignments: BTreeSet<(String, String)>,
}

impl CollectionFlow {
    fn visit(&mut self, value: &Value) {
        match value {
            Value::Array(items) => items.iter().for_each(|item| self.visit(item)),
            Value::Object(properties) => {
                for (key, value) in properties {
                    if let Value::String(text) = value {
                        if COLLECTION_KEYS.contains(&key.as_str()) && text.starts_with('$') {
                            self.sinks.insert(variable(text).to_owned());
                        } else if key.starts_with('$') {
                            self.assignments
                                .insert((variable(key).to_owned(), variable(text).to_owned()));
                        }
                    }
                    self.visit(value);
                }
            }
            _ => {}
        }
    }

    /// Records the literal values of every variable that reaches a collection property.
    fn insert_into(mut self, names: &mut Names) {
        loop {
            let reached: Vec<_> = self
                .assignments
                .iter()
                .filter(|(name, value)| {
                    self.sinks.contains(name)
                        && value.starts_with('$')
                        && !self.sinks.contains(value)
                })
                .map(|(_, value)| value.clone())
                .collect();
            if reached.is_empty() {
                break;
            }
            self.sinks.extend(reached);
        }
        for (name, value) in &self.assignments {
            if self.sinks.contains(name) {
                insert("collection", value, names);
            }
        }
    }
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
            "f0074eb0250feed93d9d1f4697e20478c5bcb3a97bb8abdef98818c187408ab1"
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
        let mut flow = CollectionFlow::default();
        flow.visit(&document);
        let mut names = Names::new();
        flow.insert_into(&mut names);
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
}
