use super::super::*;

/// Finds the pinned texture and wire contract for a mineral during compilation.
fn identity(record: &RegistryRecord) -> Option<&'static assets::facts::pack::MineralRule> {
    assets::facts::pack::rules().mineral(&record.name)
}

pub(in crate::compiler) fn is_mineral_cube_name(name: &str) -> bool {
    assets::facts::pack::rules().mineral(name).is_some()
}

pub(in crate::compiler) fn is_mineral_cube_record(record: &RegistryRecord) -> bool {
    let Some(rule) = identity(record) else {
        return false;
    };
    record.network_hash == rule.network_hash
        && record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Unknown
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.model_state.mask() == 0
        && record.face_coverage == 0
        && record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.as_ref()
            == [assets::CollisionBox {
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
                ..assets::CollisionBox::default()
            }]
        && record.provenance
            == assets::RegistryProvenance::PMMP
                | assets::RegistryProvenance::DRAGONFLY
                | assets::RegistryProvenance::PRISMARINE
}

pub(in crate::compiler) fn mineral_cube_inventory_is_exact(records: &[RegistryRecord]) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_mineral_cube_name(&record.name))
        .collect::<Vec<_>>();
    selected.len() == assets::facts::pack::rules().minerals.len()
        && selected.into_iter().all(is_mineral_cube_record)
}

pub(in crate::compiler) fn mineral_cube_material_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<(Descriptor, Box<str>)> {
    let rule = identity(record)?;
    let expected_key = rule.key.as_str();
    let expected_path = rule.path.as_str();
    if pack
        .blocks
        .get_exact_scalar_plain(record.name.strip_prefix("minecraft:")?, expected_key)?
        != expected_key
    {
        return None;
    }
    let key: Box<str> = expected_key.into();
    let path = pack.terrain.get_exact_static_plain(&key)?;
    if path != expected_path
        || pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile.as_ref() == key.as_ref() || flipbook.texture_path.as_ref() == path
        })
    {
        return None;
    }
    Some((
        Descriptor {
            state_variant: 0,
            flipbook_selector: None,
            path: path.into(),
            texture_key: key.clone(),
            flags: 0,
        },
        key,
    ))
}

pub(in crate::compiler) fn mineral_cube_sources_are_exact(
    root: &Path,
    pack: &PackSources,
    records: &[RegistryRecord],
) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_mineral_cube_name(&record.name))
        .collect::<Vec<_>>();
    selected.len() == assets::facts::pack::rules().minerals.len()
        && selected.into_iter().all(|record| {
            let Some((descriptor, key)) = mineral_cube_material_descriptor(pack, record) else {
                return false;
            };
            let Ok(path) = static_texture_path(root, &descriptor.path, &key) else {
                return false;
            };
            let Ok(rgba8) = decode_static_texture(&path, &key) else {
                return false;
            };
            rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == u8::MAX)
        })
}
