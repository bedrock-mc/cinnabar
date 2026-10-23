//! Ordered edges expire with their frame while accepted gameplay work keeps its owner.
use super::*;

#[test]
fn ordered_frame_local_refused_taps_never_replay() {
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        let (mut app, mut captured) = app_fixture(8);
        fill(&app, 8);
        tap(app.world_mut(), button);
        frame(&mut app);
        captured.drain();
        for _ in 0..3 {
            frame(&mut app);
            assert!(
                captured.drain().iter().all(|packet| {
                    observe_transaction(packet).is_none()
                        && *packet != protocol::actor_pick_request_packet(99, false)
                }),
                "a refused {button:?} tap must not survive its input frame"
            );
        }
    }
}

#[test]
fn ordered_frame_local_a_new_press_can_act_after_a_refused_tap() {
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        let (mut app, mut captured) = app_fixture(8);
        fill(&app, 8);
        tap(app.world_mut(), button);
        frame(&mut app);
        captured.drain();
        frame(&mut app);
        assert!(captured.drain().is_empty());
        tap(app.world_mut(), button);
        frame(&mut app);
        let packets = captured.drain();
        assert_eq!(
            packets
                .iter()
                .filter(|packet| {
                    observe_transaction(packet).is_some()
                        || **packet == protocol::actor_pick_request_packet(99, false)
                })
                .count(),
            1,
            "a fresh {button:?} press must use the new frame's admission opportunity"
        );
    }
}

#[test]
fn ordered_frame_local_new_use_forgets_the_previous_block_resolution() {
    let (mut world, mut captured) = mouse_fixture();
    tap(&mut world, MouseButton::Right);
    mouse_frame(&mut world, MouseButton::Right, &[]);
    assert!(world.resource::<BlockUseRuntime>().press_interacted());
    captured.drain();
    let scope = item_use_tests::hold_snowball(&mut world);
    world.insert_resource(crate::item_use::ModItemUsePolicy { scope: Some(scope) });
    mouse_frame(&mut world, MouseButton::Left, &[ButtonState::Pressed]);
    captured.drain();
    tap(&mut world, MouseButton::Right);
    mouse_frame(&mut world, MouseButton::Right, &[]);
    assert_eq!(
        captured
            .drain()
            .iter()
            .filter_map(observe_transaction)
            .filter(|tx| tx.item_use().is_some())
            .count(),
        1,
        "the earlier interaction must not consume a new air-use press"
    );
}

#[test]
fn ordered_frame_local_overflow_cannot_restore_a_cancelled_attack() {
    let (mut app, mut captured) = app_fixture(8);
    remove_actor(app.world_mut());
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    mouse_frame(app.world_mut(), MouseButton::Left, &[ButtonState::Pressed]);
    assert!(
        app.world()
            .resource::<MeleeRuntime>()
            .has_queued_work(app.world().resource::<MovementTicker>())
    );
    // Keep a different physical button down so the overflow cannot rearm on neutrality.
    mouse_frame(app.world_mut(), MouseButton::Back, &[ButtonState::Pressed]);
    mouse_frame_after_collection(
        app.world_mut(),
        MouseButton::Left,
        &[ButtonState::Released],
        semantic_input::DesktopEvents::invalidate,
    );
    assert!(!crate::ordered_interactions::held(
        app.world()
            .get_resource::<crate::ordered_interactions::InteractionRuntime>(),
        app.world()
            .resource::<client_state::SemanticInputSnapshot>(),
        Action::Attack,
    ));
    captured.drain();
    attack_progress_tests::ticks(app.world_mut(), 102, 105);
    frame(&mut app);
    assert!(!crate::ordered_interactions::held(
        app.world()
            .get_resource::<crate::ordered_interactions::InteractionRuntime>(),
        app.world()
            .resource::<client_state::SemanticInputSnapshot>(),
        Action::Attack,
    ));
    assert!(
        captured
            .drain()
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .next()
            .is_some(),
        "the already accepted mining carrier must still be delivered"
    );
}

#[test]
fn ordered_frame_local_overflow_ends_accepted_item_use_before_neutral_rearm() {
    use client_interaction::item_use::ItemUseRuntime;

    for identifier in ["minecraft:bow", "minecraft:trident"] {
        for release in [false, true] {
            let (mut app, mut captured) = app_fixture(8);
            remove_actor(app.world_mut());
            item_use_tests::hold_air_item(app.world_mut(), identifier, 1, [].into());
            app.world_mut()
                .resource_mut::<client_state::PlayerRuntime>()
                .facts
                .publish_player_game_mode(PlayerGameMode::Creative);
            {
                let mut client = app
                    .world_mut()
                    .resource_mut::<crate::runtime::world::ClientWorld>();
                let stream = client.stream.as_mut().unwrap();
                stream.predict_block([4, 2, 6], 0, stream.air_block_id());
            }
            mouse_frame(app.world_mut(), MouseButton::Right, &[ButtonState::Pressed]);
            assert!(
                app.world().resource::<ItemUseRuntime>().is_using(),
                "{identifier}"
            );
            assert!(
                app.world()
                    .resource::<ItemUseRuntime>()
                    .movement_modifier()
                    .is_some()
            );
            captured.drain();

            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            let states = if release {
                &[ButtonState::Released][..]
            } else {
                &[]
            };
            mouse_frame_after_collection(
                app.world_mut(),
                MouseButton::Right,
                states,
                semantic_input::DesktopEvents::invalidate,
            );
            after_physics(&mut app);
            let mut packets = captured.drain();
            for _ in 0..2 {
                frame(&mut app);
                packets.extend(captured.drain());
                assert!(crate::ordered_interactions::is_blocked(
                    app.world()
                        .get_resource::<crate::ordered_interactions::InteractionRuntime>()
                ));
                assert!(
                    !app.world().resource::<ItemUseRuntime>().is_using(),
                    "{identifier}, release: {release}"
                );
                assert_eq!(
                    app.world().resource::<ItemUseRuntime>().movement_modifier(),
                    None
                );
            }
            tap(app.world_mut(), MouseButton::Right);
            frame(&mut app);
            packets.extend(captured.drain());
            assert!(!app.world().resource::<ItemUseRuntime>().is_using());
            assert_eq!(
                packets
                    .iter()
                    .filter_map(observe_transaction)
                    .filter(|tx| tx.release().is_some())
                    .count(),
                1,
                "the accepted {identifier} use must end exactly once, release: {release}"
            );
            assert!(
                packets
                    .iter()
                    .filter_map(observe_transaction)
                    .all(|tx| tx.item_use().is_none())
            );
        }
    }
}
