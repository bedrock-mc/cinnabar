use super::super::*;
use super::state::exact_tagged_int;

pub(in crate::compiler) fn is_cactus_name(name: &str) -> bool {
    name == "minecraft:cactus"
}

pub(in crate::compiler) fn exact_cactus_age(record: &RegistryRecord) -> Option<u32> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if state.len() != 1 {
        return None;
    }
    let age = exact_tagged_int(state.get("age")?, 15)?;
    let mask = 1 << (ModelStateField::Growth as u8 - 1);
    if record.model_state.mask() != mask
        || record.model_state.get(ModelStateField::Growth) != Some(age)
    {
        return None;
    }
    Some(age)
}

pub(in crate::compiler) fn is_cactus_record(record: &RegistryRecord) -> bool {
    is_cactus_name(&record.name)
        && record.model_family == ModelFamily::Cuboid
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.face_coverage == 0
        && record.collision_seed.shape_id == 84
        && record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.as_ref()
            == [assets::CollisionBox {
                min_x: 6_250_000,
                min_y: 0,
                min_z: 6_250_000,
                max_x: 93_750_000,
                max_y: 100_000_000,
                max_z: 93_750_000,
            }]
        && exact_cactus_age(record).is_some()
}

pub(in crate::compiler) fn cactus_inventory_is_exact(records: &[RegistryRecord]) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_cactus_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 16 {
        return false;
    }
    let mut seen = [false; 16];
    for record in selected {
        if !is_cactus_record(record) {
            return false;
        }
        let Some(age) = exact_cactus_age(record) else {
            return false;
        };
        if record.sequential_id != 13_606 + age || seen[age as usize] {
            return false;
        }
        seen[age as usize] = true;
    }
    seen.into_iter().all(|present| present)
}

pub(in crate::compiler) fn cactus_material_descriptors(
    pack: &PackSources,
) -> Option<[(Descriptor, Box<str>); 3]> {
    if pack.blocks.get_exact_side_caps("cactus")? != ["cactus_side", "cactus_bottom", "cactus_top"]
    {
        return None;
    }
    let routes = [
        ("cactus_side", "textures/blocks/cactus_side"),
        ("cactus_bottom", "textures/blocks/cactus_bottom"),
        ("cactus_top", "textures/blocks/cactus_top"),
    ];
    let mut descriptors = Vec::with_capacity(3);
    for (key, expected_path) in routes {
        let path = pack.terrain.get_exact_static_no_tint(key)?;
        if path != expected_path
            || pack.flipbooks.iter().any(|flipbook| {
                flipbook.atlas_tile.as_ref() == key || flipbook.texture_path.as_ref() == path
            })
        {
            return None;
        }
        descriptors.push((
            Descriptor {
                path: path.into(),
                texture_key: key.into(),
                flags: MATERIAL_FLAG_ALPHA_CUTOUT,
            },
            key.into(),
        ));
    }
    descriptors.try_into().ok()
}
