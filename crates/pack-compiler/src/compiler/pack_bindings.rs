//! Carrier source facts used by optional terrain catalogs and block bindings.

use super::*;
use assets::material_keys::{BlockTextureSource, MaterialTextureSource};

/// Keeps state selectors, weighted leaves, and per-block identities in the sidecar.
pub(super) fn retain(
    keys: MaterialKeys,
    pack: (&PackSources, &Path),
    descriptors: &BTreeMap<Descriptor, u32>,
    materials: &[Material],
    blocks: (&[RegistryRecord], &[BlockVisual]),
    lily_pad_keys: &[(u32, Box<str>)],
    seasonal_copies: &[(u32, u32)],
) -> Result<MaterialKeys, AssetError> {
    let (pack, root) = pack;
    let (records, visuals) = blocks;
    let mut sources = BTreeMap::new();
    for (descriptor, &id) in descriptors {
        sources.insert(
            id,
            MaterialTextureSource {
                key: descriptor.texture_key.clone(),
                variant: descriptor.state_variant,
                path: descriptor.path.clone(),
                raster: raster_identity(root, &descriptor.path)?,
                baked_tint: None,
            },
        );
        let material = materials[id as usize];
        if material.variation_count != 0
            && let Some(choices) = pack
                .terrain
                .position_variations
                .get(&(descriptor.texture_key.clone(), descriptor.state_variant))
        {
            for (index, choice) in choices.iter().enumerate() {
                sources.insert(
                    material.variation_start + index as u32,
                    MaterialTextureSource {
                        key: descriptor.texture_key.clone(),
                        variant: descriptor.state_variant,
                        path: choice.path.clone(),
                        raster: raster_identity(root, &choice.path)?,
                        baked_tint: None,
                    },
                );
            }
        }
    }
    for (id, key) in lily_pad_keys {
        if let Some(path) = pack.terrain.get(key) {
            sources.insert(
                *id,
                MaterialTextureSource {
                    key: key.clone(),
                    variant: 0,
                    path: path.into(),
                    raster: raster_identity(root, path)?,
                    baked_tint: keys.fixed_tints().find_map(|(tinted_key, tint)| {
                        (tinted_key == key.as_ref()).then_some(tint)
                    }),
                },
            );
        }
    }
    for &(copy, original) in seasonal_copies {
        if let Some(source) = sources.get(&original).cloned() {
            sources.insert(copy, source);
        }
    }
    let blocks = records
        .iter()
        .map(|record| {
            let (binding_name, source_faces, source_rotations) =
                pack_compile_input::pack::runtime_binding_identity(&pack.blocks, record);
            BlockTextureSource {
                visual: record.sequential_id,
                name: record.name.clone(),
                binding_name,
                source_faces,
                source_rotations,
                cube_binding_eligible: cube_binding_eligible(
                    pack,
                    record,
                    &visuals[record.sequential_id as usize],
                    materials,
                ),
                carried_fallback_keys: BlockFace::ALL.map(|face| {
                    pack_compile_input::pack::resolve_carried_fallback_key(
                        &pack.blocks,
                        record,
                        face,
                    )
                }),
                carried_keys: BlockFace::ALL.map(|face| {
                    pack_compile_input::pack::resolve_carried_face_key(&pack.blocks, record, face)
                        .map(Into::into)
                }),
            }
        })
        .collect::<Vec<_>>();
    Ok(keys.with_sources(pack.terrain.runtime_entries(), sources, blocks))
}

/// Keeps the filename selected by compilation without retaining the local pack directory.
fn raster_identity(root: &Path, path: &str) -> Result<Box<str>, AssetError> {
    let resolved = static_texture_path(root, path, path)?;
    resolved
        .strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .map(|path| path.trim_start_matches("./").into())
        .ok_or_else(|| AssetError::InvalidCompiledAssets {
            detail: "resolved terrain raster is outside the source pack".into(),
        })
}

/// Admits one-to-one full-cube world and carried faces as a single rebinding route.
fn cube_binding_eligible(
    pack: &PackSources,
    record: &RegistryRecord,
    visual: &BlockVisual,
    materials: &[Material],
) -> bool {
    record.model_family == assets::ModelFamily::Cube
        && visual.kind == VisualKind::Cube
        && visual.support == VisualSupport::Exact
        && visual.model_template == assets::NO_MODEL_TEMPLATE
        && visual.variant == 0
        && visual.flags.contains(BlockFlags::CUBE_GEOMETRY)
        && !visual.flags.contains(BlockFlags::LEAF_MODEL)
        && pack_compile_input::pack::has_runtime_cube_face_mapping(record)
        && visual.faces.iter().all(|&id| {
            id != assets::DIAGNOSTIC_MATERIAL
                && materials
                    .get(id as usize)
                    .is_some_and(|material| material.flags & assets::MATERIAL_FLAG_TINT_MASK == 0)
        })
        && BlockFace::ALL.into_iter().all(|face| {
            pack_compile_input::pack::resolve_carried_face_key(&pack.blocks, record, face)
                .map(Into::<Box<str>>::into)
                .or_else(|| {
                    pack_compile_input::pack::resolve_carried_fallback_key(
                        &pack.blocks,
                        record,
                        face,
                    )
                })
                .is_some_and(|key| !pack.terrain.requires_tint(&key))
        })
}

#[cfg(test)]
mod tests;
