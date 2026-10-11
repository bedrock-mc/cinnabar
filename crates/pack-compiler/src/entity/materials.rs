use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{AssetError, EntityAssetSource, EntityRenderMaterialState};
use serde_json::{Map, Value};

use super::{SourcePayloads, json::parse_unique_json};

struct Definition {
    parent: Option<Box<str>>,
    fields: Map<String, Value>,
}

pub(super) struct MaterialStates {
    definitions: BTreeMap<Box<str>, Option<Definition>>,
}

impl MaterialStates {
    /// Reads retained material declarations once for this compilation.
    pub(super) fn load(
        root: &Path,
        payloads: &SourcePayloads,
        sources: &[EntityAssetSource],
    ) -> Result<Self, AssetError> {
        let mut definitions = BTreeMap::new();
        for source in sources.iter().filter(|source| {
            source.path.starts_with("materials/") && source.path.ends_with(".material")
        }) {
            let bytes = payloads
                .get(source.path.as_ref())
                .ok_or_else(|| super::invalid("retained material source payload is absent"))?;
            let value = parse_unique_json(&root.join(source.path.as_ref()), bytes)?;
            let Some(entries) = value.get("materials").and_then(Value::as_object) else {
                continue;
            };
            for (declaration, value) in entries {
                let Some(fields) = value.as_object() else {
                    continue;
                };
                let (name, parent) = declaration
                    .split_once(':')
                    .map_or((declaration.as_str(), None), |(name, parent)| {
                        (name, Some(parent.into()))
                    });
                let name = name.strip_prefix('+').unwrap_or(name);
                if name.is_empty() || definitions.len() >= assets::MAX_ENTITY_ASSET_SYMBOLS {
                    continue;
                }
                definitions
                    .entry(name.into())
                    .and_modify(|existing| *existing = None)
                    .or_insert_with(|| {
                        Some(Definition {
                            parent,
                            fields: fields.clone(),
                        })
                    });
            }
        }
        Ok(Self { definitions })
    }

    /// Resolves the dye mask from authored defines, preserving an unknown parent as unresolved.
    pub(super) fn color_mask(&self, target: &str) -> assets::EquipmentColorMask {
        use assets::EquipmentColorMask;
        let mut name = target.strip_suffix(".skinning").unwrap_or(target);
        let mut seen = BTreeSet::new();
        let mut chain = Vec::new();
        let mut enabled;
        loop {
            if !seen.insert(name) {
                return EquipmentColorMask::Unresolved;
            }
            let Some(definition) = self.definitions.get(name) else {
                enabled = match name {
                    "armor_leather" | "armor_leather_enchanted" => Some(true),
                    "armor" | "armor_enchanted" | "elytra" => Some(false),
                    _ => builtin(name).map(|_| false),
                };
                break;
            };
            let Some(definition) = definition else {
                return EquipmentColorMask::Unresolved;
            };
            chain.push(&definition.fields);
            if let Some(parent) = definition.parent.as_deref() {
                name = parent;
            } else {
                enabled = None;
                break;
            }
        }
        for fields in chain.into_iter().rev() {
            let replace = fields.get("defines").is_some_and(|value| !value.is_null());
            if replace {
                enabled = Some(false);
            }
            for (key, value) in [("defines", true), ("+defines", true), ("-defines", false)] {
                if replace != (key == "defines") {
                    continue;
                }
                let Some(defines) = fields.get(key).filter(|value| !value.is_null()) else {
                    continue;
                };
                let Some(defines) = defines.as_array() else {
                    return EquipmentColorMask::Unresolved;
                };
                for define in defines {
                    let Some(define) = define.as_str() else {
                        return EquipmentColorMask::Unresolved;
                    };
                    if define == "USE_COLOR_MASK" {
                        enabled = Some(value);
                    }
                }
            }
        }
        match enabled {
            Some(true) => EquipmentColorMask::Dye,
            Some(false) => EquipmentColorMask::NoMask,
            None => EquipmentColorMask::Unresolved,
        }
    }

