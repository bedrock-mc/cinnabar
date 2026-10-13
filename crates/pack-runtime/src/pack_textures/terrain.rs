//! State arrays remain separate from choices selected by block position.

use super::{MAX_CATALOG_ENTRIES, parse_pack_json};
use resource_pack::LayeredPackView;
use serde_json::Value;
use std::collections::HashMap;

/// One path and its normalized positional weight inside a state variant.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub path: String,
    pub weight: f32,
    pub tint: Option<[u8; 3]>,
    /// Overlay-mask processing is unsupported by session carried cube art.
    pub overlay_color: bool,
}

/// A terrain entry's outer state array and independent weighted choice lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    variants: Vec<Vec<Choice>>,
    pub grid: u8,
}

impl Entry {
    /// Supplies a carrier multiplier only for aliases that have no full terrain entry.
    pub fn with_base_tint(mut self, tint: Option<[u8; 3]>) -> Self {
        for choices in &mut self.variants {
            for choice in choices {
                choice.tint = tint;
            }
        }
        self
    }

    /// Number of admitted outer state indices, independent of positional alternatives.
    pub fn variant_count(&self) -> usize {
        self.variants.len()
    }

    /// Out-of-range state selectors use the default variant.
    pub fn variant(&self, variant: u32) -> &[Choice] {
        &self.variants[self.resolved_variant(variant) as usize]
    }

    /// Resolves the state index used by both raster selection and animation metadata.
    pub fn resolved_variant(&self, variant: u32) -> u32 {
        if (variant as usize) < self.variants.len() {
            variant
        } else {
            0
        }
    }
}

/// Merges whole valid entries, preserving the lower entry when a layer is malformed.
pub fn entries(view: &LayeredPackView) -> HashMap<String, Entry> {
    let mut result = HashMap::new();
    for layer in view.read_layers("textures/terrain_texture.json") {
        let Some(Value::Object(data)) =
            parse_pack_json(&layer).map(|mut root| root["texture_data"].take())
        else {
            continue;
        };
        merge_entries(&mut result, data);
    }
    result
}

/// Keeps later overrides eligible even when new keys no longer fit in the catalog.
fn merge_entries(result: &mut HashMap<String, Entry>, data: serde_json::Map<String, Value>) {
    for (key, value) in data {
        if result.len() >= MAX_CATALOG_ENTRIES && !result.contains_key(&key) {
            continue;
        }
        if let Some(entry) = parse_entry(&value) {
            result.insert(key, entry);
        }
    }
}

