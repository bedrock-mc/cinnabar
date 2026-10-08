use assets::AssetError;
use serde_json::Value;

use super::parse::{MAX_TEXTURE_VARIANTS, validate_texture_path};

/// One position-selected path inside a state-selected terrain entry.
#[derive(Debug, Clone)]
pub(crate) struct WeightedPath {
    pub(crate) path: Box<str>,
    pub(crate) weight: f32,
}

/// Retains nested alternatives while leaving the outer state selector intact.
pub(super) fn extract(value: &mut Value) -> Result<Vec<WeightedPath>, AssetError> {
    let Some(alternatives) = value.get("variations") else {
        return Ok(Vec::new());
    };
    if value.as_object().is_some_and(|fields| fields.len() != 1) {
        return Err(invalid(
            "variation group metadata requires a matching material route",
        ));
    }
    let alternatives = alternatives
        .as_array()
        .ok_or_else(|| invalid("variations must be an array"))?;
    if alternatives.is_empty() || alternatives.len() > MAX_TEXTURE_VARIANTS {
        return Err(invalid(
            "variations must contain a bounded, nonempty path list",
        ));
    }
    let mut paths = Vec::with_capacity(alternatives.len());
    for alternative in alternatives {
        if alternative.as_object().is_some_and(|fields| {
            fields
                .keys()
                .any(|key| !matches!(key.as_str(), "path" | "weight"))
        }) {
            return Err(invalid(
                "variation tint, UV and other metadata require a matching material route",
            ));
        }
        let path = alternative
            .as_str()
            .or_else(|| alternative.get("path").and_then(Value::as_str))
            .ok_or_else(|| invalid("variation path is missing"))?;
        validate_texture_path(path)?;
        // Vanilla clamps weights before normalizing them.
        let weight = if alternative.is_string() {
            1.0
        } else {
            alternative
                .get("weight")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32
        };
        paths.push(WeightedPath {
            path: path.into(),
            weight: weight.clamp(0.05, 1_000_000.0),
        });
    }
    let total: f32 = paths.iter().map(|entry| entry.weight).sum();
    for entry in &mut paths {
        entry.weight /= total;
    }
    *value = Value::String(paths[0].path.to_string());
    Ok(paths)
}

/// Reports unsupported terrain metadata without accepting an incomplete carrier.
fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_clamp_before_normalizing_and_state_arrays_stay_separate() {
        let mut value = serde_json::json!({"variations": [
            {"path": "textures/a", "weight": 0},
            {"path": "textures/b", "weight": -2},
            {"path": "textures/c", "weight": 0.1}
        ]});
        let paths = extract(&mut value).unwrap();
        assert_eq!(
            paths.iter().map(|p| p.weight).collect::<Vec<_>>(),
            [0.25, 0.25, 0.5]
        );
        assert_eq!(value, "textures/a");
        let mut states = serde_json::json!(["textures/a", "textures/b"]);
        assert!(extract(&mut states).unwrap().is_empty());
        assert!(states.is_array());
    }

    #[test]
    fn unsafe_or_unimplemented_variant_metadata_is_not_silently_discarded() {
        for value in [
            serde_json::json!({"variations": []}),
            serde_json::json!({"variations": ["../secret"]}),
            serde_json::json!({"variations": [{"path":"textures/a", "tint_color":"#ffffff"}]}),
        ] {
            assert!(extract(&mut value.clone()).is_err());
        }
    }

    #[test]
    fn string_and_object_paths_use_their_distinct_native_default_weights() {
        let mut value = serde_json::json!({"variations": [
            "textures/a", {"path": "textures/b"}, {"path": "textures/c", "weight": 0}
        ]});
        let paths = extract(&mut value).unwrap();
        assert!((paths[0].weight - 10.0 / 11.0).abs() < 0.000001);
        assert!((paths[1].weight - 1.0 / 22.0).abs() < 0.000001);
        assert_eq!(paths[2].weight, paths[1].weight);
    }
}
