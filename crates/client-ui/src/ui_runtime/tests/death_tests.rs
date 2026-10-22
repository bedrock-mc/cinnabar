use super::*;

#[test]
fn respawn_readiness_survives_unknown_and_repeated_zero_health() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply_respawn_ready(1, 1, 0).unwrap();
    assert_eq!(runtime.local_player_alive(), None);
    runtime.publish_local_player_alive(false);
    let generation = runtime.local_player_death_generation();
    runtime.publish_local_actor_health(None);
    assert_eq!(runtime.local_player_alive(), None);
    assert!(runtime.local_player_respawn_ready());
    runtime.publish_local_player_alive(false);
    runtime.apply_death_rules(protocol::DeathRules {
        immediate_respawn: Some(true),
        ..Default::default()
    });
    runtime.publish_local_player_alive(false);
    assert_eq!(runtime.local_player_alive(), Some(false));
    assert_eq!(runtime.local_player_death_generation(), generation);
    assert!(runtime.local_player_respawn_ready());

    runtime.publish_local_player_alive(true);
    runtime.publish_local_actor_health(None);
    runtime.publish_local_player_alive(false);
    assert_eq!(runtime.local_player_death_generation(), generation + 1);
    assert!(!runtime.local_player_respawn_ready());
}

#[test]
fn respawn_readiness_shares_the_ui_frontier_and_cannot_cross_sessions() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    runtime
        .apply(
            &mut player,
            envelope(1, 3, UiEvent::Hud(HudEvent::Health { health: 0 })),
        )
        .unwrap();
    for (session, sequence, millis) in [(2, 4, 30), (1, 3, 30), (1, 4, 29)] {
        assert!(
            runtime
                .apply_respawn_ready(session, sequence, millis)
                .is_err()
        );
        assert!(!runtime.local_player_respawn_ready());
    }
    runtime.apply_respawn_ready(1, 4, 30).unwrap();
    assert!(matches!(
        runtime.apply(
            &mut player,
            envelope(1, 4, UiEvent::Hud(HudEvent::Health { health: 20 }))
        ),
        Err(UiRuntimeError::StaleFifoSequence { .. })
    ));
    runtime.begin_session(2);
    assert!(!runtime.local_player_respawn_ready());
    assert_eq!(runtime.local_player_death_generation(), 0);
    assert_eq!(runtime.local_player_alive(), None);
    assert!(runtime.apply_respawn_ready(1, 5, 11).is_err());
    runtime.publish_local_player_alive(false);
    assert!(!runtime.local_player_respawn_ready());
}

/// Supplies a positive actor snapshot that can lag the committed local life state.
fn healthy_actor_snapshot() -> client_world::ActorSnapshot {
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: protocol::ActorKind::Player {
                    uuid: [2; 16],
                    username: "Player".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
                attributes: Arc::from([protocol::ActorAttribute {
                    name: protocol_ids::Attribute::Health.wire_name().into(),
                    min: 0.0,
                    max: 20.0,
                    current: 20.0,
                    default: None,
                    modifiers: Default::default(),
                }]),
            })),
        )
        .unwrap();
    stream.authority().actor(2).unwrap().clone()
}

#[test]
fn actor_health_projection_cannot_reorder_committed_life_transitions() {
    let actor = healthy_actor_snapshot();
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    runtime.publish_local_actor_health(Some(&actor));
    assert_eq!(runtime.local_player_alive(), None);
    runtime
        .apply(
            &mut player,
            envelope(1, 1, UiEvent::Hud(HudEvent::Health { health: 20 })),
        )
        .unwrap();
    runtime
        .apply(
            &mut player,
            envelope(1, 2, UiEvent::Hud(HudEvent::Health { health: 0 })),
        )
        .unwrap();
    runtime.publish_local_actor_health(Some(&actor));
    assert_eq!(runtime.local_player_alive(), Some(false));
    let generation = runtime.local_player_death_generation();
    runtime.apply_respawn_ready(1, 3, 30).unwrap();
    runtime.publish_local_actor_health(None);
    runtime.publish_local_actor_health(Some(&actor));
    assert_eq!(runtime.local_player_alive(), Some(false));
    assert_eq!(runtime.local_player_death_generation(), generation);
    assert!(runtime.local_player_respawn_ready());
    runtime
        .apply(
            &mut player,
            envelope(1, 4, UiEvent::Hud(HudEvent::Health { health: 20 })),
        )
        .unwrap();
    runtime.publish_local_actor_health(Some(&actor));
    assert_eq!(runtime.local_player_alive(), Some(true));
}
