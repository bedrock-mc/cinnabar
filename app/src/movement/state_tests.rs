//! Exact per-mode wire-flag witnesses for processed movement state (VPA-011).
//!
//! Flag identity and wire order are pinned by Mojang's published protocol
//! documentation and the vendored protocol-2168 packet definitions. The exact
//! vanilla lifecycle of each flag has not been measured against a
//! version-matched native client, so the sequences below pin Cinnabar's
//! provisional processed-movement contract: raw button families follow the
//! physical button, while the processed `Jumping`, sneaking, and sprinting
//! families describe what the simulation acted on. A future native measurement
//! replaces these rules deliberately, never silently.

use std::time::Duration;

use protocol::PlayerInputFlags;
use sim::MovementInput;

use super::integration_tests::VersionedFloor;
use super::settle_tests::settled_sample;
use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionMode,
    PhysicsMovementSample, ProcessedMovementState, physics_movement_input,
    reconcile_candidate_physics_correction,
};

const TICK: Duration = Duration::from_millis(50);

/// Raw jump-button carriers that mean "the physical button is down" plus the
/// discrete press announcement. They track held input, never the arc.
fn raw_held_jump_mask() -> u64 {
    (PlayerInputFlags::JUMP_DOWN
        | PlayerInputFlags::JUMP_CURRENT_RAW
        | PlayerInputFlags::START_JUMPING
        | PlayerInputFlags::JUMP_PRESSED_RAW)
        .bits()
}

struct Harness {
    physics: LocalPhysicsController,
    ticker: MovementTicker,
}

fn flag_harness() -> Harness {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    // Flag-sequence assertions are orthogonal to the provisional spawn-settle
    // window; dedicated gate coverage lives in `settle_tests`.
    ticker.testing_lift_spawn_settle_gate();
    Harness { physics, ticker }
}

/// Advances exactly one fixed tick, admits it for transmission, and returns
/// the wire-visible flag set alongside the completed simulator sample. The
/// admitted admission stays queued for correction witnesses that reconcile a
/// retained range.
fn step_retained(harness: &mut Harness, input: MovementInput) -> (u64, PhysicsMovementSample) {
    let frame = harness.physics.advance(TICK, input, &VersionedFloor(1));
    assert!(
        frame.blocked.is_none(),
        "unexpected blocked tick: {:?}",
        frame.blocked
    );
    assert_eq!(frame.samples.len(), 1, "50 ms advances exactly one tick");
    let sample = frame
        .samples
        .into_iter()
        .next()
        .expect("one completed tick");
    harness
        .ticker
        .enqueue_completed_physics(sample.clone())
        .expect("completed tick is admissible");
    let snapshot = harness
        .ticker
        .pending_snapshots()
        .pop()
        .expect("queued admission");
    (snapshot.flags.bits(), sample)
}

/// [`step_retained`] that consumes the admission afterwards so bounded
/// outboxes can absorb the long sequences these witnesses drive.
fn step(harness: &mut Harness, input: MovementInput) -> (u64, PhysicsMovementSample) {
    let witnessed = step_retained(harness, input);
    harness.ticker.pop_pending().expect("queued admission");
    witnessed
}

fn jump_input(jumping: bool) -> MovementInput {
    MovementInput {
        jumping,
        ..MovementInput::default()
    }
}

fn sneak_input(sneaking: bool) -> MovementInput {
    MovementInput {
        sneaking,
        ..MovementInput::default()
    }
}

#[test]
fn jump_arc_opens_on_a_ground_takeoff_and_closes_on_landing() {
    let takeoff = ProcessedMovementState::next(false, true, false, false, false);
    assert!(takeoff.jump_initiated);
    assert!(takeoff.jump_arc_active);

    let airborne = ProcessedMovementState::next(true, false, false, false, false);
    assert!(!airborne.jump_initiated);
    assert!(
        airborne.jump_arc_active,
        "the arc rides the airborne window"
    );

    let landing = ProcessedMovementState::next(true, false, true, false, false);
    assert!(!landing.jump_arc_active, "ground contact closes the arc");

    let settled = ProcessedMovementState::next(false, false, true, false, false);
    assert!(!settled.jump_arc_active);
}

