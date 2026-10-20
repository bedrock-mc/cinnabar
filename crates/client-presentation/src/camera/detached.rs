//! Host-owned detached camera presentation, independent of the body's input pose.

use bevy::prelude::*;

/// Local camera motion requested by an admitted extension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DetachedView {
    Orbit {
        distance: f32,
    },
    Fly {
        horizontal_speed: f32,
        vertical_speed: f32,
    },
}

impl DetachedView {
    /// Advances only the camera, using routed controls and host-measured frame time.
    pub fn advance(
        self,
        position: Vec3,
        rotation: Quat,
        movement: [f32; 2],
        vertical: f32,
        seconds: f32,
    ) -> Vec3 {
        let Self::Fly {
            horizontal_speed,
            vertical_speed,
        } = self
        else {
            return position;
        };
        let (yaw, _, _) = rotation.to_euler(EulerRot::YXZ);
        let heading = Quat::from_rotation_y(yaw);
        let horizontal = Vec2::from_array(movement).clamp_length_max(1.0);
        let velocity = (heading * Vec3::X * horizontal.x + heading * Vec3::NEG_Z * horizontal.y)
            * horizontal_speed
            + Vec3::Y * vertical.clamp(-1.0, 1.0) * vertical_speed;
        position + velocity * seconds.clamp(0.0, 0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flying_camera_moves_without_changing_the_body_pose() {
        let mode = DetachedView::Fly {
            horizontal_speed: 8.0,
            vertical_speed: 4.0,
        };
        let body = Vec3::new(10.0, 20.0, 30.0);
        let next = mode.advance(body, Quat::IDENTITY, [0.0, 1.0], 1.0, 0.05);
        assert_eq!(next, Vec3::new(10.0, 20.2, 29.6));
        assert_eq!(body, Vec3::new(10.0, 20.0, 30.0));
        assert_eq!(
            DetachedView::Orbit { distance: 4.0 }.advance(
                body,
                Quat::IDENTITY,
                [1.0, 1.0],
                1.0,
                0.05
            ),
            body
        );
    }
}
