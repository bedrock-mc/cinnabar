//! Bounded authored block models admitted by compiled render facts.
use super::{
    PackSources,
    parse::{MAX_TEXTURE_KEYS, read_json},
};
use assets::{
    AssetError, RegistryRecord,
    facts::{BlockRenderKind, block_render_fact},
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

/// One state's resolved geometry, texture and raster policy.
#[derive(Debug, Clone)]
pub struct ComponentModel {
    pub identifier: Box<str>,
    pub document: Arc<Value>,
    pub texture: Box<str>,
    pub material_instance: Box<str>,
    pub flags: u32,
    pub rotation: u8,
}

/// Reads finite JSON inputs without following directory links or exceeding the catalog bound.
fn documents(root: &Path) -> Result<Vec<Value>, AssetError> {
    let mut pending = vec![root.to_owned()];
    let mut result = Vec::new();
    let mut count = 0;
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
        let mut entries =
            entries
                .collect::<Result<Vec<_>, _>>()
                .map_err(|source| AssetError::Io {
                    path: directory,
                    source,
                })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            count += 1;
            if count > MAX_TEXTURE_KEYS {
                return Err(AssetError::InvalidCompiledAssets {
                    detail: "authored model input exceeds its traversal bound".into(),
                });
            }
            let path = entry.path();
            let kind = entry.file_type().map_err(|source| AssetError::Io {
                path: path.clone(),
                source,
            })?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                result.push(read_json(&path)?);
            }
        }
    }
    Ok(result)
}

/// Evaluates the bounded equality/conjunction grammar used by admitted vanilla model permutations.
fn matches(condition: &str, state: &Value) -> Option<bool> {
    let mut matched = true;
    for term in condition.split("&&") {
        let term = term.trim().strip_prefix("query.block_state('")?;
        let (property, comparison) = term.split_once("')")?;
        let expected = comparison.trim().strip_prefix("==")?.trim();
        let actual = state.get(property)?.get("value")?;
        let equal = if let Some(text) = expected
            .strip_prefix('\'')
            .and_then(|text| text.strip_suffix('\''))
        {
            actual.as_str()? == text
        } else {
            actual.as_i64()? == expected.parse::<i64>().ok()?
        };
        matched &= equal;
    }
    Some(matched)
}

/// Resolves only state models with quarter-turn transforms and one alpha-tested material.
fn resolve(
    block: &Value,
    record: &RegistryRecord,
    models: &BTreeMap<String, Arc<Value>>,
) -> Option<ComponentModel> {
    let mut components = block.get("components")?.as_object()?.clone();
    let state: Value = serde_json::from_str(&record.canonical_state).ok()?;
    if let Some(permutations) = block.get("permutations") {
        let permutations = permutations.as_array()?;
        if permutations.len() > 256 {
            return None;
        }
        for permutation in permutations {
            if matches(permutation.get("condition")?.as_str()?, &state)? {
                components.extend(permutation.get("components")?.as_object()?.clone());
            }
        }
    }
    let geometry = components.get("minecraft:geometry")?;
    let identifier = geometry
        .as_str()
        .or_else(|| geometry.get("identifier")?.as_str())?;
    let instances = components
        .get("minecraft:material_instances")?
        .as_object()?;
    if instances.len() != 1 {
        return None;
    }
    let (instance, material) = instances.iter().next()?;
    let material = material.as_object()?;
    if material.get("render_method")?.as_str()? != "alpha_test_single_sided"
        || material.get("ambient_occlusion")?.as_f64()? != 0.0
    {
        return None;
    }
    let mut flags = assets::MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_DISABLE_AO;
    if material.get("face_dimming").and_then(Value::as_bool) == Some(false) {
        flags |= assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING;
    }
    let rotation = if let Some(transform) = components.get("minecraft:transformation") {
        let transform = transform.as_object()?;
        if transform.len() != 1 {
            return None;
        }
        let angles = transform.get("rotation")?.as_array()?;
        if angles.len() != 3 || angles[0].as_f64()? != 0.0 || angles[2].as_f64()? != 0.0 {
            return None;
        }
        let degrees = angles[1].as_f64()?;
        if ![0.0, 90.0, 180.0, 270.0].contains(&degrees) {
            return None;
        }
        (degrees / 90.0) as u8
    } else {
        0
    };
    Some(ComponentModel {
        identifier: identifier.into(),
        document: models.get(identifier)?.clone(),
        texture: material.get("texture")?.as_str()?.into(),
        material_instance: instance.as_str().into(),
        flags,
        rotation,
    })
}

/// Imports model-bearing behavior definitions only for explicitly admitted render identities.
pub(super) fn install(
    root: &Path,
    behavior: &Path,
    pack: &mut PackSources,
    records: &[RegistryRecord],
) -> Result<(), AssetError> {
    let admitted = records
        .iter()
        .filter(|record| block_render_fact(&record.name).kind == BlockRenderKind::AuthoredModel)
        .collect::<Vec<_>>();
    if admitted.is_empty() {
        return Ok(());
    }
    let mut models = BTreeMap::new();
    for document in documents(&root.join("models/blocks"))? {
        let document = Arc::new(document);
        if let Some(entries) = document.get("minecraft:geometry").and_then(Value::as_array) {
            for entry in entries {
                if let Some(identifier) = entry
                    .pointer("/description/identifier")
                    .and_then(Value::as_str)
                {
                    models.insert(identifier.to_owned(), document.clone());
                }
            }
        }
    }
    for document in documents(&behavior.join("blocks"))? {
        let Some(block) = document.get("minecraft:block") else {
            continue;
        };
        let Some(identifier) = block
            .pointer("/description/identifier")
            .and_then(Value::as_str)
        else {
            continue;
        };
        for record in admitted
            .iter()
            .filter(|record| record.name.as_ref() == identifier)
        {
            if let Some(model) = resolve(block, record, &models) {
                if pack.terrain.get(&model.texture).is_none() {
                    return Err(AssetError::MissingTerrainKey {
                        block: identifier.into(),
                        key: model.texture.clone(),
                    });
                }
                if pack
                    .blocks
                    .component_models
                    .insert(record.sequential_id, model)
                    .is_some()
                {
                    return Err(AssetError::InvalidCompiledAssets {
                        detail: "duplicate authored block definition".into(),
                    });
                }
            }
        }
    }
    Ok(())
}
