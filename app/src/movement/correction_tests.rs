//! Server-correction and replay semantics for retained prediction state.
//!
//! Split from `integration_tests` to keep each test module inside the
//! architecture policy line limit.

use std::time::Duration;

use super::integration_tests::{VersionedWall, forward_physics_input};
use super::{LocalPhysicsController, PhysicsCorrectionMode, PhysicsSampleContext};

/// Axis collisions describe the motion that produced a position, so they cannot
/// be reconstructed from a corrected anchor alone. A server correction that
/// moves the player repudiates that motion, and now that retained collisions
/// gate the discrete ladder-climb branch, carrying stale flags across such a
/// correction would apply an upward impulse the server never sanctioned.
#[test]
fn a_position_changing_correction_clears_retained_axis_collisions() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(400),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);

    let retained = physics.state().unwrap();
    assert!(
        retained.collisions.z,
        "the witness needs a retained horizontal collision, got {:?}",
        retained.collisions
    );
    let corrected_tick = retained.tick - 1;

    let mut moved = physics.clone();
    moved
        .apply_correction(
            [4.0, 2.620_01, 0.0],
            corrected_tick,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &world,
        )
        .unwrap();
    assert!(
        !moved.state().unwrap().collisions.z,
        "a position-changing correction must not retain repudiated collisions"
    );
}

/// A correction that confirms the client's own predicted position does not
/// repudiate the motion that produced it, so the retained flags stay valid and
/// a legitimate wall climb must not stutter on every confirming correction.
#[test]
fn a_confirming_correction_preserves_retained_axis_collisions() {
    let world = VersionedWall(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(400),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);

    let corrected_tick = physics.state().unwrap().tick - 1;
    let confirmed = frame
        .samples
        .iter()
        .find(|sample| sample.tick == corrected_tick)
        .expect("the correction tick is retained")
        .position;
    let collisions_at_correction = physics.retained_collisions_at(corrected_tick).unwrap();
    assert!(
        collisions_at_correction.z,
        "the witness needs a retained collision at the correction tick"
    );

    let mut physics = physics.clone();
    physics
        .apply_correction(
            confirmed,
            corrected_tick,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &world,
        )
        .unwrap();
    assert!(
        physics.retained_collisions_at(corrected_tick).unwrap().z,
        "a confirming correction must preserve the retained collisions it re-derives from"
    );
}
