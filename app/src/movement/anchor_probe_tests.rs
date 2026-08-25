//! Regression witnesses for the bounded spawn-anchor depenetration cure.
//!
//! Live third-party evidence (2026-08-22/25) showed spawn anchors installed
//! inside solids producing genuine oscillating motion under zero input —
//! the exact signature third-party anti-cheats reject ("movement cheats").
//! These witnesses pin the provisional recovery contract: probed anchors
//! rest clear of solids before their first transmitted sample, unresolvable
//! embedment freezes advancement and transmission until a server anchor
//! re-probes or the unchanged cap fails open, and the per-epoch failure
//! budget bounds correction fight-loops. All constants are provisional
//! recovery policy, not vanilla parity claims.

use std::time::Duration;

use serde_json::Value;

use super::anchor_probe::{ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS, AnchorProbeState, BeforeTick};
use super::anchor_probe_evidence::{
    MARKER_BYTE_CAP, MARKER_PREFIX, MAX_SEALING_COLLIDERS, failure_marker_lines,
};
use super::integration_tests::evidence_context;
use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionMode,
    flush_player_auth_inputs, reconcile_candidate_physics_correction,
};
use protocol::PLAYER_NETWORK_OFFSET;
use sim::{Aabb, CollisionQuery, CollisionWorld, MovementInput, Vec3, WorldQueryError};

const SETTLE_TIMEOUT_TICKS: u64 = super::settle::SETTLE_TIMEOUT_TICKS;

/// Flat surface whose walkable top is exactly `y = 70`.
struct SurfaceFloor;

impl CollisionWorld for SurfaceFloor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 69.0, -64.0), Vec3::new(64.0, 70.0, 64.0));
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

/// Sealed pocket: interior gaps are narrower than the standing player on
/// every axis, so every escape direction collides and no bounded
/// minimal-translation walk can resolve the embedment.
struct SealedRoom;

impl SealedRoom {
    const SOLIDS: [Aabb; 6] = [
        // Floor (top face y=65).
        Aabb::new(Vec3::new(-8.0, 64.0, -8.0), Vec3::new(8.0, 65.0, 8.0)),
        // Ceiling (bottom face y=66.6 leaves a 1.6-block gap < player height).
        Aabb::new(Vec3::new(-8.0, 66.6, -8.0), Vec3::new(8.0, 68.0, 8.0)),
        // West/east walls (0.38-block interior gap < player width).
        Aabb::new(Vec3::new(-8.0, 64.0, -8.0), Vec3::new(0.31, 68.0, 8.0)),
        Aabb::new(Vec3::new(0.69, 64.0, -8.0), Vec3::new(8.0, 68.0, 8.0)),
        // North/south walls (0.5-block interior gap < player depth).
        Aabb::new(Vec3::new(-8.0, 64.0, -8.0), Vec3::new(8.0, 68.0, 0.25)),
        Aabb::new(Vec3::new(-8.0, 64.0, 0.75), Vec3::new(8.0, 68.0, 8.0)),
    ];
}

impl CollisionWorld for SealedRoom {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            Self::SOLIDS
                .iter()
                .copied()
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }
}

fn feet_of(sample_position: [f32; 3]) -> Vec3 {
    Vec3::new(
        f64::from(sample_position[0]),
        f64::from(sample_position[1] - PLAYER_NETWORK_OFFSET),
        f64::from(sample_position[2]),
    )
}

