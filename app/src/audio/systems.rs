//! App schedule and borrowed observations for presentation audio.
use bevy::prelude::IntoScheduleConfigs;
use bevy::prelude::{App, Local, MessageReader, Res, ResMut, Time, Update};
use client_audio::audio::{
    AudioSettings,
    local::LocalMotion,
    systems::{AmbientState, IngestState, UiSoundCue},
};
use client_audio::audio_ingress::SequencedAudioEvent;
use render_scene::{ParticleSimulation, RuntimeStage, RuntimeStageProfiler};
use std::collections::HashSet;
use {
    super::predicted::{drive_actor_audio, drive_block_cues, drive_consume_audio},
    client_audio::audio::AudioEngine,
};
use {
    crate::{particles::ParticleInbox, runtime::world::ClientWorld},
    client_interaction::movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    client_presentation::local_player::LocalViewPose,
};

/// Installs presentation audio in its existing order. Each system times only its own work, so
/// systems the executor runs between them are never attributed to audio.
pub(crate) fn configure(app: &mut App) {
    super::synchronized::configure(app);
    app.add_plugins(client_audio::ClientAudioPlugin)
        .add_systems(
            Update,
            (
                ingest_audio_events,
                drive_inventory_audio,
                drive_local_motion,
                drive_ambience,
                super::weather::drive_rain_audio.after(crate::environment::update_atmosphere_frame),
                drive_weather_and_particles,
                // Local break and place cues play in the frame that predicts them.
                drive_block_cues
                    .after(crate::survival_mining::produce_survival_mining)
                    .after(crate::block_use::produce_block_use),
                drive_consume_audio,
                drive_actor_audio,
                pump_audio.after(client_state::ClientFrameSet::Camera),
            )
                .chain()
                .after(crate::ui_runtime::drive_world_inventory_keys)
                .after(crate::named_audio::drain_live_named_audio),
        );
}

pub(crate) fn drive_inventory_audio(
    mut player_runtime: ResMut<client_state::PlayerRuntime>,
    world: Res<ClientWorld>,
    view: Res<LocalViewPose>,
    engine: ResMut<AudioEngine>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_audio::audio::inventory::drive_inventory_audio(
        &mut player_runtime,
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        view.eye_translation(),
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
    client_audio::audio::systems::ingest_audio_events(
        messages,
        cues,
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_audio::observations::CollisionLookup),
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
    client_audio::audio::systems::drive_local_motion(
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_audio::observations::CollisionLookup),
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
    player_runtime: Option<Res<client_state::PlayerRuntime>>,
    ui: Option<Res<client_ui::ui_runtime::UiRuntime>>,
    engine: ResMut<AudioEngine>,
    state: Local<AmbientState>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_audio::audio::systems::drive_ambience(
        time,
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_audio::observations::CollisionLookup),
        view.eye_translation(),
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
    mut particles: Option<ResMut<ParticleSimulation>>,
    mut inbox: Option<ResMut<ParticleInbox>>,
    engine: ResMut<AudioEngine>,
    seen_bolts: Local<HashSet<i64>>,
    pending_bolts: Local<Vec<(f32, [f32; 3])>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    client_audio::audio::systems::drive_weather_and_particles(
        time,
        client_audio::observations::WorldObservation {
            stream: world.stream.as_ref(),
        },
        collisions
            .as_deref()
            .map(|value| value as &dyn client_audio::observations::CollisionLookup),
        particles.as_deref_mut().map(|particles| &mut **particles),
        inbox
            .as_deref_mut()
            .map(|value| value as &mut dyn client_audio::observations::ParticleAudioObservation),
        engine,
        seen_bolts,
        pending_bolts,
    );
}

/// Samples the published listener and attributes only the output pump's work to audio.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pump_audio(
    time: Res<Time>,
    view: Res<LocalViewPose>,
    camera: bevy::prelude::Query<
        &bevy::prelude::Transform,
        bevy::prelude::With<client_presentation::camera::FlyCamera>,
    >,
    server_camera: Option<Res<client_presentation::camera::ServerCameraView>>,
    settings: Res<AudioSettings>,
    engine: ResMut<AudioEngine>,
    device: Option<bevy::prelude::NonSendMut<client_audio::named_audio::AudioDevice>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::Audio));
    let listener = client_audio::audio::listener::camera_listener(
        view.eye_translation(),
        view.rotation(),
        camera.single().ok(),
        server_camera
            .as_deref()
            .and_then(|camera| camera.active_listener()),
    );
    client_audio::audio::systems::pump_audio(time, listener, settings, engine, device);
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Audio is timed inside its own systems; a span opened around the chain also counted every
    /// system the single-threaded executor ran between its first and last system.
    #[test]
    fn pump_attributes_its_own_work_to_the_audio_stage() {
        use bevy::prelude::{App, Update};
        let profiler = render_scene::RuntimeStageProfiler::new(true);
        let mut app = App::new();
        app.insert_resource(profiler.clone())
            .init_resource::<Time>()
            .init_resource::<LocalViewPose>()
            .init_resource::<AudioSettings>()
            .init_resource::<AudioEngine>()
            .add_systems(Update, pump_audio);
        app.update();
        app.update();
        let snapshot = profiler
            .take_snapshot_if_due(std::time::Duration::ZERO)
            .expect("enabled profiler");
        assert_eq!(
            snapshot.samples[render_scene::RuntimeStage::Audio as usize].count,
            2
        );
    }
}
