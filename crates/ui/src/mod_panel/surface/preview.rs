//! Isolated, noninteractive presentations of controls from the active pack catalog.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{binding_name, identifier};

/// Renderer selecting a separately bound pack control by its declared preview ID.
pub const RENDERER: &str = "cinnabar_control_preview";
pub const MAX_PREVIEWS: usize = 32;
pub const MAX_PREVIEW_BYTES: usize = 128 * 1024;
pub const MAX_PREVIEW_EXTENT: f64 = 4096.0;

/// A pack control and independent representative data; none enters gameplay bindings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlPreview {
    pub reference: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub globals: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub collections: BTreeMap<String, Vec<BTreeMap<String, Value>>>,
    /// A reference canvas in GUI units; zero measures content within a bounded canvas.
    #[serde(default)]
    pub size: [f64; 2],
}

/// Checks all declarations once before their owning surface can be retained.
pub(super) fn validate(previews: &BTreeMap<String, ControlPreview>) -> Result<(), String> {
    if previews.len() > MAX_PREVIEWS
        || serde_json::to_vec(previews)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_PREVIEW_BYTES
    {
        return Err("control previews exceed their retained budget".into());
    }
    for (id, preview) in previews {
        let reference = preview.reference.split_once('.');
        if !identifier(id)
            || !reference.is_some_and(|(namespace, name)| identifier(namespace) && identifier(name))
            || preview
                .size
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0 || *value > MAX_PREVIEW_EXTENT)
            || preview.variables.len() > 64
            || preview.globals.len() > 128
            || preview.collections.len() > 16
        {
            return Err("control preview declaration is invalid".into());
        }
        let mut count = 0;
        for (name, value) in &preview.variables {
            if !name.strip_prefix('$').is_some_and(identifier) {
                return Err("control preview variable name is invalid".into());
            }
            value_bounds(value, 0, &mut count)?;
        }
        for (name, value) in &preview.globals {
            if !binding_name(name) {
                return Err("control preview global name is invalid".into());
            }
            value_bounds(value, 0, &mut count)?;
        }
        for (name, rows) in &preview.collections {
            if !identifier(name) || rows.len() > 128 {
                return Err("control preview collection exceeds its bounds".into());
            }
            for row in rows {
                if row.len() > 32 {
                    return Err("control preview row exceeds its bounds".into());
                }
                for (name, value) in row {
                    if !binding_name(name) {
                        return Err("control preview collection binding name is invalid".into());
                    }
                    value_bounds(value, 0, &mut count)?;
                }
            }
        }
    }
    Ok(())
}

/// Bounds nested fixture values separately from ordinary scalar surface bindings.
fn value_bounds(value: &Value, depth: usize, count: &mut usize) -> Result<(), String> {
    *count += 1;
    if depth > 6 || *count > 4096 {
        return Err("control preview data exceeds its structural budget".into());
    }
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(number) => {
            if number
                .as_f64()
                .is_some_and(|number| number.is_finite() && number.abs() <= 1e6)
            {
                Ok(())
            } else {
                Err("control preview number is invalid".into())
            }
        }
        Value::String(text) => {
            if text.len() <= 16 * 1024
                && !text
                    .chars()
                    .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
            {
                Ok(())
            } else {
                Err("control preview text exceeds its bounds".into())
            }
        }
        Value::Array(values) if values.len() <= 128 => {
            for value in values {
                value_bounds(value, depth + 1, count)?;
            }
            Ok(())
        }
        Value::Object(values) if values.len() <= 128 => {
            for (name, value) in values {
                if name.len() > 96 || name.chars().any(char::is_control) {
                    return Err("control preview value key is invalid".into());
                }
                value_bounds(value, depth + 1, count)?;
            }
            Ok(())
        }
        _ => Err("control preview value exceeds its bounds".into()),
    }
}

/// Requires an authored literal selector; runtime globals cannot redirect catalog access.
pub(super) fn node(
    value: &Value,
    previews: &BTreeMap<String, ControlPreview>,
) -> Result<(), String> {
    if value.get("renderer").and_then(Value::as_str) == Some(RENDERER)
        && !value
            .get("preview_id")
            .and_then(Value::as_str)
            .is_some_and(|id| previews.contains_key(id))
    {
        return Err("control preview node must name a declared preview".into());
    }
    if let Some(children) = value.get("controls").and_then(Value::as_array) {
        for child in children {
            if let Some(object) = child.as_object() {
                for value in object.values() {
                    node(value, previews)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> BTreeMap<String, ControlPreview> {
        BTreeMap::from([(
            "example".into(),
            ControlPreview {
                reference: "example.actual_control".into(),
                globals: BTreeMap::from([("#text".into(), Value::String("line\n".repeat(2000)))]),
                ..Default::default()
            },
        )])
    }

    #[test]
    fn control_previews_accept_independent_multiline_data_without_relaxing_surface_scalars() {
        let previews = example();
        assert!(validate(&previews).is_ok());
        assert!(
            crate::mod_panel::SurfaceValue::Text("line\n".repeat(2000))
                .validate()
                .is_err()
        );
        let node = serde_json::json!({"type":"custom","renderer":RENDERER,"preview_id":"example"});
        assert!(super::node(&node, &previews).is_ok());
        assert!(super::node(&node, &BTreeMap::new()).is_err());
    }

    #[test]
    fn control_previews_bound_declarations_and_nested_fixture_values() {
        for invalid in ["", "unqualified", "../path.control", "x.é"] {
            let mut previews = example();
            previews.get_mut("example").unwrap().reference = invalid.into();
            assert!(validate(&previews).is_err(), "{invalid}");
        }
        for value in [
            serde_json::json!("x".repeat(16 * 1024 + 1)),
            serde_json::json!(vec![0; 129]),
            serde_json::json!("bad\u{0}"),
        ] {
            let mut previews = example();
            previews
                .get_mut("example")
                .unwrap()
                .globals
                .insert("#value".into(), value);
            assert!(validate(&previews).is_err());
        }
        let mut previews = example();
        previews
            .get_mut("example")
            .unwrap()
            .variables
            .insert("bad".into(), Value::Bool(true));
        assert!(validate(&previews).is_err());
        let mut previews = example();
        previews.get_mut("example").unwrap().size = [f64::NAN, 1.0];
        assert!(validate(&previews).is_err());
    }
}
