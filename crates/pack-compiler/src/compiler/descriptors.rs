use {super::*, assets::BlockFace};

pub(super) fn descriptor_for(
    fallback: &visuals::fallback::FallbackInventory,
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let TextureKey { key, rotate_uv } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    let (path, state_variant) = if is_model_visual(record) {
        pack.terrain.get_for_model_record(&key, record)?
    } else {
        pack.terrain.get_for_record_variant(&key, record)?
    };
    if !is_model_visual(record)
        && !is_liquid(record)
        && !fallback.contains(record)
        && source_is_deferred(pack, record, &key, path)
    {
        return None;
    }
    let mut flags = if rotate_uv {
        MATERIAL_FLAG_ROTATE_UV
    } else {
        0
    };
    if (record.flags.contains(BlockFlags::CUBE_GEOMETRY)
        && !record.flags.contains(BlockFlags::LEAF_MODEL))
        || record.name.as_ref() == "minecraft:grass_path"
    {
        flags |= pack.blocks.isotropic_face_flags(record)[face as usize];
    }
    if visuals::end_portal_frame::is_record(record)
        || visuals::bamboo::is_record(record)
        || matches!(
            record.name.as_ref(),
            "minecraft:flower_pot" | "minecraft:lantern" | "minecraft:soul_lantern"
        )
    {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if let Some(fallback_flags) = fallback.material_flags(record) {
        flags |= fallback_flags;
    } else if visuals::portal::is_record(record) || is_stained_glass_cube(record) {
        flags |= MATERIAL_FLAG_ALPHA_BLEND;
    } else if is_copper_grate(record) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if is_translucent_cube(record) {
        flags |= translucent_cube_material_flags(&record.name);
    } else if let Some(named_flags) = named_block_material_flags(record) {
        flags |= named_flags;
    } else if is_pane(record) {
        flags |= if record.name.contains("stained_glass_pane") {
            MATERIAL_FLAG_ALPHA_BLEND
        } else {
            MATERIAL_FLAG_ALPHA_CUTOUT
        };
    } else if is_fence(record) && record.name.contains("bamboo") {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if is_cutout_model_visual(record) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT | cutout_model_tint_flags(&record.name);
    } else if is_liquid(record) {
        flags |= liquid_material_flags(&record.name);
    } else if record.flags.contains(BlockFlags::LEAF_MODEL) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
        flags |= leaf_tint_flags(&record.name);
    }
    if record.name.as_ref() == "minecraft:glass" {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    }
    if record.name.as_ref() == "minecraft:grass_block" {
        flags |= match face {
            BlockFace::Down => 0,
            BlockFace::Up => MATERIAL_FLAG_GRASS_TINT,
            BlockFace::West | BlockFace::East | BlockFace::North | BlockFace::South => {
                MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
            }
        };
    }
    if record.name.as_ref() == "minecraft:leaf_litter" {
        // Grayscale art; the biome dry-foliage colour applies on every geometry route.
        flags |= MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE;
    }
    Some((
        Descriptor {
            state_variant,
            path: path.into(),
            texture_key: key.clone(),
            flags,
        },
        key,
    ))
}

#[cfg(test)]
#[path = "descriptors_tests.rs"]
mod tests;
