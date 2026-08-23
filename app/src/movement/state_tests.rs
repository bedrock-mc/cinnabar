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
use sim::{Aabb, CollisionQuery, CollisionWorld, MovementInput, Vec3, WorldQueryError};

use super::integration_tests::VersionedFloor;
use super::settle_tests::settled_sample;
use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionMode,
    PhysicsMovementSample, ProcessedMovementState, physics_movement_input,
    reconcile_candidate_physics_correction,
};

const TICK: Duration = Duration::from_millis(50);

/// Every raw and processed jump family the outbound encoder can emit.
fn all_jump_flags() -> u64 {
    (PlayerInputFlags::JUMP_DOWN
        | PlayerInputFlags::JUMP_CURRENT_RAW
        | PlayerInputFlags::START_JUMPING
        | PlayerInputFlags::JUMP_PRESSED_RAW
        | PlayerInputFlags::JUMP_RELEASED_RAW
        | PlayerInputFlags::JUMPING)
        .bits()
}

/// Floor plus a ceiling low enough that a held jump bonks and settles back
/// onto the floor while the simulator's ten-tick post-jump cooldown is still
/// running. A full-height hop outlives that window, so this is the plain
/// shape that reaches grounded-with-cooldown.
struct CooldownFloor;

impl CollisionWorld for CooldownFloor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut base = VersionedFloor(1).collision_boxes(query)?;
        let ceiling = Aabb::new(Vec3::new(-64.0, 2.9, -64.0), Vec3::new(64.0, 3.9, 64.0));
        if ceiling.intersects(query) {
            base.value.push(ceiling);
        }
        Ok(base)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        VersionedFloor(1).block_physics(block)
    }
}

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

#[test]
fn a_fresh_edge_inside_the_post_jump_cooldown_initiates_nothing() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    // Flag-sequence assertions are orthogonal to the provisional spawn-settle
    // window; dedicated gate coverage lives in `settle_tests`.
    ticker.testing_lift_spawn_settle_gate();

    // Take off under the low ceiling and hold the button through the bonk
    // until the controller reports ground contact again while the simulator's
    // post-jump cooldown is still running.
    let mut takeoff_seen = false;
    let mut grounded_in_cooldown = false;
    for _ in 0..30 {
        let mut frame = physics.advance(TICK, jump_input(true), &CooldownFloor);
        assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
        assert_eq!(frame.samples.len(), 1, "50 ms advances exactly one tick");
        let sample = frame.samples.pop().expect("one completed tick");
        let grounded_now = sample.grounded_after_tick;
        if !takeoff_seen {
            assert!(
                sample.processed.jump_arc_active,
                "the witnessed hop takes off"
            );
            takeoff_seen = true;
        } else if grounded_now {
            grounded_in_cooldown = true;
        }
        // Keep the admission stream contiguous so the refused tick below
        // enqueues against the ticker's expected sequence.
        ticker.enqueue_completed_physics(sample).unwrap();
        ticker.pop_pending().expect("queued admission");
        if grounded_in_cooldown {
            break;
        }
    }
    assert!(
        takeoff_seen && grounded_in_cooldown,
        "the fixture must land back inside the post-jump cooldown"
    );
    let delay = physics.state().expect("anchored controller").jump_delay;
    assert!(
        delay > 0,
        "the witness requires a live post-jump cooldown, got {delay}"
    );

    // Release and re-press across two zero-due-tick render frames: the edge
    // latch runs on every advance but no simulated tick observes the released
    // button, so the retained cooldown is never cleared before the re-press.
    let empty = physics.advance(Duration::from_millis(10), jump_input(false), &CooldownFloor);
    assert!(
        empty.samples.is_empty(),
        "zero-tick frames complete no ticks"
    );
    let repressed = physics.advance(Duration::from_millis(10), jump_input(true), &CooldownFloor);
    assert!(repressed.samples.is_empty());

    // The next due tick carries that fresh edge into a grounded cooldown
    // tick: the simulator refuses it, so neither the initiation fold nor the
    // wire may claim a jump for the tick.
    let mut refused = physics.advance(TICK, jump_input(true), &CooldownFloor);
    assert_eq!(refused.samples.len(), 1);
    let sample = refused.samples.pop().expect("one completed tick");
    assert!(sample.grounded_before_tick);
    assert!(
        sample.grounded_after_tick,
        "the refused tick stays grounded"
    );
    assert!(
        !sample.processed.jump_initiated,
        "a press edge inside the cooldown initiates nothing"
    );
    assert!(!sample.processed.jump_arc_active);
    ticker.enqueue_completed_physics(sample).unwrap();
    let snapshot = ticker.pending_snapshots().pop().expect("queued admission");
    assert_eq!(
        snapshot.flags.bits() & PlayerInputFlags::JUMPING.bits(),
        0,
        "no JUMPING bit may be asserted for a tick the simulator refused"
    );

    // The gate is not a lockout: once the retained cooldown expires under a
    // held button, the simulator's own repeated-request path initiates again.
    let mut reinitiated = false;
    for _ in 0..15 {
        let frame = physics.advance(TICK, jump_input(true), &CooldownFloor);
        let Some(completed) = frame.samples.first() else {
            continue;
        };
        if completed.processed.jump_initiated {
            reinitiated = true;
            break;
        }
        assert!(!completed.processed.jump_arc_active);
    }
    assert!(
        reinitiated,
        "holding through the expired cooldown must initiate normally"
    );
}