#[test]
fn reanchor_into_solid_probes_before_first_sample() {
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 0, [0.5, 71.0, 0.5]);
    ticker.set_source(MovementSource::Physics);
    // The production surface-spawn resolve anchors with authoritative ground
    // contact (`runtime::world` passes `on_ground = true`), so the probed
    // anchor's first tick is a settled standing tick rather than an airborne
    // settling one.
    physics.reanchor_network_position([0.5, 71.0, 0.5], 0, true);
    assert!(
        !ticker.holding_embedded_anchor(),
        "a fresh anchor starts outside any hold"
    );

    let frame = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &SurfaceFloor,
    );
    assert!(frame.blocked.is_none());
    assert!(
        !frame.embedded_anchor_hold_engaged,
        "a resolvable embedment clears"
    );
    assert_eq!(frame.samples.len(), 1, "exactly one fixed tick completed");
    assert_eq!(frame.embedded_hold_ticks, 0);

    let sample = frame.samples.into_iter().next().expect("one sample");
    let player = Aabb::player_at(feet_of(sample.position));
    let floor = Aabb::new(Vec3::new(-64.0, 69.0, -64.0), Vec3::new(64.0, 70.0, 64.0));
    assert!(
        !player.intersects(floor),
        "the first produced sample must rest overlap-free, feet {:?}",
        feet_of(sample.position),
    );
    assert!(
        sample.grounded_after_tick,
        "the probed anchor stands on the surface instead of launching"
    );
    assert!(
        sample.velocity[1] <= 0.0 && sample.velocity[1] > -0.2,
        "no depenetration launch impulse may reach the sample: {:?}",
        sample.velocity,
    );
    assert!(!sample.horizontal_collision);

    // The MOVEMENT_TX_GATE episode engaged at reset precedes any hand-off:
    // the probed sample stays withheld by the armed settle window.
    ticker.enqueue_completed_physics(sample).unwrap();
    let mut sent = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |identity, _packet| {
            sent.push(identity.tick);
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    assert!(
        sent.is_empty(),
        "the engaged gate withholds the probed sample"
    );
}

