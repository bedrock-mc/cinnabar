//! Regression coverage for the provisional post-spawn transmission-settle
//! gate.
//!
//! Live third-party evidence (2026-08-22) showed colliding lobby spawns
//! producing sustained inputless horizontal displacement that server
//! anti-cheats reject or silently drop. After each spawn anchor the gate now
//! withholds the outbound `PlayerAuthInput` hand-off until prediction
//! reports a bounded run of stable grounded samples, failing open on a cap
//! so a permanently weird spawn cannot silence the stream forever. The
//! constant values are explicitly provisional pending version-matched native
//! Bedrock measurement (VPA-109 family); these witnesses pin the contract,
//! not a vanilla parity claim.

use std::time::Duration;

use super::integration_tests::{VersionedFloor, evidence_context, forward_physics_input};
use super::{
    LocalPhysicsController, MovementOutboxReconciliation, MovementSource, MovementTicker,
    OUTBOX_CAPACITY, PhysicsCorrectionMode, PhysicsCorrectionOutcome, PhysicsMovementSample,
    PhysicsSampleContext, flush_player_auth_inputs, reconcile_candidate_physics_correction,
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
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: true,
        grounded_after_tick: true,
        horizontal_collision: false,
        vertical_collision: false,
        jump_repeated: false,
        world_identity: fixture_world_identity(),
    }
}

/// The observed colliding-spawn pathology: no ground contact plus a retained
/// horizontal collision while gravity is the only motion.
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
fn colliding_spawn_samples_withhold_the_transport_hand_off() {
    let mut ticker = physics_ticker(7, 40);

    for tick in 41..=43 {
        ticker
            .enqueue_completed_physics(colliding_sample(tick, [0.5, 69.9, 0.25]))
            .unwrap();
    }
    assert_eq!(
        ticker.next_tick(),
        44,
        "admission keeps tick scheduling contiguous"
    );
    assert_eq!(recorded_sends(&mut ticker, 8), Vec::<u64>::new());
    assert_eq!(
        ticker.pending_count(),
        0,
        "withheld samples are never queued for replay"
    );
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained,
    );

    for tick in 44..=46 {
        ticker
            .enqueue_completed_physics(colliding_sample(tick, [1.1, 69.8, 0.5]))
            .unwrap();
    }
    assert_eq!(recorded_sends(&mut ticker, 8), Vec::<u64>::new());
    assert_eq!(ticker.sent_physics_packet_count(), 0);
}

#[test]
fn settled_samples_lift_the_gate_without_replaying_suppressed_ticks() {
    let mut ticker = physics_ticker(7, 40);

    ticker
        .enqueue_completed_physics(colliding_sample(41, [0.0, 69.9, 0.0]))
        .unwrap();
    ticker
        .enqueue_completed_physics(colliding_sample(42, [0.1, 69.85, 0.0]))
        .unwrap();
    assert_eq!(recorded_sends(&mut ticker, 8), Vec::<u64>::new());

    // Nineteen consecutive settled samples leave the window one short.
    for tick in 43..=61 {
        ticker
            .enqueue_completed_physics(settled_sample(tick, [0.2, 70.0, 0.1]))
            .unwrap();
    }
    assert_eq!(recorded_sends(&mut ticker, 32), Vec::<u64>::new());

    // The twentieth consecutive settled sample lifts the gate and is the
    // first transmitted tick: numbering continues with no gaps and none of
    // the suppressed ticks are replayed.
    ticker
        .enqueue_completed_physics(settled_sample(62, [0.3, 70.0, 0.2]))
        .unwrap();
    assert_eq!(ticker.next_tick(), 63);
    assert_eq!(acknowledged_sends(&mut ticker, 32), vec![62]);
    assert_eq!(ticker.pending_count(), 0);

    // Transmission continues normally after the lift.
    ticker
        .enqueue_completed_physics(settled_sample(63, [0.4, 70.0, 0.3]))
        .unwrap();
    ticker
        .enqueue_completed_physics(settled_sample(64, [0.5, 70.0, 0.4]))
        .unwrap();
    assert_eq!(recorded_sends(&mut ticker, 8), vec![63, 64]);
}