#[test]
fn an_airborne_tap_opens_no_arc_and_gestates_no_later_initiation() {
    let mut harness = flag_harness();
    let (_, takeoff) = step(&mut harness, jump_input(true));
    assert!(takeoff.processed.jump_initiated);
    let (_, released) = step(&mut harness, jump_input(false));
    assert!(
        !released.grounded_after_tick,
        "the witness requires real airtime after takeoff"
    );

    // Tap the button for exactly one airborne tick: the fresh edge lands on a
    // tick the simulator cannot turn into a jump.
    let (tap_flags, tap) = step(&mut harness, jump_input(true));
    assert!(!tap.grounded_before_tick);
    assert!(
        !tap.processed.jump_initiated,
        "an air tap initiates nothing"
    );
    assert_ne!(
        tap_flags & PlayerInputFlags::JUMPING.bits(),
        0,
        "the carried arc from the real takeoff rides through the tap"
    );
    let _ = step(&mut harness, jump_input(false));

    // Fall back to rest: nothing may gestate into a later initiation, and the
    // landing itself keeps every processed jump family silent afterwards.
    let mut landed = false;
    for _ in 0..40 {
        let (flags, sample) = step(&mut harness, jump_input(false));
        assert!(
            !sample.processed.jump_initiated,
            "tick {} gestated a ghost initiation",
            sample.tick
        );
        if sample.grounded_after_tick {
            landed = true;
            assert_eq!(
                flags & PlayerInputFlags::JUMPING.bits(),
                0,
                "landing closes the carried arc with no replacement"
            );
        } else {
            assert!(!landed, "grounded report followed by airborne sample");
        }
    }
    assert!(landed, "the witnessed fall settles within 40 ticks");
}