#[test]
fn unresolvable_embedment_holds_until_server_reanchors() {
    // --- Stage 1: the failed probe freezes advancement and transmission.
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.5, 66.620_01, 0.5]);
    ticker.set_source(MovementSource::Physics);
    physics.reanchor_network_position([0.5, 66.620_01, 0.5], 40, false);

    let mut sent_total = 0usize;
    fn flush_sends(ticker: &mut MovementTicker) -> usize {
        let mut sent = 0usize;
        flush_player_auth_inputs(
            ticker,
            16,
            Some(evidence_context()),
            |_identity, _packet| {
                sent += 1;
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        sent
    }

    let engage = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &SealedRoom,
    );
    assert!(
        engage.embedded_anchor_hold_engaged,
        "an unresolvable embedment freezes advancement"
    );
    assert!(engage.samples.is_empty());
    assert_eq!(engage.embedded_hold_ticks, 0);
    ticker.note_embedded_anchor();
    assert!(ticker.holding_embedded_anchor());
    sent_total += flush_sends(&mut ticker);
    assert_eq!(sent_total, 0, "ZERO PlayerAuthInput while held");

    // --- Stage 2: repeated advances produce no new samples and each hold
    // episode fails open at exactly the unchanged 200-tick cap.
    let mut engaged = 1_u32;
    let mut lifted = 0_u32;
    let mut episode_held = 0_u64;
    let mut held_at_lift = Vec::new();
    for _ in 0..600 {
        let frame = physics.advance(
            Duration::from_millis(50),
            MovementInput::default(),
            &SealedRoom,
        );
        if frame.embedded_anchor_hold_engaged {
            engaged += 1;
            episode_held = 0;
            ticker.note_embedded_anchor();
            assert!(frame.samples.is_empty());
        } else if frame.embedded_hold_ticks != 0 {
            assert!(
                frame.samples.is_empty() && ticker.holding_embedded_anchor(),
                "held frames carry no samples"
            );
            episode_held += frame.embedded_hold_ticks;
            if ticker.observe_embedded_anchor_hold(frame.embedded_hold_ticks) {
                lifted += 1;
                held_at_lift.push(episode_held);
                physics.release_embedded_anchor_hold();
            }
        }
        sent_total += flush_sends(&mut ticker);
        if engaged == 2 && lifted == 2 {
            break;
        }
    }
    assert_eq!(
        engaged, 2,
        "the second failed probe re-enters one more bounded hold"
    );
    assert_eq!(lifted, 2, "every hold fails open at the unchanged cap");
    assert_eq!(
        held_at_lift,
        vec![SETTLE_TIMEOUT_TICKS, SETTLE_TIMEOUT_TICKS],
    );
    sent_total += flush_sends(&mut ticker);
    assert_eq!(sent_total, 0, "still ZERO PlayerAuthInput handed off");

    // --- Stage 3: the third failure spends the epoch budget and degrades to
    // today's fail-open streaming behavior instead of holding again.
    let mut degrade_samples = 0_usize;
    for _ in 0..10 {
        let frame = physics.advance(
            Duration::from_millis(50),
            MovementInput::default(),
            &SealedRoom,
        );
        assert!(
            !frame.embedded_anchor_hold_engaged,
            "a spent epoch must never freeze again"
        );
        degrade_samples += frame.samples.len();
    }
    assert!(
        degrade_samples > 0,
        "degraded epochs stream today's behavior"
    );
    assert!(!ticker.holding_embedded_anchor());

    // --- Exit (a): a server snap to a clear position re-anchors, re-probes,
    // and resumes within one frame.
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.5, 66.620_01, 0.5]);
    ticker.set_source(MovementSource::Physics);
    physics.reanchor_network_position([0.5, 66.620_01, 0.5], 100, false);
    let engage = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &SealedRoom,
    );
    assert!(engage.embedded_anchor_hold_engaged);
    ticker.note_embedded_anchor();
    assert!(ticker.holding_embedded_anchor());

    let snap_tick = ticker.next_tick();
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [50.5, 71.620_01, 50.5],
        snap_tick,
        true,
        PhysicsCorrectionMode::Snap,
        &SurfaceFloor,
    )
    .expect("a clear-position snap applies");
    assert!(
        !ticker.holding_embedded_anchor(),
        "the snap reanchor ends the hold with a fresh settle window"
    );
    assert!(ticker.pending_snapshots().is_empty());

    // Reanchor-before-advance discards exactly the elapsed time that
    // preceded the new anchor, so the following frame probes and resumes.
    let discarded = physics.advance(Duration::ZERO, MovementInput::default(), &SurfaceFloor);
    assert!(discarded.samples.is_empty());
    let resume = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &SurfaceFloor,
    );
    assert!(!resume.embedded_anchor_hold_engaged);
    assert_eq!(
        resume.samples.len(),
        1,
        "resume within one frame of the snap"
    );
    let resumed = resume
        .samples
        .into_iter()
        .next()
        .expect("one resumed sample");
    let player = Aabb::player_at(feet_of(resumed.position));
    let floor = Aabb::new(Vec3::new(-64.0, 69.0, -64.0), Vec3::new(64.0, 70.0, 64.0));
    assert!(!player.intersects(floor));
}

#[test]
fn transient_collision_unavailability_keeps_todays_blocked_behavior() {
    struct DeferredRoom {
        available: std::cell::Cell<bool>,
    }
    impl CollisionWorld for DeferredRoom {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            if !self.available.get() {
                return Err(WorldQueryError::UnloadedChunk(world::ChunkKey::new(
                    0, 0, 0,
                )));
            }
            SealedRoom.collision_boxes(query)
        }
    }

    let room = DeferredRoom {
        available: std::cell::Cell::new(false),
    };
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.5, 66.620_01, 0.5], 40, false);

    let blocked = physics.advance(Duration::from_millis(50), MovementInput::default(), &room);
    assert!(
        matches!(
            blocked.blocked,
            Some(sim::SimulationError::World(WorldQueryError::UnloadedChunk(
                _
            )))
        ),
        "unavailable data keeps today's transient-blocked behavior exactly"
    );
    assert!(!blocked.embedded_anchor_hold_engaged);
    assert_eq!(blocked.embedded_hold_ticks, 0);
    assert_eq!(blocked.dropped_ticks, 0);

    // Once data arrives the probe observes the sealed pocket and holds
    // instead of simulating garbage.
    room.available.set(true);
    let engaged = physics.advance(Duration::from_millis(50), MovementInput::default(), &room);
    assert!(engaged.embedded_anchor_hold_engaged);
}

