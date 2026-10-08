use assets::{BIOME_RULE_FLAG_GRASS_SHADED, BiomeRule, CompiledBiomeAssets, TintMapId, TintSource};

/// Builds a constant-byte palette so the shading operation is independent of climate lookup.
fn fixture(grass: TintSource) -> CompiledBiomeAssets {
    let mut assets = CompiledBiomeAssets::diagnostic();
    for pixel in assets.tint_maps_rgb8.chunks_exact_mut(3) {
        pixel.copy_from_slice(&[100, 200, 40]);
    }
    assets.rules = vec![BiomeRule {
        id: 1,
        name: "test:shaded".into(),
        flags: BIOME_RULE_FLAG_GRASS_SHADED,
        grass,
        foliage: TintSource::map(TintMapId::Foliage),
        dry_foliage: TintSource::map(TintMapId::DryFoliage),
        water: TintSource::direct(0x617b64),
        temperature_bits: 0.5f32.to_bits(),
        downfall_bits: 0.5f32.to_bits(),
    }]
    .into_boxed_slice();
    assets
}

/// Converts a linear result back to the original byte for exact packed-color assertions.
fn bytes(color: [f32; 4]) -> [u8; 3] {
    std::array::from_fn(|i| {
        let c = color[i];
        ((if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }) * 255.0)
            .round() as u8
    })
}

#[test]
fn shaded_default_grass_uses_reference_packed_byte_transform() {
    let resolved = fixture(TintSource::map(TintMapId::Grass))
        .resolve_live(&[])
        .unwrap();
    assert_eq!(bytes(resolved.records[1].grass), [70, 126, 25]);
}

#[test]
fn shaded_flag_does_not_change_custom_grass_override() {
    let resolved = fixture(TintSource::direct(0x64c828))
        .resolve_live(&[])
        .unwrap();
    assert_eq!(bytes(resolved.records[1].grass), [100, 200, 40]);
}

#[test]
fn swamp_palette_keeps_its_bottom_row_and_spatial_marker() {
    let mut assets = fixture(TintSource::map(TintMapId::SwampGrass));
    let row = (TintMapId::SwampGrass as usize * assets::TINT_MAP_SIZE as usize
        + assets::TINT_MAP_SIZE as usize
        - 1)
        * assets::TINT_MAP_SIZE as usize
        * 3;
    assets.tint_maps_rgb8[row..row + 3].copy_from_slice(&[1, 2, 3]);
    let resolved = assets.resolve_live(&[]).unwrap();
    assert_eq!(
        resolved.swamp_grass_palette.len(),
        assets::TINT_MAP_SIZE as usize
    );
    assert_eq!(bytes(resolved.swamp_grass_palette[0]), [1, 2, 3]);
    assert_ne!(
        resolved.records[1].flags & assets::BIOME_TINT_FLAG_SWAMP_GRASS,
        0
    );
}

#[test]
fn water_opacity_rejects_invalid_values_and_preserves_shading() {
    let mut assets = fixture(TintSource::map(TintMapId::Grass));
    for invalid in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
        assert!(assets.rules[0].set_water_opacity(invalid).is_err());
    }
    assets.rules[0].set_water_opacity(0.65).unwrap();
    let resolved = assets.resolve_live(&[]).unwrap();
    assert_eq!(resolved.records[1].water[3], 165.0 / 255.0);
    assert_eq!(bytes(resolved.records[1].grass), [70, 126, 25]);
}

#[test]
fn grass_noise_seed_and_offset_draws_match_current_reference_vectors() {
    let mut random = assets::ClientRandom::new(2345);
    assert_eq!(
        std::array::from_fn::<_, 8, _>(|_| random.next_u32()),
        [
            2837445712, 859401479, 1776401248, 3494520208, 2869478636, 1731913201, 987599469,
            2954181634
        ]
    );
    assert_eq!(
        &assets::grass_noise_permutation()[..8],
        &[144, 102, 233, 57, 254, 23, 182, 116]
    );
}

#[test]
fn review_nonfinite_live_climates_do_not_discard_valid_siblings() {
    let catalog = fixture(TintSource::map(TintMapId::Grass));
    let valid = assets::LiveBiomeDefinition {
        name: "test:shaded",
        biome_id: Some(1),
        temperature: 0.25,
        downfall: 0.75,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_argb: 0xff617b64,
    };
    let invalid = assets::LiveBiomeDefinition {
        temperature: f32::NAN,
        ..valid
    };
    let expected = catalog.resolve_live(&[valid]).unwrap();
    let actual = catalog.resolve_live(&[invalid, valid]).unwrap();
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.skipped_definitions, 1);
}

#[test]
fn nonfinite_live_snow_does_not_discard_valid_siblings() {
    let catalog = fixture(TintSource::map(TintMapId::Grass));
    let valid = assets::LiveBiomeDefinition {
        name: "test:shaded",
        biome_id: Some(1),
        temperature: 0.25,
        downfall: 0.75,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_argb: 0xff617b64,
    };
    let invalid = assets::LiveBiomeDefinition {
        snow_foliage: f32::NAN,
        ..valid
    };
    let expected = catalog.resolve_live(&[valid]).unwrap();
    let actual = catalog.resolve_live(&[invalid, valid]).unwrap();
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.skipped_definitions, 1);
}
