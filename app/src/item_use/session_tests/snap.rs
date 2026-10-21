//! A backpressured release and its launch keep the same correction lifetime.

use super::*;
use client_interaction::movement::{LocalPhysicsController, MovementTicker};
use gameplay::movement::{
    MovementSource, PhysicsCorrectionMode, reconcile_candidate_physics_correction,
};

#[test]
fn riptide_release_and_launch_survive_snap_and_failed_replay_together() {
    for mode in [
        PhysicsCorrectionMode::Snap,
        PhysicsCorrectionMode::ReplayIfRetained,
    ] {
        for (facing_written, predicted_ticks, snap_after_admission) in [
            (true, 0, false),
            (true, 2, false),
            (true, 0, true),
            (false, 0, false),
            (false, 2, false),
        ] {
            assert_release_after_snap(mode, facing_written, predicted_ticks, snap_after_admission);
        }
    }
}

/// Corrects a staged release before admission, optionally after its launch was predicted.
fn assert_release_after_snap(
    mode: PhysicsCorrectionMode,
    facing_written: bool,
    predicted_ticks: u32,
    snap_after_admission: bool,
) {
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let origin = [0.0, 65.620_01, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let sample = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        )
        .samples
        .remove(0);
    ticker.enqueue_completed_physics(sample.clone()).unwrap();
    let frame = use_frame(sample.tick);
    let selection = frame.selection.unwrap();
    let release = protocol::release_item_packet(protocol::HeldItemRequest {
        hand: protocol::ItemUseHand::Mainhand,
        selected_slot: selection.slot,
        selected_item: selection.item,
        player_position: sample.position,
    })
    .unwrap();
    ticker.hold_release_after_tick(sample.tick, vec![release]);
    let facing = super::super::riptide::release_facing(
        None,
        &client_presentation::local_player::LocalViewPose::default(),
    );
    ticker.request_riptide(&mut physics, 3, facing.rotation);
    let mut written = None;
    gameplay::movement::flush_player_auth_inputs_guarded(
        &mut ticker,
        1,
        Some(evidence()),
        |identity, packet, guard, _| {
            written = Some(identity);
            network.send_physics_packet(identity, packet, guard)
        },
    )
    .unwrap();
    if facing_written {
        assert!(ticker.acknowledge_physics_send(written.unwrap()));
    } else {
        ticker.resolve_cancelled_physics_send(written.unwrap(), true);
    }
    captured.drain();
    ticker.send_held_release(|_| Err(gameplay::BatchSendError::Full));
    for _ in 0..predicted_ticks {
        let frame = physics.advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        );
        ticker
            .enqueue_completed_physics(frame.samples[0].clone())
            .unwrap();
    }
    assert!(
        captured.drain().is_empty(),
        "no launch input overtakes its release"
    );
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        sample.position,
        if mode == PhysicsCorrectionMode::Snap {
            sample.tick
        } else {
            0
        },
        false,
        mode,
        &RiptideWorld,
    )
    .unwrap();
    assert_eq!(ticker.has_held_release(), facing_written);
    if facing_written {
        ticker.send_held_release(|packets| network.send_inventory_packets(packets));
    } else {
        ticker.send_held_release(|_| panic!("an unconfirmed release must be discarded"));
    }
    assert_eq!(captured.drain().len(), usize::from(facing_written));
    if snap_after_admission {
        let tick = physics.state().unwrap().tick;
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            sample.position,
            tick,
            false,
            PhysicsCorrectionMode::Snap,
            &RiptideWorld,
        )
        .unwrap();
    }
    physics.advance(
        std::time::Duration::ZERO,
        sim::MovementInput::default(),
        &RiptideWorld,
    );
    let frame = physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &RiptideWorld,
    );
    assert_eq!(
        frame.samples[0].processed.spin_started, facing_written,
        "a written release must retain its launch across a snap"
    );
    ticker
        .enqueue_completed_physics(frame.samples[0].clone())
        .unwrap();
    let mut flags = Vec::new();
    gameplay::movement::flush_player_auth_inputs_guarded(
        &mut ticker,
        1,
        Some(evidence()),
        |identity, packet, guard, _| {
            flags.push(
                protocol::player_auth_input_trace_sample(&packet)
                    .unwrap()
                    .flag_names,
            );
            network.send_physics_packet(identity, packet, guard)
        },
    )
    .unwrap();
    assert_eq!(flags[0].contains(&"StartSpinAttack"), facing_written);
    ticker.send_held_release(|_| panic!("the release must not be duplicated"));
    let next = physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &RiptideWorld,
    );
    assert!(
        !next.samples[0].processed.spin_started,
        "a snap must not duplicate the launch"
    );
}

