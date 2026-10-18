//! Mixed physical input uses the real bounded session FIFO and the production cursor.
use super::*;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use protocol::ActorUseAction;

/// Supplies known equipment authority for a second empty hotbar cell.
fn selection_fixture() -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = mouse_fixture();
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        ui.publish_local_runtime_id(&mut player, 7, 42).unwrap();
        ui.inventory_ledger_mut(&mut player)
            .apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(0),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    slot: 1,
                },
                stack: protocol::NetworkItemStack::empty(),
                storage_item: None,
            }));
    });
    (world, captured)
}

/// Appends one complete button gesture to the same backend stream as ordinary mouse input.
fn tap(world: &mut World, button: MouseButton) {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    for state in [ButtonState::Pressed, ButtonState::Released] {
        let event = MouseButtonInput {
            button,
            state,
            window,
        };
        world
            .resource_mut::<Messages<MouseButtonInput>>()
            .write(event);
        world
            .resource_mut::<Messages<WindowEvent>>()
            .write(event.into());
    }
}

/// Appends a wheel selection without introducing another ordering source.
fn next_slot(world: &mut World) {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    world.resource_mut::<Messages<WindowEvent>>().write(
        MouseWheel {
            unit: MouseScrollUnit::Line,
            phase: bevy::input::touch::TouchPhase::Moved,
            x: 0.0,
            y: -1.0,
            window,
        }
        .into(),
    );
}

/// Projects outgoing actor transactions into action and selected-slot order.
fn actor_order(captured: &mut client_session::CapturedPackets) -> Vec<(ActorUseAction, i32)> {
    captured
        .drain()
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.actor_use().cloned())
        .map(|tx| (tx.action.unwrap(), tx.slot))
        .collect()
}

#[test]
fn ordered_mouse_attack_select_attack_uses_each_slots_descriptor_in_order() {
    let (mut world, mut captured) = selection_fixture();
    tap(&mut world, MouseButton::Left);
    next_slot(&mut world);
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(
        actor_order(&mut captured),
        [(ActorUseAction::Attack, 0), (ActorUseAction::Attack, 1)]
    );
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(1)
    );
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert!(captured.drain().is_empty());
}

#[test]
fn ordered_mouse_use_before_attack_is_not_replaced_by_a_frame_wide_attack_veto() {
    let (mut world, mut captured) = mouse_fixture();
    tap(&mut world, MouseButton::Right);
    tap(&mut world, MouseButton::Left);
    tap(&mut world, MouseButton::Right);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(
        actor_order(&mut captured),
        [(ActorUseAction::Interact, 0), (ActorUseAction::Attack, 0)],
        "the final use still respects the recent-attack rule"
    );
}

#[test]
fn ordered_mouse_attack_use_attack_preserves_both_attacks_and_the_use_timing_policy() {
    let (mut world, mut captured) = mouse_fixture();
    let scope = item_use_tests::hold_snowball(&mut world);
    world.insert_resource(crate::item_use::ModItemUsePolicy { scope: Some(scope) });
    tap(&mut world, MouseButton::Left);
    tap(&mut world, MouseButton::Right);
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    let packets = captured.drain();
    let kinds: Vec<_> = packets
        .iter()
        .filter_map(observe_transaction)
        .map(|tx| {
            if let Some(actor) = tx.actor_use() {
                assert_eq!(actor.action, Some(ActorUseAction::Attack));
                "attack"
            } else {
                assert!(tx.item_use().is_some());
                "use"
            }
        })
        .collect();
    assert_eq!(kinds, ["attack", "use", "attack"]);
}

