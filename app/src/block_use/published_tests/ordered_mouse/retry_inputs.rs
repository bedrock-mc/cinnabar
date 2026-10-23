//! Retries rebuild admission facts while held attacks keep observing their target.
use super::*;

const TARGET: [i32; 3] = [4, 2, 6];

/// Reads or replaces the fixture's target through the collision owner.
fn block(world: &mut World, replacement: Option<u32>) -> u32 {
    let collisions = world.resource::<PhysicsCollisionRegistries>();
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    let before = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
        Default::default(),
    )
    .primary_runtime_id(TARGET)
    .unwrap();
    if let Some(replacement) = replacement {
        world
            .resource_mut::<crate::runtime::world::ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .predict_block(TARGET, 0, replacement);
    }
    before
}

/// Clears the block behind the fixture actor without losing its collision column.
fn clear_block(world: &mut World) -> u32 {
    let air = world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .air_block_id();
    block(world, Some(air))
}

/// Changes only the authoritative game mode.
fn mode(world: &mut World, mode: PlayerGameMode) {
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(mode);
}

#[test]
fn ordered_mouse_retry_inputs_held_actor_or_air_attack_can_start_mining() {
    for actor in [false, true] {
        let (mut app, mut captured) = app_fixture(16);
        let original = if actor {
            block(app.world_mut(), None)
        } else {
            remove_actor(app.world_mut());
            clear_block(app.world_mut())
        };
        let world = app.world_mut();
        let window = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap();
        let event = MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        };
        world
            .resource_mut::<Messages<MouseButtonInput>>()
            .write(event);
        world
            .resource_mut::<Messages<WindowEvent>>()
            .write(event.into());
        frame(&mut app);
        assert!(app.world().resource::<MeleeRuntime>().actor_in_front());
        captured.drain();
        if actor {
            remove_actor(app.world_mut());
        } else {
            block(app.world_mut(), Some(original));
        }
        let mut sent = Vec::new();
        for tick in 102..106 {
            attack_progress_tests::ticks(app.world_mut(), tick, tick + 1);
            frame(&mut app);
            sent.extend(captured.drain());
        }
        assert!(
            sent.iter()
                .flat_map(protocol::test_support::packets::input_block_actions)
                .next()
                .is_some(),
            "holding Attack must begin mining after the target becomes a block; actor={actor}"
        );
        assert!(
            sent.iter()
                .filter_map(observe_transaction)
                .all(|tx| tx.actor_use().is_none()),
            "refreshing a held target must not admit another actor press"
        );
    }
}

#[test]
fn ordered_mouse_retry_inputs_mode_change_cancels_refused_instant_break() {
    let (mut app, mut captured) = app_fixture(2);
    remove_actor(app.world_mut());
    mode(app.world_mut(), PlayerGameMode::Creative);
    let before = block(app.world_mut(), None);
    fill(&app, 2);
    tap(app.world_mut(), MouseButton::Left);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    mode(app.world_mut(), PlayerGameMode::Survival);
    frame(&mut app);
    assert_eq!(
        block(app.world_mut(), None),
        before,
        "a refused creative break must not become a survival instant break"
    );
    attack_progress_tests::ticks(app.world_mut(), 102, 104);
    frame(&mut app);
    assert!(
        captured
            .drain()
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .next()
            .is_none()
    );
    assert!(!crate::ordered_interactions::pending(app.world()));
}

#[test]
fn ordered_mouse_retry_inputs_mode_change_cancels_refused_creative_throw() {
    let (mut app, mut captured) = app_fixture(2);
    remove_actor(app.world_mut());
    clear_block(app.world_mut());
    item_use_tests::hold_snowball(app.world_mut());
    mode(app.world_mut(), PlayerGameMode::Creative);
    fill(&app, 2);
    tap(app.world_mut(), MouseButton::Right);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    mode(app.world_mut(), PlayerGameMode::Survival);
    for _ in 0..3 {
        frame(&mut app);
        assert!(
            captured
                .drain()
                .iter()
                .filter_map(observe_transaction)
                .all(|tx| tx.item_use().is_none()),
            "a refused creative throw must not bypass survival consumption"
        );
    }
    assert_eq!(
        verified_use_selection(
            app.world().resource::<client_state::PlayerRuntime>(),
            app.world().resource::<UiRuntimeResource>()
        )
        .unwrap()
        .item
        .count(),
        16
    );
    assert!(!crate::ordered_interactions::pending(app.world()));
}

