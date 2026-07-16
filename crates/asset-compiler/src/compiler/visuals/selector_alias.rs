use super::super::*;
use super::state::{exact_tagged_byte, exact_tagged_int, exact_tagged_string};

const SELECTOR_ALIAS_CUBE_NAMES: [&str; 7] = [
    "minecraft:bone_block",
    "minecraft:chiseled_quartz_block",
    "minecraft:hay_block",
    "minecraft:purpur_block",
    "minecraft:quartz_block",
    "minecraft:smooth_quartz",
    "minecraft:tnt",
];

pub(in crate::compiler) fn is_selector_alias_cube_name(name: &str) -> bool {
    SELECTOR_ALIAS_CUBE_NAMES.binary_search(&name).is_ok()
}

pub(in crate::compiler) fn exact_selector_alias_cube_state(record: &RegistryRecord) -> Option<u32> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if record.name.as_ref() == "minecraft:tnt" {
        if state.len() != 1 || record.model_state.mask() != 0 {
            return None;
        }
        return Some(13_112 + u32::from(exact_tagged_byte(state.get("explode_bit")?, 1)?));
    }

    let has_deprecated = matches!(
        record.name.as_ref(),
        "minecraft:hay_block" | "minecraft:bone_block"
    );
    if state.len() != if has_deprecated { 2 } else { 1 } {
        return None;
    }
    let (axis_index, orientation) = match exact_tagged_string(state.get("pillar_axis")?)? {
        "y" => (0, 1),
        "x" => (1, 0),
        "z" => (2, 2),
        _ => return None,
    };
    let orientation_mask = 1 << (ModelStateField::Orientation as u8 - 1);
    if record.model_state.mask() != orientation_mask
        || record.model_state.get(ModelStateField::Orientation) != Some(orientation)
    {
        return None;
    }
    let (base, stride, deprecated) = match record.name.as_ref() {
        "minecraft:hay_block" => (2_907, 4, exact_tagged_int(state.get("deprecated")?, 3)?),
        "minecraft:bone_block" => (6_465, 4, exact_tagged_int(state.get("deprecated")?, 3)?),
        "minecraft:quartz_block" => (5_442, 1, 0),
        "minecraft:smooth_quartz" => (7_081, 1, 0),
        "minecraft:chiseled_quartz_block" => (14_685, 1, 0),
        "minecraft:purpur_block" => (15_344, 1, 0),
        _ => return None,
    };
    Some(base + axis_index * stride + deprecated)
}

pub(in crate::compiler) fn is_selector_alias_cube_record(record: &RegistryRecord) -> bool {
    is_selector_alias_cube_name(&record.name)
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && record.flags == BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        && record.face_coverage == 0x3f
        && record.collision_seed.shape_id == 1
        && record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.as_ref()
            == [assets::CollisionBox {
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
                ..assets::CollisionBox::default()
            }]
        && exact_selector_alias_cube_state(record) == Some(record.sequential_id)
}

pub(in crate::compiler) fn selector_alias_cube_inventory_is_exact(
    records: &[RegistryRecord],
) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_selector_alias_cube_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 38 {
        return false;
    }
    let mut ids = BTreeSet::new();
    selected
        .into_iter()
        .all(|record| is_selector_alias_cube_record(record) && ids.insert(record.sequential_id))
}

pub(in crate::compiler) fn selector_alias_cube_material_descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<[(Descriptor, Box<str>); 6]> {
    let block_name = record.name.strip_prefix("minecraft:")?;
    let exact_route = match block_name {
        "bone_block" => {
            pack.blocks.get_exact_pillar(block_name)?
                == ["bone_block_top", "bone_block_top", "bone_block_side"]
        }
        "chiseled_quartz_block" => {
            pack.blocks.get_exact_pillar(block_name)?
                == [
                    "chiseled_quartz_block_top",
                    "chiseled_quartz_block_top",
                    "chiseled_quartz_block_side",
                ]
        }
        "hay_block" => {
            pack.blocks.get_exact_pillar(block_name)?
                == ["hayblock_top", "hayblock_top", "hayblock_side"]
        }
        "quartz_block" => {
            pack.blocks.get_exact_pillar(block_name)?
                == [
                    "flattened_quartz_block_top",
                    "flattened_quartz_block_top",
                    "flattened_quartz_block_side",
                ]
        }
        "tnt" => {
            pack.blocks.get_exact_pillar(block_name)?
                == [
                    "flattened_tnt_bottom",
                    "flattened_tnt_top",
                    "flattened_tnt_side",
                ]
        }
        "purpur_block" => pack.blocks.get_exact_scalar(block_name)? == "flattened_purpur_block",
        "smooth_quartz" => pack.blocks.get_exact_scalar(block_name)? == "smooth_quartz",
        _ => false,
    };
    if !exact_route {
        return None;
    }

    let mut descriptors = Vec::with_capacity(6);
    for face in BlockFace::ALL {
        let TextureKey { key, rotate_uv } = resolve_texture_key(&pack.blocks, record, face);
        let key = key?;
        let path = pack.terrain.get_exact_static_no_tint(&key)?;
        let expected_path = match key.as_ref() {
            "bone_block_top" => "textures/blocks/bone_block_top",
            "bone_block_side" => "textures/blocks/bone_block_side",
            "chiseled_quartz_block_top" => "textures/blocks/quartz_block_chiseled_top",
            "chiseled_quartz_block_side" => "textures/blocks/quartz_block_chiseled",
            "hayblock_top" => "textures/blocks/hay_block_top",
            "hayblock_side" => "textures/blocks/hay_block_side",
            "flattened_purpur_block" => "textures/blocks/purpur_block",
            "flattened_quartz_block_top" => "textures/blocks/quartz_block_top",
            "flattened_quartz_block_side" => "textures/blocks/quartz_block_side",
            "smooth_quartz" => "textures/blocks/quartz_block_bottom",
            "flattened_tnt_bottom" => "textures/blocks/tnt_bottom",
            "flattened_tnt_side" => "textures/blocks/tnt_side",
            "flattened_tnt_top" => "textures/blocks/tnt_top",
            _ => return None,
        };
        if path != expected_path
            || pack.flipbooks.iter().any(|flipbook| {
                flipbook.atlas_tile.as_ref() == key.as_ref()
                    || flipbook.texture_path.as_ref() == path
            })
        {
            return None;
        }
        descriptors.push((
            Descriptor {
                path: path.into(),
                texture_key: key.clone(),
                flags: u32::from(rotate_uv) * MATERIAL_FLAG_ROTATE_UV,
            },
            key,
        ));
    }
    descriptors.try_into().ok()
}