#[test]
fn requests_that_cannot_take_off_never_claim_a_jump_arc() {
    // An input edge consumed in mid-air initiates nothing: the simulator can
    // only act on a jump request from the ground.
    let air_tap = ProcessedMovementState::next(false, false, false, false, false);
    assert!(!air_tap.jump_arc_active);
    let free_fall = ProcessedMovementState::next(false, false, false, false, false);
    assert!(!free_fall.jump_arc_active);

    // A wall-blocked attempt may consume its initiation tick, but the next
    // grounded report closes the arc again instead of sticking open.
    let blocked_attempt = ProcessedMovementState::next(false, true, true, false, false);
    assert!(blocked_attempt.jump_arc_active);
    let settled = ProcessedMovementState::next(true, false, true, false, false);
    assert!(!settled.jump_arc_active);
}

#[test]
fn processed_arc_drives_wire_jumping_even_when_the_button_is_released() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);

    let mut takeoff = settled_sample(41, [0.0, 64.620_01, 0.0]);
    takeoff.jumping = true;
    takeoff.processed = ProcessedMovementState {
        jump_initiated: true,
        jump_arc_active: true,
        sneaking: false,
        sprinting: false,
    };
    let mut released = settled_sample(42, [0.0, 64.9, 0.0]);
    // The button is up but the simulated arc is still in progress.
    released.processed.jump_arc_active = true;

    ticker.enqueue_completed_physics(takeoff).unwrap();
    ticker.enqueue_completed_physics(released).unwrap();

    let takeoff_snapshot = ticker.pop_pending().expect("takeoff queued").snapshot;
    for mask in [
        PlayerInputFlags::JUMP_DOWN,
        PlayerInputFlags::JUMP_CURRENT_RAW,
        PlayerInputFlags::START_JUMPING,
        PlayerInputFlags::JUMP_PRESSED_RAW,
        PlayerInputFlags::JUMPING,
    ] {
        assert_ne!(
            takeoff_snapshot.flags.bits() & mask.bits(),
            0,
            "takeoff must carry {mask:?}"
        );
    }

    let released_snapshot = ticker.pop_pending().expect("release queued").snapshot;
    assert_ne!(
        released_snapshot.flags.bits() & PlayerInputFlags::JUMPING.bits(),
        0,
        "the processed arc keeps Jumping asserted after the button releases"
    );
    assert_ne!(
        released_snapshot.flags.bits() & PlayerInputFlags::JUMP_RELEASED_RAW.bits(),
        0,
        "the raw release edge is reported exactly once"
    );
    assert_eq!(
        released_snapshot.flags.bits()
            & (PlayerInputFlags::JUMP_DOWN | PlayerInputFlags::JUMP_CURRENT_RAW).bits(),
        0,
        "raw held-button carriers drop when the button releases"
    );
    assert_eq!(
        released_snapshot.flags.bits() & raw_held_jump_mask(),
        0,
        "no fresh start edges exist after release"
    );
}