// ---------------------------------------------------------------------------
// Failure-evidence markers (RUST_MCBE_ANCHOR_PROBE family).
//
// Live third-party evidence (2026-08-25) could not answer WHICH blocks seal
// an unescapable spawn pocket after two embedded-anchor holds failed open.
// These witnesses pin the bounded, zero-behavior-change evidence contract:
// every failed attempt renders one single-line marker naming the sealing
// colliders, the epoch-degrade transition adds exactly one degraded marker,
// truncation stays inside hard caps, and instrumentation can never move a
// gate, hold, or transmission decision.
// ---------------------------------------------------------------------------

/// Anchored feet for the unit-cell shaft fixtures below. The standing player
/// box spans x/z in [0.2001, 0.7999] and y in [65.5, 67.3].
const SHAFT_FEET: Vec3 = Vec3::new(0.5, 65.5, 0.5);

const FLOOR_CELL: Aabb = Aabb::new(Vec3::new(0.0, 65.0, 0.0), Vec3::new(1.0, 66.0, 1.0));
const CEILING_CELL: Aabb = Aabb::new(Vec3::new(0.0, 67.0, 0.0), Vec3::new(1.0, 68.0, 1.0));

fn probe_query(feet: Vec3) -> Aabb {
    Aabb::player_at(feet).grown(ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS)
}

fn parse_marker(line: &str) -> Value {
    let payload = line
        .strip_prefix(MARKER_PREFIX)
        .expect("marker lines carry the registered family prefix");
    serde_json::from_str(payload).expect("marker payload must be valid JSON")
}

fn assert_bounded_line(line: &str) {
    assert!(
        !line.contains('\n'),
        "evidence markers must stay single-line"
    );
    assert!(
        line.len() <= MARKER_BYTE_CAP,
        "marker line {} bytes exceeds the hard cap {MARKER_BYTE_CAP}",
        line.len(),
    );
}

/// Unit-cell sealed shaft: floor cell and ceiling cell one block apart cap
/// the anchored player column vertically (a 1.0-block gap < standing player
/// height), so depenetration oscillates until its iteration budget exhausts
/// and the probe reports unresolvable embedment. Two neighbouring shaft-wall
/// cells never touch the anchored box and prove non-overlapping geometry is
/// excluded from the sealing report.
struct UnitShaft;

