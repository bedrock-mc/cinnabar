//! Speed-derived water sound volume shared by local motion and packet routing.

pub(super) const VOLUME_DATA_SCALE: f32 = 16_777_215.0;
pub(super) const SPLASH_SCALE: f32 = 0.2;
pub(super) const SWIM_SCALE: f32 = 0.35;
const HORIZONTAL_WEIGHT: f32 = 0.2;

/// Converts weighted motion in blocks per tick into the clamped, serialized sound volume.
pub(super) fn motion_volume([x, y, z]: [f32; 3], scale: f32) -> f32 {
    let speed = (z * z * HORIZONTAL_WEIGHT + y * y + x * x * HORIZONTAL_WEIGHT).sqrt();
    let data = (speed * scale).min(1.0) * VOLUME_DATA_SCALE;
    encoded_volume(data as i32)
}

/// Decodes the volume already computed by the sound's actor, without applying it twice.
pub(super) fn encoded_volume(data: i32) -> f32 {
    data as f32 / VOLUME_DATA_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_weights_horizontal_speed_and_clamps_fast_impacts() {
        for (speed, expected) in [
            (0.0, 0.0),
            (0.25, 0.05),
            (0.4, 0.08),
            (1.0, 0.2),
            (2.5, 0.5),
            (5.0, 1.0),
            (10.0, 1.0),
        ] {
            let volume = motion_volume([0.0, -speed, 0.0], SPLASH_SCALE);
            assert!((volume - expected).abs() < 1e-6);
            assert_eq!(volume, motion_volume([0.0, speed, 0.0], SPLASH_SCALE));
        }
        let diagonal = motion_volume([3.0, 4.0, 5.0], SPLASH_SCALE);
        assert!((diagonal - 22.8_f32.sqrt() * SPLASH_SCALE).abs() < 1e-6);
        let horizontal = motion_volume([1.0, 0.0, 0.0], SWIM_SCALE);
        assert!((horizontal - 0.156_524_76).abs() < 1e-6);
    }
}
