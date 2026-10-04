use crate::{
    camera::FlyCamera,
    environment::WorldClock,
    local_player::{CameraPose, LocalPlayerFrameCarrier},
    local_player_camera_receipt::CameraPublicationAttempt,
    runtime::{audio::SequencedAudioEvent, world::ClientWorld},
};
use bevy::prelude::*;
pub use client_presentation::named_audio::{AudioDevice, NamedAudio};

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_live_named_audio(
    messages: MessageReader<SequencedAudioEvent>,
    world: Res<ClientWorld>,
    clock: Res<WorldClock>,
    receipt: Res<CameraPublicationAttempt>,
    frame: Res<LocalPlayerFrameCarrier>,
    camera_pose: Res<CameraPose>,
    physics: Res<crate::movement::LocalPhysicsController>,
    cameras: Query<&Transform, With<FlyCamera>>,
    state: ResMut<NamedAudio>,
    device: Option<NonSendMut<AudioDevice>>,
) {
    client_presentation::named_audio::drain_live_named_audio(
        messages,
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        client_presentation::observations::SessionObservation(clock.session_generation()),
        receipt,
        frame,
        camera_pose,
        &*physics,
        cameras,
        state,
        device,
    );
}

#[cfg(test)]
#[path = "named_audio/composed_tests.rs"]
mod composed_tests;
