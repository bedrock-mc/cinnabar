//! Shared conservative extents for the CPU and GPU terrain visibility paths.
use bevy::{camera::primitives::Aabb, math::Vec3A};

const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;
pub(in crate::chunk) const FULL_BOUNDS: [[i32; 3]; 2] = [[0; 3], [SIDE; 3]];
// Round outward after adding the admitted component to a one-block model overhang.
const MODEL_PADDING: i32 = (1.0 + block_transform::random_offset::MAX_AXIS_OFFSET) as i32 + 1;
/// Conservative model extents including admitted displacement and waving.
pub(in crate::chunk) const MODEL_BOUNDS: [[i32; 3]; 2] =
    [[-MODEL_PADDING; 3], [SIDE + MODEL_PADDING; 3]];

pub(in crate::chunk) fn aabb(models: bool) -> Aabb {
    let [low, high] = if models { MODEL_BOUNDS } else { FULL_BOUNDS };
    let low = Vec3A::from_array(low.map(|value| value as f32));
    let high = Vec3A::from_array(high.map(|value| value as f32));
    Aabb {
        center: (low + high) * 0.5,
        half_extents: (high - low) * 0.5,
    }
}
