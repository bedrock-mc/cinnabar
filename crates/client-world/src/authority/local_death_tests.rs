use super::tests::local_health_feed;
use super::*;

#[test]
fn local_death_controls_follow_accepted_health_and_actor_transitions() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 1,
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
    // SetHealth can precede the synthetic local pose. Invalid and repeated values do not rearm.
    for (sequence, health, died) in [
        (1, -1, false),
        (2, 0, true),
        (3, 0, false),
        (4, 20, false),
        (5, 0, true),
    ] {
        authority
            .apply_ordered_event(
                WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health })),
                Some(sequence),
            )
            .unwrap();
        let controls = authority.take_committed_controls();
        assert_eq!(
            controls,
            if died {
                vec![CommittedControlEvent::LocalDeath { sequence }]
            } else {
                vec![]
            }
        );
    }
    authority.sync_local_player_pose(&local_health_feed());
    for (sequence, runtime_id, kind, died) in [
        (6, 1, protocol::ActorStatusKind::SpawnAlive, false),
        (7, 2, protocol::ActorStatusKind::Death, false),
        (8, 1, protocol::ActorStatusKind::Death, true),
        (9, 1, protocol::ActorStatusKind::Death, false),
    ] {
        authority
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Status(protocol::ActorStatusEvent {
                    runtime_id,
                    kind,
                    data: 0,
                })),
                Some(sequence),
            )
            .unwrap();
        let controls = authority.take_committed_controls();
        assert_eq!(
            controls,
            if died {
                vec![CommittedControlEvent::LocalDeath { sequence }]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn health_attributes_only_publish_local_deaths_in_the_active_dimension() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 1,
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
    for (sequence, runtime_id, dimension, current, died) in [
        (1, 2, 1, 0.0, false),
        (2, 1, 0, 0.0, false),
        (3, 1, 1, 0.0, true),
        (4, 1, 1, 0.0, false),
        (5, 1, 1, 20.0, false),
        (6, 1, 1, 0.0, true),
    ] {
        authority
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Attributes(
                    protocol::ActorAttributesUpdateEvent {
                        runtime_id,
                        dimension,
                        tick: 0,
                        attributes: Arc::from([protocol::ActorAttribute {
                            name: protocol_ids::Attribute::Health.wire_name().into(),
                            min: 0.0,
                            max: 20.0,
                            current,
                            default: None,
                            modifiers: Default::default(),
                        }]),
                    },
                )),
                Some(sequence),
            )
            .unwrap();
        let controls = authority.take_committed_controls();
        assert_eq!(
            controls,
            if died {
                vec![CommittedControlEvent::LocalDeath { sequence }]
            } else {
                vec![]
            }
        );
    }
    authority.reset_dimension(7, 0);
    authority
        .apply_ordered_event(
            WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health: 0 })),
            Some(8),
        )
        .unwrap();
    assert!(
        authority.take_committed_controls().is_empty(),
        "actor replacement does not create a new life"
    );
}
