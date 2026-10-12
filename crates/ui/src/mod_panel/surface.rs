//! Bounded extension-authored JSON-UI and its changing scalar bindings.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
mod static_templates;
use static_templates::{acyclic, authored, bound_expansion, local_reference, resolved_value};

/// Host-owned track action consumed without invoking an extension control.
pub const SCROLL_TRACK_ACTION: &str = "mod.scroll_track";

pub const MAX_SURFACE_BYTES: usize = 96 * 1024;
pub const MAX_SURFACE_NODES: usize = 1024;
pub const MAX_SURFACE_BINDINGS: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    pub screen: String,
    pub document: String,
    #[serde(default)]
    pub bindings: BTreeMap<String, SurfaceValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SurfaceValue {
    Bool(bool),
    Number(f64),
    Text(String),
    Vector(Vec<f64>),
}

impl Surface {
    /// Rejects oversized trees and dynamic factories before the host retains them.
    pub fn validate(&self) -> Result<(), String> {
        let _ = self.walk()?;
        if self.bindings.len() > MAX_SURFACE_BINDINGS {
            return Err("surface has too many bindings".into());
        }
        for (name, value) in &self.bindings {
            if !binding_name(name) {
                return Err("surface binding name is invalid".into());
            }
            let finite = |number: &f64| number.is_finite() && number.abs() <= 1_000_000.0;
            let valid = match value {
                SurfaceValue::Bool(_) => true,
                SurfaceValue::Number(number) => finite(number),
                SurfaceValue::Text(text) => {
                    text.len() <= 256 && !text.chars().any(char::is_control)
                }
                SurfaceValue::Vector(numbers) => {
                    (2..=4).contains(&numbers.len()) && numbers.iter().all(finite)
                }
            };
            if !valid {
                return Err("surface binding value exceeds its bounds".into());
            }
        }
        Ok(())
    }

    /// Returns authored pressed actions so the owning capability validates routes.
    pub fn actions(&self) -> Result<Vec<String>, String> {
        self.walk()
    }

    /// Returns a canonical static document consumed by the native renderer.
    /// No host context can later select an authored branch that escaped validation.
    pub fn resolved_document(&self) -> Result<String, String> {
        let root = self.resolved_root()?;
        let namespace = self
            .screen
            .split_once('.')
            .ok_or("surface screen must be qualified")?
            .0;
        let name = self.screen.split_once('.').expect("qualified checked").1;
        let mut document = serde_json::Map::new();
        document.insert("namespace".into(), Value::String(namespace.into()));
        document.insert(name.into(), root);
        Ok(Value::Object(document).to_string())
    }
    /// Collects capability-routed actions from the fully validated static tree.
    fn walk(&self) -> Result<Vec<String>, String> {
        let root = self.resolved_root()?;
        let mut count = 0;
        let mut actions = Vec::new();
        node(&root, 0, &mut count, &mut actions)?;
        Ok(actions)
    }
    /// Resolves only static local templates before validating the emitted tree.
    fn resolved_root(&self) -> Result<Value, String> {
        if self.document.len() > MAX_SURFACE_BYTES {
            return Err("surface document exceeds byte limit".into());
        }
        let Some((namespace, name)) = self.screen.split_once('.') else {
            return Err("surface screen must be qualified".into());
        };
        if !identifier(namespace) || !identifier(name) {
            return Err("surface screen is invalid".into());
        }
        if matches!(namespace, "cinnabar_personal" | "cinnabar_hud_editor") {
            return Err("surface namespace is reserved by the host".into());
        }
        let document: Value = serde_json::from_str(&self.document)
            .map_err(|error| format!("invalid surface JSON: {error}"))?;
        let object = document
            .as_object()
            .ok_or("surface document must be an object")?;
        if object.get("namespace").and_then(Value::as_str) != Some(namespace) {
            return Err("surface document must contain its own namespace".into());
        }
        let mut definitions = BTreeMap::new();
        for (key, body) in object.iter().filter(|(key, _)| *key != "namespace") {
            let (local, base) = local_reference(key, namespace)?;
            if definitions.insert(local, (body, base)).is_some() {
                return Err("duplicate surface definition".into());
            }
        }
        if !definitions.contains_key(name) {
            return Err("surface screen is missing".into());
        }
        let mut graph = BTreeMap::new();
        let mut authored_nodes = 0;
        for (key, (body, base)) in &definitions {
            let mut references = Vec::new();
            if let Some(base) = base {
                references.push(base.clone());
            }
            authored(body, namespace, 0, &mut authored_nodes, &mut references)?;
            graph.insert(key.clone(), references);
        }
        for key in graph.keys() {
            acyclic(
                key,
                &graph,
                &mut Vec::new(),
                &mut std::collections::BTreeSet::new(),
            )?;
        }
        bound_expansion(name, namespace, &definitions)?;
        let files = br#"{"ui_defs":["ui/extension_surface.json"]}"#;
        let catalog = json_ui::Catalog::from_files([
            ("ui/_global_variables.json", &b"{}"[..]),
            ("ui/_ui_defs.json", &files[..]),
            ("ui/extension_surface.json", self.document.as_bytes()),
        ])
        .map_err(|error| error.to_string())?;
        if !catalog.diagnostics().is_empty() {
            return Err(catalog.diagnostics().join("; "));
        }
        let resolved = json_ui::resolve_with_limits(
            &catalog,
            &self.screen,
            &json_ui::Context::default(),
            33,
            MAX_SURFACE_NODES,
        );
        if !resolved.diagnostics.is_empty() {
            return Err(resolved.diagnostics.join("; "));
        }
        let root = resolved
            .control
            .as_ref()
            .ok_or("surface root cannot be resolved")?;
        let root = resolved_value(root)?;
        let mut count = 0;
        let mut actions = Vec::new();
        node(&root, 0, &mut count, &mut actions)?;
        Ok(root)
    }
}

