//! Authored rounded shadow bounds; dynamic binding values clamp at paint time.

use json_ui::rounded::{MAX_SHADOW_BLUR, MAX_SHADOW_OFFSET, MAX_SHADOW_SPREAD};
use serde_json::{Map, Value};

/// Rejects malformed literals and direct binding overrides before admitting a surface.
pub(super) fn validate(object: &Map<String, Value>) -> Result<(), String> {
    for (name, value) in object {
        let name = name.strip_prefix('#').unwrap_or(name);
        let valid = match name {
            "shadow_blur" => number(value, 0.0, MAX_SHADOW_BLUR),
            "shadow_spread" => number(value, -MAX_SHADOW_SPREAD, MAX_SHADOW_SPREAD),
            "shadow_offset" => value.as_array().is_some_and(|items| {
                items.len() == 2
                    && items
                        .iter()
                        .all(|value| number(value, -MAX_SHADOW_OFFSET, MAX_SHADOW_OFFSET))
            }),
            "shadow_color" => value.as_array().is_some_and(|items| {
                items.len() == 4 && items.iter().all(|value| number(value, 0.0, 1.0))
            }),
            _ => continue,
        };
        if !valid {
            return Err(format!(
                "surface rounded {name} is malformed or exceeds its bounds"
            ));
        }
    }
    Ok(())
}

/// Whether an authored value is a finite numeric length or color channel in range.
fn number(value: &Value, min: f64, max: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|value| value.is_finite() && (min..=max).contains(&value))
}

#[cfg(test)]
mod tests {
    use super::super::{Surface, SurfaceValue};
    use super::{MAX_SHADOW_BLUR, MAX_SHADOW_OFFSET, MAX_SHADOW_SPREAD};
    use serde_json::{Value, json};

    /// A rounded panel with the supplied shadow properties and no external fixtures.
    fn surface(properties: Value) -> Surface {
        let mut node = json!({"type":"custom","renderer":"cinnabar_rounded_rectangle",
            "size":[80,40]});
        node.as_object_mut()
            .unwrap()
            .extend(properties.as_object().unwrap().clone());
        Surface {
            screen: "shadow.root".into(),
            document: json!({"namespace":"shadow","root":node}).to_string(),
            previews: Default::default(),
            bindings: Default::default(),
            transitions: Default::default(),
            collections: Default::default(),
        }
    }

    #[test]
    fn rounded_shadows_reject_invalid_literals() {
        for properties in [
            json!({"shadow_blur":49}),
            json!({"shadow_blur":-1}),
            json!({"shadow_blur":"wide"}),
            json!({"shadow_offset":[0]}),
            json!({"shadow_offset":[0,0,0]}),
            json!({"shadow_offset":[0,4097]}),
            json!({"shadow_spread":49}),
            json!({"shadow_color":[0,0,0,2]}),
            json!({"shadow_color":[0,0,0]}),
        ] {
            assert!(
                surface(properties.clone()).validate().is_err(),
                "{properties}"
            );
        }
    }

    #[test]
    fn rounded_shadows_accept_boundaries_and_binding_transitions() {
        let mut surface = surface(json!({"shadow_color":[0,0,0,0.5],
            "shadow_offset":[-MAX_SHADOW_OFFSET,MAX_SHADOW_OFFSET],
            "shadow_blur":MAX_SHADOW_BLUR,"shadow_spread":-MAX_SHADOW_SPREAD,
            "bindings":[{"binding_name":"#ink","binding_name_override":"#shadow_color"},
                {"binding_name":"#shift","binding_name_override":"#shadow_offset"}]}));
        surface.bindings.insert(
            "#ink".into(),
            SurfaceValue::Vector(vec![0.0, 0.0, 0.0, 0.5]),
        );
        surface
            .bindings
            .insert("#shift".into(), SurfaceValue::Vector(vec![1.0, 2.0]));
        surface.validate().unwrap();
        surface.transitions =
            serde_json::from_value(json!({"#ink":{"duration":0.2},"#shift":{"duration":0.2}}))
                .unwrap();
        surface.validate().unwrap();
    }
}
