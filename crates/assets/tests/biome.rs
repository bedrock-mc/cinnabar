use assets::{
    BIOME_REGISTRY_MAGIC, BIOME_RULE_FLAG_GRASS_SHADED, BiomeRule, CompiledBiomeAssets,
    LiveBiomeDefinition, MISSING_BIOME_DENSE_INDEX, RAW_BIOME_ID_COUNT, TINT_MAP_BYTES, TintMapId,
    TintSource, colormap_coordinate, read_biome_registry,
};

fn registry_bytes(records: &[(u32, &str)]) -> Vec<u8> {
    let mut bytes = BIOME_REGISTRY_MAGIC.to_vec();
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("small fixture")
            .to_le_bytes(),
    );
    for &(id, name) in records {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(
            &u16::try_from(name.len())
                .expect("small fixture name")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(name.as_bytes());
    }
    bytes
}

#[test]
fn bioreg_reader_is_strict_and_bounded() {
    let bytes = registry_bytes(&[(0, "minecraft:ocean"), (7, "minecraft:plains")]);
    let records = read_biome_registry(&bytes).expect("read valid BIOREG01");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, 0);
    assert_eq!(records[1].name.as_ref(), "minecraft:plains");

    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(read_biome_registry(&trailing).is_err());
    assert!(
        read_biome_registry(&registry_bytes(&[
            (7, "minecraft:plains"),
            (7, "minecraft:ocean"),
        ]))
        .is_err()
    );
    assert!(
        read_biome_registry(&registry_bytes(&[
            (0, "minecraft:plains"),
            (1, "minecraft:plains"),
        ]))
        .is_err()
    );
    assert!(read_biome_registry(&registry_bytes(&[(u32::from(u16::MAX) + 1, "x")])).is_err());
}

#[test]
fn bedrock_colormap_coordinates_apply_temperature_weighted_humidity() {
    assert_eq!(colormap_coordinate(0.8, 0.4), [50, 173]);
    assert_eq!(colormap_coordinate(1.5, -1.0), [0, 255]);
}

