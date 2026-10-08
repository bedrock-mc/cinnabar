//! Catalogs built from indexed in-memory JSON definition bytes (the compiled carrier) and the
//! resource-pack overlay a joined server applies on top. An overlay control with
//! the same namespace and name as an existing one merges into it: each property
//! it names replaces the old value, a `controls` array replaces the children, and
//! `modifications` edits arrays in place (insert/remove/replace/move/swap by
//! control name for `controls`, by a `where` field match for other arrays). A
//! control the base lacks is simply added. Overlay `_global_variables.json`
//! values override the base's.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::catalog::{Catalog, LoadError, RawControl, child_controls, parse_ui_defs, split_key};
use crate::json5;

const GLOBALS: &str = "ui/_global_variables.json";
const UI_DEFS: &str = "ui/_ui_defs.json";

impl Catalog {
    /// Pack-relative definition paths named by a comment-tolerant `ui/_ui_defs.json` index.
    /// The index, rather than a directory or extension, selects UI documents.
    pub fn declared_paths(bytes: &[u8]) -> Result<Vec<String>, LoadError> {
        parse_ui_defs(Path::new(UI_DEFS), &String::from_utf8_lossy(bytes))
    }

    /// Load a base catalog from `(pack-relative path, bytes)` pairs, honouring the
    /// `ui/_ui_defs.json` load order. Both index files are required.
    pub fn from_files<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Result<Self, LoadError> {
        let files: BTreeMap<&str, &[u8]> = files.into_iter().collect();
        let text = |path: &str| -> Result<String, LoadError> {
            let bytes = files.get(path).ok_or_else(|| LoadError::Shape {
                path: Path::new(path).to_path_buf(),
            })?;
            Ok(String::from_utf8_lossy(bytes).into_owned())
        };
        let mut catalog = Catalog::default();
        catalog.load_globals_text(Path::new(GLOBALS), &text(GLOBALS)?)?;
        let entries = parse_ui_defs(Path::new(UI_DEFS), &text(UI_DEFS)?)?;
        catalog.list(entries.iter().cloned());
        for entry in entries {
            match files.get(entry.as_str()) {
                Some(bytes) => catalog.load_text(&entry, &String::from_utf8_lossy(bytes)),
                None => catalog.note(format!("{entry}: listed but absent")),
            }
        }
        Ok(catalog)
    }

