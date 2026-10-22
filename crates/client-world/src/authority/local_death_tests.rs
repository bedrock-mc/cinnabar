use super::tests::local_health_feed;
use super::*;

#[test]
fn local_death_controls_follow_accepted_health_and_actor_transitions() {
    let mut authority = authority();
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
    let mut authority = authority();
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

/// Starts an isolated authority with an unknown local life state in the Nether.
fn authority() -> WorldAuthority {
    WorldAuthority::new(
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
    )
}

/// Builds a local health update with the same wire range used by the player HUD.
fn health_update(current: f32, max: f32) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Attributes(
        protocol::ActorAttributesUpdateEvent {
            runtime_id: 1,
            dimension: 1,
            tick: 0,
            attributes: Arc::from([protocol::ActorAttribute {
                name: protocol_ids::Attribute::Health.wire_name().into(),
                min: 0.0,
                max,
                current,
                default: None,
                modifiers: Default::default(),
            }]),
        },
    ))
}

#[test]
fn readiness_before_initial_health_requires_a_new_life_before_another_death() {
    for attributes in [false, true] {
        let mut authority = authority();
        authority.commit_local_respawn_ready(1);
        for (sequence, health, died) in [(2, 0, false), (3, 0, false), (4, 20, false), (5, 0, true)]
        {
            let event = if attributes {
                health_update(health as f32, 20.0)
            } else {
                WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health }))
            };
            authority
                .apply_ordered_event(event, Some(sequence))
                .unwrap();
            assert_eq!(
                authority.take_committed_controls(),
                if died {
                    vec![CommittedControlEvent::LocalDeath { sequence }]
                } else {
                    vec![]
                }
            );
        }
    }
}

#[test]
fn rejected_health_attributes_cannot_arm_or_rearm_a_death_hold() {
    for with_pose in [false, true] {
        let mut authority = authority();
        if with_pose {
            authority.sync_local_player_pose(&local_health_feed());
        }
        for (index, (current, max)) in [
            (-1.0, 20.0),
            (21.0, 20.0),
            (f32::NAN, 20.0),
            (f32::INFINITY, 20.0),
            (0.0, 0.0),
            (0.0, f32::NAN),
            (0.001, 20.0),
        ]
        .into_iter()
        .enumerate()
        {
            authority
                .apply_ordered_event(health_update(current, max), Some(index as u64 + 1))
                .unwrap();
            assert!(
                authority.take_committed_controls().is_empty(),
                "{current}/{max}"
            );
        }
        authority
            .apply_ordered_event(health_update(0.0, 20.0), Some(8))
            .unwrap();
        assert_eq!(
            authority.take_committed_controls(),
            [CommittedControlEvent::LocalDeath { sequence: 8 }]
        );
        authority
            .apply_ordered_event(health_update(21.0, 20.0), Some(9))
            .unwrap();
        authority
            .apply_ordered_event(health_update(0.0, 20.0), Some(10))
            .unwrap();
        assert!(authority.take_committed_controls().is_empty());
    }
}
