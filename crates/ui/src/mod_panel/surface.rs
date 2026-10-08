//! Bounded extension-authored JSON-UI and its changing scalar bindings.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

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

    /// Loads a single catalog definition while bounding every authored child.
    fn walk(&self) -> Result<Vec<String>, String> {
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
        if object.len() != 2 || object.get("namespace").and_then(Value::as_str) != Some(namespace) {
            return Err("surface document must contain its namespace and one screen".into());
        }
        let root = object.get(name).ok_or("surface screen is missing")?;
        let mut count = 0;
        let mut actions = Vec::new();
        node(root, 0, &mut count, &mut actions)?;
        Ok(actions)
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
        "screen" | "panel" | "button" | "label" | "custom" | "stack_panel"
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
                matches!(name, "cinnabar_rounded_rectangle" | "cinnabar_vector_icon")
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
            actions.push(action.to_owned());
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