    /// Overlay a resource pack's ui files (pack-relative paths). As the vanilla
    /// client does, only paths some `_ui_defs.json` lists load, in sorted order;
    /// first-file partial values are retained and invalid later files are skipped.
    pub fn apply_pack<'a>(&mut self, files: impl IntoIterator<Item = (&'a str, &'a [u8])>) {
        let files: BTreeMap<&str, &[u8]> = files.into_iter().collect();
        if let Some(bytes) = files.get(GLOBALS)
            && let Err(error) =
                self.load_globals_text(Path::new(GLOBALS), &String::from_utf8_lossy(bytes))
        {
            self.note(format!("pack {GLOBALS}: {error}"));
        }
        if let Some(bytes) = files.get(UI_DEFS) {
            match Self::declared_paths(bytes) {
                Ok(entries) => self.list(entries),
                Err(error) => self.note(format!("pack {UI_DEFS}: {error}")),
            }
        }
        for (path, bytes) in files {
            if path == GLOBALS || path == UI_DEFS {
                continue;
            }
            if self.lists(path) {
                self.merge_overlay_file(path, &String::from_utf8_lossy(bytes));
            } else {
                self.note(format!(
                    "pack {path}: not listed in any _ui_defs.json; skipped"
                ));
            }
        }
    }

    /// The namespaces a pack's ui files define or extend (a file may omit the
    /// namespace of the vanilla file it overrides).
    pub fn overlay_namespaces<'a>(
        &self,
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> std::collections::BTreeSet<String> {
        let mut namespaces = std::collections::BTreeSet::new();
        self.visit_overlay_documents(files, |namespace, _| {
            namespaces.insert(namespace.to_owned());
        });
        namespaces
    }

    /// Authored control paths in the accepted, indexed documents of a pack layer.
    pub fn overlay_controls<'a>(
        &self,
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> std::collections::BTreeSet<crate::ControlRef> {
        let mut controls = std::collections::BTreeSet::new();
        self.visit_overlay_documents(files, |namespace, object| {
            controls.extend(
                object
                    .iter()
                    .filter(|(key, body)| key.as_str() != "namespace" && body.is_object())
                    .map(|(key, _)| crate::ControlRef::new(namespace, split_key(key).0)),
            );
        });
        controls
    }

    fn visit_overlay_documents<'a>(
        &self,
        files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
        mut visit: impl FnMut(&str, &Map<String, Value>),
    ) {
        let files: BTreeMap<&str, &[u8]> = files.into_iter().collect();
        let declared = files
            .get(UI_DEFS)
            .and_then(|bytes| Self::declared_paths(bytes).ok())
            .unwrap_or_default();
        for (path, bytes) in files {
            if !(self.lists(path) || declared.iter().any(|entry| entry == path))
                || path == GLOBALS
                || path == UI_DEFS
                || self.file_failed(path)
            {
                continue;
            }
            let (value, error) = json5::parse_partial(&String::from_utf8_lossy(bytes));
            if error.is_some() && self.has_file(path) {
                continue;
            }
            if let Value::Object(object) = value
                && let Some(namespace) = object
                    .get("namespace")
                    .and_then(Value::as_str)
                    .or_else(|| self.file_namespace(path))
            {
                visit(namespace, &object);
            }
        }
    }

    pub(crate) fn merge_overlay_file(&mut self, entry: &str, text: &str) {
        let Some(value) = self.read_document(entry, text) else {
            return;
        };
        let Value::Object(object) = value else {
            return self.note(format!("pack {entry}: top level is not an object"));
        };
        // A file overriding a vanilla path may omit the namespace it extends.
        let namespace = match object.get("namespace") {
            Some(Value::String(namespace)) => namespace.clone(),
            _ => self
                .file_namespace(entry)
                .unwrap_or(crate::catalog::ROOT_NAMESPACE)
                .to_owned(),
        };
        self.remember_file_namespace(entry, &namespace);
        for (key, body) in &object {
            if key == "namespace" {
                continue;
            }
            let mut diagnostics = Vec::new();
            // `parent/child` addresses a nested control by instance names.
            if let Some((top, rest)) = key.split_once('/') {
                let target = self
                    .lookup_mut(&namespace, top)
                    .and_then(|control| descendant(control, rest));
                match target {
                    Some(existing) => merge_into(existing, None, body, &mut diagnostics),
                    None => diagnostics.push(format!("{key}: nested control not found")),
                }
            } else {
                let (name, base) = split_key(key);
                match self.lookup_mut(&namespace, &name) {
                    Some(existing) => merge_into(existing, base, body, &mut diagnostics),
                    None => {
                        let control =
                            RawControl::from_entry(&namespace, key, body, &mut diagnostics);
                        self.insert(control);
                    }
                }
            }
            for message in diagnostics {
                self.note(format!("pack {entry}: {message}"));
            }
        }
    }
}

/// The control at a `/`-joined instance-name path below `control`.
fn descendant<'a>(control: &'a mut RawControl, path: &str) -> Option<&'a mut RawControl> {
    path.split('/').try_fold(control, |node, name| {
        node.children.iter_mut().find(|child| child.name == name)
    })
}

fn merge_into(
    existing: &mut RawControl,
    base: Option<String>,
    body: &Value,
    diagnostics: &mut Vec<String>,
) {
    let body = match body {
        Value::Object(body) => body,
        Value::Array(items) if items.is_empty() => return,
        _ => {
            diagnostics.push(format!("{}: control body is not an object", existing.name));
            return;
        }
    };
    if base.is_some() {
        existing.base = base;
    }
    for (property, value) in body {
        match property.as_str() {
            "controls" if value.is_string() => {
                existing.children.clear();
                existing.has_controls = true;
                existing.props.insert(property.clone(), value.clone());
            }
            "controls" => {
                existing.props.remove("controls");
                existing.has_controls = true;
                existing.children = child_controls(&existing.owner_ns, value, diagnostics);
            }
            "modifications" => {}
            _ => merge_value(existing.props.entry(property.clone()), value),
        }
    }
    // Modifications run after the overlay's ordinary properties have merged.
    if let Some(Value::Array(items)) = body.get("modifications") {
        apply_modifications(existing, items, diagnostics);
    }
}

