//! Borrows gameplay observations for predicted sound presentation.
use super::AudioEngine;
use crate::{
    local_player::LocalViewPose, movement::PhysicsCollisionRegistries, particles::ParticleInbox,
    runtime::world::ClientWorld, survival_mining::SurvivalMiningRuntime,
};
use bevy::prelude::{Local, MessageReader, Res, ResMut, Time};
use client_presentation::audio::predicted::{ConsumeAudio, LocalBlockCue, MiningAudio};
use client_ui::ui_runtime::UiRuntime;
use std::collections::HashSet;

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
) {
    client_presentation::audio::predicted::drive_block_cues(
        cues,
        time,
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        survival
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::MiningObservation),
        engine,
        mining,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_consume_audio(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    time: Res<Time>,
    ui: Option<Res<UiRuntime>>,
    world: Res<ClientWorld>,
    view: Res<LocalViewPose>,
    engine: ResMut<AudioEngine>,
    state: Local<ConsumeAudio>,
) {
    client_presentation::audio::predicted::drive_consume_audio(
        &player_runtime,
        time,
        ui.as_deref(),
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        view,
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
    popped: Local<HashSet<u64>>,
) {
    client_presentation::audio::predicted::drive_actor_audio(
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        inbox.as_deref_mut().map(|value| {
            value as &mut dyn client_presentation::observations::ParticleAudioObservation
        }),
        engine,
        popped,
    );
}
