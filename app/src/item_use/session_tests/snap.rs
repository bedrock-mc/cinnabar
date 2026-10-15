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
        selected_slot: selection.slot,
        selected_item: selection.item,
        player_position: sample.position,
    })
    .unwrap();
    ticker.hold_release_after_tick(sample.tick, vec![release]);
    super::super::riptide::request_launch(
        3,
        &mut physics,
        &mut ticker,
        &client_presentation::local_player::LocalViewPose::default(),
    );
    let mut written = None;
    gameplay::movement::flush_player_auth_inputs_guarded(
        &mut ticker,
        1,
        Some(evidence()),
        |identity, packet, guard| {
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
        |identity, packet, guard| {
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
