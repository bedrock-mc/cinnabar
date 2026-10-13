use bevy::prelude::*;
use render_scene::CameraMotionBlur;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct ExposureUniform {
    pub previous_clip_from_clip: Mat4,
    pub strength: Vec4,
    pub viewport: Vec4,
}

#[derive(Clone, Copy)]
pub(crate) struct CameraHistory {
    clip_from_view: Mat4,
    world_from_view: Mat4,
    viewport: UVec4,
    reset_epoch: u64,
}

impl CameraHistory {
    /// Computes the camera inverse once for all actor placements in this view.
    pub fn actor_projection(&self) -> ActorProjection {
        ActorProjection(self.clip_from_view.as_dmat4() * self.world_from_view.as_dmat4().inverse())
    }

    pub fn viewport(&self) -> UVec4 {
        self.viewport
    }

    pub fn new(
        clip_from_view: Mat4,
        world_from_view: Mat4,
        viewport: UVec4,
        reset_epoch: u64,
    ) -> Self {
        Self {
            clip_from_view,
            world_from_view,
            viewport,
            reset_epoch,
        }
    }

    /// Advance even on rejected frames so the next exposure starts at the new anchor.
    pub(super) fn advance(&mut self, current: Self, settings: CameraMotionBlur) -> ExposureUniform {
        self.advance_with_actor_motion(current, settings, false)
    }

    /// Whether the preceding rendered frame can contribute camera and actor exposure.
    pub fn continuous(&self, current: Self, settings: CameraMotionBlur) -> bool {
        let dt = settings.delta_seconds;
        let translation = current.world_from_view.w_axis - self.world_from_view.w_axis;
        let forward_dot = current
            .world_from_view
            .z_axis
            .dot(self.world_from_view.z_axis);
        self.reset_epoch == current.reset_epoch
            && self.viewport == current.viewport
            && translation.length_squared() < 64.0 * 64.0
            && forward_dot > 0.0
            && dt.is_finite()
            && dt > 0.000001
            && dt <= 0.25
            && current.clip_from_view.is_finite()
            && self.clip_from_view.is_finite()
            && current.world_from_view.is_finite()
            && self.world_from_view.is_finite()
            && current.clip_from_view.determinant().abs() > f32::EPSILON
            && settings.exposure_seconds.is_finite()
            && settings.exposure_seconds > 0.0
            && settings.samples >= 3
    }

    /// Object movement can require exposure even when the camera itself stays still.
    pub(super) fn advance_with_actor_motion(
        &mut self,
        current: Self,
        settings: CameraMotionBlur,
        actor_moved: bool,
    ) -> ExposureUniform {
        let admitted = self.continuous(current, settings)
            && (actor_moved || self.world_from_view != current.world_from_view);
        let old = std::mem::replace(self, current);
        let translation = current.world_from_view.w_axis - old.world_from_view.w_axis;
        // Subtract camera origins before transforming to avoid distant-world cancellation.
        let mut previous_pose = old.world_from_view;
        previous_pose.w_axis = Vec4::W;
        let mut current_pose = current.world_from_view;
        current_pose.w_axis = translation.truncate().extend(1.0);
        let reprojection = old.clip_from_view
            * previous_pose.inverse()
            * current_pose
            * current.clip_from_view.inverse();
        let strength = if admitted && reprojection.is_finite() {
            settings.exposure_seconds / settings.delta_seconds
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

/// Double precision retains camera-relative placement when world coordinates are distant.
pub(crate) struct ActorProjection(glam::DMat4);

impl ActorProjection {
    /// Projects an actor root without doing another camera inverse for each instance.
    pub fn clip_from_actor(&self, rows: [[f32; 4]; 3]) -> Mat4 {
        let actor = Mat4::from_cols_array_2d(&[
            [rows[0][0], rows[1][0], rows[2][0], 0.0],
            [rows[0][1], rows[1][1], rows[2][1], 0.0],
            [rows[0][2], rows[1][2], rows[2][2], 0.0],
            [rows[0][3], rows[1][3], rows[2][3], 1.0],
        ]);
        (self.0 * actor.as_dmat4()).as_mat4()
    }
}
