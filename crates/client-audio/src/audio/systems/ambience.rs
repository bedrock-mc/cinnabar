//! Environment loops, biome ambience and music scheduling.

use bevy::prelude::{Local, Res, ResMut, Time};
use sim::PaletteWorld;

use super::super::{
    ambient::{
        ADDITIONS_INTERVAL, IntervalTimer, MOOD_INTERVAL, MusicScheduler, dimension_ambience,
        music_key,
    },
    engine::{AudioEngine, LoopSpec, SoundRequest},
};
use super::is_water;

/// Retains underwater transitions and ambience/music timers between frames.
pub struct AmbientState {
    underwater: bool,
    mood: IntervalTimer,
    additions: IntervalTimer,
    music: MusicScheduler,
}

impl Default for AmbientState {
    fn default() -> Self {
        Self {
            underwater: false,
            mood: IntervalTimer::new(MOOD_INTERVAL),
            additions: IntervalTimer::new(ADDITIONS_INTERVAL),
            music: MusicScheduler::default(),
        }
    }
}

/// Ambience definition prefix for the eye position: the biome's own set when the pack defines it.
fn ambience_prefix(
    stream: &chunk_pipeline::WorldStream,
    dimension: i32,
    eye: [f32; 3],
    engine: &AudioEngine,
) -> Option<String> {
    let fallback = dimension_ambience(dimension)?;
    let biome = stream.camera_biome_id(eye).and_then(|id| {
        stream
            .biome_definitions_snapshot()
            .iter()
            .find(|definition| u32::from(definition.biome_id.unwrap_or(u16::MAX)) == id)
            .map(|definition| definition.name.to_string())
    });
    let own = biome
        .map(|name| format!("ambient.{name}"))
        .filter(|prefix| {
            engine
                .bank()
                .is_some_and(|bank| bank.definition(&format!("{prefix}.loop")).is_some())
        });
    Some(own.unwrap_or_else(|| fallback.to_owned()))
}

/// Advances environmental loops and music from current world and player observations.
#[allow(clippy::too_many_arguments)]
pub fn drive_ambience(
    time: Res<Time>,
    world: crate::observations::WorldObservation<'_>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    eye: bevy::prelude::Vec3,
    player_runtime: Option<&player_state::PlayerState>,
    credits_active: bool,
    mut engine: ResMut<AudioEngine>,
    mut state: Local<AmbientState>,
) {
    if !engine.has_bank() {
        return;
    }
    let dt = time.delta_secs();
    let stream = world.stream.as_ref();
    let dimension = stream.map_or(0, |stream| stream.current_dimension());
    let creative = player_runtime
        .and_then(|player| player.facts.player_game_mode())
        .is_some_and(|mode| matches!(mode, protocol::PlayerGameMode::Creative));

    let underwater = match (stream, collisions) {
        (Some(stream), Some(collisions)) => {
            let mode = stream.network_id_mode();
            let palette = PaletteWorld::new(
                stream.collision_store(),
                collisions.registry(mode),
                stream.current_dimension(),
                Default::default(),
            );
            let block = [
                eye.x.floor() as i32,
                eye.y.floor() as i32,
                eye.z.floor() as i32,
            ];
            palette
                .primary_runtime_id(block)
                .ok()
                .is_some_and(|id| is_water(collisions.registry(mode), id))
        }
        _ => false,
    };
    if underwater != state.underwater {
        let name = if underwater {
            "ambient.underwater.enter"
        } else {
            "ambient.underwater.exit"
        };
        engine.enqueue(SoundRequest::new(name));
        state.underwater = underwater;
    }
    let loop_spec = |name: &str| {
        Some(LoopSpec {
            name: name.into(),
            volume: 1.0,
        })
    };
    engine.set_loop(
        "underwater",
        underwater
            .then(|| loop_spec("ambient.underwater.loop"))
            .flatten(),
    );
    let prefix: Option<String> = stream
        .and_then(|stream| ambience_prefix(stream, dimension, [eye.x, eye.y, eye.z], &engine));
    engine.set_loop(
        "dimension",
        prefix
            .as_deref()
            .and_then(|prefix| loop_spec(&format!("{prefix}.loop"))),
    );

    // Unloaded chunks read as unlit, so mood waits for the eye's chunk to be present.
    let dark = stream.is_some_and(|stream| {
        let at = [eye.x, eye.y, eye.z];
        let (block, sky) = stream.light_level_at(at);
        stream.camera_biome_id(at).is_some() && block == 0 && sky == 0
    });
    let unit = engine.unit();
    if state.mood.tick(dark && !underwater, dt, unit) {
        let name = prefix
            .as_deref()
            .map_or_else(|| "ambient.cave".to_owned(), |p| format!("{p}.mood"));
        engine.enqueue(SoundRequest::new(name));
    }
    let unit = engine.unit();
    if let Some(prefix) = prefix.as_deref()
        && state.additions.tick(true, dt, unit)
    {
        engine.enqueue(SoundRequest::new(format!("{prefix}.additions")));
    }

    let key = music_key(stream.is_some(), dimension, creative, credits_active);
    super::super::music::drive_music(&mut engine, &mut state.music, key, dt);
}