#[test]
fn live_biomes_resolve_to_one_fallback_prefixed_dense_table() {
    let mut maps = vec![0_u8; TINT_MAP_BYTES];
    let [x, y] = colormap_coordinate(0.25, 0.5);
    let grass_pixel =
        ((TintMapId::Grass as usize * 256 * 256) + (usize::from(y) * 256 + usize::from(x))) * 3;
    maps[grass_pixel..grass_pixel + 3].copy_from_slice(&[255, 0, 0]);
    let birch_pixel =
        ((TintMapId::Birch as usize * 256 * 256) + (usize::from(y) * 256 + usize::from(x))) * 3;
    maps[birch_pixel..birch_pixel + 3].copy_from_slice(&[0, 0, 255]);
    let evergreen_pixel =
        ((TintMapId::Evergreen as usize * 256 * 256) + (usize::from(y) * 256 + usize::from(x))) * 3;
    maps[evergreen_pixel..evergreen_pixel + 3].copy_from_slice(&[255, 255, 255]);
    let compiled = CompiledBiomeAssets {
        tint_maps_rgb8: maps.into_boxed_slice(),
        rules: vec![BiomeRule {
            id: 7,
            name: "minecraft:plains".into(),
            flags: BIOME_RULE_FLAG_GRASS_SHADED,
            grass: TintSource::map(TintMapId::Grass),
            foliage: TintSource::direct(0x00ff00),
            dry_foliage: TintSource::direct(0xff00ff),
            water: TintSource::direct(0xffff00),
            temperature_bits: 0.8_f32.to_bits(),
            downfall_bits: 0.4_f32.to_bits(),
        }]
        .into_boxed_slice(),
    };
    let resolved = compiled
        .resolve_live(&[
            LiveBiomeDefinition {
                name: "minecraft:plains",
                biome_id: None,
                temperature: 0.25,
                downfall: 0.5,
                map_water_argb: 0xff00_0000,
            },
            LiveBiomeDefinition {
                name: "example:custom",
                biome_id: Some(900),
                temperature: 0.8,
                downfall: 0.4,
                map_water_argb: 0xff12_3456,
            },
            LiveBiomeDefinition {
                name: "example:unmapped",
                biome_id: None,
                temperature: 0.8,
                downfall: 0.4,
                map_water_argb: 0,
            },
        ])
        .expect("resolve valid live definitions");

    assert_eq!(resolved.records.len(), 3);
    assert_eq!(
        resolved.records[0].raw_id,
        u32::MAX,
        "fallback is slot zero"
    );
    assert_eq!(resolved.records[1].raw_id, 7);
    assert_eq!(resolved.records[1].flags, 1);
    assert_eq!(resolved.records[1].grass, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(resolved.records[1].foliage, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(resolved.records[1].birch, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(resolved.records[1].evergreen, [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(resolved.records[1].dry_foliage, [1.0, 0.0, 1.0, 1.0]);
    assert_eq!(resolved.records[1].water, [1.0, 1.0, 0.0, 1.0]);
    assert_eq!(resolved.records[2].raw_id, 900);
    assert_eq!(resolved.raw_id_to_dense.len(), RAW_BIOME_ID_COUNT);
    assert_eq!(resolved.raw_id_to_dense[7], 1);
    assert_eq!(resolved.raw_id_to_dense[900], 2);
    assert_eq!(resolved.raw_id_to_dense[8], MISSING_BIOME_DENSE_INDEX);
    assert_eq!(resolved.dense_index(7), 1);
    assert_eq!(resolved.dense_index(8), MISSING_BIOME_DENSE_INDEX);
    assert_eq!(resolved.dense_index(u32::MAX), MISSING_BIOME_DENSE_INDEX);
}

#[test]
fn live_biome_duplicates_fail_closed() {
    let compiled = CompiledBiomeAssets {
        tint_maps_rgb8: vec![u8::MAX; TINT_MAP_BYTES].into_boxed_slice(),
        rules: vec![BiomeRule {
            id: 7,
            name: "minecraft:plains".into(),
            flags: 0,
            grass: TintSource::direct(0),
            foliage: TintSource::direct(0),
            dry_foliage: TintSource::direct(0),
            water: TintSource::direct(0),
            temperature_bits: 0.8_f32.to_bits(),
            downfall_bits: 0.4_f32.to_bits(),
        }]
        .into_boxed_slice(),
    };
    let duplicate = LiveBiomeDefinition {
        name: "example:custom",
        biome_id: Some(900),
        temperature: 0.8,
        downfall: 0.4,
        map_water_argb: 0xff12_3456,
    };
    assert!(compiled.resolve_live(&[duplicate, duplicate]).is_err());
    let static_collision = LiveBiomeDefinition {
        biome_id: Some(7),
        ..duplicate
    };
    assert!(compiled.resolve_live(&[static_collision]).is_err());
    let known = LiveBiomeDefinition {
        name: "minecraft:plains",
        biome_id: None,
        ..duplicate
    };
    assert!(compiled.resolve_live(&[known, known]).is_err());
}

#[test]
fn custom_biome_dense_order_is_deterministic_by_raw_id() {
    let compiled = CompiledBiomeAssets::diagnostic();
    let low = LiveBiomeDefinition {
        name: "example:low",
        biome_id: Some(10),
        temperature: 0.8,
        downfall: 0.4,
        map_water_argb: 0xff12_3456,
    };
    let high = LiveBiomeDefinition {
        name: "example:high",
        biome_id: Some(900),
        ..low
    };
    let forward = compiled
        .resolve_live(&[low, high])
        .expect("resolve forward");
    let reverse = compiled
        .resolve_live(&[high, low])
        .expect("resolve reverse");
    assert_eq!(forward, reverse);
    assert_eq!(
        forward
            .records
            .iter()
            .map(|record| record.raw_id)
            .collect::<Vec<_>>(),
        [u32::MAX, 10, 900]
    );
}
