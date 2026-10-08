//! Independent simplex fields for the three camera-shake axes.

use std::hash::BuildHasher;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Noise {
    permutations: [[u8; 256]; 3],
}

impl Default for Noise {
    /// Seeds independent permutations once when a shake component is created.
    fn default() -> Self {
        let random = std::collections::hash_map::RandomState::new();
        Self {
            permutations: std::array::from_fn(|axis| {
                let mut permutation = std::array::from_fn(|index| index as u8);
                for index in 0..256 {
                    let choice = index + random.hash_one((axis, index)) as usize % (256 - index);
                    permutation.swap(index, choice);
                }
                permutation
            }),
        }
    }
}

impl Noise {
    /// Evaluates independent axes at the same two-dimensional noise coordinate.
    pub(super) fn sample(&self, x: f32, y: f32) -> bevy::prelude::Vec3 {
        bevy::prelude::Vec3::from_array(self.permutations.each_ref().map(|p| sample(p, x, y)))
    }
}

/// Uses twelve projected cube-edge gradients and fourth-power corner attenuation.
fn sample(permutation: &[u8; 256], x: f32, y: f32) -> f32 {
    const F2: f32 = 0.36602542;
    const G2: f32 = 0.21132487;
    const GRADIENTS: [[f32; 2]; 12] = [
        [1.0, 1.0],
        [-1.0, 1.0],
        [1.0, -1.0],
        [-1.0, -1.0],
        [1.0, 0.0],
        [-1.0, 0.0],
        [1.0, 0.0],
        [-1.0, 0.0],
        [0.0, 1.0],
        [0.0, -1.0],
        [0.0, 1.0],
        [0.0, -1.0],
    ];
    let skew = (x + y) * F2;
    let i = (x + skew) as i32 - i32::from(x + skew <= 0.0);
    let j = (y + skew) as i32 - i32::from(y + skew <= 0.0);
    let unskew = i.wrapping_add(j) as f32 * G2;
    let x0 = x - (i as f32 - unskew);
    let y0 = y - (j as f32 - unskew);
    let (di, dj) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let corners = [
        (0, 0, x0, y0),
        (di, dj, x0 - di as f32 + G2, y0 - dj as f32 + G2),
        (1, 1, x0 - 1.0 + G2 + G2, y0 - 1.0 + G2 + G2),
    ];
    let mut value = 0.0;
    for (di, dj, dx, dy) in corners {
        let attenuation = 0.5 - dx * dx - dy * dy;
        if attenuation >= 0.0 {
            let j = j.wrapping_add(dj) as usize & 255;
            let hash = permutation
                [(i.wrapping_add(di) as usize).wrapping_add(permutation[j] as usize) & 255];
            let [gx, gy] = GRADIENTS[hash as usize % GRADIENTS.len()];
            value += (dx * gx + dy * gy) * attenuation.powi(4);
        }
    }
    value * 70.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_permutation_golden_samples() {
        let permutation = std::array::from_fn(|i| i as u8);
        assert!(sample(&permutation, 0.0, 0.0).abs() < 1e-6);
        assert!((sample(&permutation, 0.25, 0.0) - 0.6804301).abs() < 1e-5);
        assert!((sample(&permutation, 0.0, 0.25) - 0.6018422).abs() < 1e-5);
    }
}