#[test]
fn ordered_mouse_retry_inputs_clear_actor_pick_rechecks_terrain() {
    for obstruct in [false, true] {
        let (mut app, mut captured) = app_fixture(8);
        let original = clear_block(app.world_mut());
        fill(&app, 8);
        tap(app.world_mut(), MouseButton::Middle);
        frame(&mut app);
        assert!(!crate::ordered_interactions::pending(app.world()));
        captured.drain();
        if obstruct {
            let mut client = app
                .world_mut()
                .resource_mut::<crate::runtime::world::ClientWorld>();
            client
                .stream
                .as_mut()
                .unwrap()
                .predict_block([4, 2, 8], 0, original);
        }
        let mut sent = Vec::new();
        for _ in 0..3 {
            frame(&mut app);
            sent.extend(captured.drain());
        }
        assert_eq!(
            sent.iter()
                .filter(|packet| **packet == protocol::actor_pick_request_packet(99, false))
                .count(),
            0,
            "a refused pick must not replay even when the ray is unchanged"
        );
        assert!(!crate::ordered_interactions::pending(app.world()));
    }
}

#[test]
fn ordered_mouse_refused_air_use_does_not_consume_inventory_later() {
    let (mut app, mut captured) = app_fixture(2);
    remove_actor(app.world_mut());
    clear_block(app.world_mut());
    item_use_tests::hold_snowball(app.world_mut());
    fill(&app, 2);
    tap(app.world_mut(), MouseButton::Right);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    attack_progress_tests::ticks(app.world_mut(), 102, 104);
    let mut sent = Vec::new();
    for _ in 0..3 {
        frame(&mut app);
        sent.extend(captured.drain());
    }
    let uses: Vec<_> = sent
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.item_use().cloned())
        .collect();
    assert!(uses.is_empty());
    assert_eq!(
        verified_use_selection(
            app.world().resource::<client_state::PlayerRuntime>(),
            app.world().resource::<UiRuntimeResource>()
        )
        .unwrap()
        .item
        .count(),
        16
    );
    assert!(!crate::ordered_interactions::pending(app.world()));
}

#[test]
fn ordered_mouse_retry_inputs_mode_change_preserves_an_accepted_mining_carrier() {
    let (mut app, mut captured) = app_fixture(8);
    remove_actor(app.world_mut());
    mode(app.world_mut(), PlayerGameMode::Creative);
    tap(app.world_mut(), MouseButton::Left);
    frame(&mut app);
    assert!(!app.world().resource::<MeleeRuntime>().press_pending());
    assert!(
        app.world()
            .resource::<MeleeRuntime>()
            .has_queued_work(app.world().resource::<MovementTicker>())
    );
    captured.drain();
    mode(app.world_mut(), PlayerGameMode::Survival);
    attack_progress_tests::ticks(app.world_mut(), 102, 105);
    frame(&mut app);
    let packets = captured.drain();
    assert!(
        packets
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .next()
            .is_some(),
        "already accepted mining must still publish its movement carrier"
    );
    frame(&mut app);
    assert!(
        captured
            .drain()
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .next()
            .is_none(),
        "accepted mining must not publish twice"
    );
    assert!(!crate::ordered_interactions::pending(app.world()));
}

#[test]
fn ordered_mouse_missing_pick_does_not_postpone_the_use_edge() {
    for actor in [false, true] {
        let (mut app, mut captured) = app_fixture(8);
        if !actor {
            remove_actor(app.world_mut());
            clear_block(app.world_mut());
            item_use_tests::hold_snowball(app.world_mut());
        }
        let authority = app
            .world()
            .resource::<MovementTicker>()
            .position_authority_identity();
        app.world_mut()
            .resource_mut::<BlockUseRuntime>()
            .retain_pick(&InteractionOriginSnapshot::default(), authority);
        tap(app.world_mut(), MouseButton::Right);
        next_slot(app.world_mut());
        frame(&mut app);
        assert!(!crate::ordered_interactions::pending(app.world()));
        assert!(
            captured
                .drain()
                .iter()
                .all(|packet| observe_transaction(packet).is_none())
        );
        app.world_mut()
            .run_system_cached(client_interaction::block_use::retain_block_use_pick)
            .unwrap();
        let mut sent = Vec::new();
        for _ in 0..3 {
            frame(&mut app);
            sent.extend(captured.drain());
        }
        assert_eq!(
            sent.iter()
                .filter_map(observe_transaction)
                .filter(|tx| {
                    if actor {
                        tx.actor_use().is_some()
                    } else {
                        tx.item_use().is_some()
                    }
                })
                .count(),
            0,
            "newly available evidence must not replay an old use edge; actor={actor}"
        );
        assert!(sent.iter().all(|packet| equipment(packet).is_none()));
        assert!(!crate::ordered_interactions::pending(app.world()));
    }
}