#[test]
fn the_suppression_cap_fails_open_and_resumes_transmission() {
    let mut ticker = physics_ticker(7, 40);

    // Mimic the production cadence: at most eight admissions per frame and a
    // bounded flush budget that drains the withheld queue every frame.
    let cap_tick = 40 + super::settle::SETTLE_TIMEOUT_TICKS;
    for tick in 41..cap_tick {
        ticker
            .enqueue_completed_physics(colliding_sample(tick, [0.25, 69.9, 0.0]))
            .unwrap();
        if tick % 8 == 0 {
            assert_eq!(recorded_sends(&mut ticker, 16), Vec::<u64>::new());
        }
    }
    assert!(ticker.pending_count() < OUTBOX_CAPACITY);

    // The cap's final colliding admission fails open and is the first
    // transmitted tick even though it is still unstable.
    ticker
        .enqueue_completed_physics(colliding_sample(cap_tick, [9.0, 69.9, 0.0]))
        .unwrap();
    assert_eq!(
        recorded_sends(&mut ticker, 16),
        vec![cap_tick],
        "the timeout must fail open instead of silencing the stream"
    );

    ticker
        .enqueue_completed_physics(colliding_sample(cap_tick + 1, [9.5, 69.9, 0.0]))
        .unwrap();
    assert_eq!(
        recorded_sends(&mut ticker, 16),
        vec![cap_tick + 1],
        "after the cap the episode stays lifted until the next anchor"
    );
}

#[test]
fn session_reset_re_engages_the_settle_gate() {
    let mut ticker = physics_ticker(7, 40);

    for tick in 41..=59 {
        ticker
            .enqueue_completed_physics(settled_sample(tick, [0.1, 70.0, 0.0]))
            .unwrap();
    }
    ticker
        .enqueue_completed_physics(settled_sample(60, [0.2, 70.0, 0.0]))
        .unwrap();
    assert_eq!(recorded_sends(&mut ticker, 32), vec![60]);
    ticker
        .enqueue_completed_physics(settled_sample(61, [0.3, 70.0, 0.0]))
        .unwrap();
    assert_eq!(recorded_sends(&mut ticker, 8), vec![61]);

    // A replacement StartGame anchors a fresh episode: previously settled
    // history must not carry across the session boundary.
    ticker.reset(9, 500, [8.0, 71.0, 9.0]);
    for tick in 501..=503 {
        ticker
            .enqueue_completed_physics(colliding_sample(tick, [8.5, 70.9, 9.0]))
            .unwrap();
    }
    assert_eq!(recorded_sends(&mut ticker, 8), Vec::<u64>::new());

    // And a fresh full window of clean samples settles the new episode.
    for tick in 504..=522 {
        ticker
            .enqueue_completed_physics(settled_sample(tick, [8.6, 71.0, 9.0]))
            .unwrap();
    }
    ticker
        .enqueue_completed_physics(settled_sample(523, [8.7, 71.0, 9.0]))
        .unwrap();
    assert_eq!(recorded_sends(&mut ticker, 32), vec![523]);
}

