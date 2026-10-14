use crate::{Vec3, normalize_direction};

/// Float segment used for traversal, clipping and the resulting wire-visible intercept.
#[derive(Clone, Copy)]
pub(super) struct FloatRay {
    pub origin: [f32; 3],
    pub delta: [f32; 3],
}

impl FloatRay {
    /// Rounds the endpoint before subtracting the origin, preserving distant-coordinate spacing.
    pub fn new(origin: Vec3, direction: Vec3, reach: f64) -> Self {
        let origin = [origin.x as f32, origin.y as f32, origin.z as f32];
        let direction = [direction.x as f32, direction.y as f32, direction.z as f32];
        let reach = reach as f32;
        let delta = std::array::from_fn(|axis| {
            let end = origin[axis] + direction[axis] * reach;
            end - origin[axis]
        });
        Self { origin, delta }
    }

    /// Widens the rounded segment point only after all interpolation operations finish.
    pub fn point(self, fraction: f32) -> Vec3 {
        let [x, y, z] = std::array::from_fn(|axis| self.origin[axis] + self.delta[axis] * fraction);
        Vec3::new(f64::from(x), f64::from(y), f64::from(z))
    }

    /// Measures the rounded intercept with the same float products as actor picking.
    pub fn distance(self, point: Vec3) -> f64 {
        let x = point.x as f32 - self.origin[0];
        let y = point.y as f32 - self.origin[1];
        let z = point.z as f32 - self.origin[2];
        f64::from((z * z + y * y + x * x).sqrt())
    }
}

/// Adapts the public trace vector to the shared float normalization boundary.
pub(super) fn normalized(direction: Vec3) -> Option<Vec3> {
    let [x, y, z] =
        normalize_direction([direction.x as f32, direction.y as f32, direction.z as f32])?;
    Some(Vec3::new(f64::from(x), f64::from(y), f64::from(z)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_endpoint_rounds_before_fraction_and_distance_calculations() {
        let ray = FloatRay::new(
            Vec3::new(16_777_216.0, 0.5, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            3.0,
        );
        assert_eq!(ray.delta, [4.0, 0.0, 0.0]);
        let midpoint = ray.point(0.5);
        assert_eq!(midpoint, Vec3::new(16_777_218.0, 0.5, 0.5));
        assert_eq!(ray.distance(midpoint), 2.0);
    }

    #[test]
    fn normalization_divides_by_a_float_length() {
        let direction = normalize_direction([1.0, 1.0, 1.0]).unwrap();
        assert_eq!(direction, [f32::from_bits(0x3f13_cd3a); 3]);
        assert!(normalize_direction([0.0, 0.0, 0.0]).is_none());
        assert!(normalize_direction([0.000_099, 0.0, 0.0]).is_none());
        assert_eq!(
            normalize_direction([0.0001, 0.0, 0.0]),
            Some([1.0, 0.0, 0.0])
        );
    }
}
