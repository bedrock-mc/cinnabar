//! Legacy cube texture bindings supplied by the admitted resource pack stack.

use std::collections::HashMap;

use protocol::{CustomMaterialInstance, CustomVisualComponents};
use resource_pack::LayeredPackView;
use serde_json::Value;

use super::geometry::FACE_NAMES;
use pack_runtime::pack_textures::{MAX_CATALOG_ENTRIES, parse_pack_json};

pub(super) struct TextureBindings {
    materials: HashMap<String, Box<[CustomMaterialInstance]>>,
    carried: HashMap<String, Box<[CustomMaterialInstance]>>,
    pub(super) invalid: u32,
}

/// A resolved key and the binding shape that controls pillar UV rotation.
pub(super) struct TextureBinding<'a> {
    pub(super) key: &'a str,
    pub(super) per_face: bool,
}

impl TextureBindings {
    pub(super) fn new(view: &LayeredPackView) -> Self {
        let mut bindings = Self {
            materials: HashMap::new(),
            carried: HashMap::new(),
            invalid: 0,
        };
        for layer in view.read_layers("blocks.json") {
            let Some(Value::Object(entries)) = parse_pack_json(&layer) else {
                bindings.invalid = bindings.invalid.saturating_add(1);
                continue;
            };
            for (name, entry) in entries {
                for (field, target) in [
                    ("textures", &mut bindings.materials),
                    ("carried_textures", &mut bindings.carried),
                ] {
                    let Some(textures) = entry.get(field) else {
                        continue;
                    };
                    let Some(materials) = parse_textures(textures) else {
                        bindings.invalid = bindings.invalid.saturating_add(1);
                        continue;
                    };
                    if target.len() < MAX_CATALOG_ENTRIES || target.contains_key(&name) {
                        target.insert(name.clone(), materials);
                    }
                }
            }
        }
        bindings
    }

    /// Resolves a binding face, accepting the registry name or its pack alias.
    pub(super) fn texture(
        &self,
        name: &str,
        alias: &str,
        face: usize,
        carried: bool,
    ) -> Option<&str> {
        self.binding(name, alias, face, carried)
            .map(|binding| binding.key)
    }

    /// Resolves a face while retaining whether its winning entry is scalar or per-face.
    pub(super) fn binding(
        &self,
        name: &str,
        alias: &str,
        face: usize,
        carried: bool,
    ) -> Option<TextureBinding<'_>> {
        let bindings = if carried {
            &self.carried
        } else {
            &self.materials
        };
        let materials = bindings
            .get(name)
            .or_else(|| bindings.get(name.strip_prefix("minecraft:").unwrap_or(name)))
            .or_else(|| bindings.get(alias))?;
        let face = FACE_NAMES[face];
        let horizontal = !matches!(face, "up" | "down");
        std::iter::once(face)
            .chain(horizontal.then_some("side"))
            .chain(std::iter::once("*"))
            .find_map(|name| {
                materials
                    .iter()
                    .find(|material| material.name.as_ref() == name)
            })
            .map(|material| TextureBinding {
                key: material.texture.as_ref(),
                per_face: material.name.as_ref() != "*",
            })
    }

    /// Applies the legacy binding to blocks without a geometry component.
    /// `blocks.json` is the fallback for that shape, even when a server adds an
    /// automatic `material_instances` component. A geometry component instead
    /// keeps its terrain-texture material instances.
    pub(super) fn apply(&self, name: &str, components: &mut CustomVisualComponents) {
        if components.geometry.is_some() {
            return;
        }
        if let Some(materials) = self.materials.get(name).or_else(|| {
            name.strip_prefix("minecraft:")
                .and_then(|name| self.materials.get(name))
        }) {
            components.materials = Some(materials.clone());
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
