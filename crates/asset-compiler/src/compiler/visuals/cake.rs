use super::super::*;

pub(in crate::compiler) fn is_cake_name(name: &str) -> bool {
    name == "minecraft:cake"
}

pub(in crate::compiler) fn exact_cake_bite(record: &RegistryRecord) -> Option<u32> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactCakeState {
        bite_counter: ExactCakeTaggedInt,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExactCakeTaggedInt {
        #[serde(rename = "type")]
        kind: Box<str>,
        value: i64,
    }

    let state = serde_json::from_str::<ExactCakeState>(&record.canonical_state).ok()?;
    if state.bite_counter.kind.as_ref() != "int" || !(0..=6).contains(&state.bite_counter.value) {
        return None;
    }
    let bite = state.bite_counter.value as u32;
    let mask = 1 << (ModelStateField::Growth as u8 - 1);
    if record.model_state.mask() != mask
        || record.model_state.get(ModelStateField::Growth) != Some(bite)
    {
        return None;
    }
    Some(bite)
}

pub(in crate::compiler) fn cake_collision_is_exact(record: &RegistryRecord, bite: u32) -> bool {
    let min_x = [
        6_250_000, 18_750_000, 31_250_000, 43_750_000, 56_250_000, 68_750_000, 81_250_000,
    ];
    bite <= 6
        && record.collision_seed.shape_id == 89 + bite as u16
        && record.collision_seed.confidence == assets::CollisionConfidence::CollisionOnly
        && record.collision_seed.boxes.as_ref()
            == [assets::CollisionBox {
                min_x: min_x[bite as usize],
                min_y: 0,
                min_z: 6_250_000,
                max_x: 93_750_000,
                max_y: 50_000_000,
                max_z: 93_750_000,
            }]
}

pub(in crate::compiler) fn is_cake_record(record: &RegistryRecord) -> bool {
    let Some(bite) = exact_cake_bite(record) else {
        return false;
    };
    is_cake_name(&record.name)
        && record.model_family == ModelFamily::Cuboid
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.face_coverage == 0
        && cake_collision_is_exact(record, bite)
}

pub(in crate::compiler) fn cake_inventory_is_exact(records: &[RegistryRecord]) -> bool {
    let selected = records
        .iter()
        .filter(|record| is_cake_name(&record.name))
        .collect::<Vec<_>>();
    if selected.len() != 7 {
        return false;
    }
    let mut seen = [false; 7];
    for record in selected {
        if !is_cake_record(record) {
            return false;
        }
        let Some(bite) = exact_cake_bite(record) else {
            return false;
        };
        if record.sequential_id != 14_055 + bite || seen[bite as usize] {
            return false;
        }
        seen[bite as usize] = true;
    }
    seen.into_iter().all(|present| present)
}

pub(in crate::compiler) fn cake_material_descriptors(
    pack: &PackSources,
) -> Option<[(Descriptor, Box<str>); 4]> {
    pack.blocks.get_exact_cake_faces()?;
    let pairs = [
        (
            "cake_bottom",
            ["textures/blocks/cake_bottom", "textures/blocks/cake_bottom"],
        ),
        (
            "cake_side",
            ["textures/blocks/cake_side", "textures/blocks/cake_side"],
        ),
        (
            "cake_top",
            ["textures/blocks/cake_top", "textures/blocks/cake_top"],
        ),
        (
            "cake_west",
            ["textures/blocks/cake_side", "textures/blocks/cake_inner"],
        ),
    ];
    for (key, expected) in pairs {
        if pack.terrain.get_exact_pair_no_tint(key)? != expected {
            return None;
        }
        if pack.flipbooks.iter().any(|flipbook| {
            flipbook.atlas_tile.as_ref() == key
                || expected.contains(&flipbook.texture_path.as_ref())
        }) {
            return None;
        }
    }
    Some(
        [
            ("cake_side", "textures/blocks/cake_side"),
            ("cake_bottom", "textures/blocks/cake_bottom"),
            ("cake_top", "textures/blocks/cake_top"),
            ("cake_west", "textures/blocks/cake_inner"),
        ]
        .map(|(key, path)| {
            (
                Descriptor {
                    path: path.into(),
                    texture_key: key.into(),
                    flags: MATERIAL_FLAG_ALPHA_CUTOUT,
                },
                key.into(),
            )
        }),
    )
}

pub(in crate::compiler) fn cake_source_alpha_is_exact(root: &Path, pack: &PackSources) -> bool {
    if cake_material_descriptors(pack).is_none() {
        return false;
    }
    let sources = [
        ("cake_bottom", "textures/blocks/cake_bottom", [1, 1, 14, 14]),
        ("cake_side", "textures/blocks/cake_side", [1, 8, 14, 15]),
        ("cake_top", "textures/blocks/cake_top", [1, 1, 14, 14]),
        ("cake_west", "textures/blocks/cake_inner", [1, 8, 14, 15]),
    ];
    sources.into_iter().all(|(key, source, bounds)| {
        let Ok(path) = static_texture_path(root, source, key) else {
            return false;
        };
        let Ok(rgba8) = decode_static_texture(&path, key) else {
            return false;
        };
        rgba8.chunks_exact(4).enumerate().all(|(index, pixel)| {
            let x = index % assets::TILE_SIZE as usize;
            let y = index / assets::TILE_SIZE as usize;
            let inside = x >= bounds[0] && x <= bounds[2] && y >= bounds[1] && y <= bounds[3];
            pixel[3] == if inside { u8::MAX } else { 0 }
        })
    })
}
