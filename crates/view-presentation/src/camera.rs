//! Portable per-frame camera motion shared by client and streamed views.

use bevy::prelude::{EulerRot, Mat4, Quat, Resource};

pub mod bob;
mod hurt;
pub use bob::{HandSwayState, ViewEffect, WalkBobState, walk_bob_effect};
pub use hurt::{CameraHurtState, LocalHurtEvent};

/// First-person hand motion for the equipment lane, all in view space.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FirstPersonHandMotion {
    pub bob: ViewEffect,
    pub hurt: Mat4,
    pub sway_pitch_radians: f32,
    pub sway_yaw_radians: f32,
    /// World-space eye correction, independent of view bob and the gameplay origin.
    pub eye_height_adjustment: f32,
}

impl Default for FirstPersonHandMotion {
    fn default() -> Self {
        Self {
            bob: ViewEffect::NONE,
            hurt: Mat4::IDENTITY,
            sway_pitch_radians: 0.0,
            sway_yaw_radians: 0.0,
            eye_height_adjustment: 0.0,
        }
    }
}

impl FirstPersonHandMotion {
    #[must_use]
    pub fn view_matrix(&self) -> Mat4 { self.hurt * self.bob.matrix() }

    #[must_use]
    pub fn matrix(&self) -> Mat4 {
        self.view_matrix()
            * Mat4::from_rotation_x(self.sway_pitch_radians)
            * Mat4::from_rotation_y(self.sway_yaw_radians)
    }
}

/// Converts the full-window FOV to vertical radians; the projection handles pixel aspect.
#[must_use]
pub fn projection_fov_radians(fov_degrees: f32) -> f32 {
    let degrees = if fov_degrees.is_finite() { fov_degrees } else { ui::DEFAULT_FOV_DEGREES as f32 };
    degrees.to_radians().clamp(std::f32::consts::PI / 180.0, std::f32::consts::PI - std::f32::consts::PI / 180.0)
}

/// Converts Bedrock yaw and pitch into the render camera axes.
#[must_use]
pub fn bedrock_camera_rotation(yaw_degrees: f32, pitch_degrees: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, (180.0 - yaw_degrees).to_radians(), -pitch_degrees.to_radians(), 0.0)
}

/// Finite projection aspect, including a minimized window.
#[must_use]
pub fn projection_aspect(width: f32, height: f32) -> f32 {
    let aspect = width / height;
    if aspect.is_finite() && aspect > 0.0 { aspect } else { 16.0 / 9.0 }
}
