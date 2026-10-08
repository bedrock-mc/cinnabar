use bevy::prelude::{Transform, Vec3};

use super::engine::Listener;
use crate::local_player::LocalViewPose;

/// Camera audio follows the rendered pose unless the active preset selects the player.
pub(super) fn camera_listener(
    view: &LocalViewPose,
    camera: Option<&Transform>,
    player: bool,
) -> Listener {
    let (position, rotation) = match camera.filter(|_| !player) {
        Some(camera) => (camera.translation, camera.rotation),
        None => (view.eye_translation(), view.rotation()),
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
        let view = LocalViewPose::default();
        let camera = Transform::from_xyz(10.0, 20.0, 30.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let actual = camera_listener(&view, Some(&camera), false);
        assert_eq!(actual.position, [10.0, 20.0, 30.0]);
        assert!(Vec3::from_array(actual.right).distance(Vec3::NEG_Z) < 1e-6);
        for actual in [
            camera_listener(&view, Some(&camera), true),
            camera_listener(&view, None, false),
        ] {
            assert_eq!(actual.position, view.eye_translation().to_array());
            assert_eq!(actual.right, (view.rotation() * Vec3::X).to_array());
        }
    }
}
