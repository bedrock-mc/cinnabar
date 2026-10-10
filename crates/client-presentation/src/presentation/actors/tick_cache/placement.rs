use super::PoseConversions;
use crate::presentation::actors::{ActorRigPresentation, sampled_rig_placement};
use client_world::{ActorRigSnapshot, ActorSnapshot};

impl PoseConversions {
    /// Replaces the cached world-body pose and scale at the already sampled render feet.
    pub fn apply_pose(
        &mut self,
        presentation: &mut ActorRigPresentation,
        rig: &ActorRigSnapshot<'_>,
        actor: &ActorSnapshot,
        partial_tick: f32,
    ) -> bool {
        let placed = ActorRigSnapshot {
            previous_body_yaw: presentation.world_yaw_degrees,
            body_yaw: presentation.world_yaw_degrees,
            ..*rig
        };
        let Some((mut rows, _)) = sampled_rig_placement(
            &placed,
            actor,
            partial_tick,
            [
                rig.scale,
                rig.axis_scale[0],
                rig.axis_scale[1],
                rig.axis_scale[2],
            ],
        ) else {
            return false;
        };
        for (row, sampled) in rows
            .iter_mut()
            .zip(presentation.submission.world_from_actor)
        {
            row[3] = sampled[3];
        }
        let Some((previous, current)) = self.convert(rig) else {
            return false;
        };
        if current.len() != presentation.submission.input.current_bones.len() {
            return false;
        }
        presentation.submission.world_from_actor = rows;
        presentation.authored_scale = rig.scale;
        presentation.submission.input.previous_bones = previous;
        presentation.submission.input.current_bones = current;
        presentation.submission.input.completed_tick = rig.completed_tick;
        presentation.submission.input.reset_generation = rig.reset_generation;
        true
    }
}