    /// Resolves supported render states through a bounded, acyclic parent chain.
    pub(super) fn resolve(&self, target: &str) -> Option<EntityRenderMaterialState> {
        let target = target.strip_suffix(".skinning").unwrap_or(target);
        let mut name = target;
        let mut seen = BTreeSet::new();
        let mut chain = Vec::new();
        let mut state;
        loop {
            if !seen.insert(name) {
                return None;
            }
            let Some(definition) = self.definitions.get(name) else {
                state = builtin(name)?;
                break;
            };
            let definition = definition.as_ref()?;
            chain.push(&definition.fields);
            match definition.parent.as_deref() {
                Some(parent) => name = parent,
                None => {
                    state = builtin(name).unwrap_or(EntityRenderMaterialState {
                        disable_overlay: true,
                        ..Default::default()
                    });
                    break;
                }
            }
        }
        for fields in chain.into_iter().rev() {
            apply(fields, &mut state)?;
        }
        // Native JSON actors select the transparent pass before legacy ALPHA_TEST;
        // that Actor shader pass preserves sampled opacity without a cutout discard.
        state.alpha_test &= !state.blend;
        Some(state)
    }
}

fn builtin(name: &str) -> Option<EntityRenderMaterialState> {
    let mut state = EntityRenderMaterialState::default();
    match name {
        "entity" => {}
        "entity_static" => state.disable_overlay = true,
        "entity_nocull" => state.cull = false,
        "entity_alphatest" | "skeleton" => {
            state.alpha_test = true;
            state.cull = false;
        }
        "entity_alphatest_one_sided" => state.alpha_test = true,
        "entity_emissive" => state.emissive = true,
        "entity_emissive_alpha" | "entity_emissive_alpha_one_sided" => {
            state.emissive = true;
            state.alpha_test = true;
            state.cull = name.ends_with("_one_sided");
        }
        "experience_orb" => state.alpha_test = true,
        "entity_alphablend" | "entity_alphablend_nocolor" | "player_spectator" | "slime_outer" => {
            state.blend = true;
        }
        "breeze_wind" => {
            state.alpha_test = true;
            state.blend = true;
            state.cull = false;
            state.depth_write = false;
            state.disable_overlay = true;
        }
        "charged_creeper" => {
            state.alpha_test = true;
            state.cull = false;
            state.blend = true;
            state.additive = true;
            state.disable_overlay = true;
        }
        _ => return None,
    }
    Some(state)
}

fn apply(fields: &Map<String, Value>, state: &mut EntityRenderMaterialState) -> Option<()> {
    let replace_states = fields.get("states").is_some_and(|value| !value.is_null());
    let replace_defines = fields.get("defines").is_some_and(|value| !value.is_null());
    if replace_states {
        state.cull = true;
        state.blend = false;
        state.depth_write = true;
    }
    if replace_defines {
        state.alpha_test = false;
        state.emissive = false;
        state.disable_overlay = true;
    }
    for (key, enabled) in [("states", true), ("+states", true), ("-states", false)] {
        if replace_states != (key == "states") {
            continue;
        }
        let Some(values) = fields.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        for value in values.as_array()? {
            match value.as_str()? {
                "Blending" => state.blend = enabled,
                "DisableCulling" => state.cull = !enabled,
                "DisableDepthWrite" => state.depth_write = !enabled,
                _ => {}
            }
        }
    }
    for (key, enabled) in [("defines", true), ("+defines", true), ("-defines", false)] {
        if replace_defines != (key == "defines") {
            continue;
        }
        let Some(values) = fields.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        for value in values.as_array()? {
            match value.as_str()? {
                "ALPHA_TEST" => state.alpha_test = enabled,
                "USE_EMISSIVE" => state.emissive = enabled,
                "USE_OVERLAY" => state.disable_overlay = !enabled,
                _ => {}
            }
        }
    }
    // Only the always-passing comparison is admitted; the default LessEqual restores normal
    // testing, and other functions keep the inherited state.
    match fields.get("depthFunc").and_then(Value::as_str) {
        Some("Always") => state.depth_always = true,
        Some("LessEqual") => state.depth_always = false,
        _ => {}
    }
    let source = fields.get("blendSrc").and_then(Value::as_str);
    let destination = fields.get("blendDst").and_then(Value::as_str);
    if source.is_some() || destination.is_some() {
        let inherited = if state.additive {
            (
                if state.additive_alpha {
                    "SourceAlpha"
                } else {
                    "One"
                },
                "One",
            )
        } else {
            ("SourceAlpha", "OneMinusSrcAlpha")
        };
        (state.additive, state.additive_alpha) = match (
            source.unwrap_or(inherited.0),
            destination.unwrap_or(inherited.1),
        ) {
            ("One", "One") => (true, false),
            ("SourceAlpha" | "SrcAlpha", "One") => (true, true),
            ("SourceAlpha" | "SrcAlpha", "OneMinusSrcAlpha") => (false, false),
            _ => return None,
        };
    }
    Some(())
}