#[test]
fn plain_falls_never_assert_any_jump_family() {
    let mut physics = LocalPhysicsController::default();
    // Spawn mid-air with the button untouched: gravity-only free fall.
    physics.reanchor_network_position([0.0, 12.620_01, 0.0], 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, [0.0, 12.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.testing_lift_spawn_settle_gate();

    let mut landed_ticks = 0;
    for _ in 0..90 {
        let mut frame = physics.advance(TICK, MovementInput::default(), &VersionedFloor(1));
        assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
        assert_eq!(frame.samples.len(), 1, "50 ms advances exactly one tick");
        let sample = frame.samples.pop().expect("one completed tick");
        assert!(!sample.processed.jump_initiated);
        assert!(
            !sample.processed.jump_arc_active,
            "a plain fall never carries a processed arc"
        );
        let grounded = sample.grounded_after_tick;
        ticker.enqueue_completed_physics(sample).unwrap();
        let snapshot = ticker.pending_snapshots().pop().expect("queued admission");
        ticker.pop_pending().expect("queued admission");
        assert_eq!(
            snapshot.flags.bits() & all_jump_flags(),
            0,
            "tick {} asserted a jump family during a buttonless fall",
            snapshot.tick
        );
        if grounded {
            landed_ticks += 1;
        }
    }
    assert!(
        landed_ticks >= 3,
        "the fall must reach and hold ground contact, got {landed_ticks} grounded ticks"
    );
}

#[test]
fn grounded_correction_at_the_initiation_tick_outranks_the_retained_initiation() {
    let mut harness = flag_harness();
    let _ = step_retained(&mut harness, MovementInput::default());
    let (_, takeoff) = step_retained(&mut harness, jump_input(true));
    assert!(takeoff.processed.jump_initiated);
    for _ in 0..3 {
        let (_, sample) = step_retained(&mut harness, jump_input(false));
        assert!(!sample.grounded_after_tick);
    }

    // The server corrects the initiated takeoff tick itself and reports
    // ground contact there, contradicting this client's takeoff prediction.
    // The replay seed deliberately lets that server-reported outcome outrank
    // the retained initiation: seeding the arc open would assert Jumping
    // through replayed ticks anchored on a grounded report. This documents
    // the accepted provisional precedence (see `apply_correction`); a native
    // correction-heavy measurement replaces it deliberately or confirms it.
    reconcile_candidate_physics_correction(
        &mut harness.ticker,
        &mut harness.physics,
        takeoff.position,
        takeoff.tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .expect("correction at the initiated tick replays");

    let replayed = harness.ticker.pending_snapshots();
    assert!(
        !replayed.is_empty(),
        "the replayed remainder past the anchor exists"
    );
    assert!(
        replayed.iter().all(|snapshot| snapshot.tick > takeoff.tick),
        "queued work up to the corrected anchor is dropped"
    );
    for snapshot in &replayed {
        assert_eq!(
            snapshot.flags.bits() & PlayerInputFlags::JUMPING.bits(),
            0,
            "tick {} must not carry an arc seeded by the outranked initiation",
            snapshot.tick
        );
    }
}

#[test]
fn processed_sneak_and_sprint_lanes_never_repeat_stop_edges_while_raw_buttons_stay_held() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);

    // Both buttons physically held while the processed states are active.
    let mut held = settled_sample(41, [0.0; 3]);
    held.sneaking = true;
    held.sprinting = true;
    held.processed.sneaking = true;
    held.processed.sprinting = true;

    // The pose-gated divergence these lanes must survive: the raw buttons
    // stay physically held while a future rule narrows the processed states.
    let mut narrowed = held.clone();
    narrowed.tick = 42;
    narrowed.processed.sneaking = false;
    narrowed.processed.sprinting = false;

    // ...and they stay narrowed on the next tick without any physical change.
    let still_narrowed = {
        let mut sample = narrowed.clone();
        sample.tick = 43;
        sample
    };

    ticker.enqueue_completed_physics(held).unwrap();
    ticker.enqueue_completed_physics(narrowed).unwrap();
    ticker.enqueue_completed_physics(still_narrowed).unwrap();

    let held_snapshot = ticker.pop_pending().expect("held queued").snapshot;
    for mask in [
        PlayerInputFlags::START_SNEAKING,
        PlayerInputFlags::SNEAK_DOWN,
        PlayerInputFlags::SNEAKING,
        PlayerInputFlags::SNEAK_PRESSED_RAW,
        PlayerInputFlags::START_SPRINTING,
        PlayerInputFlags::SPRINT_DOWN,
        PlayerInputFlags::SPRINTING,
    ] {
        assert_ne!(
            held_snapshot.flags.bits() & mask.bits(),
            0,
            "held must carry {mask:?}"
        );
    }

    let narrowed_snapshot = ticker.pop_pending().expect("narrowed queued").snapshot;
    assert_ne!(
        narrowed_snapshot.flags.bits() & PlayerInputFlags::STOP_SNEAKING.bits(),
        0,
        "the first processed drop reports stop edges exactly once"
    );
    assert_ne!(
        narrowed_snapshot.flags.bits() & PlayerInputFlags::SNEAK_RELEASED_RAW.bits(),
        0,
        "the physical release edge is reported when processed drops while raw stays held"
    );
    assert_ne!(
        narrowed_snapshot.flags.bits() & PlayerInputFlags::STOP_SPRINTING.bits(),
        0
    );

    let still_snapshot = ticker
        .pop_pending()
        .expect("still-narrowed queued")
        .snapshot;
    assert_eq!(
        still_snapshot.flags.bits()
            & (PlayerInputFlags::STOP_SNEAKING
                | PlayerInputFlags::SNEAK_RELEASED_RAW
                | PlayerInputFlags::STOP_SPRINTING)
                .bits(),
        0,
        "no repeated stop edges while the raw buttons stay physically held"
    );
    assert_eq!(
        still_snapshot.flags.bits()
            & (PlayerInputFlags::START_SNEAKING | PlayerInputFlags::START_SPRINTING).bits(),
        0,
        "no fresh start edges exist without a physical change"
    );
}
