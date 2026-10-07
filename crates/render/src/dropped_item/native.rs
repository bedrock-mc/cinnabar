//! Dropped items as 1.26.50.26 renders them. Item-local
//! geometry already contains the default display correction; copies translate before actor scale.

use super::dropped_item_transform;

const TIME_RATE: f32 = 0.05;
const BOB_AMPLITUDE: f32 = 0.1;
const BLOCK_LIFT: f32 = 0.2;
const CLOSE_SPAWN_DURATION: f32 = 0.4;
const CLOSE_SPAWN_YAW_DEGREES: f32 = 80.0;
const RADIANS_TO_DEGREES: f32 = 57.295776;
const DEGREES_TO_RADIANS: f32 = 0.017453292;
const SPRITE_GROUP_SCALE: f32 = 0.3;
const SPRITE_DEFAULT_SCALE: f32 = 1.5;
const CUBE_SCALE: f32 = 0.25;

/// Ordinary model routes admitted by this dropped-item renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DroppedItemShape {
    Sprite,
    Cube,
}

/// Native per-item first-render capture. Walking closer later never restarts the animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DroppedItemSpawnPose {
    close_yaw_degrees: Option<f32>,
}

impl DroppedItemSpawnPose {
    #[must_use]
    pub fn new(position: [f32; 3], camera: Option<([f32; 3], f32)>) -> Self {
        let close_yaw_degrees = camera.and_then(|(eye, yaw)| {
            let [x, y, z] = std::array::from_fn(|axis| position[axis] - eye[axis]);
            let squared_distance = z * z + y * y + x * x;
            (squared_distance < 1.0 && yaw.is_finite()).then_some(CLOSE_SPAWN_YAW_DEGREES - yaw)
        });
        Self { close_yaw_degrees }
    }

    /// Vanilla's render Y offset table-index truncation and cubic ease-in are retained. Computing
    /// just the addressed sine sample avoids retaining a 65536-float table per renderer.
    #[must_use]
    pub fn bob(self, age_ticks: f32, phase: f32, shape: DroppedItemShape) -> f32 {
        let time = age_ticks * TIME_RATE;
        let angle = time + time;
        let (phase, rise) = if self.close_yaw_degrees.is_some() {
            let fraction = (time / CLOSE_SPAWN_DURATION).clamp(0.0, 1.0);
            let rise = CLOSE_SPAWN_DURATION
                + (0.0 - CLOSE_SPAWN_DURATION) * fraction * fraction * fraction;
            (angle.clamp(0.0, 1.0) * phase, rise)
        } else {
            (phase, 0.0)
        };
        let sine = crate::native_trig::sine(angle + phase);
        let lift = if shape == DroppedItemShape::Cube {
            BLOCK_LIFT
        } else {
            0.0
        };
        lift + sine * BOB_AMPLITUDE + BOB_AMPLITUDE + rise
    }

    #[must_use]
    pub fn yaw(self, spin_radians: f32) -> f32 {
        (spin_radians * RADIANS_TO_DEGREES + self.close_yaw_degrees.unwrap_or(0.0))
            * DEGREES_TO_RADIANS
    }
}

/// `T(origin) Ry(yaw) S(group) T(copy/group) S(actor) defaultItem`. Copy jitter
/// rotates with yaw, but neither actor nor default-item scale stretches the spread.
#[must_use]
pub fn native_dropped_item_transform(
    origin: [f32; 3],
    yaw: f32,
    copy_offset: [f32; 3],
    actor_scale: f32,
    shape: DroppedItemShape,
) -> [[f32; 4]; 3] {
    let (sine, cosine) = yaw.sin_cos();
    let [x, y, z] = copy_offset;
    let center = [
        origin[0] + cosine * x + sine * z,
        origin[1] + y,
        origin[2] - sine * x + cosine * z,
    ];
    let scale = actor_scale
        * match shape {
            DroppedItemShape::Sprite => SPRITE_GROUP_SCALE * SPRITE_DEFAULT_SCALE,
            DroppedItemShape::Cube => CUBE_SCALE,
        };
    dropped_item_transform(center, yaw, scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_bob_has_no_provisional_center_lift_and_uses_sine_index_truncation() {
        let pose = DroppedItemSpawnPose::new([0.0; 3], None);
        assert_eq!(pose.bob(0.0, 0.0, DroppedItemShape::Sprite), BOB_AMPLITUDE);
        assert_eq!(
            pose.bob(0.0, 0.0, DroppedItemShape::Cube),
            BLOCK_LIFT + BOB_AMPLITUDE
        );
        let phase = 0.23456;
        let indexed = crate::native_trig::sine(phase);
        assert_eq!(
            pose.bob(0.0, phase, DroppedItemShape::Sprite),
            indexed * BOB_AMPLITUDE + BOB_AMPLITUDE
        );
        assert_eq!(pose.yaw(0.0), 0.0);
    }

    #[test]
    fn close_spawn_captures_camera_once_and_eases_for_eight_ticks() {
        let close = DroppedItemSpawnPose::new([0.0; 3], Some(([0.5, 0.0, 0.0], 30.0)));
        let far = DroppedItemSpawnPose::new([0.0; 3], Some(([1.0, 0.0, 0.0], 30.0)));
        assert_eq!(
            close.bob(0.0, 2.0, DroppedItemShape::Sprite),
            BOB_AMPLITUDE + CLOSE_SPAWN_DURATION
        );
        assert_eq!(far.close_yaw_degrees, None);
        assert!((close.yaw(0.0) - 50.0 * DEGREES_TO_RADIANS).abs() < 1e-7);
        let time = 4.0 * TIME_RATE;
        let phase = 0.4 * 2.0;
        let sine = crate::native_trig::sine(time + time + phase);
        assert!(
            (close.bob(4.0, 2.0, DroppedItemShape::Sprite)
                - (sine * BOB_AMPLITUDE + BOB_AMPLITUDE + 0.35))
                .abs()
                < 1e-6
        );
        assert_eq!(
            close.bob(10.0, 2.0, DroppedItemShape::Sprite),
            far.bob(10.0, 2.0, DroppedItemShape::Sprite)
        );
    }

    #[test]
    fn copy_spread_rotates_before_actor_scale_and_sprite_is_floor_origin() {
        let yaw = std::f32::consts::FRAC_PI_2;
        let rows = native_dropped_item_transform(
            [1.0, 2.0, 3.0],
            yaw,
            [0.2, 0.1, 0.0],
            2.0,
            DroppedItemShape::Sprite,
        );
        assert!((rows[0][3] - 1.0).abs() < 1e-6);
        assert!((rows[1][3] - 2.1).abs() < 1e-6);
        assert!((rows[2][3] - 2.8).abs() < 1e-6);
        assert!((rows[1][1] - SPRITE_GROUP_SCALE * SPRITE_DEFAULT_SCALE * 2.0).abs() < 1e-6);
        let cube =
            native_dropped_item_transform([0.0; 3], 0.0, [0.0; 3], 1.0, DroppedItemShape::Cube);
        assert_eq!(cube, dropped_item_transform([0.0; 3], 0.0, CUBE_SCALE));
    }
}