/// Matches the names accepted by a local scalar data source.
fn binding_name(name: &str) -> bool {
    name.strip_prefix('#').is_some_and(identifier)
}

/// Restricts names to an unambiguous local identifier.
fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
}

/// Checks a finite authored tree without resolving factories or inheritance.
fn node(
    value: &Value,
    depth: usize,
    count: &mut usize,
    actions: &mut Vec<String>,
) -> Result<(), String> {
    *count += 1;
    if depth > 32 || *count > MAX_SURFACE_NODES {
        return Err("surface tree exceeds structural limits".into());
    }
    let object = value
        .as_object()
        .ok_or("surface control must be an object")?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("surface control type is missing")?;
    if !matches!(
        kind,
        "screen"
            | "panel"
            | "button"
            | "label"
            | "custom"
            | "stack_panel"
            | "scroll_view"
            | "scroll_track"
            | "scrollbar_box"
    ) || (kind == "screen" && depth != 0)
    {
        return Err("surface control type is unsupported".into());
    }
    for (name, value) in object {
        if name.starts_with('$') {
            return Err("surface variable declarations are unsupported".into());
        }
        if name != "controls" {
            property(value, 0)?;
        }
        if matches!(
            name.as_str(),
            "alpha" | "uv" | "color" | "clip_ratio" | "size" | "offset"
        ) && value.as_str().is_some_and(|text| text.starts_with('@'))
        {
            return Err("surface animation references are unsupported".into());
        }
    }
    for name in [
        "factory",
        "variables",
        "anims",
        "animations",
        "collection_name",
        "grid_dimensions",
        "property_bag",
        "property_bag_for_children",
    ] {
        if object.contains_key(name) {
            return Err("surface dynamic expansion is unsupported".into());
        }
    }
    if let Some(bindings) = object.get("bindings") {
        for binding in bindings
            .as_array()
            .ok_or("surface bindings must be an array")?
        {
            let binding = binding
                .as_object()
                .ok_or("surface binding must be an object")?;
            if binding
                .get("binding_type")
                .is_some_and(|kind| kind.as_str() != Some("global"))
            {
                return Err("surface bindings must use the typed global data source".into());
            }
        }
    }
    if kind == "custom"
        && !object
            .get("renderer")
            .and_then(Value::as_str)
            .is_some_and(|name| {
                matches!(
                    name,
                    "cinnabar_rounded_rectangle" | "cinnabar_vector_icon" | "gradient_renderer"
                )
            })
    {
        return Err("surface custom renderer is unsupported".into());
    }
    if let Some(mappings) = object.get("button_mappings") {
        let mappings = mappings
            .as_array()
            .ok_or("surface mappings must be an array")?;
        if mappings.len() > 4 {
            return Err("surface has too many button mappings".into());
        }
        for mapping in mappings {
            if mapping.get("from_button_id").and_then(Value::as_str) != Some("button.menu_select")
                || mapping.get("mapping_type").and_then(Value::as_str) != Some("pressed")
            {
                return Err("surface buttons require pressed menu-select mappings".into());
            }
            let action = mapping
                .get("to_button_id")
                .and_then(Value::as_str)
                .ok_or("surface action is missing")?;
            if action.len() > 96 {
                return Err("surface action exceeds its bound".into());
            }
            if kind != "scroll_track" || action != SCROLL_TRACK_ACTION {
                actions.push(action.to_owned());
            }
        }
    }
    if let Some(children) = object.get("controls") {
        for entry in children
            .as_array()
            .ok_or("surface controls must be an array")?
        {
            let entry = entry.as_object().ok_or("surface child must be named")?;
            if entry.len() != 1 {
                return Err("surface child must have one name".into());
            }
            let (name, body) = entry.iter().next().expect("one member checked");
            if !identifier(name) || name.contains('@') {
                return Err("surface child inheritance is unsupported".into());
            }
            node(body, depth + 1, count, actions)?;
        }
    }
    Ok(())
}

/// Rejects structured aliases and inline animation graphs before JSON-UI substitution.
fn property(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Err("surface property exceeds structural limits".into());
    }
    match value {
        Value::String(text) if text.starts_with('$') => {
            return Err("surface variable references are unsupported".into());
        }
        Value::Array(items) => {
            for item in items {
                property(item, depth + 1)?;
            }
        }
        Value::Object(object) => {
            for (name, value) in object {
                if name.starts_with('$') || name == "anim_type" {
                    return Err(
                        "surface variable declarations and inline animations are unsupported"
                            .into(),
                    );
                }
                property(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
