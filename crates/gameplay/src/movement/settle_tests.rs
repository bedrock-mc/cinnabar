//! Post-spawn transmission contract: `PlayerAuthInput` is sent every tick.
//!
//! Vanilla sends one `PlayerAuthInput` per tick from the first tick, including
//! across StartGame, teleports, and corrections (gophertunnel
//! `player_auth_input.go`: "the client will send this packet once every tick").
//! The former spawn-settle window withheld the hand-off for up to 200 ticks,
//! which anti-cheat servers read as either a movement cheat once drift resumed
//! or an idle timeout. These witnesses pin the replacement contract: every
//! admitted tick transmits with no gap. This module also owns the shared
//! completed-sample fixtures used across the movement tests.

use std::time::Duration;

use super::integration_tests::{VersionedFloor, evidence_context, forward_physics_input};
use super::{
    LocalPhysicsController, MovementOutboxReconciliation, MovementSource, MovementTicker,
    PhysicsCorrectionMode, PhysicsCorrectionOutcome, PhysicsMovementSample, PhysicsSampleContext,
    ProcessedMovementState, flush_player_auth_inputs, reconcile_candidate_physics_correction,
    reconcile_committed_correction,
};
use protocol::PlayerInputMode;
use sim::{CollisionIdSpace, CollisionRegistryIdentity, WorldCollisionIdentity};

fn fixture_world_identity() -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            protocol: 1001,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [1; 32],
        },
        [],
    )
    .unwrap()
}

/// A stable grounded completed tick: resting contact without any horizontal
/// collision.
pub(super) fn settled_sample(tick: u64, position: [f32; 3]) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick,
        position,
        movement: [0.0, -0.078_4, 0.0],
        velocity: [0.0, -0.078_4, 0.0],
        move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        input: Default::default(),
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: true,
        grounded_after_tick: true,
        horizontal_collision: false,
        vertical_collision: false,
        jump_repeated: false,
        processed: ProcessedMovementState::default(),
        world_identity: fixture_world_identity(),
    }
}

/// The former colliding-spawn pathology: no ground contact plus a retained
/// horizontal collision while gravity is the only motion. It must now transmit
/// every tick like any other sample.
pub(super) fn colliding_sample(tick: u64, position: [f32; 3]) -> PhysicsMovementSample {
    PhysicsMovementSample {
        grounded_before_tick: false,
        grounded_after_tick: false,
        horizontal_collision: true,
        vertical_collision: true,
        ..settled_sample(tick, position)
    }
}

