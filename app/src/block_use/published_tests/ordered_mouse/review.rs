//! Real maintenance, movement admission and inventory ingress between ordered retries.
use super::*;
use protocol::test_support::packets::equipment;

#[path = "equipment.rs"]
mod equipment_tests;

#[path = "mining.rs"]
mod mining_tests;

#[path = "retry_inputs.rs"]
mod retry_inputs;

#[path = "frame_local.rs"]
mod frame_local;

#[path = "frame_boundary.rs"]
mod frame_boundary;

/// Installs the real network-stage schedule around the physical-input fixture.
fn app_fixture(capacity: usize) -> (App, client_session::CapturedPackets) {
    let (mut world, _) = selection_fixture();
    world.init_resource::<bevy::input::mouse::AccumulatedMouseScroll>();
    set_hotbar_stack(&mut world, 2, None);
    let (network, captured) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(capacity);
    world.insert_resource(network);
    (attack_progress_tests::interaction_app(world), captured)
}

/// Samples a backend batch and runs the ordinary pre-physics interaction opportunity.
fn before_physics(app: &mut App) {
    let world = app.world_mut();
    world.run_system_cached(mouse_button_input_system).unwrap();
    world
        .run_system_cached(crate::semantic_controls::collect_raw_input)
        .unwrap();
    world
        .run_system_cached(crate::semantic_controls::route_semantic_input)
        .unwrap();
    world
        .run_system_cached(crate::semantic_controls::finalize_semantic_input_after_ui_authority)
        .unwrap();
    world
        .run_system_cached(crate::hotbar::flush_hotbar_selection)
        .unwrap();
    world.run_system_cached(produce_block_use).unwrap();
}

/// Runs attack, release, actual movement admission and item maintenance in production order.
fn after_physics(app: &mut App) {
    app.world_mut().run_schedule(Update);
    app.world_mut()
        .resource_mut::<Messages<MouseButtonInput>>()
        .clear();
    app.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .clear();
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::ZERO;
}

/// Runs one rendered interaction frame with no extra physics completion.
fn frame(app: &mut App) {
    before_physics(app);
    after_physics(app);
}

/// Fills packet slots while retaining the production FIFO and its delimiter reservation.
fn fill(app: &App, slots: usize) {
    let network = app.world().resource::<NetworkHandle<PackApplication>>();
    for _ in 0..slots {
        network
            .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
            .unwrap();
    }
}

/// Removes the fixture actor through world ingress while leaving its block ray intact.
fn remove_actor(world: &mut World) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: 0,
                unique_id: 99,
            })),
        )
        .unwrap();
    stream.poll([4.5, 2.620_01, 8.5], 0);
    assert_eq!(stream.committed_sequence(), sequence);
}

/// Applies an authoritative inventory event through the real retained ingress and UI drain.
fn inventory_event(world: &mut World, event: InventoryEvent) {
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        ui.enqueue_inventory_event(&mut player, 7, 1, event)
            .unwrap();
        ui.drain_pending_inventory(&mut player);
    });
}

#[test]
fn ordered_mouse_review_refused_air_press_expires_before_item_maintenance() {
    let (mut app, mut captured) = app_fixture(2);
    remove_actor(app.world_mut());
    item_use_tests::hold_snowball(app.world_mut());
    {
        let mut client = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream.predict_block([4, 2, 6], 0, stream.air_block_id());
    }
    fill(&app, 2);
    tap(app.world_mut(), MouseButton::Right);
    next_slot(app.world_mut());
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    let mut sent = Vec::new();
    for _ in 0..4 {
        frame(&mut app);
        sent.extend(captured.drain());
    }
    let uses: Vec<_> = sent
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.item_use().cloned())
        .collect();
    assert!(
        uses.is_empty(),
        "refused air input must expire with its frame"
    );
    assert!(sent.iter().all(|packet| equipment(packet).is_none()));
}

