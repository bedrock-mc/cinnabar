use super::super::*;
use super::state::exact_tagged_int;

pub(in crate::compiler) fn is_resin_clump(record: &RegistryRecord) -> bool {
    matches!(record.model_family, ModelFamily::ResinClump)
        && record.name.as_ref() == "minecraft:resin_clump"
}

pub(in crate::compiler) fn is_resin_clump_name(name: &str) -> bool {
    name == "minecraft:resin_clump"
}

pub(in crate::compiler) fn exact_resin_clump_state(record: &RegistryRecord) -> Option<u32> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if state.len() != 1 {
        return None;
    }
    let connections = exact_tagged_int(state.get("multi_face_direction_bits")?, 63)?;
    let mask = 1 << (ModelStateField::Connections as u8 - 1);
    if record.model_state.mask() != mask
        || record.model_state.get(ModelStateField::Connections) != Some(connections)
    {
        return None;
    }
    Some(connections)
}

pub(in crate::compiler) fn is_resin_clump_record(record: &RegistryRecord) -> bool {
    is_resin_clump_name(&record.name)
        && record.model_family == ModelFamily::ResinClump
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.face_coverage == 0
        && record.collision_seed.shape_id == 0
        && record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.is_empty()
        && exact_resin_clump_state(record).is_some()
}

pub(in crate::compiler) fn resin_clump_inventory_is_exact(records: &[RegistryRecord]) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_resin_clump_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 64 {
        return false;
    }
    let mut seen = [false; 64];
    for record in selected {
        if !is_resin_clump_record(record) {
            return false;
        }
        let Some(connections) = exact_resin_clump_state(record) else {
            return false;
        };
        if record.sequential_id != 2930 + connections || seen[connections as usize] {
            return false;
        }
        seen[connections as usize] = true;
    }
    seen.into_iter().all(|present| present)
}

pub(in crate::compiler) fn resin_clump_material_descriptor(
    pack: &PackSources,
) -> Option<(Descriptor, Box<str>)> {
    let key = pack.blocks.get_exact_scalar("resin_clump")?;
    if key != "resin_clump" {
        return None;
    }
    let path = pack.terrain.get_exact_static_no_tint(key)?;
    if path != "textures/blocks/resin_clump"
        || pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile.as_ref() == key || flipbook.texture_path.as_ref() == path
        })
    {
        return None;
    }
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.into(),
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        },
        key.into(),
    ))
}