#[test]
fn a_clean_stream_from_the_first_admission_only_waits_the_settle_window() {
    let mut ticker = physics_ticker(7, 40);

    for tick in 41..=59 {
        ticker
            .enqueue_completed_physics(settled_sample(tick, [0.0, 70.0, 0.0]))
            .unwrap();
    }
    assert_eq!(recorded_sends(&mut ticker, 8), Vec::<u64>::new());

    ticker
        .enqueue_completed_physics(settled_sample(60, [0.1, 70.0, 0.0]))
        .unwrap();
    assert_eq!(acknowledged_sends(&mut ticker, 8), vec![60]);
    assert_eq!(ticker.sent_physics_packet_count(), 1);

    // Beyond the window the stream behaves exactly like the ordinary path:
    // immediate transmission, contiguous numbering, working acknowledgement.
    ticker
        .enqueue_completed_physics(settled_sample(61, [0.2, 70.0, 0.0]))
        .unwrap();
    let mut identities = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |identity, _packet| {
            identities.push(identity);
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    assert_eq!(
        identities.iter().map(|i| i.tick).collect::<Vec<_>>(),
        vec![61]
    );
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );
    // Both post-window transmissions published their immutable admission
    // evidence, exactly as the ordinary path does.
    assert_eq!(
        ticker
            .take_tick_evidence()
            .iter()
            .map(|sample| sample.tick)
            .collect::<Vec<_>>(),
        vec![60, 61]
    );
}

#[test]
fn free_camera_authority_is_never_blocked_by_the_settle_gate() {
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
fn a_correction_replay_does_not_restart_the_settle_window() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    // Two frames because one render frame completes at most
    // MAX_LOCAL_PHYSICS_TICKS_PER_FRAME fixed ticks.
    let mut samples = physics
        .advance_with_context(
            Duration::from_millis(400),
            forward_physics_input(),
            PhysicsSampleContext::default(),
            &VersionedFloor(1),
        )
        .samples;
    assert_eq!(samples.len(), 8);
    samples.extend(
        physics
            .advance_with_context(
                Duration::from_millis(150),
                forward_physics_input(),
                PhysicsSampleContext::default(),
                &VersionedFloor(1),
            )
            .samples,
    );
    assert_eq!(samples.len(), 11);

    let mut ticker = physics_ticker(7, 100);
    for sample in samples.clone() {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    assert_eq!(recorded_sends(&mut ticker, 16), Vec::<u64>::new());

    // A zero-rewind correction of the newest retained tick exercises the
    // Replay arm without touching the settle state.
    let corrected_position = samples.last().unwrap().position;
    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            corrected_position,
            111,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Ok(PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 111,
            replayed_ticks: 0,
        })
    );

    // Nine further settled samples reach only nineteen consecutive: if the
    // replay had restarted the window, this batch could not lift the gate.
    for tick in 112..=120 {
        let mut next = physics
            .advance(
                Duration::from_millis(50),
                forward_physics_input(),
                &VersionedFloor(1),
            )
            .samples;
        assert_eq!(next.len(), 1);
        let sample = next.pop().expect("one completed physics tick");
        assert_eq!(sample.tick, tick);
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    assert_eq!(
        recorded_sends(&mut ticker, 16),
        vec![120],
        "the correction replay must preserve settle progress"
    );
}

#[test]
fn a_correction_snap_re_engages_the_settle_window() {
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
    assert_eq!(recorded_sends(&mut ticker, 16), Vec::<u64>::new());

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [8.0, 71.620_01, 9.0],
            0,
            false,
            PhysicsCorrectionMode::Snap,
            &VersionedFloor(1),
        ),
        Ok(PhysicsCorrectionOutcome::Snapped { tick: 105 })
    );

    // The snap starts a completely fresh window: nineteen further settled
    // samples stay withheld and only the twentieth transmits.
    let resume_tick = ticker.next_tick();
    for offset in 0..19 {
        ticker
            .enqueue_completed_physics(settled_sample(resume_tick + offset, [8.1, 71.620_01, 9.0]))
            .unwrap();
    }
    assert_eq!(recorded_sends(&mut ticker, 32), Vec::<u64>::new());
    ticker
        .enqueue_completed_physics(settled_sample(resume_tick + 19, [8.2, 71.620_01, 9.0]))
        .unwrap();
    assert_eq!(recorded_sends(&mut ticker, 32), vec![resume_tick + 19]);
}
