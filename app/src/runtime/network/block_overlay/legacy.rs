//! Legacy cube texture bindings supplied by the admitted resource pack stack.

use std::collections::HashMap;

use protocol::{CustomMaterialInstance, CustomVisualComponents};
use resource_pack::LayeredPackView;
use serde_json::Value;

use super::geometry::FACE_NAMES;
use pack_runtime::pack_textures::{MAX_CATALOG_ENTRIES, parse_pack_json};

pub(super) struct TextureBindings {
    materials: HashMap<String, Box<[CustomMaterialInstance]>>,
    pub(super) invalid: u32,
}

impl TextureBindings {
    pub(super) fn new(view: &LayeredPackView) -> Self {
        let mut bindings = Self {
            materials: HashMap::new(),
            invalid: 0,
        };
        for layer in view.read_layers("blocks.json") {
            let Some(Value::Object(entries)) = parse_pack_json(&layer) else {
                bindings.invalid = bindings.invalid.saturating_add(1);
                continue;
            };
            for (name, entry) in entries {
                let Some(textures) = entry.get("textures") else {
                    continue;
                };
                let Some(materials) = parse_textures(textures) else {
                    bindings.invalid = bindings.invalid.saturating_add(1);
                    continue;
                };
                if bindings.materials.len() < MAX_CATALOG_ENTRIES
                    || bindings.materials.contains_key(&name)
                {
                    bindings.materials.insert(name, materials);
                }
            }
        }
        bindings
    }

    /// Legacy textures supply cubes whose component set declares no
    /// `minecraft:material_instances`, whether or not it names the built-in
    /// full-block geometry. A pack's `blocks.json` is the documented fallback for
    /// such blocks, matching vanilla, which only lets `minecraft:material_instances`
    /// override it.
    pub(super) fn apply(&self, name: &str, components: &mut CustomVisualComponents) {
        if components.materials.is_none() && uses_legacy_cube(components.geometry.as_deref()) {
            components.materials = self.materials.get(name).cloned();
        }
    }
}

/// Whether the named geometry is a built-in cube the legacy binding can drive:
/// absent, or one of vanilla's full-block identifiers. A pack's own geometry
/// keeps its per-face material instances, since a single legacy texture cannot
/// address its faces.
fn uses_legacy_cube(geometry: Option<&str>) -> bool {
    geometry.is_none_or(|identifier| matches!(identifier, super::FULL_BLOCK | super::FULL_BLOCK_V1))
}

fn parse_textures(textures: &Value) -> Option<Box<[CustomMaterialInstance]>> {
    if let Some(texture) = textures.as_str() {
        return Some(Box::new([material("*", texture)]));
    }
    let faces = textures.as_object()?;
    let names: &[&str] = match faces.len() {
        3 => &["up", "down", "side"],
        6 => &FACE_NAMES,
        _ => return None,
    };
    names
        .iter()
        .map(|&name| Some(material(name, faces.get(name)?.as_str()?)))
        .collect()
}

fn material(name: &str, texture: &str) -> CustomMaterialInstance {
    CustomMaterialInstance {
        name: name.into(),
        texture: texture.into(),
        render_method: None,
        tint_method: None,
        ambient_occlusion: None,
        face_dimming: None,
    }
}
