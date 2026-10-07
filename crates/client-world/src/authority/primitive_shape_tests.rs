use super::*;

#[test]
fn primitive_shapes_keep_admission_credit_until_consumed_in_order() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    for id in [7, 3, 11] {
        authority
            .apply_ordered_event(
                WorldEvent::PrimitiveShapes(PrimitiveShapesEvent {
                    changes: vec![PrimitiveShapeChange::Remove { network_id: id }],
                    skipped_entries: 0,
                }),
                Some(id),
            )
            .unwrap();
    }
    assert_eq!(authority.retained_commit_count(), 3);
    for id in [7, 3, 11] {
        let event = authority.pop_primitive_shapes().unwrap();
        assert_eq!(
            event.changes,
            vec![PrimitiveShapeChange::Remove { network_id: id }]
        );
    }
    assert!(authority.pop_primitive_shapes().is_none());
    assert_eq!(authority.retained_commit_count(), 0);
}
