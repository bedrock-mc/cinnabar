use super::super::*;

const MINERAL_CUBE_IDENTITIES: [(u32, u32, &str, &str, &str); 2] = [
    (
        12_638,
        0xbda0_2665,
        "minecraft:cinnabar",
        "cinnabar",
        "textures/blocks/cinnabar",
    ),
    (
        14_658,
        0x2d65_8dd8,
        "minecraft:sulfur",
        "sulfur",
        "textures/blocks/sulfur",
    ),
];

fn identity(
    record: &RegistryRecord,
) -> Option<(u32, u32, &'static str, &'static str, &'static str)> {
    MINERAL_CUBE_IDENTITIES
        .iter()
        .copied()
        .find(|&(_, _, name, _, _)| record.name.as_ref() == name)
}

pub(in crate::compiler) fn is_mineral_cube_name(name: &str) -> bool {
    MINERAL_CUBE_IDENTITIES
        .iter()
        .any(|&(_, _, candidate, _, _)| name == candidate)
}

pub(in crate::compiler) fn is_mineral_cube_record(record: &RegistryRecord) -> bool {
    let Some((sequential_id, network_hash, _, _, _)) = identity(record) else {
        return false;
    };
    record.sequential_id == sequential_id
        && record.network_hash == network_hash
        && record.canonical_state.as_ref() == "{}"
        && record.model_family == ModelFamily::Unknown
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.model_state.mask() == 0
        && record.face_coverage == 0
        && record.collision_seed.shape_id == 1
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
    selected.len() == MINERAL_CUBE_IDENTITIES.len()
        && selected.into_iter().all(is_mineral_cube_record)
}

pub(in crate::compiler) fn mineral_cube_material_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<(Descriptor, Box<str>)> {
    let (_, _, _, expected_key, expected_path) = identity(record)?;
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
    selected.len() == MINERAL_CUBE_IDENTITIES.len()
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
            rgba8.chunks_exact(4).all(|pixel| pixel[3] == u8::MAX)
        })
}
