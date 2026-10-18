//! Fresh edges and camera publication share one frame without replaying old work.
use super::*;

/// Appends a press whose held sample remains active through both dispatch boundaries.
fn hold_button(world: &mut World, button: MouseButton) {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    let press = MouseButtonInput {
        button,
        state: ButtonState::Pressed,
        window,
    };
    world
        .resource_mut::<Messages<MouseButtonInput>>()
        .write(press);
    world
        .resource_mut::<Messages<WindowEvent>>()
        .write(press.into());
}

/// Checks held target competition after an early press lacks its ray or movement sample.
fn held_attack_after_position_reset(actor: bool, early_ray: bool) {
    let (mut app, mut captured) = app_fixture(16);
    if !actor {
        remove_actor(app.world_mut());
    }
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    app.world_mut()
        .resource_mut::<MovementTicker>()
        .reset(7, 101, [4.5, 2.620_01, 8.5]);
    let origin = app.world().resource::<InteractionOriginSnapshot>().clone();
    if !early_ray {
        app.world_mut()
            .resource_mut::<InteractionOriginSnapshot>()
            .invalidate();
    }
    hold_button(app.world_mut(), MouseButton::Left);
    before_physics(&mut app);
    assert!(
        captured.drain().is_empty(),
        "the early press cannot be admitted"
    );

    app.world_mut().insert_resource(origin);
    attack_progress_tests::ticks(app.world_mut(), 102, 103);
    after_physics(&mut app);
    let packets = captured.drain();
    assert_eq!(
        packets
            .iter()
            .filter_map(observe_transaction)
            .filter(|transaction| transaction.actor_use().is_some())
            .count(),
        0,
        "the unavailable early press must not replay"
    );
    assert_eq!(
        app.world()
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        usize::from(!actor),
        "held mining must respect the refreshed actor pick"
    );
}

#[test]
fn ordered_frame_boundary_empty_early_capture_refreshes_held_attack_competition() {
    for actor in [true, false] {
        held_attack_after_position_reset(actor, false);
    }
}

#[test]
fn ordered_frame_boundary_missing_early_sample_keeps_held_attack_competition() {
    for actor in [true, false] {
        held_attack_after_position_reset(actor, true);
    }
}

#[test]
fn ordered_frame_boundary_delivered_mining_allows_the_next_frames_selection() {
    let (mut app, mut captured) = app_fixture(16);
    remove_actor(app.world_mut());
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    tap(app.world_mut(), MouseButton::Left);
    before_physics(&mut app);
    attack_progress_tests::ticks(app.world_mut(), 102, 103);
    after_physics(&mut app);
    assert!(
        captured
            .drain()
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .next()
            .is_some(),
        "the accepted mining carrier must enter the FIFO first"
    );
    next_slot(app.world_mut());
    before_physics(&mut app);
    assert_eq!(
        app.world()
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1),
        "delivered mining cannot consume the new frame's input"
    );
    after_physics(&mut app);
    assert_eq!(
        captured
            .drain()
            .iter()
            .filter_map(equipment)
            .map(|packet| packet.selected_slot)
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn ordered_frame_boundary_picks_use_the_updated_eye_and_direction_in_edge_order() {
    for actor in [false, true] {
        let (mut app, mut captured) = app_fixture(16);
        if !actor {
            remove_actor(app.world_mut());
        }
        let current = app.world().resource::<InteractionOriginSnapshot>().clone();
        let old = frame_origin(
            app.world(),
            Vec3::new(3.5, 2.5, 8.5),
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        );
        app.world_mut().insert_resource(old);
        next_slot(app.world_mut());
        tap(app.world_mut(), MouseButton::Middle);
        next_slot(app.world_mut());
        tap(app.world_mut(), MouseButton::Middle);
        before_physics(&mut app);
        attack_progress_tests::ticks(app.world_mut(), 102, 103);
        app.world_mut().insert_resource(current);
        after_physics(&mut app);
        let pick = if actor {
            protocol::actor_pick_request_packet(99, false)
        } else {
            protocol::block_pick_request_packet([4, 2, 6], false)
        };
        let sent = captured.drain();
        let order: Vec<_> = sent
            .iter()
            .filter_map(|packet| {
                equipment(packet)
                    .map(|equipment| equipment.selected_slot as i32)
                    .or_else(|| (*packet == pick).then_some(-1))
            })
            .collect();
        assert_eq!(order, [1, -1, 2, -1], "actor target: {actor}");
        frame(&mut app);
        assert!(captured.drain().is_empty(), "picks must not replay");
    }
}

#[test]
fn ordered_frame_boundary_selection_after_a_pick_waits_for_camera_publication() {
    let (mut app, mut captured) = app_fixture(16);
    tap(app.world_mut(), MouseButton::Middle);
    next_slot(app.world_mut());
    before_physics(&mut app);
    assert_eq!(
        app.world()
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(0)
    );
    assert!(captured.drain().is_empty());
    app.world_mut()
        .resource_mut::<client_state::ScreenFocus>()
        .ui_focused = true;
    after_physics(&mut app);
    assert!(captured.drain().is_empty());
    app.world_mut()
        .resource_mut::<client_state::ScreenFocus>()
        .ui_focused = false;
    frame(&mut app);
    assert!(captured.drain().is_empty(), "cancelled edges must expire");
}

#[test]
fn ordered_frame_boundary_held_use_cannot_overtake_a_pick() {
    let (mut app, mut captured) = app_fixture(16);
    remove_actor(app.world_mut());
    tap(app.world_mut(), MouseButton::Middle);
    hold_button(app.world_mut(), MouseButton::Right);
    before_physics(&mut app);
    assert!(
        captured.drain().is_empty(),
        "held use must wait with its edge"
    );
    after_physics(&mut app);
    let packets = captured.drain();
    let pick = packets
        .iter()
        .position(|packet| *packet == protocol::block_pick_request_packet([4, 2, 6], false))
        .expect("the pick must be sent");
    let used = packets
        .iter()
        .position(|packet| item_use(packet).is_some())
        .expect("the following use must be sent");
    assert!(pick < used);
}

#[test]
fn ordered_frame_boundary_held_mining_cannot_erase_a_pending_pick() {
    let (mut app, mut captured) = app_fixture(16);
    remove_actor(app.world_mut());
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    tap(app.world_mut(), MouseButton::Middle);
    hold_button(app.world_mut(), MouseButton::Left);
    before_physics(&mut app);
    attack_progress_tests::ticks(app.world_mut(), 102, 103);
    after_physics(&mut app);
    assert!(
        captured
            .drain()
            .contains(&protocol::block_pick_request_packet([4, 2, 6], false)),
        "the earlier pick must see the block before fresh held mining breaks it"
    );
}
