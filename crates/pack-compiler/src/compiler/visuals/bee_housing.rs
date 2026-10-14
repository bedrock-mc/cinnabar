use super::super::*;
use assets::facts::pack::{PackBlock as B, PackTexture as T};
use assets::facts::{BlockRenderKind as R, block_render_fact};

pub(in crate::compiler) fn is_bee_housing_name(name: &str) -> bool {
    block_render_fact(name).kind == R::BeeHousing
}

pub(in crate::compiler) fn exact_bee_housing_state(record: &RegistryRecord) -> Option<(u32, u32)> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactBeeHousingState {
        direction: ExactBeeHousingTaggedInt,
        honey_level: ExactBeeHousingTaggedInt,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactBeeHousingTaggedInt {
        #[serde(rename = "type")]
        kind: Box<str>,
        value: i64,
    }

    let state = serde_json::from_str::<ExactBeeHousingState>(&record.canonical_state).ok()?;
    if state.direction.kind.as_ref() != "int"
        || state.honey_level.kind.as_ref() != "int"
        || !(0..=3).contains(&state.direction.value)
        || !(0..=5).contains(&state.honey_level.value)
    {
        return None;
    }
    let direction = state.direction.value as u32;
    let honey_level = state.honey_level.value as u32;
    let orientation_mask = 1 << (ModelStateField::Orientation as u8 - 1);
    let growth_mask = 1 << (ModelStateField::Growth as u8 - 1);
    if record.model_state.mask() != orientation_mask | growth_mask
        || record.model_state.get(ModelStateField::Orientation) != Some(direction)
        || record.model_state.get(ModelStateField::Growth) != Some(honey_level)
    {
        return None;
    }
    Some((direction, honey_level))
}

pub(in crate::compiler) fn bee_housing_collision_is_exact(record: &RegistryRecord) -> bool {
    record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.as_ref()
            == [assets::CollisionBox {
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
                ..assets::CollisionBox::default()
            }]
}

pub(in crate::compiler) fn is_bee_housing_record(record: &RegistryRecord) -> bool {
    is_bee_housing_name(&record.name)
        && record.model_family == ModelFamily::Cube
        && record.contributor_role == ContributorRole::Primary
        && record.flags == BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        && record.face_coverage == 0x3f
        && bee_housing_collision_is_exact(record)
        && exact_bee_housing_state(record).is_some()
}

pub(in crate::compiler) fn bee_housing_inventory_is_exact(records: &[RegistryRecord]) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_bee_housing_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 48 {
        return false;
    }
    let mut seen = [false; 48];
    for record in selected {
        if !is_bee_housing_record(record) {
            return false;
        }
        let Some((direction, honey_level)) = exact_bee_housing_state(record) else {
            return false;
        };
        let family = if record.name.as_ref() == B::BeeNest.identifier() {
            0
        } else if record.name.as_ref() == B::Beehive.identifier() {
            1
        } else {
            return false;
        };
        let slot = family * 24 + (honey_level * 4 + direction) as usize;
        if seen[slot] {
            return false;
        }
        seen[slot] = true;
    }
    seen.into_iter().all(|present| present)
}

pub(in crate::compiler) fn bee_housing_material_descriptors(
    pack: &PackSources,
) -> Option<[(Descriptor, Box<str>); 9]> {
    if pack.blocks.get_exact_faces(B::BeeNest.bare())?
        != [
            T::BeeNestSide.key(),
            T::BeeNestSide.key(),
            T::BeeNestBottom.key(),
            T::BeeNestTop.key(),
            T::BeeNestSide.key(),
            T::BeeNestFront.key(),
        ]
        || pack.blocks.get_exact_faces(B::Beehive.bare())?
            != [
                T::BeehiveSide.key(),
                T::BeehiveSide.key(),
                T::BeehiveTop.key(),
                T::BeehiveTop.key(),
                T::BeehiveSide.key(),
                T::BeehiveFront.key(),
            ]
    {
        return None;
    }

    let routes = [
        (T::BeeNestBottom.key(), "textures/blocks/bee_nest_bottom"),
        (T::BeeNestFront.key(), "textures/blocks/bee_nest_front"),
        (
            T::BeeNestFront.key(),
            "textures/blocks/bee_nest_front_honey",
        ),
        (T::BeeNestSide.key(), "textures/blocks/bee_nest_side"),
        (T::BeeNestTop.key(), "textures/blocks/bee_nest_top"),
        (T::BeehiveFront.key(), "textures/blocks/beehive_front"),
        (T::BeehiveFront.key(), "textures/blocks/beehive_front_honey"),
        (T::BeehiveSide.key(), "textures/blocks/beehive_side"),
        (T::BeehiveTop.key(), "textures/blocks/beehive_top"),
    ];
    let nest_front = pack.terrain.get_exact_pair_plain(T::BeeNestFront.key())?;
    let hive_front = pack.terrain.get_exact_pair_plain(T::BeehiveFront.key())?;
    if nest_front
        != [
            "textures/blocks/bee_nest_front",
            "textures/blocks/bee_nest_front_honey",
        ]
        || hive_front
            != [
                "textures/blocks/beehive_front",
                "textures/blocks/beehive_front_honey",
            ]
    {
        return None;
    }

    let mut descriptors = Vec::with_capacity(routes.len());
    for (key, expected_path) in routes {
        let (path, state_variant) = if key == T::BeeNestFront.key() || key == T::BeehiveFront.key()
        {
            let paths = pack.terrain.get_exact_pair_plain(key)?;
            let index = paths.iter().position(|&path| path == expected_path)?;
            (paths[index], index as u32)
        } else {
            let path = pack.terrain.get_exact_singleton_plain(key)?;
            if path != expected_path {
                return None;
            }
            (path, 0)
        };
        if pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile.as_ref() == key || flipbook.texture_path.as_ref() == path
        }) {
            return None;
        }
        descriptors.push((
            Descriptor {
                state_variant,
                flipbook_selector: None,
                path: path.into(),
                texture_key: key.into(),
                flags: 0,
            },
            key.into(),
        ));
    }
    descriptors.try_into().ok()
}

#[cfg(test)]
#[path = "bee_housing/tests.rs"]
mod tests;
