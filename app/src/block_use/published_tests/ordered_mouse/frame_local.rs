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
    for _ in 0..semantic_input::MAX_PENDING_ACTION_EVENTS {
        tap(app.world_mut(), MouseButton::Middle);
    }
    mouse_frame(app.world_mut(), MouseButton::Left, &[ButtonState::Released]);
    assert!(!crate::ordered_interactions::held(
        app.world()
            .get_resource::<crate::ordered_interactions::InteractionRuntime>(),
        Action::Attack,
        false,
    ));
    captured.drain();
    attack_progress_tests::ticks(app.world_mut(), 102, 105);
    frame(&mut app);
    assert!(!crate::ordered_interactions::held(
        app.world()
            .get_resource::<crate::ordered_interactions::InteractionRuntime>(),
        Action::Attack,
        false,
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
