use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::*, assets::BlockFace};

pub(in crate::compiler) const fn is_terrestrial_cross(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Cross | ModelFamily::Crop)
}

pub(in crate::compiler) const fn is_torch(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Torch)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) fn is_chain(record: &RegistryRecord) -> bool {
    matches!(record.contributor_role, ContributorRole::Primary)
        && block_render_fact(&record.name).kind == R::Chain
}

/// Full cubes whose source art carries alpha (or that lack cube flags in the registry) and draw as one unit cube.
pub(in crate::compiler) fn is_translucent_cube(record: &RegistryRecord) -> bool {
    matches!(record.contributor_role, ContributorRole::Primary)
        && !record.flags.contains(BlockFlags::AIR)
        && matches!(
            block_render_fact(&record.name).kind,
            R::Ice | R::FrostedIce | R::Slime | R::Honey | R::TintedGlass | R::PowderSnow
        )
}

/// Powder snow's art is fully opaque, so it stays on the opaque model path.
pub(in crate::compiler) fn translucent_cube_material_flags(name: &str) -> u32 {
    if block_render_fact(name).kind == R::PowderSnow {
        0
    } else {
        MATERIAL_FLAG_ALPHA_BLEND
    }
}

/// Amethyst clusters and buds drawn with crossed sprite planes.
pub(in crate::compiler) fn is_crystal(record: &RegistryRecord) -> bool {
    matches!(record.contributor_role, ContributorRole::Primary)
        && block_render_fact(&record.name).kind == R::Crystal
}

pub(in crate::compiler) fn is_ladder(record: &RegistryRecord) -> bool {
    matches!(record.contributor_role, ContributorRole::Primary)
        && block_render_fact(&record.name).kind == R::Ladder
}

pub(in crate::compiler) const fn is_rail(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Rail)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) fn is_aquatic_cross(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Seagrass
}

pub(in crate::compiler) fn is_cross_visual(record: &RegistryRecord) -> bool {
    is_terrestrial_cross(record) || is_aquatic_cross(record)
}

pub(in crate::compiler) fn is_kelp(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Kelp
}

pub(in crate::compiler) fn is_flowerbed(record: &RegistryRecord) -> bool {
    record.model_family == ModelFamily::FlowerBed
}

pub(in crate::compiler) fn is_vine(record: &RegistryRecord) -> bool {
    record.model_family == ModelFamily::Vine
}

pub(in crate::compiler) fn is_glow_lichen(record: &RegistryRecord) -> bool {
    record.model_family == ModelFamily::GlowLichen
}

pub(in crate::compiler) fn is_sculk_vein(record: &RegistryRecord) -> bool {
    record.model_family == ModelFamily::SculkVein
}

pub(in crate::compiler) fn is_multiface(record: &RegistryRecord) -> bool {
    is_glow_lichen(record) || is_sculk_vein(record) || is_resin_clump(record)
}

pub(in crate::compiler) const fn is_door(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Door)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_trapdoor(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Trapdoor)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_wall(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Wall)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_pressure_plate(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::PressurePlate)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_button(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Button)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_carpet(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Carpet)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) fn is_pale_moss_carpet(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::PaleMossCarpet
}

pub(in crate::compiler) const fn is_gate(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Gate)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_pane(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Pane)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) fn is_stained_glass_cube(record: &RegistryRecord) -> bool {
    record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && block_render_fact(&record.name).kind == R::StainedGlass
}

/// Vanilla glass is not solid: faces touching it still draw, and only glass culls glass.
pub(in crate::compiler) fn is_clear_glass_cube(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::ClearGlass
        && record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && record.flags.contains(BlockFlags::CUBE_GEOMETRY)
        && !record.flags.contains(BlockFlags::AIR)
}

pub(in crate::compiler) fn is_ordinary_stained_glass_name(name: &str) -> bool {
    block_render_fact(name).kind == R::StainedGlass
}

pub(in crate::compiler) fn is_copper_grate(record: &RegistryRecord) -> bool {
    record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.is_empty()
        && !record.flags.contains(BlockFlags::AIR)
        && record.flags.contains(BlockFlags::CUBE_GEOMETRY)
        && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        && !record.flags.contains(BlockFlags::LEAF_MODEL)
        && is_copper_grate_name(&record.name)
}

pub(in crate::compiler) fn is_copper_grate_name(name: &str) -> bool {
    block_render_fact(name).kind == R::CopperGrate
}

pub(in crate::compiler) const fn is_fence(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Fence)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_sign(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Sign)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_slab(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Slab)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) const fn is_stair(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Stair)
        && matches!(record.contributor_role, ContributorRole::Primary)
}

pub(in crate::compiler) fn is_cutout_model_visual(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind.route().cutout
        || visuals::heavy_core::is_record(record)
        || matches!(
            record.model_family,
            ModelFamily::Cross
                | ModelFamily::Crop
                | ModelFamily::FlowerBed
                | ModelFamily::Vine
                | ModelFamily::GlowLichen
                | ModelFamily::SculkVein
                | ModelFamily::ResinClump
                | ModelFamily::Door
                | ModelFamily::Trapdoor
                | ModelFamily::Torch
                | ModelFamily::Rail
        )
}

