//! A new life cannot inherit releases retained for ordinary position corrections.

use super::*;
use crate::movement::integration_tests::VersionedFloor;
use gameplay::movement::flush_player_auth_inputs_guarded;
use simulation_clock::TICK_DURATION;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
enum ReleaseStage {
    Pending,
    Held,
    FacingWritten,
    Admitted,
}

/// Writes queued inputs through the session FIFO and settles their socket receipts.
fn write_inputs(ticker: &mut MovementTicker, network: &NetworkHandle<PackApplication>) {
    let mut receipts = Vec::new();
    flush_player_auth_inputs_guarded(
        ticker,
        8,
        Some(evidence_context()),
        |identity, packet, guard, _| {
            receipts.push(identity);
            network.send_physics_packet(identity, packet, guard)
        },
    )
    .unwrap();
    for receipt in receipts {
        assert!(ticker.acknowledge_physics_send(receipt));
    }
}

/// Stages a real release at each transport boundary, then runs committed respawn handling.
fn assert_respawn_discards_release(stage: ReleaseStage, search: bool, predicted: bool) {
    let origin = [0.0, 70.0, 0.0];
    let destination = [8.5, 71.620_01, -4.25];
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let mut ticker = MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let facing = physics
        .advance(
            TICK_DURATION,
            sim::MovementInput::default(),
            &VersionedFloor(1),
        )
        .samples
        .remove(0);
    ticker.enqueue_completed_physics(facing.clone()).unwrap();
    if matches!(stage, ReleaseStage::Pending) {
        assert!(physics.request_riptide(3, [-30.0, 90.0]));
    } else {
        let stack = protocol::NetworkItemStack {
            network_id: 2,
            count: 1,
            stack_network_id: 9,
            ..protocol::NetworkItemStack::empty()
        };
        let item =
            protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
        let release = protocol::release_item_packet(protocol::HeldItemRequest {
            hand: protocol::ItemUseHand::Mainhand,
            selected_slot: 0,
            selected_item: item,
            player_position: facing.position,
        })
        .unwrap();
        ticker.hold_release_after_tick(facing.tick, vec![release]);
        ticker.retain_release_use_modifier(Some(0.35));
        ticker.request_riptide(&mut physics, 3, [-30.0, 90.0]);
        if matches!(stage, ReleaseStage::FacingWritten | ReleaseStage::Admitted) {
            write_inputs(&mut ticker, &network);
            ticker.send_held_release(|_| Err(gameplay::BatchSendError::Full));
            assert!(ticker.has_held_release());
        }
        if matches!(stage, ReleaseStage::Admitted) {
            ticker.send_held_release(|packets| network.send_inventory_packets(packets));
            assert!(!ticker.has_held_release());
            assert_eq!(captured.drain().len(), 2, "facing input precedes release");
        }
    }
    if predicted {
        let launch = physics
            .advance(
                TICK_DURATION,
                sim::MovementInput::default(),
                &VersionedFloor(1),
            )
            .samples
            .remove(0);
        assert!(launch.processed.spin_started);
        ticker.enqueue_completed_physics(launch).unwrap();
    }
    captured.drain();
    let mut app = wiring_app(ticker, physics);
    app.insert_resource(network)
        .insert_resource(client_presentation::camera::AutoFly::new(false))
        .init_resource::<client_state::SemanticInputSnapshot>()
        .add_systems(
            Update,
            (
                advance_dimension_transfer,
                crate::movement::advance_local_physics,
            )
                .chain()
                .after(reconcile_world_stream_before_physics),
        );
    if search {
        submit(
            &mut app,
            1,
            WorldEvent::Respawn(RespawnEvent {
                position: [300.5, 32_767.0, 400.5],
                state: 0,
                runtime_entity_id: 0,
            }),
        );
        let before = app.world().resource::<MovementTicker>().completed_tick();
        app.world_mut()
            .resource_mut::<bevy::prelude::Time<bevy::time::Real>>()
            .advance_by(TICK_DURATION * 2);
        app.update();
        assert!(app.world().resource::<ClientWorld>().respawn.input_held());
        let ticker = app.world().resource::<MovementTicker>();
        assert_eq!(ticker.completed_tick(), before + 2);
        assert!(
            !ticker.has_unsent_inputs(),
            "search must discard old launch inputs"
        );
        assert!(
            !ticker.has_held_release(),
            "search must revoke even written-facing releases"
        );
        assert_eq!(ticker.release_use_modifier(), None);
    }
    app.world_mut()
        .resource_mut::<bevy::prelude::Time<bevy::time::Real>>()
        .advance_by(Duration::ZERO);
    submit(
        &mut app,
        if search { 2 } else { 1 },
        WorldEvent::Respawn(RespawnEvent {
            position: destination,
            state: 1,
            runtime_entity_id: 0,
        }),
    );
    app.update();
    assert!(!app.world().resource::<ClientWorld>().respawn.input_held());
    let mut ticker = app.world_mut().remove_resource::<MovementTicker>().unwrap();
    let mut physics = app
        .world_mut()
        .remove_resource::<LocalPhysicsController>()
        .unwrap();
    assert!(!ticker.has_held_release(), "ready must revoke old releases");
    assert_eq!(ticker.release_use_modifier(), None);
    ticker.send_held_release(|_| panic!("release from a previous life"));
    assert_eq!(physics.network_position(), Some(destination));
    let mut ordinary = LocalPhysicsController::default();
    ordinary.reanchor_network_position(destination, ticker.completed_tick(), false);
    // The edge anchor discards its current frame's elapsed time.
    physics.advance(
        Duration::ZERO,
        sim::MovementInput::default(),
        &VersionedFloor(1),
    );
    for _ in 0..3 {
        let sample = physics
            .advance(
                TICK_DURATION,
                sim::MovementInput::default(),
                &VersionedFloor(1),
            )
            .samples
            .remove(0);
        let expected = ordinary
            .advance(
                TICK_DURATION,
                sim::MovementInput::default(),
                &VersionedFloor(1),
            )
            .samples
            .remove(0);
        assert!(
            !sample.processed.spin_started,
            "pre-death {stage:?} launch survived respawn (search={search}, predicted={predicted})"
        );
        assert_eq!(sample.position, expected.position);
        assert_eq!(sample.velocity, expected.velocity);
        assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let network = app.world().resource::<NetworkHandle<PackApplication>>();
    captured.drain();
    write_inputs(&mut ticker, network);
    let packets = captured.drain();
    assert_eq!(packets.len(), 3);
    for packet in packets {
        let sample = player_auth_input_trace_sample(&packet).unwrap();
        assert!(!sample.flag_names.contains(&"StartSpinAttack"));
        assert!(!sample.flag_names.contains(&"StopSpinAttack"));
    }
}

#[test]
fn riptide_admitted_release_is_discarded_by_respawn_search() {
    for predicted in [false, true] {
        assert_respawn_discards_release(ReleaseStage::Admitted, true, predicted);
    }
}

#[test]
fn riptide_admitted_release_is_discarded_by_direct_ready_respawn() {
    for predicted in [false, true] {
        assert_respawn_discards_release(ReleaseStage::Admitted, false, predicted);
    }
}

#[test]
fn riptide_held_release_and_modifier_are_discarded_by_respawn() {
    for stage in [ReleaseStage::Held, ReleaseStage::FacingWritten] {
        for search in [false, true] {
            assert_respawn_discards_release(stage, search, false);
        }
    }
}

#[test]
fn riptide_pending_request_is_discarded_by_respawn() {
    for search in [false, true] {
        assert_respawn_discards_release(ReleaseStage::Pending, search, false);
    }
}