#[test]
fn tap_jump_flag_sequence_tracks_the_processed_arc_until_landing() {
    let mut harness = flag_harness();

    let (takeoff_flags, takeoff) = step(&mut harness, jump_input(true));
    assert!(
        takeoff.grounded_before_tick && !takeoff.grounded_after_tick,
        "the witnessed tick is a real takeoff"
    );
    for mask in [
        PlayerInputFlags::JUMP_DOWN,
        PlayerInputFlags::JUMP_CURRENT_RAW,
        PlayerInputFlags::START_JUMPING,
        PlayerInputFlags::JUMP_PRESSED_RAW,
        PlayerInputFlags::JUMPING,
    ] {
        assert_ne!(takeoff_flags & mask.bits(), 0);
    }

    let mut airborne_after_release = 0;
    let mut landing_seen = false;
    for _ in 0..40 {
        let (flags, sample) = step(&mut harness, jump_input(false));
        let jumping = flags & PlayerInputFlags::JUMPING.bits();
        let raw_held = flags & raw_held_jump_mask();
        if !landing_seen {
            if flags & PlayerInputFlags::JUMP_RELEASED_RAW.bits() != 0 {
                // The release tick itself: raw family drops, the arc stays.
                assert_ne!(jumping, 0, "the release tick is still inside the arc");
                assert_eq!(raw_held, 0);
            } else if !sample.grounded_after_tick {
                assert_ne!(jumping, 0, "airborne ticks keep claiming the processed arc");
                assert_eq!(raw_held, 0, "released buttons carry no raw held flags");
                airborne_after_release += 1;
            } else {
                // First grounded report after the arc: the window closes.
                assert_eq!(jumping, 0, "the landing tick reports ground contact");
                assert_eq!(raw_held, 0);
                landing_seen = true;
            }
        } else {
            // Settled walking after landing claims nothing jump-related.
            assert_eq!(jumping, 0);
            assert_eq!(raw_held, 0);
        }
    }
    assert!(landing_seen, "a tap jump must land within 40 ticks");
    assert!(
        airborne_after_release >= 2,
        "the witness requires a real multi-tick arc, got {airborne_after_release}"
    );
}

#[test]
fn held_jump_claims_jumping_only_while_an_arc_is_in_progress() {
    let mut harness = flag_harness();

    let mut takeoffs = 0;
    let mut landing_ticks = 0;
    for _ in 0..75 {
        let (flags, sample) = step(&mut harness, jump_input(true));
        let jumping = flags & PlayerInputFlags::JUMPING.bits();
        if sample.processed.jump_initiated {
            takeoffs += 1;
            assert_ne!(jumping, 0, "an initiation always claims Jumping");
        } else if !sample.grounded_after_tick {
            assert_ne!(jumping, 0, "airborne continuations claim Jumping");
        } else {
            // Grounded reports close or keep the arc closed: both the landing
            // tick of each hop and any grounded pause between hops stay silent
            // on the processed carrier. Continuous holding re-initiates as
            // soon as the simulator's own jump-delay gate reopens, so pauses
            // are rare here; the closed-on-ground property itself is pinned at
            // the fold level by `jump_arc_opens_on_a_ground_takeoff_and_closes_on_landing`.
            assert_eq!(jumping, 0, "grounded non-initiation ticks claim no Jumping");
            if !sample.grounded_before_tick {
                landing_ticks += 1;
            }
        }
    }
    assert!(takeoffs >= 2, "holding jump repeats hops");
    assert!(
        landing_ticks >= 2,
        "each repeated hop must land before repeating"
    );
}

