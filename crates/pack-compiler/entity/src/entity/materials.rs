use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{AssetError, EntityAssetSource, EntityRenderMaterialState};
use serde_json::{Map, Value};

use super::SourceDocuments;

struct Definition {
    parent: Option<Box<str>>,
    fields: Map<String, Value>,
}

#[derive(Default)]
pub(super) struct MaterialStates {
    definitions: BTreeMap<Box<str>, Option<Definition>>,
}

impl MaterialStates {
    /// Reads retained material declarations once for this compilation.
    pub(super) fn load(
        root: &Path,
        payloads: &SourceDocuments<'_>,
        sources: &[EntityAssetSource],
    ) -> Result<Self, AssetError> {
        let mut states = Self::default();
        for source in sources.iter().filter(|source| {
            source.path.starts_with("materials/") && source.path.ends_with(".material")
        }) {
            let value = payloads.read(root, &source.path)?;
            states.read(value);
        }
        Ok(states)
    }

    /// Admits declarations with the compiler's duplicate and count rules.
    pub(super) fn read(&mut self, value: &Value) {
        let Some(entries) = value.get("materials").and_then(Value::as_object) else {
            return;
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
            if name.is_empty() || self.definitions.len() >= assets::MAX_ENTITY_ASSET_SYMBOLS {
                continue;
            }
            self.definitions
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

    /// Replaces lower declarations with a separately admitted layer and returns its names.
    pub(super) fn overlay(&mut self, upper: Self) -> BTreeSet<String> {
        let names = upper.definitions.keys().map(ToString::to_string).collect();
        self.definitions.extend(upper.definitions);
        names
    }

    /// Includes the same normalized names and parent links used by material resolution.
    pub(super) fn extend_dependencies(&self, names: &mut BTreeSet<String>) {
        let mut pending = names
            .iter()
            .map(|name| material_name(name).to_owned())
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            names.insert(name.clone());
            if let Some(Some(definition)) = self.definitions.get(name.as_str())
                && let Some(parent) = &definition.parent
            {
                pending.push(parent.to_string());
            }
        }
    }

    /// Resolves the dye mask from authored defines, preserving an unknown parent as unresolved.
    pub(super) fn color_mask(&self, target: &str) -> assets::EquipmentColorMask {
        use assets::EquipmentColorMask;
        let mut name = material_name(target);
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
                    "armor" | "armor_enchanted" => Some(false),
                    _ if assets::facts::pack::rules().material(name)
                        == Some(assets::facts::pack::MaterialKind::UnmaskedArmor) =>
                    {
                        Some(false)
                    }
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
        let target = material_name(target);
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

/// Skinning variants use their base material's declaration.
fn material_name(target: &str) -> &str {
    target.strip_suffix(".skinning").unwrap_or(target)
}

/// Resolves supported native material bases before authored overrides.
fn builtin(name: &str) -> Option<EntityRenderMaterialState> {
    if let Some(state) = assets::facts::pack::rules().material_state(name) {
        return Some(state);
    }
    let mut state = EntityRenderMaterialState::default();
    match name {
        "harness" => return builtin("entity_alphatest"),
        "entity" => {}
        "entity_static" => state.disable_overlay = true,
        "entity_nocull" => state.cull = false,
        "entity_alphatest_one_sided" => state.alpha_test = true,
        "entity_emissive" => state.emissive = true,
        "entity_emissive_alpha" => {
            state.emissive = true;
            state.alpha_test = true;
            state.cull = false;
        }
        "entity_emissive_alpha_one_sided" => {
            state.emissive = true;
            state.alpha_test = true;
        }
        "entity_alphablend" | "entity_alphablend_nocolor" => {
            state.blend = true;
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
