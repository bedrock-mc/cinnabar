//! App schedule and borrowed observations for presentation audio.
use super::{
    AudioEngine,
    predicted::{drive_actor_audio, drive_block_cues, drive_consume_audio},
};
use crate::{
    local_player::LocalViewPose,
    movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    particles::ParticleInbox,
    runtime::world::ClientWorld,
};
use bevy::prelude::{App, IntoScheduleConfigs, Local, MessageReader, Res, ResMut, Time, Update};
use client_presentation::audio::{
    local::LocalMotion,
    systems::{AmbientState, IngestState, UiSoundCue, pump_audio},
};
use client_presentation::audio_ingress::SequencedAudioEvent;
use render::{ParticleSimulation, PrecipitationMix, RuntimeStage, RuntimeStageProfiler};
use std::collections::HashSet;

/// Installs presentation audio in its existing order. Each system times only its own work, so
/// systems the executor runs between them are never attributed to audio.
pub(crate) fn configure(app: &mut App) {
    super::synchronized::configure(app);
    app.add_plugins(client_presentation::audio::AudioPresentationPlugin)
        .add_systems(
            Update,
            (
                ingest_audio_events,
                drive_inventory_audio,
                drive_local_motion,
                drive_ambience,
                drive_weather_and_particles,
                // Local break and place cues play in the frame that predicts them.
                drive_block_cues
                    .after(crate::survival_mining::produce_survival_mining)
                    .after(crate::block_use::produce_block_use),
                drive_consume_audio,
                drive_actor_audio,
                pump_audio.after(crate::app::ClientFrameSet::Camera),
            )
                .chain()
                .after(crate::ui_runtime::drive_world_inventory_keys)
                .after(crate::named_audio::drain_live_named_audio),
        );
}

pub(crate) fn drive_inventory_audio(
    mut player_runtime: ResMut<crate::player_runtime::PlayerRuntime>,
    world: Res<ClientWorld>,
    view: Res<LocalViewPose>,
    engine: ResMut<AudioEngine>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_presentation::audio::inventory::drive_inventory_audio(
        &mut player_runtime,
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        view,
        engine,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ingest_audio_events(
    messages: MessageReader<SequencedAudioEvent>,
    cues: MessageReader<UiSoundCue>,
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    engine: ResMut<AudioEngine>,
    state: Local<IngestState>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_presentation::audio::systems::ingest_audio_events(
        messages,
        cues,
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        engine,
        state,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_local_motion(
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    physics: Res<LocalPhysicsController>,
    engine: ResMut<AudioEngine>,
    motion: Local<LocalMotion>,
    last_tick: Local<Option<u64>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_presentation::audio::systems::drive_local_motion(
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        &*physics,
        engine,
        motion,
        last_tick,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_ambience(
    time: Res<Time>,
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    view: Res<LocalViewPose>,
    player_runtime: Option<Res<crate::player_runtime::PlayerRuntime>>,
    ui: Option<Res<client_ui::ui_runtime::UiRuntime>>,
    engine: ResMut<AudioEngine>,
    state: Local<AmbientState>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_presentation::audio::systems::drive_ambience(
        time,
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        view,
        player_runtime.as_deref().map(|value| &**value),
        ui.as_deref().is_some_and(|ui| ui.credits().owns_input()),
        engine,
        state,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_weather_and_particles(
    time: Res<Time>,
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    mix: Option<Res<PrecipitationMix>>,
    particles: Option<ResMut<ParticleSimulation>>,
    mut inbox: Option<ResMut<ParticleInbox>>,
    engine: ResMut<AudioEngine>,
    seen_bolts: Local<HashSet<i64>>,
    pending_bolts: Local<Vec<(f32, [f32; 3])>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_presentation::audio::systems::drive_weather_and_particles(
        time,
        client_presentation::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        mix,
        particles,
        inbox.as_deref_mut().map(|value| {
            value as &mut dyn client_presentation::observations::ParticleAudioObservation
        }),
        engine,
        seen_bolts,
        pending_bolts,
    );
}
