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