fn physics_ticker(session_generation: u64, initial_tick: u64) -> MovementTicker {
    let mut ticker = MovementTicker::default();
    ticker.reset(session_generation, initial_tick, [0.0, 70.0, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
}

/// Sends one flush without acknowledging, returning the transmitted ticks.
fn recorded_sends(ticker: &mut MovementTicker, budget: usize) -> Vec<u64> {
    let mut sent_ticks = Vec::new();
    flush_player_auth_inputs(
        ticker,
        budget,
        Some(evidence_context()),
        |identity, _packet| {
            sent_ticks.push(identity.tick);
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    sent_ticks
}

/// Sends and fully acknowledges one flush, returning the transmitted ticks.
fn acknowledged_sends(ticker: &mut MovementTicker, budget: usize) -> Vec<u64> {
    let mut identities = Vec::new();
    let mut sent_ticks = Vec::new();
    flush_player_auth_inputs(
        ticker,
        budget,
        Some(evidence_context()),
        |identity, _packet| {
            sent_ticks.push(identity.tick);
            identities.push(identity);
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );
    sent_ticks
}

#[test]
fn every_admitted_tick_transmits_from_the_first_after_start_game() {
    let mut ticker = physics_ticker(7, 40);
    for tick in 41..=43 {
        ticker
            .enqueue_completed_physics(settled_sample(tick, [0.1, 70.0, 0.0]))
            .unwrap();
    }
    assert_eq!(
        acknowledged_sends(&mut ticker, 8),
        vec![41, 42, 43],
        "no spawn-settle window may withhold the first ticks after StartGame"
    );
    assert_eq!(ticker.sent_physics_packet_count(), 3);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained
    );
}

#[test]
fn colliding_spawn_samples_still_transmit_every_tick() {
    let mut ticker = physics_ticker(7, 40);
    for tick in 41..=44 {
        ticker
            .enqueue_completed_physics(colliding_sample(tick, [0.5, 69.9, 0.25]))
            .unwrap();
    }
    assert_eq!(
        recorded_sends(&mut ticker, 8),
        vec![41, 42, 43, 44],
        "an odd-but-well-formed colliding sample transmits like any other tick"
    );
}

#[test]
fn free_camera_authority_transmits_no_physics_input() {
    let mut ticker = physics_ticker(7, 40);
    ticker.set_source(MovementSource::FreeCamera);

    let mut sent_packets = 0;
    let flushed = flush_player_auth_inputs(&mut ticker, 8, None, |_identity, _packet| {
        sent_packets += 1;
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(flushed, 0);
    assert_eq!(sent_packets, 0);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::NotAuthoritative
    );
}

#[test]
fn transmission_never_gaps_across_start_game_and_a_teleport_snap() {
    // StartGame: every produced tick transmits immediately.
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = physics_ticker(7, 100);

    let start_game = physics
        .advance_with_context(
            Duration::from_millis(200),
            forward_physics_input(),
            PhysicsSampleContext::default(),
            &VersionedFloor(1),
        )
        .samples;
    let start_ticks: Vec<u64> = start_game.iter().map(|s| s.tick).collect();
    assert!(!start_ticks.is_empty());
    for sample in start_game {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    assert_eq!(
        acknowledged_sends(&mut ticker, 16),
        start_ticks,
        "StartGame transmits every tick with no gap"
    );

    // A teleport snap re-anchors hard. It clears the pre-teleport queue and
    // discards exactly one render frame's pre-anchor elapsed, then resumes
    // transmitting every tick — it must not open a multi-tick silence window.
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [8.0, 71.620_01, 9.0],
        0,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(1),
    )
    .expect("a teleport snap applies");
    let discard = physics.advance(Duration::ZERO, forward_physics_input(), &VersionedFloor(1));
    assert!(
        discard.samples.is_empty(),
        "only the pre-anchor frame is discarded"
    );

    let mut resumed = Vec::new();
    for _ in 0..3 {
        let mut frame = physics
            .advance(
                Duration::from_millis(50),
                forward_physics_input(),
                &VersionedFloor(1),
            )
            .samples;
        assert_eq!(frame.len(), 1);
        let sample = frame.pop().unwrap();
        let tick = sample.tick;
        ticker.enqueue_completed_physics(sample).unwrap();
        resumed.extend(acknowledged_sends(&mut ticker, 16));
        assert_eq!(
            resumed.last().copied(),
            Some(tick),
            "each post-teleport tick transmits the same frame it completes"
        );
    }
    assert_eq!(resumed.len(), 3, "no gap after the teleport");
}

#[test]
fn a_correction_replay_keeps_transmitting_every_tick() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let samples = physics
        .advance_with_context(
            Duration::from_millis(250),
            forward_physics_input(),
            PhysicsSampleContext::default(),
            &VersionedFloor(1),
        )
        .samples;
    assert_eq!(samples.len(), 5);

    let mut ticker = physics_ticker(7, 100);
    for sample in samples.clone() {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    // Nothing is withheld: the whole batch transmits before the correction.
    assert_eq!(
        acknowledged_sends(&mut ticker, 16),
        samples.iter().map(|s| s.tick).collect::<Vec<_>>(),
    );

    let corrected_position = samples.last().unwrap().position;
    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            corrected_position,
            samples.last().unwrap().tick,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Ok(PhysicsCorrectionOutcome::Replayed {
            corrected_tick: samples.last().unwrap().tick,
            replayed_ticks: 0,
        })
    );

    // Further ticks continue transmitting immediately after a replay.
    let mut next = physics
        .advance(
            Duration::from_millis(50),
            forward_physics_input(),
            &VersionedFloor(1),
        )
        .samples;
    let sample = next.pop().expect("one completed physics tick");
    let tick = sample.tick;
    ticker.enqueue_completed_physics(sample).unwrap();
    assert_eq!(recorded_sends(&mut ticker, 8), vec![tick]);
}

#[test]
fn a_confirming_correction_mutates_nothing_and_keeps_the_stream_flowing() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(250),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 5);

    let mut ticker = physics_ticker(7, 100);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    assert_eq!(ticker.pending_count(), 5);

    let position = physics.network_position().unwrap();
    let (tick, on_ground) = {
        let state = physics.state().unwrap();
        (state.tick, state.on_ground)
    };
    assert_eq!(
        reconcile_committed_correction(
            &mut ticker,
            &mut physics,
            position,
            tick,
            on_ground,
            None,
            &VersionedFloor(1),
        ),
        Ok(None),
        "an exactly-agreeing correction confirms without mutating prediction"
    );

    // The confirmation leaves the queued stream intact and transmitting.
    assert_eq!(ticker.pending_count(), 5);
    assert_eq!(recorded_sends(&mut ticker, 16).len(), 5);
}
