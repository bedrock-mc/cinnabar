//! Bounds for Cinnabar paint transforms on surface controls, and the clip combinations
//! rotation cannot draw.

use json_ui::transform::{
    MAX_ROTATION_DEGREES, MAX_SCALE, ORIGIN_BINDING, ORIGIN_FIELD, ORIGIN_RANGE, ROTATION_BINDING,
    ROTATION_FIELD, SCALE_BINDING, SCALE_FIELD, TRANSFORM_PROPERTY,
};
use serde_json::{Map, Value};

/// Checks one control's transform and returns whether it may rotate. `rotated` reports a
/// rotating ancestor: rotated content cannot clip, because clips are axis-aligned scissors.
pub(super) fn control(object: &Map<String, Value>, rotated: bool) -> Result<bool, String> {
    let overrides = bound_targets(object);
    if let Some(value) = object.get(json_ui::position::PROPERTY) {
        let parsed = json_ui::position::parse(value)
            .ok_or("surface normalized position must be a finite pair")?;
        if value.as_array().is_none_or(|pair| {
            pair.iter()
                .zip(parsed)
                .any(|(value, parsed)| value.as_f64() != Some(parsed))
        }) {
            return Err("surface normalized position exceeds its bounds".into());
        }
    } else if overrides.contains(&json_ui::position::BINDING) {
        return Err("surface normalized position binding requires its property".into());
    }
    let declared = object.get(TRANSFORM_PROPERTY);
    if declared.is_none()
        && overrides
            .iter()
            .any(|target| [SCALE_BINDING, ROTATION_BINDING, ORIGIN_BINDING].contains(target))
    {
        return Err(format!(
            "surface transform bindings require a {TRANSFORM_PROPERTY} property"
        ));
    }
    let rotates = match declared {
        Some(transform) => {
            let rotation = transform_fields(transform)?;
            rotation != 0.0 || overrides.contains(&ROTATION_BINDING)
        }
        None => false,
    };
    let clips = ["clips_children", "clip_children"]
        .iter()
        .any(|key| object.get(*key) == Some(&Value::Bool(true)))
        || object.get("type").and_then(Value::as_str) == Some("scroll_view");
    if (rotated || rotates) && clips {
        return Err("surface rotation cannot contain clipping controls or scroll views".into());
    }
    Ok(rotated || rotates)
}

/// Rejects unknown fields and values outside the transform bounds; returns the rotation.
fn transform_fields(transform: &Value) -> Result<f64, String> {
    let fields = transform
        .as_object()
        .ok_or("surface transform must be an object")?;
    let invalid = || format!("surface {TRANSFORM_PROPERTY} exceeds its bounds");
    let within = |value: &Value, low: f64, high: f64| {
        value
            .as_f64()
            .filter(|value| value.is_finite() && (low..=high).contains(value))
    };
    let pair = |value: &Value, low: f64, high: f64| match value.as_array().map(Vec::as_slice) {
        Some([x, y]) => within(x, low, high).is_some() && within(y, low, high).is_some(),
        _ => false,
    };
    let mut rotation = 0.0;
    for (name, value) in fields {
        let valid = match name.as_str() {
            SCALE_FIELD => within(value, 0.0, MAX_SCALE).is_some() || pair(value, 0.0, MAX_SCALE),
            ROTATION_FIELD => match within(value, -MAX_ROTATION_DEGREES, MAX_ROTATION_DEGREES) {
                Some(degrees) => {
                    rotation = degrees;
                    true
                }
                None => false,
            },
            ORIGIN_FIELD => pair(value, *ORIGIN_RANGE.start(), *ORIGIN_RANGE.end()),
            _ => return Err(format!("surface {TRANSFORM_PROPERTY} field is unsupported")),
        };
        if !valid {
            return Err(invalid());
        }
    }
    Ok(rotation)
}

/// The binding targets a control's bindings write.
pub(super) fn bound_targets(object: &Map<String, Value>) -> Vec<&str> {
    object
        .get("bindings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|binding| {
            binding
                .get("binding_name_override")
                .or_else(|| binding.get("binding_name"))
                .and_then(Value::as_str)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Validates one control object under an optional rotating ancestor.
    fn check(control: Value, rotated: bool) -> Result<bool, String> {
        super::control(control.as_object().unwrap(), rotated)
    }

    #[test]
    fn transforms_accept_bounded_scale_rotation_and_origin() {
        let scaled = json!({"type":"panel","cinnabar_transform":{"scale":[0.97, 1.23]}});
        assert_eq!(check(scaled, false), Ok(false));
        let turned = json!({"type":"panel","cinnabar_transform":{
            "rotation":45, "origin":[0.5, 0.5], "scale":1.15}});
        assert_eq!(check(turned, false), Ok(true));
        let bound = json!({"type":"panel","cinnabar_transform":{},"bindings":[
            {"binding_name":"#chevron_turn","binding_name_override":"#cinnabar_rotation"}]});
        assert_eq!(check(bound, false), Ok(true));
        let inside = json!({"type":"label","text":"x"});
        assert_eq!(check(inside, true), Ok(true));
    }

    #[test]
    fn normalized_positions_require_bounded_pairs_and_declared_binding_targets() {
        assert_eq!(check(json!({"cinnabar_position":[0,1]}), false), Ok(false));
        for position in [json!([-1, 0]), json!([0, 2]), json!([0]), json!([0, null])] {
            assert!(check(json!({"cinnabar_position":position}), false).is_err());
        }
        assert!(
            check(
                json!({"bindings":[{"binding_name":"#at",
            "binding_name_override":"#cinnabar_position"}]}),
                false
            )
            .is_err()
        );
    }

    #[test]
    fn transforms_reject_unbounded_values_and_unknown_fields() {
        for transform in [
            json!({"scale": 4.5}),
            json!({"scale": -0.1}),
            json!({"scale": [1, 9]}),
            json!({"rotation": 400}),
            json!({"origin": [3, 0]}),
            json!({"origin": [0.5]}),
            json!({"skew": 10}),
            json!(1.2),
        ] {
            let control = json!({"type":"panel","cinnabar_transform":transform});
            assert!(check(control, false).is_err(), "{transform}");
        }
        let unbound = json!({"type":"panel","bindings":[
            {"binding_name":"#press","binding_name_override":"#cinnabar_scale"}]});
        assert!(check(unbound, false).is_err());
    }

    #[test]
    fn rotation_rejects_clips_and_scroll_views_but_scale_keeps_them() {
        let clipping = json!({"type":"panel","clips_children":true,
            "cinnabar_transform":{"rotation":10}});
        assert!(check(clipping, false).is_err());
        assert!(check(json!({"type":"scroll_view"}), true).is_err());
        assert!(check(json!({"type":"panel","clip_children":true}), true).is_err());
        let scaled = json!({"type":"panel","clips_children":true,
            "cinnabar_transform":{"scale":0.97}});
        assert_eq!(check(scaled, false), Ok(false));
        assert_eq!(check(json!({"type":"scroll_view"}), false), Ok(false));
    }
}