#[test]
fn ordered_mouse_review_refused_attack_does_not_delay_movement() {
    let (mut app, mut captured) = app_fixture(2);
    before_physics(&mut app);
    attack_progress_tests::ticks(app.world_mut(), 102, 103);
    after_physics(&mut app);
    captured.drain();
    fill(&app, 1);
    tap(app.world_mut(), MouseButton::Left);
    before_physics(&mut app);
    attack_progress_tests::ticks(app.world_mut(), 103, 104);
    after_physics(&mut app);
    let mut sent = captured.drain();
    for _ in 0..4 {
        frame(&mut app);
        sent.extend(captured.drain());
    }
    let attacks: Vec<_> = sent
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.actor_use().cloned())
        .filter(|tx| tx.action == Some(ActorUseAction::Attack))
        .collect();
    assert!(
        attacks.is_empty(),
        "a refused attack cannot postpone movement or replay later"
    );
    assert!(sent.iter().any(|packet| {
        protocol::player_auth_input_trace_sample(packet).is_some_and(|sample| sample.tick == 103)
    }));
}

/// Produces a real controller cycle and lets its ordinary flusher encounter Full.
fn refused_controller_selection(app: &mut App) {
    let mut gamepad = bevy::input::gamepad::Gamepad::default();
    gamepad
        .digital_mut()
        .press(bevy::input::gamepad::GamepadButton::RightTrigger);
    gamepad.digital_mut().clear();
    let controller = app.world_mut().spawn(gamepad).id().index().index();
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    let snapshot = router
        .route_and_finalize(semantic_input::DeviceFrame {
            controllers: vec![semantic_input::ControllerFrame {
                device_id: controller,
                buttons: vec![5],
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::X;
    app.world_mut().insert_resource(router);
    app.world_mut().insert_resource(SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    app.world_mut()
        .run_system_cached(client_interaction::hotbar::queue_hotbar_selection)
        .unwrap();
    app.world_mut()
        .run_system_cached(crate::hotbar::flush_hotbar_selection)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<client_state::PlayerRuntime>()
            .inventory
            .pending_hotbar_selection(),
        Some(1)
    );
}

#[test]
fn ordered_mouse_review_idle_desktop_announces_an_older_controller_selection_on_the_next_tick() {
    let (mut app, mut captured) = app_fixture(2);
    fill(&app, 2);
    refused_controller_selection(&mut app);
    captured.drain();
    frame(&mut app);
    assert!(captured.drain().is_empty());
    attack_progress_tests::ticks(app.world_mut(), 102, 103);
    frame(&mut app);
    let slots: Vec<_> = captured
        .drain()
        .iter()
        .filter_map(equipment)
        .map(|e| e.selected_slot)
        .collect();
    assert_eq!(slots, [1]);
    assert_eq!(
        app.world()
            .resource::<client_state::PlayerRuntime>()
            .inventory
            .pending_hotbar_selection(),
        None
    );
}

#[test]
fn ordered_mouse_review_new_selection_supersedes_the_older_controller_selection() {
    let (mut app, mut captured) = app_fixture(4);
    fill(&app, 4);
    refused_controller_selection(&mut app);
    captured.drain();
    next_slot(app.world_mut());
    tap(app.world_mut(), MouseButton::Left);
    let mut sent = Vec::new();
    for _ in 0..4 {
        frame(&mut app);
        sent.extend(captured.drain());
    }
    let slots: Vec<_> = sent
        .iter()
        .filter_map(equipment)
        .map(|e| e.selected_slot)
        .collect();
    assert_eq!(slots, [2]);
    let attacks: Vec<_> = sent
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.actor_use().cloned())
        .collect();
    assert_eq!(attacks.len(), 1);
    assert_eq!(attacks[0].slot, 2);
}

#[test]
fn ordered_mouse_review_forced_server_selection_cancels_a_refused_desktop_selection() {
    let (mut app, mut captured) = app_fixture(2);
    fill(&app, 2);
    next_slot(app.world_mut());
    frame(&mut app);
    captured.drain();
    inventory_event(
        app.world_mut(),
        InventoryEvent::SelectedSlot(protocol::SelectedSlotEvent {
            container: ContainerIdentity::window(0),
            slot: 5,
            select_slot: true,
        }),
    );
    for _ in 0..2 {
        frame(&mut app);
        assert!(captured.drain().iter().all(|p| equipment(p).is_none()));
    }
    assert_eq!(
        app.world()
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(5)
    );
}

#[test]
fn ordered_mouse_review_authoritative_cell_write_preserves_the_local_selection() {
    let (mut app, mut captured) = app_fixture(2);
    fill(&app, 2);
    next_slot(app.world_mut());
    frame(&mut app);
    captured.drain();
    inventory_event(
        app.world_mut(),
        InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 1,
            },
            stack: protocol::NetworkItemStack {
                network_id: 1,
                count: 1,
                stack_network_id: 77,
                ..protocol::NetworkItemStack::empty()
            },
            storage_item: None,
        }),
    );
    for _ in 0..2 {
        frame(&mut app);
        assert!(captured.drain().iter().all(|p| equipment(p).is_none()));
    }
    assert_eq!(
        app.world()
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1)
    );
}