#[test]
fn sprint_flags_keep_the_forward_gated_sequence_byte_identical() {
    let mut harness = flag_harness();
    let sprint_forward = physics_movement_input([0.0, 1.0], 180.0, true, false, false, true, false);
    let walk_forward = physics_movement_input([0.0, 1.0], 180.0, true, false, false, false, false);

    let (first, _) = step(&mut harness, sprint_forward);
    for mask in [
        PlayerInputFlags::START_SPRINTING,
        PlayerInputFlags::SPRINT_DOWN,
        PlayerInputFlags::SPRINTING,
    ] {
        assert_ne!(first & mask.bits(), 0);
    }
    for _ in 0..2 {
        let (held, _) = step(&mut harness, sprint_forward);
        assert_ne!(held & PlayerInputFlags::SPRINTING.bits(), 0);
        assert_ne!(held & PlayerInputFlags::SPRINT_DOWN.bits(), 0);
        assert_eq!(
            held & (PlayerInputFlags::START_SPRINTING | PlayerInputFlags::STOP_SPRINTING).bits(),
            0,
            "continuous sprint emits no repeat edges"
        );
    }

    let (stop, _) = step(&mut harness, walk_forward);
    assert_ne!(stop & PlayerInputFlags::STOP_SPRINTING.bits(), 0);
    assert_eq!(stop & PlayerInputFlags::SPRINTING.bits(), 0);
    assert_eq!(stop & PlayerInputFlags::SPRINT_DOWN.bits(), 0);
    for _ in 0..2 {
        let (walked, _) = step(&mut harness, walk_forward);
        assert_eq!(
            walked
                & (PlayerInputFlags::SPRINTING
                    | PlayerInputFlags::SPRINT_DOWN
                    | PlayerInputFlags::START_SPRINTING
                    | PlayerInputFlags::STOP_SPRINTING)
                    .bits(),
            0,
            "walking after stopping claims no sprint family"
        );
    }

    // The forward gate is untouched by processed-state routing: backward-held
    // sprint requests still never produce sprint flags.
    let mut backward_harness = flag_harness();
    let backward_sprint =
        physics_movement_input([0.0, -1.0], 180.0, true, false, false, true, false);
    for _ in 0..2 {
        let (flags, _) = step(&mut backward_harness, backward_sprint);
        assert_eq!(
            flags
                & (PlayerInputFlags::SPRINTING
                    | PlayerInputFlags::SPRINT_DOWN
                    | PlayerInputFlags::START_SPRINTING)
                    .bits(),
            0,
            "backward input cannot sprint"
        );
    }
}

#[test]
fn sneak_flags_track_the_simulator_state_pending_pose_authority() {
    // No shared pose/mode authority exists yet (VPA-012), so processed sneak
    // equals held sneak and these bytes are unchanged. Any future pose-gated
    // rule must replace this witness deliberately.
    let mut harness = flag_harness();

    let (first, _) = step(&mut harness, sneak_input(true));
    for mask in [
        PlayerInputFlags::START_SNEAKING,
        PlayerInputFlags::SNEAK_DOWN,
        PlayerInputFlags::SNEAKING,
        PlayerInputFlags::SNEAK_PRESSED_RAW,
    ] {
        assert_ne!(first & mask.bits(), 0);
    }
    for _ in 0..2 {
        let (held, _) = step(&mut harness, sneak_input(true));
        assert_ne!(held & PlayerInputFlags::SNEAKING.bits(), 0);
        assert_ne!(held & PlayerInputFlags::SNEAK_DOWN.bits(), 0);
        assert_eq!(
            held & (PlayerInputFlags::START_SNEAKING | PlayerInputFlags::STOP_SNEAKING).bits(),
            0,
            "continuous sneak emits no repeat edges"
        );
    }

    let (stop, _) = step(&mut harness, sneak_input(false));
    assert_ne!(stop & PlayerInputFlags::STOP_SNEAKING.bits(), 0);
    assert_ne!(stop & PlayerInputFlags::SNEAK_RELEASED_RAW.bits(), 0);
    assert_eq!(stop & PlayerInputFlags::SNEAKING.bits(), 0);
    let (settled, _) = step(&mut harness, sneak_input(false));
    assert_eq!(
        settled
            & (PlayerInputFlags::SNEAKING
                | PlayerInputFlags::SNEAK_DOWN
                | PlayerInputFlags::START_SNEAKING
                | PlayerInputFlags::STOP_SNEAKING)
                .bits(),
        0,
    );
}

