use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::*, assets::BlockFace};

pub(super) fn descriptor_for(
    fallback: &visuals::fallback::FallbackInventory,
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    if visuals::command_block::is_record(record) {
        return visuals::command_block::descriptor(pack, record, face);
    }
    if visuals::stem::is_record(record) {
        return visuals::stem::descriptor(pack, record, face);
    }
    if visuals::pitcher_crop::is_record(record) {
        return visuals::pitcher_crop::descriptor(pack, record, face);
    }
    if visuals::barrel::is_record(record) {
        return visuals::barrel::descriptor(pack, record, face);
    }
    // Facing cubes select the pack face drawn on this side; malformed states keep the authored face.
    let face = visuals::facing_cube::is_record(record)
        .then(|| visuals::facing_cube::source_face(record, face))
        .flatten()
        .map_or(face, |(source, _)| source);
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
        || block_render_fact(&record.name).kind == R::GrassPath
    {
        flags |= pack.blocks.isotropic_face_flags(record)[face as usize];
    }
    if let Some(model) = pack.blocks.component_model(record) {
        flags |= model.flags;
    } else if let Some(material) = pack.blocks.component_cross(&record.name) {
        flags |= material.flags;
    } else if block_render_fact(&record.name).kind.route().priority_cutout {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if let Some(fallback_flags) = fallback.material_flags(record) {
        flags |= fallback_flags;
        // Fallback alpha policy does not replace the compiled model tint.
        if is_cutout_model_visual(record) {
            flags |= cutout_model_tint_flags(&record.name);
        }
    } else if visuals::portal::is_record(record) || is_stained_glass_cube(record) {
        flags |= MATERIAL_FLAG_ALPHA_BLEND;
    } else if is_copper_grate(record) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if is_translucent_cube(record) {
        flags |= translucent_cube_material_flags(&record.name);
    } else if let Some(named_flags) = named_block_material_flags(record) {
        flags |= named_flags;
    } else if is_pane(record) {
        flags |= if block_render_fact(&record.name).kind == R::StainedPane {
            MATERIAL_FLAG_ALPHA_BLEND
        } else {
            MATERIAL_FLAG_ALPHA_CUTOUT
        };
    } else if is_fence(record) && block_render_fact(&record.name).kind == R::BambooFence {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    } else if is_cutout_model_visual(record) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT | cutout_model_tint_flags(&record.name);
    } else if is_liquid(record) {
        flags |= liquid_material_flags(record);
    } else if record.flags.contains(BlockFlags::LEAF_MODEL) {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
        flags |= leaf_tint_flags(&record.name);
    }
    if is_torch(record) {
        // Native torch tessellation lights the whole torch from its own cell in plain white.
        flags |= assets::MATERIAL_FLAG_DISABLE_AO | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING;
    }
    if block_render_fact(&record.name).kind == R::ClearGlass {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    }
    if block_render_fact(&record.name).kind == R::Grass {
        flags |= match face {
            BlockFace::Down => 0,
            BlockFace::Up => MATERIAL_FLAG_GRASS_TINT,
            BlockFace::West | BlockFace::East | BlockFace::North | BlockFace::South => {
                MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
            }
        };
    }
    if block_render_fact(&record.name).tint == assets::facts::BlockRenderTint::Dry {
        // Grayscale art; the biome dry-foliage colour applies on every geometry route.
        flags |= MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE;
    }
    Some((
        Descriptor {
            state_variant,
            flipbook_selector: pack_compile_input::respawn_anchor::is_record(record)
                .then_some((state_variant, state_variant)),
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
