//! Actor interpolation timing and local visibility publication.
use crate::local_player::{LocalAvatarPresentation, LocalAvatarVisibilityCarrier};
use std::time::Duration;
const ACTOR_TICK_NANOS: u128 = client_world::ACTOR_TICK_DURATION.as_nanos();
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorFrameStep {
    pub ticks: u32,
    pub partial_tick: f32,
}

/// Publishes local body visibility from the current subject and view, clearing missing poses.
pub fn publish_local_actor_visibility(
    avatar: &LocalAvatarPresentation,
    perspective: semantic_input::PerspectiveMode,
    camera: Option<&crate::camera::ServerCameraView>,
    authoritative_subject_eye: Option<bevy::prelude::Vec3>,
    authoritative_subject_feet: Option<bevy::prelude::Vec3>,
    rotation: bevy::prelude::Quat,
    carrier: &mut LocalAvatarVisibilityCarrier,
) {
    // LocalViewPose may contain the collision-resolved, boomed camera eye in
    // third person. The body instead follows the live physics/server subject;
    // the frozen interaction frame can legitimately lag both authorities.
    let (Some(subject_eye), Some(subject_feet)) =
        (authoritative_subject_eye, authoritative_subject_feet)
    else {
        carrier.clear();
        return;
    };
    let first_person = perspective == semantic_input::PerspectiveMode::FirstPerson;
    let first_person = camera.map_or(first_person, |camera| {
        camera.renders_first_person(first_person)
    });
    let body_visibility = if first_person {
        semantic_input::PerspectiveMode::FirstPerson
    } else {
        semantic_input::PerspectiveMode::ThirdPersonBack
    };
    avatar.publish_view_visibility(
        body_visibility,
        subject_eye,
        subject_feet,
        rotation,
        carrier,
    );
}

/// Chooses the finite predicted eye, falling back to the resolved server position.
pub fn authoritative_local_actor_eye(
    predicted_eye: Option<[f32; 3]>,
    resolved_server_network_position: Option<[f32; 3]>,
) -> Option<bevy::prelude::Vec3> {
    predicted_eye
        .or(resolved_server_network_position)
        .map(bevy::prelude::Vec3::from_array)
        .filter(|eye| eye.is_finite())
}

#[derive(Debug, Default)]
pub struct ActorFrameClock {
    accumulated_nanos: u128,
}

impl ActorFrameClock {
    /// Advances the actor tick clock while retaining a fractional remainder.
    pub fn advance(&mut self, delta: Duration) -> ActorFrameStep {
        self.accumulated_nanos = self.accumulated_nanos.saturating_add(delta.as_nanos());
        let elapsed_ticks = self.accumulated_nanos / ACTOR_TICK_NANOS;
        self.accumulated_nanos %= ACTOR_TICK_NANOS;
        ActorFrameStep {
            ticks: u32::try_from(elapsed_ticks).unwrap_or(u32::MAX),
            partial_tick: self.accumulated_nanos as f32 / ACTOR_TICK_NANOS as f32,
        }
    }

    /// Clears interpolation time when the actor session changes.
    pub fn reset(&mut self) {
        self.accumulated_nanos = 0;
    }
}
