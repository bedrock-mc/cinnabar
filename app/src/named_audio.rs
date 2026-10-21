use bevy::prelude::*;
use client_audio::audio_ingress::SequencedAudioEvent;
use client_audio::named_audio::{AudioDevice, NamedAudio};
use client_presentation::local_player_camera_receipt::CameraPublicationAttempt;
use {
    crate::runtime::world::ClientWorld,
    client_presentation::environment::WorldClock,
    client_presentation::{
        camera::FlyCamera,
        local_player::{CameraPose, LocalPlayerFrameCarrier},
    },
};

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_live_named_audio(
    messages: MessageReader<SequencedAudioEvent>,
    world: Res<ClientWorld>,
    clock: Res<WorldClock>,
    receipt: Res<CameraPublicationAttempt>,
    frame: Res<LocalPlayerFrameCarrier>,
    camera_pose: Res<CameraPose>,
    physics: Res<client_interaction::movement::LocalPhysicsController>,
    cameras: Query<&Transform, With<FlyCamera>>,
    state: ResMut<NamedAudio>,
    device: Option<NonSendMut<AudioDevice>>,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::Audio));
    let camera = client_presentation::local_player_camera_receipt::published_default_camera(
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        simulation_clock::identity::SessionGeneration::new(clock.session_generation()),
        &receipt,
        &frame,
        &camera_pose,
        &*physics,
        &cameras,
    );
    client_audio::named_audio::drain_live_named_audio(
        messages,
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        client_audio::observations::SessionObservation(clock.session_generation()),
        camera,
        state,
        device,
    );
}

#[cfg(test)]
#[path = "named_audio/composed_tests.rs"]
mod composed_tests;
