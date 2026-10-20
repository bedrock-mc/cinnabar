//! Move-only geometry state retained while an attachable's readiness draw is provisional.
use super::super::*;

pub(crate) struct GeometryCheckpoint {
    rig: EntityRigId,
    sampling: [bool; 3],
    presentation_expressions: evaluation::PresentationReads,
    motion_sampling: Option<Arc<render_frame::motion_sampling::MotionSampling>>,
    binding: usize,
    bones: Vec<RuntimeBone>,
    composition: Arc<pose::Composition>,
    names: Vec<Box<str>>,
    controllers: Vec<ControllerState>,
    ui_pose: Option<Vec<BoneTransform>>,
    ui_animation: Option<hud::UiAnimationState>,
    previous: Vec<BoneTransform>,
    rest: Vec<BoneTransform>,
    current: Vec<BoneTransform>,
    reset: [bool; 2],
}

impl GeometryCheckpoint {
    /// Moves only the fields geometry selection replaces, without copying the rig or catalog.
    pub(super) fn capture(state: &mut ActorRigState) -> Self {
        Self {
            rig: state.rig,
            sampling: [
                state.samples_camera_poses,
                state.samples_swing_poses,
                state.samples_render_frames,
            ],
            presentation_expressions: state.presentation_expressions,
            motion_sampling: state.motion_sampling.take(),
            binding: state.geometry_binding,
            bones: std::mem::take(&mut state.bones),
            composition: Arc::clone(&state.composition),
            names: std::mem::take(&mut state.bone_names),
            controllers: std::mem::take(&mut state.controllers),
            ui_pose: state.ui_pose.take(),
            ui_animation: state.ui_animation.take(),
            previous: std::mem::take(&mut state.previous),
            rest: std::mem::take(&mut state.rest),
            current: std::mem::take(&mut state.current),
            reset: [state.reset_pending, state.rest_reset_pending],
        }
    }

    /// Restores the original buffers and reset flags when the provisional selection is discarded.
    pub(crate) fn restore(self, state: &mut ActorRigState) {
        state.rig = self.rig;
        [
            state.samples_camera_poses,
            state.samples_swing_poses,
            state.samples_render_frames,
        ] = self.sampling;
        state.presentation_expressions = self.presentation_expressions;
        state.motion_sampling = self.motion_sampling;
        state.geometry_binding = self.binding;
        state.bones = self.bones;
        state.composition = self.composition;
        state.bone_names = self.names;
        state.controllers = self.controllers;
        state.ui_pose = self.ui_pose;
        state.ui_animation = self.ui_animation;
        state.previous = self.previous;
        state.rest = self.rest;
        state.current = self.current;
        [state.reset_pending, state.rest_reset_pending] = self.reset;
    }
}
