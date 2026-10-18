//! Every kind of later input shares the same deferred-prefix boundary.

use super::*;
use client_presentation::aim_assist::{
    AimAssistFrame, AimAssistTarget, ServerAimAssist, TargetKind,
};
use protocol::test_support::{packets::equipment, transactions::observe_transaction};

#[derive(Clone, Copy, Debug)]
enum Edge {
    Press,
    Release,
    ConsumedRelease,
    AssistedRelease,
    Pick,
    Hotbar,
    Use,
    Attack,
    Mining,
}

#[derive(Clone, Copy, Debug)]
enum Boundary {
    DeferredPick,
    EarlyCapture,
    PostCamera,
    PendingTransport,
    ClosedSelection,
}

/// Enables action-facing assistance using the production camera and target owners.
fn assist(world: &mut World) {
    let context = client_presentation::camera::ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 90.0,
        actors: &|_| None,
    };
    let mut camera = client_presentation::camera::ServerCameraView::default();
    camera.apply(
        1,
        &protocol::CameraEvent::Presets(
            vec![protocol::CameraPreset {
                name: "minecraft:free".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    camera.apply(
        2,
        &protocol::CameraEvent::Instruction(Box::new(protocol::CameraInstructionEvent {
            set: Some(protocol::CameraSetInstruction {
                preset_id: 0,
                ease: None,
                position: None,
                rotation_degrees: None,
                facing_position: None,
                view_offset: None,
                entity_offset: None,
                default_preset: None,
                remove_ignore_starting_values: false,
            }),
            ..Default::default()
        })),
        &context,
    );
    world.insert_resource(camera);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &protocol::CameraEvent::AimAssist(protocol::CameraAimAssistSettings {
            preset_id: "".into(),
            view_angle: [60.0; 2],
            distance: 12.0,
            target_mode: protocol::CameraAimAssistTargetMode::Angle,
            action: protocol::CameraAimAssistAction::Set,
            show_debug_render: false,
        }),
    );
    let mut frame = AimAssistFrame::default();
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    let blocks = sim::PaletteWorld::new(
        stream.collision_store(),
        world
            .resource::<PhysicsCollisionRegistries>()
            .registry(stream.network_id_mode()),
        0,
        Default::default(),
    );
    frame.evaluate(
        &state,
        stream.authority(),
        &blocks,
        1,
        Vec3::new(4.5, 2.5, 8.5),
        Vec3::NEG_Z,
        None,
        None,
        |_| None,
        |_| &[],
    );
    frame.target = Some(AimAssistTarget {
        kind: TargetKind::Block {
            position: [4, 2, 6],
            face: 3,
        },
        minimum: Vec3::new(4.0, 2.0, 6.0),
        maximum: Vec3::new(5.0, 3.0, 7.0),
        point: Vec3::new(4.6, 2.4, 6.5),
        direction: Vec3::NEG_Z,
        score: 0.0,
    });
    world.insert_resource(frame);
    assert!(
        client_interaction::camera::aim_assist::action_rotation(
            world.resource::<AimAssistFrame>(),
            world.resource::<client_presentation::camera::ServerCameraView>(),
        )
        .is_some()
    );
}

/// Adds a physical button edge to the same arrival-ordered desktop batch.
fn button(events: &mut semantic_input::DesktopEvents, button: u8, pressed: bool) {
    events.push(semantic_input::DesktopEvent::Button {
        control: semantic_input::PhysicalControl::MouseButton(button),
        pressed,
        modifiers: semantic_input::ModifierChord::default(),
    });
}

/// Projects only externally visible interaction effects, leaving movement and swings aside.
fn interaction(packet: &protocol::Packet) -> bool {
    equipment(packet).is_some()
        || observe_transaction(packet).is_some()
        || !protocol::test_support::packets::input_block_actions(packet).is_empty()
}

#[test]
fn ordered_input_boundary_matrix_keeps_every_later_edge_behind_the_prefix() {
    for boundary in [
        Boundary::DeferredPick,
        Boundary::EarlyCapture,
        Boundary::PostCamera,
        Boundary::PendingTransport,
        Boundary::ClosedSelection,
    ] {
        for edge in [
            Edge::Press,
            Edge::Release,
            Edge::ConsumedRelease,
            Edge::AssistedRelease,
            Edge::Pick,
            Edge::Hotbar,
            Edge::Use,
            Edge::Attack,
            Edge::Mining,
        ] {
            let (mut world, mut captured) = respawn::charging_world(1);
            world.init_resource::<crate::melee::MeleeTarget>();
            world.init_resource::<client_state::GameplayScreenControls>();
            world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
                let mut player = world.resource_mut::<client_state::PlayerRuntime>();
                ui.publish_local_runtime_id(&mut player, 7, 42).unwrap();
            });
            set_hotbar_stack(&mut world, 1, None);
            if matches!(edge, Edge::AssistedRelease) {
                assist(&mut world);
            }
            if matches!(edge, Edge::Press | Edge::Attack) {
                actor_use_tests::spawn(&mut world, 7.5);
            }
            if matches!(edge, Edge::Use) {
                world.resource_mut::<ItemUseRuntime>().cancel_for_respawn();
                respawn::use_button(&mut world, false);
            }
            if matches!(edge, Edge::Release | Edge::ConsumedRelease) {
                attack_progress_tests::buttons(&mut world, vec![1, 2]);
                world
                    .run_system_cached(crate::melee::prepare_melee)
                    .unwrap();
                world
                    .run_system_cached(crate::melee::produce_attacks)
                    .unwrap();
                tick(&mut world);
                world
                    .run_system_cached(crate::melee::produce_attacks)
                    .unwrap();
                flush_and_acknowledge(&mut world);
                captured.drain();
                assert!(
                    world
                        .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                        .destroying_target()
                        .is_some()
                );
                if matches!(edge, Edge::ConsumedRelease) {
                    tick(&mut world);
                }
            }
            let origin = world.resource::<InteractionOriginSnapshot>().clone();
            if matches!(boundary, Boundary::EarlyCapture) {
                world
                    .resource_mut::<InteractionOriginSnapshot>()
                    .invalidate();
            }
            if matches!(boundary, Boundary::PostCamera) {
                let releasing = matches!(edge, Edge::Release);
                world.insert_resource(frame_origin(
                    &world,
                    Vec3::new(if releasing { 4.25 } else { 3.5 }, 2.5, 8.5),
                    if releasing {
                        Quat::IDENTITY
                    } else {
                        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
                    },
                ));
                if releasing {
                    tick(&mut world);
                }
            }
            let mut old_network = None;
            if matches!(boundary, Boundary::ClosedSelection) {
                let mode = world
                    .resource::<client_state::PlayerRuntime>()
                    .facts
                    .player_game_mode();
                world
                    .resource_mut::<client_state::PlayerRuntime>()
                    .inventory
                    .queue_local_hotbar_selection(1, mode);
                let (closed, _) = NetworkHandle::<PackApplication>::stub_with_control_sender();
                old_network = world.remove_resource::<NetworkHandle<PackApplication>>();
                world.insert_resource(closed);
            }
            if matches!(boundary, Boundary::PendingTransport) {
                let sample = tick(&mut world);
                world
                    .resource_mut::<MovementTicker>()
                    .hold_release_after_tick(
                        sample.tick,
                        vec![protocol::swing_arm_packet(42, protocol::SwingSource::Build)],
                    );
                flush_and_acknowledge(&mut world);
                captured.drain();
                let network = world.resource::<NetworkHandle<PackApplication>>();
                while network
                    .send_inventory_packet(protocol::swing_arm_packet(
                        42,
                        protocol::SwingSource::Build,
                    ))
                    .is_ok()
                {}
            }
            let mut events = semantic_input::DesktopEvents::default();
            if matches!(edge, Edge::ConsumedRelease) {
                button(&mut events, 1, false);
            }
            button(&mut events, 3, true);
            button(&mut events, 3, false);
            let mut held = vec![2];
            match edge {
                Edge::Press | Edge::Mining => {
                    button(&mut events, 1, true);
                    held.push(1);
                }
                Edge::Release => button(&mut events, 1, false),
                Edge::ConsumedRelease => {}
                Edge::AssistedRelease => {
                    button(&mut events, 2, false);
                    held.clear();
                }
                Edge::Pick => {
                    button(&mut events, 3, true);
                    button(&mut events, 3, false);
                }
                Edge::Hotbar => events.push(semantic_input::DesktopEvent::Scroll { next: true }),
                Edge::Use => {
                    button(&mut events, 2, true);
                    button(&mut events, 2, false);
                    held.clear();
                }
                Edge::Attack => {
                    button(&mut events, 1, true);
                    button(&mut events, 1, false);
                }
            }
            ordered_frame(&mut world, events, held);
            world
                .run_system_cached(crate::melee::prepare_melee)
                .unwrap();
            world
                .run_system_cached(crate::melee::produce_attacks)
                .unwrap();
            world
                .run_system_cached(crate::item_use::prepare_item_release)
                .unwrap();
            assert!(
                !world.resource::<ItemUseRuntime>().has_pending_release(),
                "maintenance observed a later release: {boundary:?}, {edge:?}"
            );
            let before_pick = |packet: &protocol::Packet| {
                let actions = protocol::test_support::packets::input_block_actions(packet);
                !interaction(packet)
                    || (matches!(edge, Edge::ConsumedRelease)
                        && !actions.is_empty()
                        && actions.iter().all(|action| {
                            action.0 == protocol::BlockActionKind::AbortDestroy.wire_value()
                        }))
            };
            let mut sent = captured.drain();
            assert!(
                sent.iter().all(before_pick),
                "a later effect passed the prefix: {boundary:?}, {edge:?}"
            );
            assert!(
                !world.resource::<MovementTicker>().has_held_release()
                    || matches!(boundary, Boundary::PendingTransport)
            );
            if matches!(edge, Edge::Release) {
                assert!(
                    world
                        .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                        .destroying_target()
                        .is_some(),
                    "a later button-up must not abort before the pick: {boundary:?}"
                );
            }
            if matches!(edge, Edge::ConsumedRelease)
                && matches!(
                    boundary,
                    Boundary::DeferredPick | Boundary::EarlyCapture | Boundary::PostCamera
                )
            {
                assert!(
                    world
                        .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                        .destroying_target()
                        .is_none(),
                    "a consumed button-up must stop mining before the pick: {boundary:?}"
                );
            }
            world.insert_resource(origin);
            if let Some(network) = old_network {
                // A closed selection expires this batch; restoring transport must not replay it.
                world
                    .run_system_cached(crate::item_use::produce_item_use)
                    .unwrap();
                world.insert_resource(network);
                captured.drain();
                release_and_select(&mut world, false);
                assert!(captured.drain().iter().all(|packet| {
                    *packet != protocol::block_pick_request_packet([4, 2, 6], false)
                }));
                continue;
            }
            flush_and_acknowledge(&mut world);
            world
                .run_system_cached(crate::item_use::produce_item_use)
                .unwrap();
            sent.extend(captured.drain());
            tick(&mut world);
            world
                .run_system_cached(crate::melee::prepare_melee)
                .unwrap();
            world
                .run_system_cached(crate::melee::produce_attacks)
                .unwrap();
            world
                .run_system_cached(crate::item_use::prepare_item_release)
                .unwrap();
            flush_and_acknowledge(&mut world);
            world
                .run_system_cached(crate::item_use::produce_item_use)
                .unwrap();
            sent.extend(captured.drain());
            let pick = if matches!(edge, Edge::Press | Edge::Attack) {
                protocol::actor_pick_request_packet(99, false)
            } else {
                protocol::block_pick_request_packet([4, 2, 6], false)
            };
            let first_pick = sent
                .iter()
                .position(|packet| *packet == pick)
                .unwrap_or_else(|| {
                    panic!("the deferred prefix disappeared: {boundary:?}, {edge:?}")
                });
            assert!(
                sent[..first_pick].iter().all(before_pick),
                "a later effect preceded the pick: {boundary:?}, {edge:?}"
            );
            if matches!(edge, Edge::AssistedRelease) {
                assert_eq!(
                    sent.iter()
                        .filter_map(observe_transaction)
                        .filter(|tx| tx.release().is_some())
                        .count(),
                    1,
                    "the deferred release must finish exactly once: {boundary:?}"
                );
            }
            if matches!(edge, Edge::Release | Edge::ConsumedRelease) {
                assert!(
                    world
                        .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                        .destroying_target()
                        .is_none(),
                    "the consumed release must stop mining: {boundary:?}, {edge:?}"
                );
            }
            let following_effect = match edge {
                Edge::Press | Edge::Attack => {
                    sent.iter().filter_map(observe_transaction).any(|tx| {
                        tx.actor_use().is_some_and(|actor| {
                            actor.action == Some(protocol::ActorUseAction::Attack)
                        })
                    })
                }
                Edge::Release | Edge::ConsumedRelease | Edge::Mining => sent
                    .iter()
                    .flat_map(protocol::test_support::packets::input_block_actions)
                    .any(|action| {
                        action.0
                            == if matches!(edge, Edge::Release | Edge::ConsumedRelease) {
                                protocol::BlockActionKind::AbortDestroy.wire_value()
                            } else {
                                protocol::BlockActionKind::StartDestroy.wire_value()
                            }
                    }),
                Edge::AssistedRelease => sent
                    .iter()
                    .filter_map(observe_transaction)
                    .any(|tx| tx.release().is_some()),
                Edge::Pick => sent.iter().filter(|packet| **packet == pick).count() == 2,
                Edge::Hotbar => sent
                    .iter()
                    .filter_map(equipment)
                    .any(|equipment| equipment.selected_slot == 1),
                Edge::Use => sent
                    .iter()
                    .filter_map(observe_transaction)
                    .any(|tx| tx.item_use().is_some()),
            };
            assert!(
                following_effect,
                "the later edge must resolve after the prefix: {boundary:?}, {edge:?}"
            );
        }
    }
}
