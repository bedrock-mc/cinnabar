use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{AssetError, EntityAssetSource, EntityRenderMaterialState};
use serde_json::{Map, Value};

use super::super::super::SourcePayloads;
use super::super::clip::read_json;

struct Definition {
    parent: Option<Box<str>>,
    fields: Map<String, Value>,
}

pub(super) struct MaterialStates {
    definitions: BTreeMap<Box<str>, Option<Definition>>,
}

impl MaterialStates {
    pub(super) fn load(
        root: &Path,
        payloads: &SourcePayloads,
        sources: &[EntityAssetSource],
    ) -> Result<Self, AssetError> {
        let mut definitions = BTreeMap::new();
        for source in sources.iter().filter(|source| {
            source.path.starts_with("materials/") && source.path.ends_with(".material")
        }) {
            let value = read_json(root, payloads, source)?;
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
                    state = builtin(name).unwrap_or_default();
                    break;
                }
            }
        }
        for fields in chain.into_iter().rev() {
            apply(fields, &mut state)?;
        }
        Some(state)
    }
}

fn builtin(name: &str) -> Option<EntityRenderMaterialState> {
    let mut state = EntityRenderMaterialState::default();
    match name {
        "entity" | "entity_static" => {}
        "entity_nocull" => state.cull = false,
        "entity_alphatest" | "skeleton" | "wither_boss" => {
            state.alpha_test = true;
            state.cull = false;
        }
        "charged_creeper" | "wither_boss_armor" => {
            state.alpha_test = true;
            state.cull = false;
            state.blend = true;
            state.additive = true;
        }
        "entity_alphatest_one_sided" => state.alpha_test = true,
        "entity_emissive" => state.emissive = true,
        "entity_emissive_alpha" | "entity_emissive_alpha_one_sided" => {
            state.emissive = true;
            state.alpha_test = true;
            state.cull = name.ends_with("_one_sided");
        }
        "experience_orb" => state.alpha_test = true,
        "entity_alphablend" | "slime_outer" => state.blend = true,
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
                _ => {}
            }
        }
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
