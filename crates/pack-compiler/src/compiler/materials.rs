//! Material identities retain the source route even when their GPU pixels are shared.

use super::*;

/// Compiles materials without merging distinct catalog keys, state variants, or raster paths.
pub(super) fn compile_materials(
    descriptor_keys: &BTreeMap<Descriptor, Box<str>>,
    plan: &AnimationPlan,
    alpha_paths: &BTreeSet<Box<str>>,
) -> Result<CompiledMaterials, AssetError> {
    let mut materials = vec![Material {
        texture: TextureRef::DIAGNOSTIC,
        flags: 0,
        animation: NO_ANIMATION,
        ..assets::Material::unvaried()
    }];
    let mut material_by_value = BTreeMap::new();
    let mut material_by_descriptor = BTreeMap::new();

    for descriptor in descriptor_keys.keys() {
        if alpha_paths.contains(&descriptor.path) && !descriptor_supports_alpha(descriptor) {
            continue;
        }
        let key = &descriptor.texture_key;
        let candidates = plan
            .animations
            .iter()
            .enumerate()
            .filter(|(_, source)| {
                source.atlas_tile.as_ref() == key.as_ref() && source.source_path == descriptor.path
            })
            .map(|(index, source)| (index, source.atlas_index, source.atlas_tile_variant));
        let animation = pack_compile_input::pack::select_flipbook_index(
            candidates,
            descriptor.flipbook_selector,
            &descriptor.path,
        )?
        .map(|index| u32::try_from(index).expect("bounded flipbook count"));
        let texture = if let Some(animation) = animation {
            let source = &plan.animations[animation as usize];
            plan.frames[source.frame_start as usize]
        } else if let Some(&texture) = plan
            .static_refs
            .get(&descriptor.path)
            .or_else(|| plan.strip_first_refs.get(&descriptor.path))
        {
            texture
        } else {
            continue;
        };
        let animated = animation.map(|id| {
            let source = &plan.animations[id as usize];
            (source.atlas_index, source.atlas_tile_variant)
        });
        let destination = super::material_keys::destination(descriptor, animated);
        let animation = animation.unwrap_or(NO_ANIMATION);
        let value = (
            texture,
            descriptor.flags,
            animation,
            descriptor.texture_key.as_ref(),
            descriptor.state_variant,
            descriptor.path.as_ref(),
            destination,
        );
        let material = if let Some(&material) = material_by_value.get(&value) {
            material
        } else {
            if materials.len() >= MAX_MATERIALS {
                return Err(AssetError::TooManyMaterials {
                    count: materials.len() + 1,
                    max: MAX_MATERIALS,
                });
            }
            let material =
                u32::try_from(materials.len()).map_err(|_| AssetError::BlobSizeOverflow {
                    section: "material",
                })?;
            materials.push(Material {
                texture,
                flags: descriptor.flags,
                animation,
                ..assets::Material::unvaried()
            });
            material_by_value.insert(value, material);
            material
        };
        material_by_descriptor.insert(descriptor.clone(), material);
    }
    Ok((materials.into_boxed_slice(), material_by_descriptor))
}

#[cfg(test)]
mod tests;
