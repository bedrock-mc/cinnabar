use std::ops::{Add, AddAssign, Index, IndexMut, Mul, Sub};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Simulation vector with f32 arithmetic and lossless f64 transport fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);

    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    pub fn length_squared(self) -> f64 {
        let [x, y, z] = [self.x as f32, self.y as f32, self.z as f32];
        f64::from(x * x + y * y + z * z)
    }

    #[must_use]
    pub fn horizontal_length_squared(self) -> f64 {
        let [x, z] = [self.x as f32, self.z as f32];
        f64::from(x * x + z * z)
    }

    /// Rounds imported coordinates or motion to the native simulation precision.
    pub(crate) const fn rounded(self) -> Self {
        Self::new(
            self.x as f32 as f64,
            self.y as f32 as f64,
            self.z as f32 as f64,
        )
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    #[must_use]
    pub fn component_min(self, rhs: Self) -> Self {
        Self::new(self.x.min(rhs.x), self.y.min(rhs.y), self.z.min(rhs.z))
    }

    #[must_use]
    pub fn component_max(self, rhs: Self) -> Self {
        Self::new(self.x.max(rhs.x), self.y.max(rhs.y), self.z.max(rhs.z))
    }
}

const TRIG_INDEX_SCALE: f32 = 10_430.378;

/// Samples the native float table, including float division during initialization.
fn sine_table(index: i32) -> f64 {
    // The divisor is also the lookup's index multiplier.
    static TABLE: OnceLock<Box<[f32]>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..=u16::MAX)
            .map(|i| (f32::from(i) / TRIG_INDEX_SCALE).sin())
            .collect()
    });
    f64::from(table[usize::from(index as u16)])
}

/// Looks up sine after the native float multiply and truncation toward zero.
pub(crate) fn minecraft_sin(value: f64) -> f64 {
    sine_table((value as f32 * TRIG_INDEX_SCALE) as i32)
}

/// Applies the quarter-turn offset before truncating the native float index.
pub(crate) fn minecraft_cos(value: f64) -> f64 {
    sine_table((value as f32 * TRIG_INDEX_SCALE + 16_384.0) as i32)
}

/// Native look vector used by the swimming trigger (current RVA 0x09fd25a0).
#[must_use]
pub fn view_direction(pitch_degrees: f32, yaw_degrees: f32) -> Vec3 {
    let pitch = -pitch_degrees.to_radians();
    let yaw = -yaw_degrees.to_radians() - std::f32::consts::PI;
    let horizontal = -(minecraft_cos(f64::from(pitch)) as f32);
    Vec3::new(
        f64::from(minecraft_sin(f64::from(yaw)) as f32 * horizontal),
        minecraft_sin(f64::from(pitch)),
        f64::from(minecraft_cos(f64::from(yaw)) as f32 * horizontal),
    )
}

impl Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(
            f64::from(self.x as f32 + rhs.x as f32),
            f64::from(self.y as f32 + rhs.y as f32),
            f64::from(self.z as f32 + rhs.z as f32),
        )
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(
            f64::from(self.x as f32 - rhs.x as f32),
            f64::from(self.y as f32 - rhs.y as f32),
            f64::from(self.z as f32 - rhs.z as f32),
        )
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self::Output {
        Self::new(
            f64::from(self.x as f32 * rhs as f32),
            f64::from(self.y as f32 * rhs as f32),
            f64::from(self.z as f32 * rhs as f32),
        )
    }
}

impl Index<usize> for Vec3 {
    type Output = f64;

    fn index(&self, index: usize) -> &Self::Output {
        match index {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Vec3 index {index} is out of range"),
        }
    }
}

impl IndexMut<usize> for Vec3 {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            _ => panic!("Vec3 index {index} is out of range"),
        }
    }
}