#[test]
fn ordered_mouse_full_at_each_selection_prefix_never_repeats_an_accepted_attack() {
    for capacity in [2, 3] {
        let (mut world, _) = selection_fixture();
        let (network, mut captured) =
            NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(capacity);
        world.insert_resource(network);
        tap(&mut world, MouseButton::Left);
        next_slot(&mut world);
        tap(&mut world, MouseButton::Left);
        mouse_frame(&mut world, MouseButton::Left, &[]);
        let mut attacks = actor_order(&mut captured);
        assert!(!crate::ordered_interactions::pending(&world));
        assert_eq!(attacks, [(ActorUseAction::Attack, 0)]);
        for _ in 0..3 {
            mouse_frame(&mut world, MouseButton::Left, &[]);
            attacks.extend(actor_order(&mut captured));
        }
        assert_eq!(attacks, [(ActorUseAction::Attack, 0)]);
        assert!(!crate::ordered_interactions::pending(&world));
    }
}

#[test]
fn ordered_mouse_a_refused_attack_cannot_replay_its_old_pick_or_pose() {
    let (mut world, _) = mouse_fixture();
    let (network, mut captured) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(2);
    for _ in 0..2 {
        network
            .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
            .unwrap();
    }
    world.insert_resource(network);
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert!(!crate::ordered_interactions::pending(&world));
    assert!(actor_order(&mut captured).is_empty());
    world.insert_resource(InteractionOriginSnapshot::default());
    world.insert_resource(LocalViewPose::new(
        Vec3::new(30.0, 30.0, 30.0),
        Quat::from_rotation_y(1.0),
    ));
    mouse_frame(&mut world, MouseButton::Left, &[]);
    let packets = captured.drain();
    let attacks: Vec<_> = packets
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.actor_use().cloned())
        .collect();
    assert!(
        attacks.is_empty(),
        "a refused attack must not replay with an old pose"
    );
}

#[test]
fn ordered_mouse_ui_cancellation_drops_a_full_selection_and_its_later_attack() {
    let (mut world, _) = selection_fixture();
    let (network, mut captured) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(2);
    for _ in 0..2 {
        network
            .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
            .unwrap();
    }
    world.insert_resource(network);
    next_slot(&mut world);
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert!(!crate::ordered_interactions::pending(&world));
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(0)
    );
    captured.drain();
    world.resource_mut::<client_state::ScreenFocus>().ui_focused = true;
    mouse_frame(&mut world, MouseButton::Left, &[]);
    world.resource_mut::<client_state::ScreenFocus>().ui_focused = false;
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert!(captured.drain().is_empty());
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .inventory
            .pending_hotbar_selection(),
        None
    );
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(0)
    );
}

/// Publishes old movement without marking any tick as belonging to this input frame.
fn older_tick(world: &mut World) {
    let mut sample = gameplay::test_support::survival_mining::completed(102);
    sample.position = [4.5, 2.620_01, 8.5];
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample)
        .unwrap();
}

/// Admits completed movement and supplies its matching successful-write receipts.
fn flush_movement(world: &mut World) {
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let network = world.resource::<NetworkHandle<PackApplication>>();
        let mut written = Vec::new();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            gameplay::movement::OUTBOX_CAPACITY,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| {
                network.send_physics_packet(identity, packet, guard)?;
                written.push(identity);
                Ok::<(), client_session::PacketSendError>(())
            },
        )
        .unwrap();
        for identity in written {
            assert!(movement.acknowledge_physics_send(identity));
        }
    });
}

#[test]
fn ordered_mouse_older_movement_and_held_release_fence_the_entire_mixed_burst() {
    for release in [false, true] {
        let (mut world, mut captured) = selection_fixture();
        older_tick(&mut world);
        if release {
            world
                .resource_mut::<MovementTicker>()
                .hold_release_after_tick(
                    102,
                    vec![protocol::swing_arm_packet(42, protocol::SwingSource::Build)],
                );
        }
        next_slot(&mut world);
        tap(&mut world, MouseButton::Left);
        mouse_frame(&mut world, MouseButton::Left, &[]);
        assert!(
            captured.drain().is_empty(),
            "no interaction can overtake old movement or release"
        );
        assert_eq!(
            world
                .resource::<client_state::PlayerRuntime>()
                .selected_hotbar_slot(),
            Some(0)
        );
        flush_movement(&mut world);
        assert_eq!(
            world.resource::<MovementTicker>().has_held_release(),
            release
        );
        captured.drain();
        mouse_frame(&mut world, MouseButton::Left, &[]);
        assert_eq!(
            actor_order(&mut captured),
            [],
            "blocked frame must expire; release carrier present: {release}",
        );
        assert!(!world.resource::<MovementTicker>().has_held_release());
    }
}

