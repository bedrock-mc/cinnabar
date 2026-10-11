use bevy::prelude::{Quat, Transform, Vec3};

use super::engine::Listener;

/// Server camera listener value that selects the player's eye instead of the camera.
pub const PLAYER_LISTENER: u8 = 1;

/// Camera audio follows the rendered pose unless the active preset selects the player.
pub fn camera_listener(
    eye: Vec3,
    rotation: Quat,
    camera: Option<&Transform>,
    listener: Option<u8>,
) -> Listener {
    let (position, rotation) = match camera.filter(|_| listener != Some(PLAYER_LISTENER)) {
        Some(camera) => (camera.translation, camera.rotation),
        None => (eye, rotation),
    };
    Listener {
        position: position.to_array(),
        right: (rotation * Vec3::X).to_array(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Quat;

    #[test]
    fn camera_listener_obeys_camera_player_and_unavailable_pose() {
        let eye = Vec3::new(0.0, 64.0, 0.0);
        let rotation = Quat::IDENTITY;
        let camera = Transform::from_xyz(10.0, 20.0, 30.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let actual = camera_listener(eye, rotation, Some(&camera), None);
        assert_eq!(actual.position, [10.0, 20.0, 30.0]);
        assert!(Vec3::from_array(actual.right).distance(Vec3::NEG_Z) < 1e-6);
        for actual in [
            camera_listener(eye, rotation, Some(&camera), Some(PLAYER_LISTENER)),
            camera_listener(eye, rotation, None, None),
        ] {
            assert_eq!(actual.position, eye.to_array());
            assert_eq!(actual.right, (rotation * Vec3::X).to_array());
        }
    }
}
