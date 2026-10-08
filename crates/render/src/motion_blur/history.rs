use super::CameraMotionBlur;
use bevy::prelude::*;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct ExposureUniform {
    pub previous_clip_from_clip: Mat4,
    pub strength: Vec4,
    pub viewport: Vec4,
}

#[derive(Clone, Copy)]
pub(super) struct CameraHistory {
    clip_from_world: Mat4,
    world_from_view: Mat4,
    viewport: UVec4,
    reset_epoch: u64,
}

impl CameraHistory {
    pub fn viewport(&self) -> UVec4 {
        self.viewport
    }

    pub fn new(
        clip_from_world: Mat4,
        world_from_view: Mat4,
        viewport: UVec4,
        reset_epoch: u64,
    ) -> Self {
        Self {
            clip_from_world,
            world_from_view,
            viewport,
            reset_epoch,
        }
    }

    /// Advance even on rejected frames so the next exposure starts at the new anchor.
    pub fn advance(&mut self, current: Self, settings: CameraMotionBlur) -> ExposureUniform {
        let old = std::mem::replace(self, current);
        let dt = settings.delta_seconds;
        let translation = current.world_from_view.w_axis - old.world_from_view.w_axis;
        let forward_dot = current
            .world_from_view
            .z_axis
            .dot(old.world_from_view.z_axis);
        let admitted = old.reset_epoch == current.reset_epoch
            && old.viewport == current.viewport
            && translation.length_squared() < 64.0 * 64.0
            && forward_dot > 0.0
            && dt.is_finite()
            && dt > 0.000001
            && dt <= 0.25
            && current.clip_from_world.is_finite()
            && old.clip_from_world.is_finite()
            && current.clip_from_world.determinant().abs() > f32::EPSILON
            && settings.exposure_seconds.is_finite()
            && settings.exposure_seconds > 0.0
            && settings.samples >= 3
            && old.world_from_view != current.world_from_view;
        let reprojection = old.clip_from_world * current.clip_from_world.inverse();
        let strength = if admitted && reprojection.is_finite() {
            settings.exposure_seconds / dt
        } else {
            0.0
        };
        ExposureUniform {
            previous_clip_from_clip: if strength > 0.0 {
                reprojection
            } else {
                Mat4::IDENTITY
            },
            strength: Vec4::new(strength, settings.samples.clamp(3, 32) as f32, 0.0, 0.0),
            viewport: current.viewport.as_vec4(),
        }
    }
}