/// Pack-layer merge: objects merge member by member, anything else replaces.
fn merge_value(slot: serde_json::map::Entry<'_>, value: &Value) {
    use serde_json::map::Entry;
    match (slot, value) {
        (Entry::Occupied(mut old), Value::Object(new)) if old.get().is_object() => {
            if let Value::Object(old) = old.get_mut() {
                merge_objects(old, new);
            }
        }
        (Entry::Occupied(mut old), _) => {
            old.insert(value.clone());
        }
        (Entry::Vacant(slot), _) => {
            slot.insert(value.clone());
        }
    }
}

pub(crate) fn merge_objects(old: &mut Map<String, Value>, new: &Map<String, Value>) {
    for (key, value) in new {
        merge_value(old.entry(key.clone()), value);
    }
}

/// An array a modification edits: its elements as they were before any
/// modification, and the current order of kept originals and inserted values.
struct Target {
    original: Vec<Value>,
    slots: Vec<Slot>,
}

#[derive(Clone)]
enum Slot {
    Original(usize),
    Inserted(Value),
}

/// Which element a modification names: a `control_name`, else a `where`/`target` value.
struct Condition<'a> {
    name: Option<&'a str>,
    value: Option<&'a Value>,
}

fn apply_modifications(control: &mut RawControl, items: &[Value], diagnostics: &mut Vec<String>) {
    let label = control.name.clone();
    let mut targets: Vec<(String, Target)> = Vec::new();
    for item in items.iter().filter_map(Value::as_object) {
        let name = item.get("control_name").and_then(Value::as_str);
        let mut array = native_string(item.get("array_name"));
        if array.is_empty() && name.is_some() {
            array = "controls".to_owned();
        }
        let operation = native_string(item.get("operation"));
        let message = if array.is_empty() {
            Some("missing `array_name`".to_owned())
        } else if operation.is_empty() {
            Some("missing `operation`".to_owned())
        } else if !OPERATIONS.contains(&operation.as_str()) {
            Some(format!("invalid operation `{operation}`"))
        } else {
            None
        };
        if let Some(message) = message {
            diagnostics.push(format!("{label}: modification {message}"));
            continue;
        }
        let index = match targets.iter().position(|(key, _)| *key == array) {
            Some(index) => index,
            None => {
                let original = current_array(control, &array);
                let slots = (0..original.len()).map(Slot::Original).collect();
                targets.push((array.clone(), Target { original, slots }));
                targets.len() - 1
            }
        };
        let selected = Condition {
            name,
            value: item.get("where"),
        };
        let target = &mut targets[index].1;
        let mut notes = Vec::new();
        if let Err(message) = target.apply(&operation, &selected, item, &mut notes) {
            notes.push(message);
        }
        for message in notes {
            diagnostics.push(format!(
                "{label}: modification `{operation}` on `{array}`: {message}"
            ));
        }
    }
    for (array, target) in targets {
        let values: Vec<Value> = target
            .slots
            .into_iter()
            .map(|slot| match slot {
                Slot::Original(index) => target.original[index].clone(),
                Slot::Inserted(value) => value,
            })
            .collect();
        if array == "controls" {
            control.props.remove("controls");
            control.has_controls = true;
            control.children =
                child_controls(&control.owner_ns, &Value::Array(values), diagnostics);
        } else if values.is_empty() {
            control.props.insert(array, Value::Null);
        } else {
            control.props.insert(array, Value::Array(values));
        }
    }
}

const OPERATIONS: &[&str] = &[
    "insert_back",
    "insert_front",
    "insert_after",
    "insert_before",
    "move_back",
    "move_front",
    "move_after",
    "move_before",
    "swap",
    "remove",
    "replace",
];

