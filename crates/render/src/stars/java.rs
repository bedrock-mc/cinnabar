//! Seeded geometry for the optional world sky.

use bevy::math::Vec3;

/// Generates deterministic square stars without changing the default client's random stream.
pub(crate) fn vertices() -> Vec<[f32; 4]> {
    let mut random = SkyRandom::new(10842);
    let mut vertices = Vec::with_capacity(1500 * 6);
    for _ in 0..1500 {
        let candidate = Vec3::from_array(std::array::from_fn(|_| random.float() * 2.0 - 1.0));
        let size = 0.15 + random.float() * 0.1;
        let squared = candidate.length_squared();
        if squared <= 0.010_000_001 || squared >= 1.0 {
            continue;
        }
        let direction = candidate / squared.sqrt();
        let center = direction * 100.0;
        let forward = -direction;
        let right = Vec3::Y.cross(forward).normalize();
        let up = forward.cross(right);
        let rotation = (random.double() * f64::from(std::f32::consts::PI) * 2.0) as f32;
        let (sin, cos) = (-rotation).sin_cos();
        let corners = [[size, -size], [size, size], [-size, size], [-size, -size]].map(|[x, y]| {
            let point = center + right * (cos * x - sin * y) + up * (sin * x + cos * y);
            [point.x, point.y, point.z, 1.0]
        });
        vertices.extend([0, 1, 2, 0, 2, 3].map(|corner| corners[corner]));
    }
    vertices
}

/// A private 48-bit generator keeps this fixed sky independent of all gameplay randomness.
struct SkyRandom(u64);

impl SkyRandom {
    /// Scrambles a seed into the specified 48-bit state.
    fn new(seed: u64) -> Self {
        Self((seed ^ 0x5deece66d) & ((1 << 48) - 1))
    }

    /// Advances once and returns the requested high bits.
    fn bits(&mut self, count: u32) -> u32 {
        self.0 = self.0.wrapping_mul(0x5deece66d).wrapping_add(11) & ((1 << 48) - 1);
        (self.0 >> (48 - count)) as u32
    }

    /// Returns a uniformly spaced 24-bit fraction.
    fn float(&mut self) -> f32 {
        self.bits(24) as f32 / (1 << 24) as f32
    }

    /// Combines two draws into the required 53-bit fraction.
    fn double(&mut self) -> f64 {
        f64::from(self.bits(26)) * (1_u64 << 27) as f64 / (1_u64 << 53) as f64
            + f64::from(self.bits(27)) / (1_u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_have_constant_alpha_and_tangent_square_geometry() {
        let vertices = vertices();
        assert_eq!(vertices, super::vertices());
        assert!(!vertices.is_empty());
        for quad in vertices.as_chunks::<6>().0 {
            let a = Vec3::from_slice(&quad[0][..3]);
            let b = Vec3::from_slice(&quad[1][..3]);
            let c = Vec3::from_slice(&quad[2][..3]);
            let center = (a + c) * 0.5;
            assert!((center.length() - 100.0).abs() < 0.0001);
            assert!((a.distance(b) - b.distance(c)).abs() < 0.0001);
            assert!((a - center).dot(center).abs() < 0.005);
            assert!(quad.iter().all(|vertex| vertex[3] == 1.0));
        }
    }
}
