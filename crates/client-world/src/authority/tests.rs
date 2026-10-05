use super::*;

#[test]
fn block_interactions_encode_the_current_session_palette() {
    for hashes in [false, true] {
        let mut authority = WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: hashes,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            None,
            [0.0; 3],
            None,
        );
        authority
            .set_sequential_id_remap(assets::SequentialIdRemap::from_palette(vec![0, 4, 1, 6], 7));
        if hashes {
            assert_eq!(authority.block_network_id(4), Some(4));
            assert_eq!(authority.block_network_id(0x8000_0001), Some(0x8000_0001));
            assert_eq!(authority.block_network_id(u32::MAX), Some(u32::MAX));
        } else {
            assert_eq!(authority.block_network_id(4), Some(1));
            assert_eq!(authority.block_network_id(6), Some(3));
            assert_eq!(authority.block_network_id(2), None);
            authority.set_sequential_id_remap(assets::SequentialIdRemap::default());
            assert_eq!(authority.block_network_id(4), Some(4));
        }
    }
}

#[test]
fn credits_admission_targets_the_live_local_runtime_actor() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 5,
            local_player_runtime_id: 41,
            dimension: 2,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    for (sequence, runtime_id) in [(7, 72), (8, 41)] {
        authority
            .apply_ordered_event(
                WorldEvent::Ui(UiEvent::ShowCredits(protocol::ShowCreditsEvent {
                    runtime_id,
                })),
                Some(sequence),
            )
            .unwrap();
    }
    let events = authority.take_committed_ui();
    assert!(matches!(
        events.as_slice(),
        [CommittedUiEvent::Ui {
            sequence: 8,
            event: UiEvent::ShowCredits(protocol::ShowCreditsEvent { runtime_id: 41 })
        }]
    ));
}

#[test]
fn biome_tint_revision_overflow_keeps_the_previous_atomic_snapshot() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority.biome_tint_revision = u64::MAX;
    let previous = Arc::clone(authority.resolved_biome_tints());

    let report = authority.apply_biome_definitions(Arc::from([BiomeDefinitionEvent {
        biome_id: Some(42),
        name: Arc::from("example:overflow"),
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.0,
        max_snow_accumulation: None,
        map_water_color: 0xff44_6688,
    }]));

    assert_eq!(authority.biome_tint_revision(), u64::MAX);
    assert!(authority.biome_definitions().is_empty());
    assert!(Arc::ptr_eq(&previous, authority.resolved_biome_tints()));
    assert!(report.revision_overflow);
    assert!(!report.changed);
    assert_eq!(report.resolution_failures, 0);
}