/// Vanilla's JSON string read: text as is, bools and numbers spelled out, null empty.
fn native_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// A control's array before modifications; a non-array has no elements.
fn current_array(control: &RawControl, array: &str) -> Vec<Value> {
    if array == "controls" {
        if control.props.get("controls").is_some_and(Value::is_string) {
            return Vec::new();
        }
        return control.children.iter().map(RawControl::to_entry).collect();
    }
    match control.props.get(array) {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

impl Target {
    fn apply(
        &mut self,
        operation: &str,
        selected: &Condition,
        item: &Map<String, Value>,
        notes: &mut Vec<String>,
    ) -> Result<(), String> {
        let value = item.get("value").filter(|value| !value.is_null());
        let values = || -> Result<Vec<Slot>, String> {
            match value {
                Some(Value::Array(items)) => {
                    Ok(items.iter().cloned().map(Slot::Inserted).collect())
                }
                Some(value) => Ok(vec![Slot::Inserted(value.clone())]),
                None => Err("missing `value`".to_owned()),
            }
        };
        match operation {
            "insert_back" => self.slots.extend(values()?),
            "insert_front" => {
                self.slots.splice(0..0, values()?);
            }
            "replace" => {
                let added = values()?;
                let at = self.slot_of(selected, notes)?;
                self.slots.splice(at..=at, added);
            }
            "insert_after" | "insert_before" => {
                let added = values()?;
                let at = self.slot_of(selected, notes)? + usize::from(operation == "insert_after");
                self.slots.splice(at..at, added);
            }
            "remove" => {
                let at = self.slot_of(selected, notes)?;
                self.slots.remove(at);
            }
            "move_front" | "move_back" => {
                let at = self.slot_of(selected, notes)?;
                let moved = self.slots.remove(at);
                let to = if operation == "move_front" {
                    0
                } else {
                    self.slots.len()
                };
                self.slots.insert(to, moved);
            }
            "move_after" | "move_before" | "swap" => {
                let target = match item.get("target_control").and_then(Value::as_str) {
                    Some(name) => Condition {
                        name: Some(name),
                        value: None,
                    },
                    None => Condition {
                        name: None,
                        value: item.get("target"),
                    },
                };
                let at = self.slot_of(selected, notes)?;
                let other = self.slot_of(&target, notes)?;
                if at == other {
                    return Ok(());
                }
                if operation == "swap" {
                    self.slots.swap(at, other);
                } else {
                    let moved = self.slots.remove(at);
                    let other = if other > at { other - 1 } else { other };
                    let to = other + usize::from(operation == "move_after");
                    self.slots.insert(to, moved);
                }
            }
            _ => return Err("invalid operation".to_owned()),
        }
        Ok(())
    }

    /// The current slot of the original element `condition` selects.
    fn slot_of(&self, condition: &Condition, notes: &mut Vec<String>) -> Result<usize, String> {
        let index = find_index(&self.original, condition, notes)?;
        self.slots
            .iter()
            .position(|slot| matches!(slot, Slot::Original(original) if *original == index))
            .ok_or_else(|| "selected element was already removed".to_owned())
    }
}

/// A modification's target lookup over the original elements: a name matches an
/// object's first member key before `@`; an object condition matches when any
/// member is equal; an array condition when it contains the element; a missing
/// condition falls back to the first element, as does a scalar one.
fn find_index(
    original: &[Value],
    condition: &Condition,
    notes: &mut Vec<String>,
) -> Result<usize, String> {
    let found = if let Some(name) = condition.name.filter(|name| !name.is_empty()) {
        let found = original.iter().position(|element| {
            element
                .as_object()
                .and_then(|entry| entry.keys().next())
                .is_some_and(|key| key.split_once('@').map_or(key.as_str(), |(key, _)| key) == name)
        });
        return found.ok_or_else(|| format!("no element named `{name}`"));
    } else {
        match condition.value {
            None | Some(Value::Null) => {
                notes.push("missing condition; the first element is used".to_owned());
                (!original.is_empty()).then_some(0)
            }
            Some(Value::Array(candidates)) => original
                .iter()
                .position(|element| !element.is_null() && candidates.contains(element)),
            Some(Value::Object(pattern)) => original.iter().position(|element| {
                element.as_object().is_some_and(|entry| {
                    pattern
                        .iter()
                        .any(|(key, expected)| entry.get(key) == Some(expected))
                })
            }),
            Some(_) => (!original.is_empty()).then_some(0),
        }
    };
    found.ok_or_else(|| "condition matched nothing".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Catalog {
        let globals = br##"{ "$color": "white" }"##;
        let defs = br##"{ "ui_defs": ["ui/screen.json"] }"##;
        let screen = br##"{
            "namespace": "screen",
            "panel": {
                "type": "panel",
                "size": [10, 10],
                "bindings": [ { "binding_name": "#a" }, { "binding_name": "#b" } ],
                "controls": [ { "first": { "type": "image" } }, { "second": { "type": "label" } } ]
            }
        }"##;
        Catalog::from_files([
            ("ui/_global_variables.json", globals.as_slice()),
            ("ui/_ui_defs.json", defs.as_slice()),
            ("ui/screen.json", screen.as_slice()),
        ])
        .expect("base loads")
    }

    fn names(catalog: &Catalog) -> Vec<String> {
        catalog
            .lookup("screen", "panel")
            .unwrap()
            .children
            .iter()
            .map(|child| child.name.clone())
            .collect()
    }

    #[test]
    fn overlay_properties_replace_and_keep_the_rest() {
        let mut catalog = base();
        let pack = br##"{ "namespace": "screen", "panel": { "size": [20, 5] } }"##;
        catalog.apply_pack([("ui/screen.json", pack.as_slice())]);
        let panel = catalog.lookup("screen", "panel").unwrap();
        assert_eq!(panel.props.get("size"), Some(&serde_json::json!([20, 5])));
        assert_eq!(panel.props.get("type"), Some(&serde_json::json!("panel")));
        assert_eq!(names(&catalog), ["first", "second"]);
    }

    // Later operations select only original elements: the inserted `middle` cannot move.
    #[test]
    fn control_modifications_select_original_elements_by_name() {
        let mut catalog = base();
        let pack = br##"{ "namespace": "screen", "panel": { "modifications": [
            { "array_name": "controls", "operation": "insert_after", "control_name": "first",
              "value": [ { "middle": { "type": "panel" } } ] },
            { "array_name": "controls", "operation": "remove", "control_name": "second" },
            { "array_name": "controls", "operation": "move_front", "control_name": "middle" }
        ] } }"##;
        catalog.apply_pack([("ui/screen.json", pack.as_slice())]);
        assert_eq!(names(&catalog), ["first", "middle"]);
        assert!(
            catalog
                .diagnostics()
                .iter()
                .any(|line| line.contains("no element named `middle`"))
        );
    }

    #[test]
    fn array_modifications_match_entries_by_where() {
        let mut catalog = base();
        let pack = br##"{ "namespace": "screen", "panel": { "modifications": [
            { "array_name": "bindings", "operation": "remove", "where": { "binding_name": "#a" } },
            { "array_name": "bindings", "operation": "insert_front", "value": { "binding_name": "#z" } }
        ] } }"##;
        catalog.apply_pack([("ui/screen.json", pack.as_slice())]);
        let bindings = catalog.lookup("screen", "panel").unwrap().props["bindings"].clone();
        assert_eq!(
            bindings,
            serde_json::json!([{ "binding_name": "#z" }, { "binding_name": "#b" }])
        );
    }

    #[test]
    fn overlay_globals_override_and_new_controls_are_added() {
        let mut catalog = base();
        catalog.apply_pack([
            (
                "ui/_global_variables.json",
                br##"{ "$color": "red" }"##.as_slice(),
            ),
            (
                "ui/_ui_defs.json",
                br##"{ "ui_defs": ["ui/new.json"] }"##.as_slice(),
            ),
            (
                "ui/new.json",
                br##"{ "namespace": "added", "thing": { "type": "panel" } }"##.as_slice(),
            ),
        ]);
        assert_eq!(catalog.global("color"), Some(&serde_json::json!("red")));
        assert!(catalog.lookup("added", "thing").is_some());
    }

    fn panel(pack: &[u8]) -> RawControl {
        let mut catalog = base();
        catalog.apply_pack([("ui/screen.json", pack)]);
        catalog.lookup("screen", "panel").unwrap().clone()
    }

    fn control_names(control: &RawControl) -> Vec<&str> {
        control
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect()
    }

    // Same-path overlays merge object properties member by member.
    #[test]
    fn overlay_objects_merge_recursively() {
        let mut catalog = base();
        catalog.apply_pack([(
            "ui/screen.json",
            br##"{ "panel": { "map": { "x": 1 } } }"##.as_slice(),
        )]);
        catalog.apply_pack([(
            "ui/screen.json",
            br##"{ "panel": { "map": { "y": 2 } } }"##.as_slice(),
        )]);
        let panel = catalog.lookup("screen", "panel").unwrap();
        assert_eq!(panel.props["map"], serde_json::json!({ "x": 1, "y": 2 }));
    }

    #[test]
    fn global_object_overlays_merge_recursively() {
        let mut catalog = base();
        catalog.overlay_globals_text(r#"{ "$g": { "a": 1 } }"#);
        catalog.overlay_globals_text(r#"{ "$g": { "b": 2 } }"#);
        assert_eq!(
            catalog.global("g"),
            Some(&serde_json::json!({ "a": 1, "b": 2 }))
        );
    }

    // An empty `array_name` with a `control_name` targets `controls`; none at all is an error.
    #[test]
    fn array_name_defaults_and_native_string_conversion() {
        let with_name = panel(
            br##"{ "panel": { "modifications": [
            { "array_name": "", "control_name": "first", "operation": "insert_back",
              "value": { "b": {} } } ] } }"##,
        );
        assert_eq!(control_names(&with_name), ["first", "second", "b"]);
        let without = panel(
            br##"{ "panel": { "modifications": [
            { "operation": "insert_back", "value": { "b": {} } } ] } }"##,
        );
        assert_eq!(control_names(&without), ["first", "second"]);
        let converted = panel(
            br##"{ "panel": { "true": [], "modifications": [
            { "array_name": true, "operation": "insert_back", "value": 1 } ] } }"##,
        );
        assert_eq!(converted.props["true"], serde_json::json!([1]));
    }

    // `where` and `control_name` select in any array; `target` names a move's anchor.
    #[test]
    fn selectors_accept_every_native_encoding() {
        let removed = panel(br##"{ "panel": { "modifications": [
            { "array_name": "controls", "operation": "remove", "where": { "first": { "type": "image" } } },
            { "array_name": "bindings", "operation": "remove", "control_name": "binding_name" } ] } }"##);
        assert_eq!(control_names(&removed), ["second"]);
        assert_eq!(
            removed.props["bindings"],
            serde_json::json!([{ "binding_name": "#b" }])
        );
        let moved = panel(
            br##"{ "panel": { "controls": [ { "a": {} }, { "b": {} }, { "c": {} } ],
            "modifications": [ { "array_name": "controls", "operation": "move_after",
              "control_name": "a", "target": { "b": {} } } ] } }"##,
        );
        assert_eq!(control_names(&moved), ["b", "a", "c"]);
    }

    // An object `where` matches on any member, removes the first match only, and `{}` matches nothing.
    #[test]
    fn where_matches_any_member_and_removes_one() {
        let any = panel(
            br##"{ "panel": { "bindings": [ { "k": 1, "v": 0 }, { "k": 2, "v": 3 } ],
            "modifications": [ { "array_name": "bindings", "operation": "remove",
              "where": { "k": 1, "v": 3 } } ] } }"##,
        );
        assert_eq!(
            any.props["bindings"],
            serde_json::json!([{ "k": 2, "v": 3 }])
        );
        let first = panel(
            br##"{ "panel": { "bindings": [ { "k": 1 }, { "k": 1 }, { "k": 2 } ],
            "modifications": [ { "array_name": "bindings", "operation": "remove",
              "where": { "k": 1 } } ] } }"##,
        );
        assert_eq!(
            first.props["bindings"],
            serde_json::json!([{ "k": 1 }, { "k": 2 }])
        );
        let empty = panel(br##"{ "panel": { "bindings": [ { "k": 1 } ],
            "modifications": [ { "array_name": "bindings", "operation": "remove", "where": {} } ] } }"##);
        assert_eq!(empty.props["bindings"], serde_json::json!([{ "k": 1 }]));
    }

    // A missing or scalar condition falls back to the first element.
    #[test]
    fn missing_conditions_select_the_first_element() {
        let missing = panel(
            br##"{ "panel": { "modifications": [
            { "array_name": "controls", "operation": "remove" } ] } }"##,
        );
        assert_eq!(control_names(&missing), ["second"]);
        let scalar = panel(
            br##"{ "panel": { "modifications": [
            { "array_name": "controls", "operation": "remove", "where": 5 } ] } }"##,
        );
        assert_eq!(control_names(&scalar), ["second"]);
    }

    // A missing or null `value` neither inserts nor deletes.
    #[test]
    fn missing_values_change_nothing() {
        let replaced = panel(br##"{ "panel": { "bindings": [ { "k": 1 }, { "k": 2 } ],
            "modifications": [ { "array_name": "bindings", "operation": "replace", "where": { "k": 1 } },
              { "array_name": "bindings", "operation": "insert_back", "value": null } ] } }"##);
        assert_eq!(
            replaced.props["bindings"],
            serde_json::json!([{ "k": 1 }, { "k": 2 }])
        );
    }

    // Self-relative moves are silent no-ops.
    #[test]
    fn self_relative_moves_are_valid_no_ops() {
        let mut catalog = base();
        catalog.apply_pack([(
            "ui/screen.json",
            br##"{ "panel": { "modifications": [
            { "array_name": "controls", "operation": "move_after", "control_name": "first",
              "target_control": "first" } ] } }"##
                .as_slice(),
        )]);
        assert_eq!(names(&catalog), ["first", "second"]);
        assert!(
            catalog.diagnostics().is_empty(),
            "{:?}",
            catalog.diagnostics()
        );
    }

    // Null, string and absent targets have no elements; the result rebuilds from insertions.
    #[test]
    fn non_array_targets_rebuild_from_insertions() {
        let null = panel(
            br##"{ "panel": { "bindings": null, "modifications": [
            { "array_name": "bindings", "operation": "insert_back", "value": { "k": 1 } } ] } }"##,
        );
        assert_eq!(null.props["bindings"], serde_json::json!([{ "k": 1 }]));
        let dynamic = panel(
            br##"{ "panel": { "$kids": [ { "a": {} } ], "controls": "$kids",
            "modifications": [ { "array_name": "controls", "operation": "insert_back",
              "value": { "b": {} } } ] } }"##,
        );
        assert_eq!(control_names(&dynamic), ["b"]);
        assert!(!dynamic.props.contains_key("controls"));
    }

    // Ordinary overlay properties merge before modifications run.
    #[test]
    fn overlay_properties_merge_before_modifications() {
        let merged = panel(br##"{ "panel": { "variables": [ { "$x": 2 } ], "modifications": [
            { "array_name": "variables", "operation": "insert_back", "value": { "$y": 3 } } ] } }"##);
        assert_eq!(
            merged.props["variables"],
            serde_json::json!([{ "$x": 2 }, { "$y": 3 }])
        );
    }

    // Removing every element leaves `null`, as the vanilla rebuild does.
    #[test]
    fn an_emptied_array_is_null() {
        let emptied = panel(
            br##"{ "panel": { "bindings": [ { "k": 1 } ], "modifications": [
            { "array_name": "bindings", "operation": "remove", "where": { "k": 1 } } ] } }"##,
        );
        assert_eq!(emptied.props["bindings"], serde_json::Value::Null);
    }

    // Malformed modifications report their own reasons; a non-array list is ignored.
    #[test]
    fn malformed_modifications_have_distinct_diagnostics() {
        let mut catalog = base();
        catalog.apply_pack([(
            "ui/screen.json",
            br##"{ "panel": { "modifications": [
            { "array_name": "controls", "operation": "insert" },
            { "array_name": "controls" },
            { "operation": "remove" } ] } }"##
                .as_slice(),
        )]);
        let lines = catalog.diagnostics().join("\n");
        assert!(lines.contains("invalid operation `insert`"), "{lines}");
        assert!(lines.contains("missing `operation`"), "{lines}");
        assert!(lines.contains("missing `array_name`"), "{lines}");
        let mut catalog = base();
        catalog.apply_pack([(
            "ui/screen.json",
            br##"{ "panel": { "modifications": {} } }"##.as_slice(),
        )]);
        assert!(catalog.diagnostics().is_empty());
    }
    #[test]
    fn review_invalid_overlay_bodies_preserve_the_previous_base() {
        for body in [serde_json::json!(7), serde_json::json!([])] {
            let mut notes = Vec::new();
            let mut control = RawControl::from_entry(
                "a",
                "root@a.original",
                &serde_json::json!({"type":"panel"}),
                &mut notes,
            );
            merge_into(
                &mut control,
                Some("a.replacement".into()),
                &body,
                &mut notes,
            );
            assert_eq!(control.base.as_deref(), Some("a.original"));
        }
    }
}
