//! Java's star field (`SkyRenderer.buildStars`): 1500 candidates from `RandomSource.create
//! (10842)`, a `java.util.Random` generator, kept inside the unit ball's shell, pushed out
//! to 100 blocks and turned to face the camera with a random roll. Iris draws it through
//! `gbuffers_skybasic`, so a pack that keeps vanilla stars shows Java's sky, not Bedrock's.

use bevy::math::{Mat3, Vec3};
use std::sync::LazyLock;

const SEED: i64 = 10842;
const CANDIDATES: usize = 1500;
const DISTANCE: f32 = 100.0;

/// `java.util.Random` (Minecraft's `LegacyRandomSource`).
pub(super) struct JavaRandom(u64);

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const MASK: u64 = (1 << 48) - 1;

impl JavaRandom {
    pub(super) fn new(seed: i64) -> Self {
        Self((seed as u64 ^ MULTIPLIER) & MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(MULTIPLIER).wrapping_add(0xB)) & MASK;
        (self.0 >> (48 - bits)) as i32
    }

    #[cfg(test)]
    pub(super) fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    pub(super) fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    pub(super) fn next_double(&mut self) -> f64 {
        let high = i64::from(self.next(26)) << 27;
        (high + i64::from(self.next(27))) as f64 * (1.0 / (1_u64 << 53) as f64)
    }
}

/// The corners of every star, in Java's quad order, before the celestial rotation.
pub(super) static STAR_QUADS: LazyLock<Vec<[Vec3; 4]>> = LazyLock::new(build);

fn build() -> Vec<[Vec3; 4]> {
    let mut random = JavaRandom::new(SEED);
    let mut stars = Vec::with_capacity(CANDIDATES);
    for _ in 0..CANDIDATES {
        let x = random.next_float() * 2.0 - 1.0;
        let y = random.next_float() * 2.0 - 1.0;
        let z = random.next_float() * 2.0 - 1.0;
        let size = 0.15 + random.next_float() * 0.1;
        let length_squared = x * x + y * y + z * z;
        if length_squared <= 0.010_000_001 || length_squared >= 1.0 {
            continue;
        }
        let centre = Vec3::new(x, y, z).normalize() * DISTANCE;
        let roll = (random.next_double() * f64::from(std::f32::consts::PI) * 2.0) as f32;
        // JOML `rotateTowards(-centre, +Y)`: local +Z along the direction, +X = up × it.
        let direction = (-centre).normalize();
        let left = Vec3::Y.cross(direction).normalize();
        let up = direction.cross(left);
        let basis = Mat3::from_cols(left, up, direction) * Mat3::from_rotation_z(-roll);
        stars.push(
            [[size, -size], [size, size], [-size, size], [-size, -size]]
                .map(|[a, b]| basis * Vec3::new(a, b, 0.0) + centre),
        );
    }
    stars
}