/// Reads bounded state variants, paths, literal tints, grids, and weighted choices.
pub fn parse_entry(value: &Value) -> Option<Entry> {
    let textures = value.get("textures")?;
    let values = match textures {
        Value::Array(values) => values.as_slice(),
        value => std::slice::from_ref(value),
    };
    if values.is_empty() || values.len() > assets::material_keys::MAX_TERRAIN_VARIANTS {
        return None;
    }
    let variants = values
        .iter()
        .map(|variant| {
            let mut choices = parse_variant(variant)?;
            for choice in &mut choices {
                choice.overlay_color |= value.get("overlay_color").is_some();
            }
            Some(choices)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Entry {
        variants,
        grid: (value["quad"].as_u64().unwrap_or(0) as u32 & assets::TERRAIN_QUAD_SHIFT_MASK) as u8,
    })
}

/// Normalizes weights after clamping, without mixing the outer state selector.
fn parse_variant(value: &Value) -> Option<Vec<Choice>> {
    if let Some(alternatives) = value.get("variations") {
        let alternatives = alternatives.as_array()?;
        if alternatives.is_empty()
            || alternatives.len() > assets::material_keys::MAX_TERRAIN_VARIANTS
        {
            return None;
        }
        let mut choices = alternatives
            .iter()
            .map(|value| {
                let mut choice = parse_choice(value)?;
                let weight = if value.is_string() {
                    1.0
                } else {
                    value.get("weight").and_then(Value::as_f64).unwrap_or(0.0) as f32
                };
                choice.weight = weight;
                Some(choice)
            })
            .collect::<Option<Vec<_>>>()?;
        let mut weights = choices
            .iter()
            .map(|choice| choice.weight)
            .collect::<Vec<_>>();
        if !assets::material_keys::normalize_terrain_weights(&mut weights) {
            return None;
        }
        for (choice, weight) in choices.iter_mut().zip(weights) {
            choice.weight = weight;
            choice.overlay_color |= value.get("overlay_color").is_some();
        }
        Some(choices)
    } else {
        Some(vec![parse_choice(value)?])
    }
}

/// Accepts safe relative texture paths and the literal atlas multiplier.
fn parse_choice(value: &Value) -> Option<Choice> {
    let path = value.as_str().or_else(|| value.get("path")?.as_str())?;
    let path = path.trim().trim_start_matches("./");
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|part| matches!(part, ".." | ""))
    {
        return None;
    }
    let tint = match value.get("tint_color") {
        Some(tint) => Some(pack_compile_input::parse_atlas_tint(tint.as_str()?)?),
        None => None,
    };
    Some(Choice {
        path: path.to_owned(),
        weight: 1.0,
        tint,
        overlay_color: value.get("overlay_color").is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_catalogs_still_accept_later_overrides_after_rejecting_new_keys() {
        let entry = parse_entry(&serde_json::json!({"textures":"textures/old"})).unwrap();
        let mut entries = (0..MAX_CATALOG_ENTRIES)
            .map(|index| (format!("z_{index:05}"), entry.clone()))
            .collect::<HashMap<_, _>>();
        merge_entries(
            &mut entries,
            serde_json::json!({
                "a_new":{"textures":"textures/unadmitted"},
                "z_00000":{"textures":"textures/replacement"}
            })
            .as_object()
            .unwrap()
            .clone(),
        );
        assert_eq!(entries.len(), MAX_CATALOG_ENTRIES);
        assert!(!entries.contains_key("a_new"));
        assert_eq!(
            entries["z_00000"].variant(0)[0].path,
            "textures/replacement"
        );
    }

    #[test]
    fn state_arrays_and_weighted_choices_remain_separate() {
        let entry = parse_entry(&serde_json::json!({"textures":["textures/dry", {
            "variations":[{"path":"textures/wet_a","weight":1}, {"path":"textures/wet_b","weight":3}]
        }]})).unwrap();
        assert_eq!(entry.variant(0)[0].path, "textures/dry");
        assert_eq!(
            entry
                .variant(1)
                .iter()
                .map(|choice| choice.weight)
                .collect::<Vec<_>>(),
            [0.25, 0.75]
        );
        assert_eq!(entry.variant(9)[0].path, "textures/dry");
    }

    #[test]
    fn overlay_color_metadata_survives_static_and_weighted_choice_resolution() {
        for textures in [
            serde_json::json!({"path":"textures/a","overlay_color":"#ffffff"}),
            serde_json::json!({"variations":[{"path":"textures/a","overlay_color":"#ffffff","weight":1}]}),
            serde_json::json!({"variations":["textures/a"],"overlay_color":"#ffffff"}),
        ] {
            let entry = parse_entry(&serde_json::json!({"textures":textures})).unwrap();
            assert!(entry.variant(0)[0].overlay_color);
        }
        let plain = parse_entry(&serde_json::json!({"textures":"textures/a"})).unwrap();
        assert!(!plain.variant(0)[0].overlay_color);
    }

    #[test]
    fn malformed_choices_cannot_shift_state_indices() {
        for value in [
            serde_json::json!({"textures":["textures/good", false]}),
            serde_json::json!({"textures":{"variations":[]}}),
            serde_json::json!({"textures":{"variations":["../secret"]}}),
        ] {
            assert!(parse_entry(&value).is_none());
        }
    }
}
