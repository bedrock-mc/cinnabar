//! Legacy cube texture bindings supplied by the admitted resource pack stack.

use std::collections::HashMap;

use protocol::{CustomMaterialInstance, CustomVisualComponents};
use resource_pack::LayeredPackView;
use serde_json::Value;

use super::{
    super::resource_packs::{MAX_CATALOG_ENTRIES, parse_pack_json},
    geometry::FACE_NAMES,
};

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

    /// Legacy textures supply only cubes with no component-owned geometry or materials.
    pub(super) fn apply(&self, name: &str, components: &mut CustomVisualComponents) {
        if components.geometry.is_none() && components.materials.is_none() {
            components.materials = self.materials.get(name).cloned();
        }
    }
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
