use super::state::{exact_tagged_byte, exact_tagged_int, exact_tagged_string};
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

#[cfg(test)]
#[path = "selector_alias/tests.rs"]
mod tests;

pub(in crate::compiler) fn is_selector_alias_cube_name(name: &str) -> bool {
    matches!(
        block_render_fact(name).kind,
        R::SelectorAliasCube | R::Tnt | R::DeprecatedPillar
    )
}

/// The record's slot within its block (`explode_bit`, or `axis * 4 + deprecated`), taken
/// from the typed state because sequential IDs vary by registry version.
pub(in crate::compiler) fn exact_selector_alias_cube_state(record: &RegistryRecord) -> Option<u32> {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if block_render_fact(&record.name).kind == R::Tnt {
        if state.len() != 1 || record.model_state.mask() != 0 {
            return None;
        }
        return Some(u32::from(exact_tagged_byte(
            state.get(assets::facts::BlockProperty::ExplodeBit.name())?,
            1,
        )?));
    }

    let has_deprecated = block_render_fact(&record.name).kind == R::DeprecatedPillar;
    if state.len() != if has_deprecated { 2 } else { 1 } {
        return None;
    }
    let (axis_index, orientation) =
        match exact_tagged_string(state.get(assets::facts::BlockProperty::PillarAxis.name())?)? {
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
    let deprecated = if has_deprecated {
        exact_tagged_int(
            state.get(assets::facts::BlockProperty::Deprecated.name())?,
            3,
        )?
    } else {
        0
    };
    Some(axis_index * 4 + deprecated)
}

pub(in crate::compiler) fn is_selector_alias_cube_record(record: &RegistryRecord) -> bool {
    is_selector_alias_cube_name(&record.name)
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && record.flags == BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        && record.face_coverage == 0x3f
        && record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.as_ref()
            == [assets::CollisionBox {
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
                ..assets::CollisionBox::default()
            }]
        && exact_selector_alias_cube_state(record).is_some()
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
    let mut states = BTreeSet::new();
    selected.into_iter().all(|record| {
        is_selector_alias_cube_record(record)
            && ids.insert(record.sequential_id)
            && exact_selector_alias_cube_state(record)
                .is_some_and(|slot| states.insert((record.name.as_ref(), slot)))
    })
}

pub(in crate::compiler) fn selector_alias_cube_material_descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<[(Descriptor, Box<str>); 6]> {
    let block_name = record.name.strip_prefix("minecraft:")?;
    let exact_route = match assets::facts::pack::rules().alias_cube(&record.name)? {
        assets::facts::pack::AliasCubeSource::Pillar(keys) => {
            pack.blocks.get_exact_pillar(block_name)? == keys.each_ref().map(String::as_str)
        }
        assets::facts::pack::AliasCubeSource::Scalar(key) => {
            pack.blocks.get_exact_scalar(block_name)? == key
        }
    };
    if !exact_route {
        return None;
    }

    let mut descriptors = Vec::with_capacity(6);
    for face in BlockFace::ALL {
        let TextureKey { key, rotate_uv } = resolve_texture_key(&pack.blocks, record, face);
        let key = key?;
        let path = pack.terrain.get_exact_static_no_tint(&key)?;
        let expected_path = assets::facts::pack::rules().alias_path(&key)?;
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
                state_variant: 0,
                flipbook_selector: None,
                path: path.into(),
                texture_key: key.clone(),
                flags: u32::from(rotate_uv) * MATERIAL_FLAG_ROTATE_UV,
            },
            key,
        ));
    }
    descriptors.try_into().ok()
}
