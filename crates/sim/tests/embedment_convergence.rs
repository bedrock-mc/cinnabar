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
fn embedment_wall_pocket_cycles_without_input() {
    // Floor plus two opposed walls whose gap is narrower than the standing
    // player: every horizontal escape direction collides, so the greedy
    // per-axis resolution can only cycle between minimal translations under
    // zero input. The bounded probe contract is that after 64 ticks the
    // state has EITHER exited overlap-free OR stayed inside the same
    // displacement budget the probe would have enforced before transmitting.
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
    let simulator = Simulator::default();

    for _ in 0..64 {
        simulator
            .tick(&mut state, MovementInput::default(), &world)
            .expect("pocket ticks complete against loaded collision data");
    }

    let displaced_squared = (state.position - start).length_squared();
    assert!(
        overlap_free(&world, state.position)
            || displaced_squared <= PROBE_MAX_DISPLACEMENT_BLOCKS * PROBE_MAX_DISPLACEMENT_BLOCKS,
        "zero-input pocket cycles must exit cleanly or stay bounded: feet at {:?}, \
         displaced {displaced_squared:?} (squared)",
        state.position,
    );
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