#[test]
fn ordered_mouse_review_overflow_drains_an_accepted_block_stop_before_neutral_rearm() {
    let (mut app, mut captured) = app_fixture(8);
    remove_actor(app.world_mut());
    set_hotbar_stack(app.world_mut(), 0, Some("minecraft:stone"));
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    {
        let mut client = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let stone = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:stone")
            .unwrap()
            .sequential_id;
        stream.predict_block([4, 2, 6], 0, stone);
    }
    mouse_frame(app.world_mut(), MouseButton::Right, &[ButtonState::Pressed]);
    app.world_mut()
        .run_system_cached(produce_block_use)
        .unwrap();
    assert!(
        app.world()
            .resource::<BlockUseRuntime>()
            .last_success_destination()
            .is_some(),
        "fixture must admit a real placement"
    );
    captured.drain();
    fill(&app, 8);
    mouse_frame(
        app.world_mut(),
        MouseButton::Right,
        &[ButtonState::Released],
    );
    assert!(app.world().resource::<BlockUseRuntime>().stopping());
    mouse_frame_after_collection(
        app.world_mut(),
        MouseButton::Left,
        &[ButtonState::Pressed],
        semantic_input::DesktopEvents::invalidate,
    );
    after_physics(&mut app);
    captured.drain();
    let mut sent = Vec::new();
    for _ in 0..2 {
        frame(&mut app);
        sent.extend(captured.drain());
    }
    let destination = app
        .world()
        .resource::<BlockUseRuntime>()
        .last_success_destination();
    assert_eq!(
        destination, None,
        "accepted stop must drain while a desktop button remains down"
    );
    assert_eq!(
        sent.iter()
            .filter(|p| p.kind() == protocol::stop_item_use_on_packet(42, [4, 2, 7]).kind())
            .count(),
        1
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert!(
        sent.iter()
            .filter_map(observe_transaction)
            .all(|tx| tx.item_use().is_none())
    );
}

#[test]
fn ordered_mouse_review_changed_world_cancels_a_frozen_block_pick() {
    let (mut app, mut captured) = app_fixture(2);
    remove_actor(app.world_mut());
    fill(&app, 2);
    tap(app.world_mut(), MouseButton::Middle);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    {
        let mut client = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream.predict_block([4, 2, 6], 0, stream.air_block_id());
    }
    frame(&mut app);
    assert!(
        captured
            .drain()
            .iter()
            .all(|p| p.kind() != protocol::block_pick_request_packet([4, 2, 6], false).kind())
    );
}

#[test]
fn ordered_mouse_review_changed_world_cancels_a_frozen_block_use() {
    let (mut app, mut captured) = app_fixture(8);
    remove_actor(app.world_mut());
    set_hotbar_stack(app.world_mut(), 0, Some("minecraft:stone"));
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let stone = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:stone")
            .unwrap()
            .sequential_id;
        let mut client = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        client
            .stream
            .as_mut()
            .unwrap()
            .predict_block([4, 2, 6], 0, stone);
    }
    fill(&app, 8);
    tap(app.world_mut(), MouseButton::Right);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    {
        let mut client = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream.predict_block([4, 2, 6], 0, stream.air_block_id());
    }
    frame(&mut app);
    assert!(
        captured
            .drain()
            .iter()
            .filter_map(observe_transaction)
            .all(|tx| tx.item_use().is_none())
    );
}

#[path = "world.rs"]
mod world_evidence;
