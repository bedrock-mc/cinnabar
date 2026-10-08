//! Shared conservative extents for the CPU and GPU terrain visibility paths.
use bevy::{camera::primitives::Aabb, math::Vec3A};

const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;
pub(in crate::chunk) const FULL_BOUNDS: [[i32; 3]; 2] = [[0; 3], [SIDE; 3]];
/// Models may overhang their subchunk by one block.
pub(in crate::chunk) const MODEL_BOUNDS: [[i32; 3]; 2] = [[-1; 3], [SIDE + 1; 3]];

pub(in crate::chunk) fn aabb(models: bool) -> Aabb {
    let [low, high] = if models { MODEL_BOUNDS } else { FULL_BOUNDS };
    let low = Vec3A::from_array(low.map(|value| value as f32));
    let high = Vec3A::from_array(high.map(|value| value as f32));
    Aabb {
        center: (low + high) * 0.5,
        half_extents: (high - low) * 0.5,
    }
}