#[test]
fn session_resets_clear_an_open_jump_window_and_rearm_fresh_edges() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);

    let pressed = physics.advance(TICK, jump_input(true), &VersionedFloor(1));
    assert_eq!(pressed.samples.len(), 1);
    assert!(pressed.samples[0].processed.jump_arc_active);

    // A hard reset landing inside the airborne window clears the carried arc:
    // the next neutral tick reports no jump in progress.
    physics.reanchor_network_position([5.0, 65.620_01, 9.0], 200, true);
    let resumed = physics.advance(TICK, MovementInput::default(), &VersionedFloor(1));
    assert_eq!(resumed.samples.len(), 1);
    assert!(!resumed.samples[0].processed.jump_arc_active);

    // Holding jump across the reset reads as a fresh press on solid ground,
    // so a new arc legitimately opens instead of being silently swallowed.
    physics.reanchor_network_position([5.0, 65.620_01, 9.0], 300, true);
    let held_again = physics.advance(TICK, jump_input(true), &VersionedFloor(1));
    assert_eq!(held_again.samples.len(), 1);
    assert!(held_again.samples[0].processed.jump_initiated);
    assert!(held_again.samples[0].processed.jump_arc_active);
}

#[test]
fn exact_mid_air_correction_keeps_every_replayed_flag_byte_identical() {
    let mut harness = flag_harness();
    let (_, _) = step_retained(&mut harness, MovementInput::default());
    let (_, takeoff) = step_retained(&mut harness, jump_input(true));
    let mut airborne = vec![takeoff];
    for _ in 0..3 {
        let (_, sample) = step_retained(&mut harness, jump_input(false));
        assert!(!sample.grounded_after_tick, "witnessed ticks stay airborne");
        airborne.push(sample);
    }
    // Corrections drop queued work up to the corrected tick, so the stable
    // comparison range starts right after the anchor.
    let anchor_tick = airborne[0].tick;
    let expected: Vec<_> = harness
        .ticker
        .pending_snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.tick > anchor_tick)
        .collect();

    // The server confirms the first airborne tick exactly where the client
    // predicted it and still airborne, so the replay reproduces the identical
    // trajectory and every replayed snapshot keeps its exact flag set.
    let anchor = &airborne[0];
    reconcile_candidate_physics_correction(
        &mut harness.ticker,
        &mut harness.physics,
        anchor.position,
        anchor.tick,
        false,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .expect("exact airborne correction replays");

    let replayed: Vec<_> = harness.ticker.pending_snapshots();
    assert_eq!(replayed.len(), expected.len());
    for (expected_snapshot, replayed_snapshot) in expected.iter().zip(replayed.iter()) {
        assert_eq!(expected_snapshot.tick, replayed_snapshot.tick);
        assert_eq!(
            expected_snapshot.flags, replayed_snapshot.flags,
            "tick {} must keep byte-identical flags across an exact replay",
            expected_snapshot.tick
        );
    }
}

#[test]
fn early_landing_correction_closes_the_replayed_jump_arc() {
    let mut harness = flag_harness();
    let (_, _) = step_retained(&mut harness, MovementInput::default());
    let (_, takeoff) = step_retained(&mut harness, jump_input(true));
    let mut airborne = vec![takeoff];
    for _ in 0..3 {
        let (_, sample) = step_retained(&mut harness, jump_input(false));
        assert!(!sample.grounded_after_tick);
        airborne.push(sample);
    }

    // The server reports the player already landed at the second airborne
    // tick. The replayed remainder starts from a grounded anchor with no
    // initiation there, so the following snapshot must drop Jumping instead
    // of carrying an arc the server says never happened.
    let anchor = &airborne[1];
    reconcile_candidate_physics_correction(
        &mut harness.ticker,
        &mut harness.physics,
        anchor.position,
        anchor.tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .expect("early-landing correction replays");

    let replayed = harness.ticker.pending_snapshots();
    let after_anchor = replayed
        .iter()
        .find(|snapshot| snapshot.tick > anchor.tick)
        .expect("replayed remainder exists");
    assert_eq!(
        after_anchor.flags.bits() & PlayerInputFlags::JUMPING.bits(),
        0,
        "a server-reported landing closes the arc for replayed ticks"
    );
}
