use super::*;

#[test]
fn fallback_visibility_tracks_authoritative_default_without_losing_raw_mode() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut fallback) = player_spawn(42, -7, 0.0) else {
        unreachable!();
    };
    fallback.game_mode = Some(ActorGameMode::Fallback);
    store.apply(1, 1, ActorEvent::Spawn(fallback));
    store.apply(1, 2, player_spawn(43, -8, 0.0));
    assert!(store.get(42).unwrap().is_render_eligible());

    assert_eq!(
        store.apply(
            1,
            3,
            ActorEvent::DefaultGameMode(DefaultActorGameModeEvent {
                game_mode: ActorGameMode::Spectator,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.get(42).unwrap().game_mode,
        Some(ActorGameMode::Fallback),
        "fallback resolution must not erase packet attribution"
    );
    assert!(!store.get(42).unwrap().is_render_eligible());
    assert!(store.get(43).unwrap().is_render_eligible());

    store.apply(
        1,
        4,
        ActorEvent::DefaultGameMode(DefaultActorGameModeEvent {
            game_mode: ActorGameMode::Creative,
        }),
    );
    assert!(store.get(42).unwrap().is_render_eligible());

    store.begin_session(2, 0);
    let ActorEvent::Spawn(mut fallback) = player_spawn(44, -9, 0.0) else {
        unreachable!();
    };
    fallback.game_mode = Some(ActorGameMode::Fallback);
    store.apply(2, 1, ActorEvent::Spawn(fallback));
    assert_eq!(
        store.get(44).unwrap().resolved_game_mode,
        Some(ActorGameMode::Survival)
    );
}

#[test]
fn forced_actor_move_snaps_without_teleport_attribution() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(42, -7, 0.0));
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [Some(100.0), Some(72.0), Some(-4.0)],
            position_origin: ActorPositionOrigin::Feet,
            pitch: Some(10.0),
            yaw: Some(20.0),
            head_yaw: Some(30.0),
            on_ground: Some(true),
            teleported: false,
            snap: true,
            player_mode: None,
            source_tick: Some(10),
        }),
    );

    let actor = store.get(42).unwrap();
    assert_eq!(actor.position, [100.0, 72.0, -4.0]);
    assert_eq!(actor.previous_pose, actor.received_pose);
    assert_eq!(actor.previous_pose, actor.current_pose());
    assert_eq!(actor.interpolation_ticks_remaining, 0);
    assert_eq!(actor.velocity, [0.0; 3]);
    assert!(!actor.teleported);
}
