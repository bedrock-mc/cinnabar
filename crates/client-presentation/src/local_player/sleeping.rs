//! Resolves the shared local sleeping presentation before camera and actor publication.

use super::*;

/// Resolved bed placement and level view shared by presentation consumers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SleepingPose {
    pub eye: Vec3,
    pub feet: Vec3,
    pub rotation: Quat,
}

impl LocalViewPose {
    /// Samples loaded beds and publishes one sleeping pose without changing physics or input look.
    pub fn resolve_presentation(
        &mut self,
        mut stream: Option<&mut chunk_pipeline::WorldStream>,
        collisions: &dyn crate::observations::CollisionLookup,
        perspective: PerspectiveMode,
        predicted_feet: Option<[f32; 3]>,
    ) {
        let sleeping = stream.as_deref_mut().and_then(|stream| {
            crate::actor_sampling::sample_actor_bed_rotations(stream, collisions);
            let (bed, rotation) = stream.authority().local_sleeping_bed()?;
            let (eye, feet) = sleeping_subject_position(bed);
            Some(SleepingPose {
                eye,
                feet,
                rotation: sleeping_view_rotation(rotation),
            })
        });
        self.set_sleeping_presentation(sleeping, perspective == PerspectiveMode::FirstPerson);
        if let Some(stream) = stream
            && (self.sleeping.is_some() || self.sleeping_changed)
        {
            let (_, feet) = self.actor_positions(None, predicted_feet);
            let feet = feet.unwrap_or_else(|| {
                let mut feet = stream.resolved_server_position().position;
                feet[1] -= protocol::PLAYER_NETWORK_OFFSET;
                feet
            });
            stream.set_local_actor_presentation_position(feet, self.sleeping_changed);
        }
    }

    /// Overrides actor positions only while sleeping, preserving awake predictions and fallbacks.
    pub fn actor_positions(
        &self,
        predicted_eye: Option<[f32; 3]>,
        predicted_feet: Option<[f32; 3]>,
    ) -> (Option<[f32; 3]>, Option<[f32; 3]>) {
        self.sleeping
            .map_or((predicted_eye, predicted_feet), |pose| {
                (Some(pose.eye.to_array()), Some(pose.feet.to_array()))
            })
    }

    /// Reanchors on presentation transitions and keeps stable sleeping frames on the same history.
    pub(super) fn set_sleeping_presentation(
        &mut self,
        sleeping: Option<SleepingPose>,
        first_person: bool,
    ) {
        self.sleeping_changed = self.sleeping != sleeping;
        let sleeping_camera = sleeping.is_some() && first_person;
        if self.sleeping_changed || self.sleeping_camera != sleeping_camera {
            self.reanchor_camera();
        }
        self.sleeping = sleeping;
        self.sleeping_camera = sleeping_camera;
    }

    /// Gives the retained actor and its attachments the same body position as the shared view.
    pub fn apply_actor_presentation(&self, feed: &mut client_world::LocalPlayerFeed) {
        if let Some(pose) = self.sleeping {
            feed.position = pose.feet.to_array();
        }
    }
}