impl CollisionWorld for UnitShaft {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        const SIDE_A: Aabb = Aabb::new(Vec3::new(-1.0, 65.0, -1.0), Vec3::new(0.0, 66.0, 0.0));
        const SIDE_B: Aabb = Aabb::new(Vec3::new(1.0, 65.0, 0.0), Vec3::new(2.0, 66.0, 1.0));
        Ok(CollisionQuery::synthetic(
            [FLOOR_CELL, CEILING_CELL, SIDE_A, SIDE_B]
                .iter()
                .copied()
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }
}

/// Same shaft geometry with the floor cell repeated across ten physics
/// layers/shapes — multi-shape blocks legitimately produce several colliders
/// per cell — to exercise the eight-collider report cap.
struct LayeredShaft;

impl CollisionWorld for LayeredShaft {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut colliders = vec![FLOOR_CELL; 10];
        colliders.push(CEILING_CELL);
        Ok(CollisionQuery::synthetic(
            colliders
                .into_iter()
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }
}

/// Byte-budget fixture: a modest unit-cell collider that always fits, an
/// out-of-`i32`-range collider whose coordinates cannot be attributed to any
/// block cell yet still fit comfortably, and a full ±`f64::MAX` cube that
/// overlaps everything but whose six 309-digit float-marked coordinates can
/// never fit under the line cap. The truncation outcome is independent of
/// exact digit counts.
struct ByteBudgetRoom;

impl CollisionWorld for ByteBudgetRoom {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        const VAST: f64 = f64::MAX;
        const BEYOND_I32: f64 = 3.0e9;
        let mut colliders = vec![FLOOR_CELL];
        colliders.push(Aabb::new(
            Vec3::new(-BEYOND_I32, 66.5, -BEYOND_I32),
            Vec3::new(BEYOND_I32, 67.5, BEYOND_I32),
        ));
        colliders.push(Aabb::new(
            Vec3::new(-VAST, -VAST, -VAST),
            Vec3::new(VAST, VAST, VAST),
        ));
        Ok(CollisionQuery::synthetic(
            colliders
                .into_iter()
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }
}

fn shaft_failure_lines(
    world: &impl CollisionWorld,
    enabled: bool,
    epoch_spent: bool,
) -> Vec<String> {
    let colliders = world
        .collision_boxes(probe_query(SHAFT_FEET))
        .expect("stub worlds are always loaded")
        .value;
    failure_marker_lines(
        enabled,
        SHAFT_FEET,
        &colliders,
        super::anchor_probe::ANCHOR_PROBE_MAX_ITERATIONS,
        ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS,
        epoch_spent,
    )
}

#[test]
fn failed_probe_marker_names_exact_unit_cell_sealing_colliders() {
    // The failed attempt itself must hold transmission exactly as before.
    let mut state = AnchorProbeState::new();
    state.note_hard_anchor();
    state.testing_set_evidence_enabled(true);
    assert_eq!(state.before_tick(&UnitShaft, SHAFT_FEET), BeforeTick::Hold);

    let lines = shaft_failure_lines(&UnitShaft, true, false);
    assert_eq!(lines.len(), 1, "exactly one marker per failed attempt");
    assert_bounded_line(&lines[0]);

    let parsed = parse_marker(&lines[0]);
    assert_eq!(parsed["schema"], "rust-mcbe-anchor-probe-v1");
    assert_eq!(parsed["phase"], "failed");
    assert_eq!(parsed["feet"], serde_json::json!([0.5, 65.5, 0.5]));
    assert_eq!(
        parsed["player_extents"],
        serde_json::json!([0.5998, 1.8, 0.5998])
    );
    assert_eq!(
        parsed["iterations"],
        super::anchor_probe::ANCHOR_PROBE_MAX_ITERATIONS
    );
    assert_eq!(
        parsed["max_displacement_blocks"],
        ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS
    );
    assert_eq!(parsed["overlap_free"], false);
    assert_eq!(parsed["total_sealing_count"], 2);
    assert_eq!(parsed["truncated"], false);
    assert_eq!(
        parsed["sealing"],
        serde_json::json!([
            {"min": [0.0, 65.0, 0.0], "max": [1.0, 66.0, 1.0], "block": [0, 65, 0]},
            {"min": [0.0, 67.0, 0.0], "max": [1.0, 68.0, 1.0], "block": [0, 67, 0]},
        ]),
        "each unit-cell collider names its containing block exactly",
    );

    // Instrumentation disabled: the identical failure requests nothing.
    assert!(shaft_failure_lines(&UnitShaft, false, false).is_empty());

    // No failure: a merely touching surface produces zero sealing colliders
    // and therefore zero marker lines even when instrumentation is enabled.
    const SURFACE_FEET: Vec3 = Vec3::new(0.0, 70.0, 0.0);
    let surface = SurfaceFloor
        .collision_boxes(probe_query(SURFACE_FEET))
        .expect("loaded stub world")
        .value;
    let success_lines = failure_marker_lines(
        true,
        SURFACE_FEET,
        &surface,
        super::anchor_probe::ANCHOR_PROBE_MAX_ITERATIONS,
        ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS,
        false,
    );
    assert!(
        success_lines.is_empty(),
        "successful or merely touching anchors emit no evidence"
    );
}

#[test]
fn failed_probe_marker_reports_merged_multicell_colliders_without_block_ids() {
    // The established sealed-room fixture: giant slabs spanning many cells.
    let feet = Vec3::new(0.5, 66.620_01, 0.5);
    let colliders = SealedRoom
        .collision_boxes(probe_query(feet))
        .expect("loaded stub world")
        .value;
    let lines = failure_marker_lines(
        true,
        feet,
        &colliders,
        super::anchor_probe::ANCHOR_PROBE_MAX_ITERATIONS,
        ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS,
        false,
    );
    assert_eq!(lines.len(), 1);
    assert_bounded_line(&lines[0]);

    let parsed = parse_marker(&lines[0]);
    // The floor lies fully below the anchored box; the other five solids
    // overlap it. Every overlapping slab spans many cells, so each reports
    // its quantized AABB plus merged:true and never a block id. Quantized
    // values land on the 1/64 grid (66.6 -> 66.59375, 0.31 -> 0.3125).
    assert_eq!(parsed["total_sealing_count"], 5);
    assert_eq!(parsed["truncated"], false);
    assert_eq!(
        parsed["sealing"],
        serde_json::json!([
            {"min": [-8.0, 66.59375, -8.0], "max": [8.0, 68.0, 8.0], "merged": true},
            {"min": [-8.0, 64.0, -8.0], "max": [0.3125, 68.0, 8.0], "merged": true},
            {"min": [0.6875, 64.0, -8.0], "max": [8.0, 68.0, 8.0], "merged": true},
            {"min": [-8.0, 64.0, -8.0], "max": [8.0, 68.0, 0.25], "merged": true},
            {"min": [-8.0, 64.0, 0.75], "max": [8.0, 68.0, 8.0], "merged": true},
        ]),
    );
    for entry in parsed["sealing"].as_array().expect("array") {
        assert!(
            entry.get("block").is_none(),
            "multi-cell colliders must not claim a block id: {entry}"
        );
    }
}

#[test]
fn epoch_degrade_transition_emits_one_additional_degraded_marker() {
    let colliders = UnitShaft
        .collision_boxes(probe_query(SHAFT_FEET))
        .expect("loaded stub world")
        .value;
    let common_args = (
        SHAFT_FEET,
        &colliders,
        super::anchor_probe::ANCHOR_PROBE_MAX_ITERATIONS,
        ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS,
    );

    let attempt_lines = failure_marker_lines(
        true,
        common_args.0,
        common_args.1,
        common_args.2,
        common_args.3,
        false,
    );
    assert_eq!(attempt_lines.len(), 1);
    assert!(attempt_lines[0].contains("\"phase\":\"failed\""));

    // The third failure additionally spends the epoch budget: one more
    // marker reusing identical content except phase=degraded.
    let spent_lines = failure_marker_lines(
        true,
        common_args.0,
        common_args.1,
        common_args.2,
        common_args.3,
        true,
    );
    assert_eq!(spent_lines.len(), 2);
    assert!(spent_lines[0].contains("\"phase\":\"failed\""));
    assert!(spent_lines[1].contains("\"phase\":\"degraded\""));
    assert_bounded_line(&spent_lines[1]);
    assert_eq!(
        spent_lines[1].replace("degraded", "failed"),
        spent_lines[0],
        "the degraded marker reuses the same evidence besides phase",
    );

    // Disabled instrumentation emits nothing even on the degrade transition.
    assert!(
        failure_marker_lines(
            false,
            common_args.0,
            common_args.1,
            common_args.2,
            common_args.3,
            true
        )
        .is_empty()
    );

    // The state machine reaches exactly three failures before degrading.
    let mut state = AnchorProbeState::new();
    state.note_hard_anchor();
    state.testing_set_evidence_enabled(true);
    assert_eq!(state.before_tick(&UnitShaft, SHAFT_FEET), BeforeTick::Hold);
    state.release_after_cap();
    assert_eq!(state.before_tick(&UnitShaft, SHAFT_FEET), BeforeTick::Hold);
    state.release_after_cap();
    assert_eq!(
        state.before_tick(&UnitShaft, SHAFT_FEET),
        BeforeTick::Proceed,
        "the third failure degrades instead of holding again"
    );
    assert_eq!(state.failed_probes(), 3);
    assert_eq!(
        state.before_tick(&UnitShaft, SHAFT_FEET),
        BeforeTick::Proceed,
        "a spent epoch stops probing entirely"
    );
}

#[test]
fn marker_truncates_at_eight_sealing_colliders_with_total_count() {
    let lines = shaft_failure_lines(&LayeredShaft, true, false);
    assert_eq!(lines.len(), 1);
    assert_bounded_line(&lines[0]);

    let parsed = parse_marker(&lines[0]);
    assert_eq!(parsed["total_sealing_count"], 11);
    assert_eq!(parsed["truncated"], true);
    let sealing = parsed["sealing"].as_array().expect("array");
    assert_eq!(
        sealing.len(),
        MAX_SEALING_COLLIDERS,
        "the report keeps at most eight colliders",
    );
    let expected_floor = serde_json::json!({
        "min": [0.0, 65.0, 0.0],
        "max": [1.0, 66.0, 1.0],
        "block": [0, 65, 0],
    });
    assert!(
        sealing.iter().all(|entry| *entry == expected_floor),
        "count truncation keeps the first colliders in query order",
    );
}

#[test]
fn marker_byte_cap_truncates_instead_of_exceeding_2048_bytes() {
    let lines = shaft_failure_lines(&ByteBudgetRoom, true, false);
    assert_eq!(lines.len(), 1);
    assert_bounded_line(&lines[0]);

    let parsed = parse_marker(&lines[0]);
    assert_eq!(parsed["total_sealing_count"], 3);
    assert_eq!(
        parsed["truncated"], true,
        "the vast-coordinate collider must be cut by the byte budget",
    );
    let sealing = parsed["sealing"].as_array().expect("array");
    assert_eq!(
        sealing.len(),
        2,
        "the modest and out-of-range colliders fit; the vast one never can: kept {sealing:?}",
    );
    // The kept entries: the attributed unit cell, then the unattributable
    // out-of-range collider reporting merged without any block id.
    assert_eq!(
        sealing[0],
        serde_json::json!({"min": [0.0, 65.0, 0.0], "max": [1.0, 66.0, 1.0], "block": [0, 65, 0]}),
    );
    assert_eq!(
        sealing[1],
        serde_json::json!({
            "min": [-3000000000.0, 66.5, -3000000000.0],
            "max": [3000000000.0, 67.5, 3000000000.0],
            "merged": true,
        }),
        "coordinates beyond the i32 block range never claim a block id",
    );
}

#[test]
fn evidence_instrumentation_never_changes_gate_or_hold_decisions() {
    fn drive_shaft_epoch(evidence_enabled: bool) -> Vec<BeforeTick> {
        let mut state = AnchorProbeState::new();
        state.testing_set_evidence_enabled(evidence_enabled);
        state.note_hard_anchor();
        let mut decisions = Vec::new();
        decisions.push(state.before_tick(&UnitShaft, SHAFT_FEET)); // fail #1
        decisions.push(state.before_tick(&UnitShaft, SHAFT_FEET)); // frozen-hold guard
        state.release_after_cap();
        decisions.push(state.before_tick(&UnitShaft, SHAFT_FEET)); // fail #2
        state.release_after_cap();
        decisions.push(state.before_tick(&UnitShaft, SHAFT_FEET)); // fail #3: degrade
        decisions.push(state.before_tick(&UnitShaft, SHAFT_FEET)); // spent epoch
        decisions
    }

    let expected = vec![
        BeforeTick::Hold,
        BeforeTick::Proceed,
        BeforeTick::Hold,
        BeforeTick::Proceed,
        BeforeTick::Proceed,
    ];
    assert_eq!(
        drive_shaft_epoch(false),
        expected,
        "instrumentation off keeps today's exact decision sequence"
    );
    assert_eq!(
        drive_shaft_epoch(true),
        expected,
        "instrumentation on must produce the identical decision sequence",
    );
}
