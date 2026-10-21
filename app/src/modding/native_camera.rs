//! Applies generic packet-camera output through the same presentation owners as local mods.

use client_presentation::{
    camera::{CameraSettingsAuthority, DetachedView},
    local_player::LocalViewPose,
};
use mod_api::native_camera::{DetachedView as WireView, Effects};

/// Omitting output releases detached input and restores the body's current camera view.
pub(super) fn apply(
    camera: &mut CameraSettingsAuthority,
    view: Option<&mut LocalViewPose>,
    user_freelook: bool,
    effects: Option<&Effects>,
) {
    let detached = effects
        .and_then(|effects| effects.detached)
        .map(|mode| match mode {
            WireView::Orbit { distance } => DetachedView::Orbit { distance },
            WireView::Fly {
                horizontal_speed,
                vertical_speed,
            } => DetachedView::Fly {
                horizontal_speed,
                vertical_speed,
            },
        });
    if let Some(view) = view {
        camera.apply_detached_view(view, detached, user_freelook);
    } else {
        camera.set_detached_view(detached);
    }
    if let Some(effects) = effects {
        if effects.preserve_teleport_rotation {
            camera.set_preserve_teleport_rotation(true);
        }
        if let Some(intensity) = effects.hurt_intensity {
            camera.set_hurt_camera_style(render_api::HurtCameraStyle::new(intensity, false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_position_is_retained_then_released_without_moving_the_body() {
        let mut camera = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let body = view.eye_translation();
        let effect = Effects {
            detached: Some(WireView::Fly {
                horizontal_speed: 8.0,
                vertical_speed: 4.0,
            }),
            ..Default::default()
        };
        apply(&mut camera, Some(&mut view), false, Some(&effect));
        let moved = body + bevy::prelude::Vec3::X * 10.0;
        view.move_detached_eye(moved);
        apply(&mut camera, Some(&mut view), false, Some(&effect));
        assert_eq!(view.detached_eye(), moved);
        assert_eq!(view.eye_translation(), body);
        apply(&mut camera, Some(&mut view), false, None);
        assert_eq!(view.detached_eye(), body);
        assert!(camera.detached_view().is_none());
    }

    #[test]
    fn absent_native_effect_does_not_release_the_players_own_freelook() {
        let mut camera = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        view.set_freelook(true);
        let rotation = bevy::prelude::Quat::from_rotation_y(0.5);
        view.set_look_rotation(rotation);
        apply(&mut camera, Some(&mut view), false, None);
        assert_eq!(view.camera_rotation(), rotation);
    }
}
