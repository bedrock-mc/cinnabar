use super::*;

fn stream(snow: f32) -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.replace_biome_definitions(Arc::from([BiomeDefinitionEvent {
        biome_id: Some(7),
        name: "example:cold_season".into(),
        temperature: -0.5,
        downfall: 0.8,
        snow_foliage: snow,
        max_snow_accumulation: Some(0.5),
        map_water_color: 0xff44_6688,
    }]));
    stream
}

#[test]
fn seasonal_palette_refresh_changes_colours_not_dense_identity_or_meshes() {
    let mut stream = stream(0.0);
    let before = stream.resolved_biome_tints_snapshot();
    let identity = stream.biome_tint_identity();
    let pending = stream.mesh_jobs.pending.len();
    let changes = stream.mesh_changes.len();
    let generation = stream.connectivity_generation;
    let dense = before.dense_index(7) as usize;

    // Tick zero is a native refresh, but this first tiny step quantizes to the
    // existing palette bytes. No remaining tick before 100 publishes colours.
    assert!(!stream.advance_seasonal_foliage([1.0, 1.0], true));
    for _ in 1..PALETTE_REFRESH_TICKS {
        assert!(!stream.advance_seasonal_foliage([1.0, 1.0], true));
        assert!(Arc::ptr_eq(
            &before,
            &stream.resolved_biome_tints_snapshot()
        ));
    }
    assert!(stream.advance_seasonal_foliage([1.0, 1.0], true));
    let after = stream.resolved_biome_tints_snapshot();
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(stream.biome_tint_identity(), identity);
    assert_eq!(after.raw_id_to_dense, before.raw_id_to_dense);
    assert_eq!(after.records[dense].grass, before.records[dense].grass);
    assert_eq!(after.records[dense].foliage, before.records[dense].foliage);
    assert_eq!(after.records[dense].water, before.records[dense].water);
    let covered = assets::SEASONAL_FOLIAGE_EXPOSED_OFFSET;
    assert_eq!(
        after.records[dense].seasonal_foliage[..covered],
        before.records[dense].seasonal_foliage[..covered]
    );
    assert_ne!(
        after.records[dense].seasonal_foliage[covered..],
        before.records[dense].seasonal_foliage[covered..]
    );
    assert_eq!(stream.mesh_jobs.pending.len(), pending);
    assert_eq!(stream.mesh_changes.len(), changes);
    assert_eq!(stream.connectivity_generation, generation);
    // The original authoritative definition is not a mutable palette-row alias.
    assert_eq!(stream.biome_definitions_snapshot()[0].snow_foliage, 0.0);
}

#[test]
fn seasonal_row_freezes_under_disabled_rule_and_non_weather_dimensions() {
    let mut stream = stream(0.3);
    let palette = stream.resolved_biome_tints_snapshot();
    for _ in 0..PALETTE_REFRESH_TICKS * 2 {
        assert!(!stream.advance_seasonal_foliage([1.0, 1.0], false));
    }
    assert_eq!(stream.seasonal_foliage.snow, [0.3]);
    for dimension in [1, 2, 7] {
        stream.authority.reset_dimension(1, dimension);
        for _ in 0..PALETTE_REFRESH_TICKS * 2 {
            assert!(!stream.advance_seasonal_foliage([1.0, 1.0], true));
        }
    }
    assert_eq!(stream.seasonal_foliage.snow, [0.3]);
    assert!(Arc::ptr_eq(
        &palette,
        &stream.resolved_biome_tints_snapshot()
    ));
}

#[test]
fn seasonal_dry_ticks_melt_and_new_sessions_seed_from_authority() {
    let mut old = stream(1.0);
    let before = old.resolved_biome_tints_snapshot();
    for _ in 0..PALETTE_REFRESH_TICKS * 6 + 1 {
        old.advance_seasonal_foliage([0.0, 0.0], true);
    }
    assert_eq!(old.seasonal_foliage.snow, [0.0]);
    assert_ne!(*before, *old.resolved_biome_tints_snapshot());
    let fresh = stream(0.4);
    assert_eq!(fresh.seasonal_foliage.snow, [0.4]);
    assert_eq!(fresh.seasonal_foliage.tick, 0);
    assert_ne!(
        old.authority().actor_session_id(),
        fresh.authority().actor_session_id()
    );
}

#[test]
fn seasonal_renderer_clock_survives_definition_and_dimension_changes() {
    let mut stream = stream(0.0);
    for _ in 0..17 {
        stream.advance_seasonal_foliage([1.0, 1.0], true);
    }
    let tick = stream.seasonal_foliage.tick;
    let definitions = stream.biome_definitions_snapshot();
    stream.replace_biome_definitions(definitions);
    assert_eq!(stream.seasonal_foliage.tick, tick);
    assert!(
        stream.seasonal_foliage.snow[0] > 0.0,
        "resource reload retains rows"
    );
    stream.replace_biome_definitions(Arc::from(stream.biome_definitions_snapshot().to_vec()));
    assert_eq!(stream.seasonal_foliage.tick, tick);
    assert_eq!(
        stream.seasonal_foliage.snow[0], 0.0,
        "new definitions replace rows"
    );
    stream.authority.reset_dimension(1, 1);
    stream.advance_seasonal_foliage([1.0, 1.0], true);
    assert_eq!(stream.seasonal_foliage.tick, tick + 1);
}

#[test]
fn nonfinite_seasonal_definition_keeps_valid_rows_and_records_normalization() {
    let mut stream = stream(0.0);
    let valid = stream.biome_definitions_snapshot()[0].clone();
    let mut invalid = valid.clone();
    invalid.biome_id = Some(8);
    invalid.name = "example:invalid_season".into();
    invalid.snow_foliage = f32::NAN;
    stream.replace_biome_definitions(Arc::from([invalid, valid]));
    let resolved = stream.resolved_biome_tints_snapshot();
    assert_ne!(resolved.dense_index(7), assets::MISSING_BIOME_DENSE_INDEX);
    assert_eq!(resolved.dense_index(8), assets::MISSING_BIOME_DENSE_INDEX);
    assert_eq!(
        stream
            .stats()
            .normalization_reasons
            .biome_definition_resolution_failures,
        1
    );
    for _ in 0..PALETTE_REFRESH_TICKS + 1 {
        stream.advance_seasonal_foliage([1.0, 1.0], true);
    }
    assert_eq!(
        stream
            .stats()
            .normalization_reasons
            .biome_definition_resolution_failures,
        1,
        "skipped rows do not repeatedly poison seasonal refresh"
    );
    assert!(stream.seasonal_foliage.snow[1] > 0.0);
}
