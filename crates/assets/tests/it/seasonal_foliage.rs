use assets::{
    BIOME_TINT_FLAG_SEASONAL_FOLIAGE, CompiledBiomeAssets, LiveBiomeDefinition,
    SEASONAL_FOLIAGE_EXPOSED_OFFSET, TINT_MAP_SIZE, TintMapId,
};

#[test]
fn live_snow_foliage_preserves_world_snow_brightness_without_recoloring_ordinary_tints() {
    let mut compiled = CompiledBiomeAssets::diagnostic();
    let map_bytes = TINT_MAP_SIZE as usize * TINT_MAP_SIZE as usize * 3;
    for (map, rgb) in [
        (TintMapId::Evergreen, [84, 120, 56]),
        (TintMapId::Birch, [20, 42, 64]),
        (TintMapId::Foliage, [100, 120, 140]),
    ] {
        let offset = map as usize * map_bytes;
        for pixel in compiled.tint_maps_rgb8[offset..offset + map_bytes]
            .as_chunks_mut::<3>()
            .0
        {
            pixel.copy_from_slice(&rgb);
        }
    }
    let ordinary = LiveBiomeDefinition {
        name: "example:snow",
        biome_id: Some(7),
        temperature: -0.5,
        downfall: 0.5,
        snow_foliage: 0.0,
        max_snow_accumulation: Some(1.0),
        map_water_argb: 0,
    };
    let before = compiled.resolve_live(&[ordinary]).unwrap();
    let after = compiled
        .resolve_live(&[LiveBiomeDefinition {
            snow_foliage: 1.0,
            max_snow_accumulation: Some(1.0),
            ..ordinary
        }])
        .unwrap();
    let before = &before.records[before.dense_index(7) as usize];
    let after = &after.records[after.dense_index(7) as usize];
    assert_eq!(before.evergreen, after.evergreen);
    assert_eq!(before.foliage, after.foliage);
    assert_eq!(
        before.seasonal_foliage[..SEASONAL_FOLIAGE_EXPOSED_OFFSET],
        after.seasonal_foliage[..SEASONAL_FOLIAGE_EXPOSED_OFFSET]
    );
    assert_ne!(before.seasonal_foliage[0], before.seasonal_foliage[1]);
    assert_ne!(before.flags & BIOME_TINT_FLAG_SEASONAL_FOLIAGE, 0);
    assert_ne!(after.flags & BIOME_TINT_FLAG_SEASONAL_FOLIAGE, 0);
    assert!(
        after.seasonal_foliage[SEASONAL_FOLIAGE_EXPOSED_OFFSET..]
            .iter()
            .all(|color| color[..3].iter().all(|channel| *channel > 1.0) && color[3] == 1.0)
    );
    let skipped = compiled
        .resolve_live(&[LiveBiomeDefinition {
            snow_foliage: f32::NAN,
            max_snow_accumulation: Some(1.0),
            ..ordinary
        }])
        .unwrap();
    assert_eq!(skipped.skipped_definitions, 1);
    assert_eq!(skipped.dense_index(7), assets::MISSING_BIOME_DENSE_INDEX);
}

#[test]
fn native_snow_eligibility_is_independent_of_palette_snow_fraction() {
    let assets = CompiledBiomeAssets::diagnostic();
    let climate = LiveBiomeDefinition {
        name: "example:cold",
        biome_id: Some(7),
        temperature: -0.5,
        downfall: 0.5,
        snow_foliage: 1.0,
        max_snow_accumulation: Some(1.0),
        map_water_argb: 0,
    };
    for (temperature, maximum, eligible) in [
        (-0.5, Some(1.0), true),
        (-0.5, Some(0.0), false),
        (-0.5, None, false),
        (-0.5, Some(f32::NAN), false),
        (assets::SEASONAL_FOLIAGE_COLD_THRESHOLD, Some(1.0), false),
        (0.8, Some(1.0), false),
    ] {
        let resolved = assets
            .resolve_live(&[LiveBiomeDefinition {
                temperature,
                max_snow_accumulation: maximum,
                ..climate
            }])
            .unwrap();
        assert_eq!(
            resolved.records[resolved.dense_index(7) as usize].flags
                & BIOME_TINT_FLAG_SEASONAL_FOLIAGE
                != 0,
            eligible
        );
    }
}