#[test]
fn ordered_mouse_expiry_or_binding_authority_change_cannot_replay_a_refused_tap() {
    for expire in [false, true] {
        let (mut world, _) = mouse_fixture();
        let (network, mut captured) =
            NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(2);
        for _ in 0..2 {
            network
                .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
                .unwrap();
        }
        world.insert_resource(network);
        tap(&mut world, MouseButton::Left);
        mouse_frame(&mut world, MouseButton::Left, &[]);
        assert!(!crate::ordered_interactions::pending(&world));
        captured.drain();
        if expire {
            world
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_millis(
                    gameplay::interaction_authority::MAX_PENDING_INTERACTION_MILLIS + 1,
                ));
        } else {
            world
                .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
                .replace_bindings(semantic_input::ControlSettings::default())
                .unwrap();
        }
        mouse_frame(&mut world, MouseButton::Left, &[]);
        mouse_frame(&mut world, MouseButton::Left, &[]);
        assert!(captured.drain().is_empty());
    }
}

#[test]
fn ordered_mouse_actor_runtime_reuse_cannot_redirect_a_refused_press() {
    let (mut world, _) = mouse_fixture();
    let (network, mut captured) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(2);
    for _ in 0..2 {
        network
            .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
            .unwrap();
    }
    world.insert_resource(network);
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert!(!crate::ordered_interactions::pending(&world));
    captured.drain();
    actor_use_tests::spawn(&mut world, 7.8);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert!(actor_order(&mut captured).is_empty());
}

#[test]
fn ordered_mouse_pick_remapping_preserves_repeated_pick_requests() {
    use launcher::menu::settings_options::{BindingId, SettingsOptions};
    let (mut world, mut captured) = mouse_fixture();
    world.init_resource::<client_state::GameplayScreenControls>();
    let mut settings = SettingsOptions::default();
    assert!(settings.remap(
        BindingId::PickItem.row(false).unwrap(),
        semantic_input::PhysicalControl::MouseButton(4)
    ));
    world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .replace_bindings(settings.controls().unwrap())
        .unwrap();
    mouse_frame(&mut world, MouseButton::Left, &[]);
    captured.drain();
    tap(&mut world, MouseButton::Back);
    tap(&mut world, MouseButton::Back);
    mouse_frame(&mut world, MouseButton::Back, &[]);
    assert_eq!(
        captured.drain(),
        vec![protocol::actor_pick_request_packet(99, false); 2]
    );
}

#[test]
fn ordered_mouse_refused_use_and_attack_expire_with_their_frame() {
    let (mut world, _) = mouse_fixture();
    let (network, mut captured) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(2);
    for _ in 0..2 {
        network
            .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Build))
            .unwrap();
    }
    world.insert_resource(network);
    tap(&mut world, MouseButton::Right);
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert!(actor_order(&mut captured).is_empty());
    let mut sent = Vec::new();
    for _ in 0..3 {
        mouse_frame(&mut world, MouseButton::Left, &[]);
        sent.extend(actor_order(&mut captured));
    }
    assert_eq!(sent, []);
}

#[test]
fn ordered_mouse_backend_keyboard_focus_reset_discards_the_whole_batch() {
    let (mut world, mut captured) = mouse_fixture();
    world
        .resource_mut::<Messages<WindowEvent>>()
        .write(bevy::input::keyboard::KeyboardFocusLost.into());
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(attack_count(&mut captured), 0);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    tap(&mut world, MouseButton::Left);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(attack_count(&mut captured), 1);
}

#[path = "review.rs"]
mod review;