/// Admits a release and predicts its launch before correcting the unsent movement input.
fn admitted_riptide_after_prediction(mode: PhysicsCorrectionMode, written: bool) {
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let origin = [0.0, 65.620_01, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let facing = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        )
        .samples
        .remove(0);
    ticker.enqueue_completed_physics(facing.clone()).unwrap();
    let selection = use_frame(facing.tick).selection.unwrap();
    let release = protocol::release_item_packet(protocol::HeldItemRequest {
        hand: protocol::ItemUseHand::Mainhand,
        selected_slot: selection.slot,
        selected_item: selection.item,
        player_position: facing.position,
    })
    .unwrap();
    ticker.hold_release_after_tick(facing.tick, vec![release]);
    ticker.request_riptide(&mut physics, 3, [-30.0, 90.0]);
    super::assisted::flush(&mut ticker, &network);
    captured.drain();
    ticker.send_held_release(|packets| network.send_inventory_packets(packets));
    assert!(!ticker.has_held_release());
    assert_eq!(captured.drain().len(), 1);
    let launch = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        )
        .samples
        .remove(0);
    assert!(launch.processed.spin_started);
    ticker.enqueue_completed_physics(launch).unwrap();
    if written {
        super::assisted::flush(&mut ticker, &network);
        assert_eq!(captured.drain().len(), 1);
    }
    for _ in 0..2 {
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            facing.position,
            if mode == PhysicsCorrectionMode::Snap {
                facing.tick
            } else {
                0
            },
            false,
            mode,
            &RiptideWorld,
        )
        .unwrap();
    }
    physics.advance(
        std::time::Duration::ZERO,
        sim::MovementInput::default(),
        &RiptideWorld,
    );
    let next = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        )
        .samples
        .remove(0);
    assert_eq!(
        next.processed.spin_started, !written,
        "an admitted but unwritten launch must survive correction"
    );
    if !written {
        assert!(
            next.movement[0] < -2.5,
            "the recovered launch keeps its captured facing"
        );
    }
    ticker.enqueue_completed_physics(next).unwrap();
    super::assisted::flush(&mut ticker, &network);
    let packets = captured.drain();
    assert_eq!(packets.len(), 1);
    assert_eq!(
        protocol::player_auth_input_trace_sample(&packets[0])
            .unwrap()
            .flag_names
            .contains(&"StartSpinAttack"),
        !written
    );
    ticker.send_held_release(|_| panic!("admitted release must not return"));
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        facing.position,
        facing.tick,
        false,
        PhysicsCorrectionMode::Snap,
        &RiptideWorld,
    )
    .unwrap();
    physics.advance(
        std::time::Duration::ZERO,
        sim::MovementInput::default(),
        &RiptideWorld,
    );
    let next = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        )
        .samples
        .remove(0);
    assert!(
        !next.processed.spin_started,
        "a written launch must never restart"
    );
}

#[test]
fn admitted_riptide_survives_snap_after_prediction_until_written() {
    for written in [false, true] {
        admitted_riptide_after_prediction(PhysicsCorrectionMode::Snap, written);
    }
}

#[test]
fn admitted_riptide_survives_replay_fallback_after_prediction_until_written() {
    for written in [false, true] {
        admitted_riptide_after_prediction(PhysicsCorrectionMode::ReplayIfRetained, written);
    }
}
