//! Borrows gameplay observations for predicted sound presentation.
use bevy::prelude::{Local, MessageReader, Res, ResMut, Time};
use client_audio::audio::AudioEngine;
use client_audio::audio::predicted::{ConsumeAudio, LocalBlockCue, MiningAudio};
use client_ui::ui_runtime::UiRuntime;
use {
    crate::{particles::ParticleInbox, runtime::world::ClientWorld},
    client_interaction::{
        movement::PhysicsCollisionRegistries, survival_mining::SurvivalMiningRuntime,
    },
    client_presentation::local_player::LocalViewPose,
};

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_block_cues(
    cues: MessageReader<LocalBlockCue>,
    time: Res<Time>,
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    survival: Option<Res<SurvivalMiningRuntime>>,
    engine: ResMut<AudioEngine>,
    mining: Local<MiningAudio>,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::Audio));
    client_audio::audio::predicted::drive_block_cues(
        cues,
        time,
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_audio::observations::CollisionLookup),
        survival
            .as_deref()
            .map(|value| value as &dyn client_audio::observations::MiningObservation),
        engine,
        mining,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_consume_audio(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    time: Res<Time>,
    ui: Option<Res<UiRuntime>>,
    world: Res<ClientWorld>,
    view: Res<LocalViewPose>,
    engine: ResMut<AudioEngine>,
    state: Local<ConsumeAudio>,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::Audio));
    client_audio::audio::predicted::drive_consume_audio(
        &player_runtime,
        time,
        ui.as_deref(),
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        view.eye_translation(),
        engine,
        state,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_actor_audio(
    world: Res<ClientWorld>,
    mut inbox: Option<ResMut<ParticleInbox>>,
    engine: ResMut<AudioEngine>,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::Audio));
    client_audio::audio::predicted::drive_actor_audio(
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        inbox
            .as_deref_mut()
            .map(|value| value as &mut dyn client_audio::observations::ParticleAudioObservation),
        engine,
    );
}
