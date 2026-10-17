//! Behavior-pack materials for the built-in cross geometry.

use std::{collections::BTreeSet, fs, path::Path};

use assets::{AssetError, ModelFamily, RegistryRecord};
use serde_json::Value;

use super::{
    BlockTextureMap,
    block::{BlockSourceEntry, TextureValue},
    parse::{MAX_TEXTURE_KEYS, read_json},
};

/// Material properties shared by the cross model and its terrain texture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrossMaterial {
    pub flags: u32,
    pub single_sided: bool,
}

/// Imports only stateless registry crosses with supported geometry and material semantics.
pub(super) fn install(
    blocks: &mut BlockTextureMap,
    behavior: &Path,
    records: &[RegistryRecord],
) -> Result<(), AssetError> {
    let eligible = records
        .iter()
        .filter(|record| {
            record.model_family == ModelFamily::Cross && record.canonical_state.as_ref() == "{}"
        })
        .map(|record| record.name.as_ref())
        .collect::<BTreeSet<_>>();
    let mut pending = vec![behavior.join("blocks")];
    let mut budget = MAX_TEXTURE_KEYS;
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(AssetError::Io {
                    path: directory,
                    source,
                });
            }
        };
        for entry in entries {
            let entry = entry.map_err(|source| AssetError::Io {
                path: directory.clone(),
                source,
            })?;
            budget = budget
                .checked_sub(1)
                .ok_or_else(|| AssetError::InvalidCompiledAssets {
                    detail: "behavior cross input exceeds its traversal bound".into(),
                })?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|source| AssetError::Io {
                path: path.clone(),
                source,
            })?;
            if kind.is_dir() {
                pending.push(path);
                continue;
            }
            if !kind.is_file()
                || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            {
                continue;
            }
            let value: Value = read_json(&path)?;
            let Some(block) = value.get("minecraft:block") else {
                continue;
            };
            let Some(identifier) = block
                .pointer("/description/identifier")
                .and_then(Value::as_str)
            else {
                continue;
            };
            if !eligible.contains(identifier) || block.get("permutations").is_some() {
                continue;
            }
            let Some((texture, material)) = material(block) else {
                continue;
            };
            // Component materials own this binding, even when blocks.json only supplies a sound.
            let name = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
            if !blocks.entries.contains_key(name) && blocks.entries.len() >= MAX_TEXTURE_KEYS {
                return Err(AssetError::TooManyTextureKeys {
                    count: blocks.entries.len() + 1,
                    max: MAX_TEXTURE_KEYS,
                });
            }
            if blocks
                .component_crosses
                .insert(identifier.into(), material)
                .is_some()
            {
                return Err(AssetError::DuplicateBlockKey {
                    path,
                    key: identifier.into(),
                });
            }
            blocks.entries.insert(
                name.into(),
                BlockSourceEntry {
                    textures: TextureValue::Key(texture.into()),
                    sound: None,
                    extra: Default::default(),
                },
            );
        }
    }
    Ok(())
}

/// Rejects geometry transforms and material features this compiler cannot represent.
fn material(block: &Value) -> Option<(&str, CrossMaterial)> {
    let components = block.get("components")?.as_object()?;
    if components.contains_key("minecraft:transformation") {
        return None;
    }
    let geometry = components.get("minecraft:geometry")?;
    let identifier = if let Some(identifier) = geometry.as_str() {
        identifier
    } else {
        let geometry = geometry.as_object()?;
        if geometry.len() != 1 {
            return None;
        }
        geometry.get("identifier")?.as_str()?
    };
    if identifier != "minecraft:geometry.cross" {
        return None;
    }
    let instances = components
        .get("minecraft:material_instances")?
        .as_object()?;
    if instances.len() != 1 {
        return None;
    }
    let instance = instances.get("*")?.as_object()?;
    if instance.keys().any(|key| {
        !matches!(
            key.as_str(),
            "texture" | "render_method" | "ambient_occlusion" | "face_dimming"
        )
    }) {
        return None;
    }
    let single_sided = match instance.get("render_method")?.as_str()? {
        "alpha_test_single_sided" => true,
        "alpha_test" => false,
        _ => return None,
    };
    let mut flags = assets::MATERIAL_FLAG_ALPHA_CUTOUT;
    if !single_sided {
        flags |= assets::MATERIAL_FLAG_TWO_SIDED;
    }
    if let Some(ao) = instance.get("ambient_occlusion") {
        match ao.as_f64()? {
            0.0 => flags |= assets::MATERIAL_FLAG_DISABLE_AO,
            1.0 => {}
            _ => return None,
        }
    }
    if let Some(dimming) = instance.get("face_dimming")
        && !dimming.as_bool()?
    {
        flags |= assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING;
    }
    let texture = instance.get("texture")?.as_str()?.trim();
    (!texture.is_empty()).then_some((
        texture,
        CrossMaterial {
            flags,
            single_sided,
        },
    ))
}
