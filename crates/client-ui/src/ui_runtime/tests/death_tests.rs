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