pub(in crate::compiler) fn is_model_visual(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind.route().model
        || record
            .traits
            .contains(assets::registry::BlockTraits::LANTERN_LIKE)
        || is_cutout_model_visual(record)
        || matches!(
            record.model_family,
            ModelFamily::Slab
                | ModelFamily::Stair
                | ModelFamily::Wall
                | ModelFamily::PressurePlate
                | ModelFamily::Button
                | ModelFamily::Gate
                | ModelFamily::Carpet
                | ModelFamily::Pane
                | ModelFamily::Fence
                | ModelFamily::Sign
        )
}

pub(in crate::compiler) const fn is_liquid(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::Liquid)
        && matches!(record.contributor_role, ContributorRole::LiquidAdditional)
}

pub(in crate::compiler) fn is_supported_liquid(record: &RegistryRecord) -> bool {
    is_liquid(record)
        && (record
            .traits
            .intersects(assets::registry::BlockTraits::WATER | assets::registry::BlockTraits::LAVA)
            || matches!(block_render_fact(&record.name).kind, R::Water | R::Lava))
}

pub(in crate::compiler) fn liquid_material_flags(record: &RegistryRecord) -> u32 {
    if record.traits.contains(assets::registry::BlockTraits::WATER)
        || block_render_fact(&record.name).kind == R::Water
    {
        MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
    } else if record.traits.contains(assets::registry::BlockTraits::LAVA)
        || block_render_fact(&record.name).kind == R::Lava
    {
        MATERIAL_FLAG_LIQUID_DEPTH_WRITE
    } else {
        0
    }
}

pub(in crate::compiler) fn cross_texture_face(record: &RegistryRecord) -> BlockFace {
    if block_render_fact(&record.name).kind == R::BerryBush {
        return match canonical_state_u32(
            &record.canonical_state,
            assets::facts::BlockProperty::Growth.name(),
        )
        .unwrap_or(0)
        {
            0 => BlockFace::Down,
            1 => BlockFace::Up,
            2 => BlockFace::North,
            3 => BlockFace::South,
            4 => BlockFace::West,
            _ => BlockFace::East,
        };
    }
    if canonical_state_u32(
        &record.canonical_state,
        assets::facts::BlockProperty::UpperBlockBit.name(),
    ) == Some(1)
    {
        BlockFace::Up
    } else {
        BlockFace::Down
    }
}

pub(in crate::compiler) fn canonical_state_u32(state: &str, property: &str) -> Option<u32> {
    let document =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(state).ok()?;
    let value = document.get(property)?;
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

pub(in crate::compiler) fn canonical_state_str(state: &str, property: &str) -> Option<Box<str>> {
    let document =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(state).ok()?;
    let value = document.get(property)?;
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
        .as_str()
        .map(Into::into)
}

pub(in crate::compiler) fn aquatic_cross_faces(record: &RegistryRecord) -> Option<[BlockFace; 2]> {
    match block_render_fact(&record.name).kind {
        R::Seagrass => {
            match canonical_state_str(
                &record.canonical_state,
                assets::facts::BlockProperty::SeaGrassType.name(),
            )?
            .as_ref()
            {
                "default" => Some([BlockFace::Up, BlockFace::Up]),
                "double_bot" => Some([BlockFace::Down, BlockFace::South]),
                "double_top" => Some([BlockFace::East, BlockFace::West]),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(in crate::compiler) fn cutout_model_tint_flags(name: &str) -> u32 {
    tint_flags(assets::facts::block_render_fact(name).tint)
}

pub(in crate::compiler) fn leaf_tint_flags(name: &str) -> u32 {
    tint_flags(assets::facts::block_render_fact(name).tint)
}

pub(in crate::compiler) fn record_has_deferred_material(
    pack: &PackSources,
    record: &RegistryRecord,
) -> bool {
    BlockFace::ALL.into_iter().any(|face| {
        let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
        let Some(key) = key else {
            return false;
        };
        let Some(path) = pack.terrain.get_for_record(&key, record) else {
            return false;
        };
        source_is_deferred(pack, record, &key, path)
    })
}

pub(in crate::compiler) fn source_is_deferred(
    pack: &PackSources,
    record: &RegistryRecord,
    key: &str,
    _path: &str,
) -> bool {
    block_render_fact(&record.name).kind != R::Grass && pack.terrain.requires_tint(key)
}

/// Maps the compiled tint domain to the shared material contract.
fn tint_flags(tint: assets::facts::BlockRenderTint) -> u32 {
    use assets::facts::BlockRenderTint as T;
    match tint {
        T::None => 0,
        T::Grass => MATERIAL_FLAG_GRASS_TINT,
        T::Foliage => MATERIAL_FLAG_FOLIAGE_TINT,
        T::Birch => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE,
        T::Evergreen => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        T::Dry => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE,
    }
}
