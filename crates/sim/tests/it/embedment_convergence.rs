//! Embedment-convergence witnesses for the bounded spawn-anchor probe.
//!
//! Live third-party evidence (2026-08-22/25): spawn anchors installed inside
//! solids turn depenetration minimal-translation vectors into genuine
//! oscillating position AND velocity under zero input, which server
//! anti-cheats reject as "movement cheats". These tests pin what the pinned
//! bedsim-order resolution actually does from embedded starts:
//!
//! - a single overlapping block ejects within one tick (baseline pin), while
//! - an opposed-wall pocket must either exit overlap-free or keep its total
//!   zero-input displacement inside the same bounded budget the app-side
//!   anchor probe enforces before transmitting (`1.5` blocks; see the app's
//!   `anchor_probe` module for the provisional bound).
//!
//! Neither witness claims vanilla parity: they characterize the deterministic
//! recovery envelope that the provisional anchor probe relies on.

use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, PlayerState, Simulator, Vec3,
    WorldQueryError,
};

/// The app-side anchor-probe displacement budget this file pins against
/// (kept in literal sync with the app's provisional constant).
const PROBE_MAX_DISPLACEMENT_BLOCKS: f64 = 1.5;

struct BoxWorld {
    colliders: Vec<Aabb>,
}

impl BoxWorld {
    fn new(colliders: Vec<Aabb>) -> Self {
        Self { colliders }
    }
}

impl CollisionWorld for BoxWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.colliders
                .iter()
                .copied()
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }
}

fn overlap_free(world: &BoxWorld, feet: Vec3) -> bool {
    let player = Aabb::player_at(feet);
    world
        .colliders
        .iter()
        .all(|collider| !player.intersects(*collider))
}

#[test]
fn single_block_embedment_exits_in_one_tick() {
    // Baseline pin: one fully-overlapping block resolves through a single
    // minimal-translation application on the first zero-input tick.
    let world = BoxWorld::new(vec![Aabb::new(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 1.0, 1.0),
    )]);
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    let simulator = Simulator::default();

    let result = simulator
        .tick(&mut state, MovementInput::default(), &world)
        .expect("embedded single-block tick completes");

    assert_eq!(result.tick, 1);
    assert!(
        overlap_free(&world, state.position),
        "one tick must eject a single-block embedment, feet at {:?}",
        state.position,
    );
}

#[test]
fn embedment_wall_pocket_reports_no_horizontal_drift() {
    // Floor plus two opposed walls whose gap is narrower than the standing
    // player: every horizontal escape direction collides. Per-axis resolution
    // reduces intended motion toward zero on each axis and never turns a
    // depenetration minimal-translation into reported horizontal motion, so a
    // zero-input pocket start transmits no horizontal displacement (the
    // "movement cheats" signature a positional push-out must not produce). The
    // bounded positional push-out is the app-side probe's job, not the tick's.
    let world = BoxWorld::new(vec![
        Aabb::new(Vec3::new(-64.0, -2.0, -64.0), Vec3::new(64.0, 0.0, 64.0)),
        Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(0.30, 3.0, 64.0)),
        Aabb::new(Vec3::new(0.36, 0.0, -64.0), Vec3::new(64.0, 3.0, 64.0)),
    ]);
    let start = Vec3::new(0.33, 0.0, 0.0);
    assert!(
        !overlap_free(&world, start),
        "the pocket fixture must start embedded"
    );
    let mut state = PlayerState::new(start);
    state.on_ground = true;
    let simulator = Simulator::default();

    for _ in 0..64 {
        let result = simulator
            .tick(&mut state, MovementInput::default(), &world)
            .expect("pocket ticks complete against loaded collision data");
        assert_eq!(
            (result.movement.x, result.movement.z),
            (0.0, 0.0),
            "zero-input embedment must report no horizontal movement",
        );
        assert_eq!(
            (result.velocity.x, result.velocity.z),
            (0.0, 0.0),
            "zero-input embedment must report no horizontal velocity",
        );
    }

    // Move finalization recomputes the centre of the f32 AABB.
    assert!((state.position.x - start.x).abs() <= f64::from(f32::EPSILON));
    assert_eq!(state.position.z, start.z);
}

#[test]
fn embedded_horizontal_start_reports_zero_horizontal_motion() {
    // A grounded player whose box overlaps a wall on its smallest-penetration
    // (horizontal) axis. Before the per-axis fix, motion resolution ejected the
    // embedded box along that axis and wrote the ejection into both the reported
    // movement (PosDelta) and velocity, fabricating inputless horizontal motion.
    // Vanilla per-axis resolution only shortens intended motion, so a zero-input
    // embedded tick must report exactly zero horizontal movement and velocity.
    let world = BoxWorld::new(vec![
        Aabb::new(Vec3::new(-8.0, -1.0, -8.0), Vec3::new(8.0, 0.0, 8.0)),
        Aabb::new(Vec3::new(0.5, 0.0, -1.0), Vec3::new(1.5, 3.0, 1.0)),
    ]);
    let start = Vec3::new(0.4, 0.0, 0.0);
    assert!(
        !overlap_free(&world, start),
        "the fixture must start embedded in the wall"
    );
    let mut state = PlayerState::new(start);
    state.on_ground = true;
    let simulator = Simulator::default();

    let result = simulator
        .tick(&mut state, MovementInput::default(), &world)
        .expect("embedded horizontal tick completes");

    assert_eq!(result.movement.x, 0.0, "no fabricated horizontal PosDelta");
    assert_eq!(result.movement.z, 0.0, "no fabricated horizontal PosDelta");
    assert_eq!(result.velocity.x, 0.0, "no fabricated horizontal velocity");
    assert_eq!(result.velocity.z, 0.0, "no fabricated horizontal velocity");
    assert!((state.position.x - start.x).abs() <= f64::from(f32::EPSILON));
    assert_eq!(state.position.z, start.z, "no horizontal drift");
}

#[test]
fn depenetrate_resolves_a_single_overlapping_block() {
    let block = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
    let cleared = sim::depenetrate_player(
        Vec3::new(0.5, 0.5, 0.5),
        &[block],
        8,
        PROBE_MAX_DISPLACEMENT_BLOCKS,
    )
    .expect("a single-block embedment resolves inside the bounds");
    assert!(!Aabb::player_at(cleared).intersects(block));
    // The minimal axis is vertical here; the fix must not wander sideways.
    assert_eq!(cleared.x, 0.5);
    assert_eq!(cleared.z, 0.5);
}

#[test]
fn depenetrate_refuses_displacement_beyond_the_bound() {
    // Deeply embedded in one huge box: every minimal translation alone
    // exceeds the provisional bound, so the probe must fail closed instead
    // of inventing a large teleport.
    let huge = Aabb::new(Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 10.0));
    assert_eq!(
        sim::depenetrate_player(Vec3::ZERO, &[huge], 8, PROBE_MAX_DISPLACEMENT_BLOCKS,),
        None,
    );
}

#[test]
fn depenetrate_reports_clear_when_no_collider_overlaps() {
    let elsewhere = Aabb::new(Vec3::new(50.0, 50.0, 50.0), Vec3::new(51.0, 51.0, 51.0));
    assert_eq!(
        sim::depenetrate_player(Vec3::ONE, &[elsewhere], 8, PROBE_MAX_DISPLACEMENT_BLOCKS),
        Some(Vec3::ONE),
    );
    assert_eq!(
        sim::depenetrate_player(Vec3::ONE, &[], 8, 1.5),
        Some(Vec3::ONE)
    );
}
