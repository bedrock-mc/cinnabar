use crate::{Aabb, Vec3};

/// Vanilla liquid material probe, using the preceding collision pose.
pub(crate) fn liquid_probe_bounds(aabb: Aabb, water: bool) -> Aabb {
    let inset = if water {
        [0.001_f32, 0.401, 0.001]
    } else {
        [0.1_f32, 0.4, 0.1]
    };
    let mut low = Vec3::ZERO;
    let mut high = Vec3::ZERO;
    for axis in 0..3 {
        let min = aabb.min[axis] as f32;
        let max = aabb.max[axis] as f32;
        let center = (min + max) * 0.5;
        low[axis] = f64::from((min + inset[axis]).min(center));
        high[axis] = f64::from((max - inset[axis]).max(center));
    }
    Aabb::new(low, high)
}
