use super::super::*;

pub(in crate::compiler) fn is_farmland_name(name: &str) -> bool {
    name == "minecraft:farmland"
}

pub(in crate::compiler) fn exact_farmland_moisture(record: &RegistryRecord) -> Option<u32> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactFarmlandState {
        moisturized_amount: ExactFarmlandTaggedInt,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactFarmlandTaggedInt {
        #[serde(rename = "type")]
        kind: Box<str>,
        value: i64,
    }

    let state = serde_json::from_str::<ExactFarmlandState>(&record.canonical_state).ok()?;
    if state.moisturized_amount.kind.as_ref() != "int"
        || !(0..=7).contains(&state.moisturized_amount.value)
    {
        return None;
    }
    let amount = state.moisturized_amount.value as u32;
    let mask = 1 << (ModelStateField::Growth as u8 - 1);
    if record.model_state.mask() != mask
        || record.model_state.get(ModelStateField::Growth) != Some(amount)
    {
        return None;
    }
    Some(amount)
}

pub(in crate::compiler) fn farmland_collision_is_exact(record: &RegistryRecord) -> bool {
    record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.as_ref()
            == [assets::CollisionBox {
                min_x: 0,
                min_y: 0,
                min_z: 0,
                max_x: 100_000_000,
                max_y: 93_750_000,
                max_z: 100_000_000,
            }]
}

pub(in crate::compiler) fn is_farmland_record(record: &RegistryRecord) -> bool {
    exact_farmland_moisture(record).is_some()
        && is_farmland_name(&record.name)
        && record.model_family == ModelFamily::Cuboid
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.face_coverage == 0
        && farmland_collision_is_exact(record)
}

pub(in crate::compiler) fn farmland_inventory_is_exact(records: &[RegistryRecord]) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_farmland_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 8 {
        return false;
    }
    let mut seen = [false; 8];
    let mut sequential_ids = BTreeSet::new();
    let mut network_hashes = BTreeSet::new();
    for record in selected {
        if !is_farmland_record(record) {
            return false;
        }
        let Some(amount) = exact_farmland_moisture(record) else {
            return false;
        };
        // Registry ordering changes between versions. The typed state and
        // geometry identify farmland; its network identities come from BREG.
        if !sequential_ids.insert(record.sequential_id)
            || !network_hashes.insert(record.network_hash)
            || seen[amount as usize]
        {
            return false;
        }
        seen[amount as usize] = true;
    }
    seen.into_iter().all(|present| present)
}

pub(in crate::compiler) fn farmland_material_descriptors(
    pack: &PackSources,
) -> Option<[(Descriptor, Box<str>); 3]> {
    if pack.blocks.get_exact_side_caps("farmland")?
        != ["farmland_side", "farmland_side", "farmland"]
        || pack.terrain.get_exact_farmland_side()? != "textures/blocks/dirt"
    {
        return None;
    }
    let dry = pack.terrain.get_exact_farmland_top(0)?;
    let wet = pack.terrain.get_exact_farmland_top(1)?;
    if dry != ("textures/blocks/farmland_dry", 1)
        || wet != ("textures/blocks/farmland_wet", 0)
        || pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile.as_ref() == "farmland"
                || flipbook.atlas_tile.as_ref() == "farmland_side"
                || matches!(
                    flipbook.texture_path.as_ref(),
                    "textures/blocks/dirt"
                        | "textures/blocks/farmland_wet"
                        | "textures/blocks/farmland_dry"
                )
        })
    {
        return None;
    }
    Some(
        [
            ("farmland_side", "textures/blocks/dirt", 0),
            ("farmland", "textures/blocks/farmland_wet", 0),
            ("farmland", "textures/blocks/farmland_dry", 1),
        ]
        .map(|(key, path, state_variant)| {
            (
                Descriptor {
                    state_variant,
                    path: path.into(),
                    texture_key: key.into(),
                    flags: 0,
                },
                key.into(),
            )
        }),
    )
}

pub(in crate::compiler) fn farmland_source_alpha_is_exact(root: &Path, pack: &PackSources) -> bool {
    if farmland_material_descriptors(pack).is_none() {
        return false;
    }
    [
        ("farmland_side", "textures/blocks/dirt"),
        ("farmland", "textures/blocks/farmland_wet"),
        ("farmland", "textures/blocks/farmland_dry"),
    ]
    .into_iter()
    .all(|(key, source)| {
        let Ok(path) = static_texture_path(root, source, key) else {
            return false;
        };
        let Ok(rgba8) = decode_static_texture(&path, key) else {
            return false;
        };
        rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == u8::MAX)
    })
}
