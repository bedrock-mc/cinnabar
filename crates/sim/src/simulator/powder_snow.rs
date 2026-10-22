//! Powder snow's mover-dependent collision shape.

use crate::{Aabb, Vec3};

/// Offset added to the block top before comparing it with the mover's feet.
const TOP_EPSILON: f32 = -1.192_092_9e-7;
/// Fall distance beyond which any mover lands on the lowered fall-into shape.
const FALL_INTO_DISTANCE: f32 = 2.5;
/// Height of the fall-into shape within the block.
const FALL_INTO_HEIGHT: f32 = 0.9;

/// The mover facts powder snow's collision reads, fixed for one tick's move.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct PowderSnowStance {
    /// Feet height at the start of the tick.
    pub feet_y: f64,
    /// This tick's descent through a climbable block (`DescendThroughBlock`).
    pub descending: bool,
    /// Fall distance retained from the previous tick.
    pub fall_distance: f32,
    /// Snow-standing capability (leather boots).
    pub can_stand_on_snow: bool,
}

/// The shape powder snow at `block` presents to a mover: nothing unless the
/// feet are at its top and the mover is not descending; then the lowered
/// fall-into shape after a long fall, or the full block for a mover that can
/// stand on snow.
pub(super) fn collision_shape(block: [i32; 3], stance: PowderSnowStance) -> Option<Aabb> {
    let [x, y, z] = block.map(|axis| axis as f32);
    if y + 1.0 + TOP_EPSILON > stance.feet_y as f32 || stance.descending {
        return None;
    }
    let min = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
    let top = if stance.fall_distance > FALL_INTO_DISTANCE {
        y + FALL_INTO_HEIGHT
    } else if stance.can_stand_on_snow {
        y + 1.0
    } else {
        return None;
    };
    Some(Aabb::new(
        min,
        Vec3::new(f64::from(x + 1.0), f64::from(top), f64::from(z + 1.0)),
    ))
}
