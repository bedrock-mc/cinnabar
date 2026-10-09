//! Frame systems feeding the audio engine: packets, local motion, ambience, weather and output.

use crate::local_player::LocalViewPose;
use crate::{
    audio_ingress::SequencedAudioEvent, named_audio::AudioDevice,
    observations::CollisionLookup as PhysicsCollisionRegistries,
};

use std::collections::HashSet;

use bevy::prelude::{
    Local, Message, MessageReader, NonSendMut, Query, Res, ResMut, Time, Transform, With,
};
use render::{ParticleSimulation, PrecipitationMix};
use sim::PaletteWorld;

use super::{
    ambient::{
        ADDITIONS_INTERVAL, IntervalTimer, MOOD_INTERVAL, MusicScheduler, dimension_ambience,
        music_key,
    },
    echo::{EchoOrigin, EchoSubject},
    engine::{AudioEngine, LoopSpec, SoundRequest},
    local::{LocalCue, LocalMotion, MotionSample},
    route,
    settings::{AudioCategory, AudioSettings},
};

const PLAYER: &str = "minecraft:player";
const FEET_PROBE_BELOW: f64 = 0.2;
const WATER_IDENTIFIERS: [&str; 2] = ["minecraft:water", "minecraft:flowing_water"];
const THUNDER_GRACE_SECONDS: f32 = 0.3;
pub use client_ui::sound_requests::ui_sound;

/// A local interface sound request by sound definition name; ECS callers may send this instead of
/// using the named interface sound queue.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct UiSoundCue(pub &'static str);

pub fn block_lookup<'a>(
    collisions: Option<&'a dyn PhysicsCollisionRegistries>,
    mode: assets::NetworkIdMode,
) -> impl Fn(u32) -> Option<String> + 'a {
    move |runtime_id| {
        collisions?
            .block_identifier(mode, runtime_id)
            .map(str::to_owned)
    }
}

fn network_block_lookup<'a>(
    collisions: Option<&'a dyn PhysicsCollisionRegistries>,
    stream: &'a chunk_pipeline::WorldStream,
) -> impl Fn(u32) -> Option<String> + 'a {
    let lookup = block_lookup(collisions, stream.network_id_mode());
    move |wire_id| lookup(stream.resolve_block_network_id(wire_id))
}

pub fn identifier_at(
    world: &PaletteWorld<'_>,
    collisions: &dyn PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    block: [i32; 3],
) -> Option<String> {
    let runtime_id = world.primary_runtime_id(block).ok()?;
    collisions
        .block_identifier(mode, runtime_id)
        .map(str::to_owned)
}

pub fn is_water(identifier: Option<&str>) -> bool {
    identifier.is_some_and(|name| WATER_IDENTIFIERS.contains(&name))
}

#[derive(Default)]
pub struct IngestState {
    stream: u64,
    epoch: u64,
    last_sequence: u64,
    /// Jukebox cells whose record may still be playing, bounded by [`MAX_RECORDS`].
    records: HashSet<[i32; 3]>,
}

impl IngestState {
    /// Rebinds to `session` and its dimension `epoch`, silencing what the old binding owned.
    fn bind(&mut self, session: u64, epoch: u64, engine: &mut AudioEngine) {
        if self.stream != session || self.epoch != epoch {
            if self.stream != session {
                self.last_sequence = 0;
            }
            self.stream = session;
            self.epoch = epoch;
            self.records.clear();
            engine.stop_all();
        }
    }

    /// Replaces the record of the jukebox at `position` with `request`, or silences it for `None`.
    fn start_record(
        &mut self,
        position: [f32; 3],
        request: Option<SoundRequest>,
        engine: &mut AudioEngine,
    ) {
        let cell = position.map(|axis| axis.floor() as i32);
        self.records.remove(&cell);
        engine.stop_jukebox(cell);
        let Some(mut request) = request else { return };
        self.records.retain(|cell| engine.is_jukebox_active(*cell));
        if self.records.len() >= MAX_RECORDS {
            engine.stats.voice_limit += 1;
            return;
        }
        self.records.insert(cell);
        request.jukebox = Some(cell);
        engine.enqueue(request);
    }

    /// A jukebox `record.*` level sound: starts that cell's record, or stops it for `record.null`.
    fn record_sound(
        &mut self,
        level: &protocol::LevelAudioEvent,
        block_identifier: &dyn Fn(u32) -> Option<String>,
        engine: &mut AudioEngine,
    ) {
        let stop = &*level.sound_event == RECORD_STOP_SOUND;
        let request = (!stop)
            .then(|| engine.bank())
            .flatten()
            .and_then(|bank| route::level_sound_request(bank.tables(), level, block_identifier));
        if request.is_none() && !stop {
            engine.stats.unrouted += 1;
            return;
        }
        self.start_record(level.position, request, engine);
    }

    /// Admits ordinary transport order and released sounds from live actor lifetimes.
    fn admits(
        &mut self,
        event: &SequencedAudioEvent,
        dimension: i32,
        synchronized_actor_alive: bool,
    ) -> bool {
        let fresh = event.origin_stream_session_id == self.stream
            && event.dimension == dimension
            && event.dimension_epoch == self.epoch
            && if event.actor_synchronization.is_some() {
                synchronized_actor_alive
                    && matches!(&event.event, protocol::AudioEvent::Level(level) if level.fire_at_position.is_some())
            } else {
                event.sequence > self.last_sequence
            };
        if fresh {
            self.last_sequence = self.last_sequence.max(event.sequence);
        }
        fresh
    }
}

/// Level sound events the client also voices itself: block events by cell, actor events by actor.
const ECHOED_BLOCK_EVENTS: [&str; 2] = ["place", "break"];
const ECHOED_ACTOR_EVENTS: [&str; 2] = ["hurt", "death"];
pub const BLOCK_ECHO_SECONDS: f64 = 0.6;
pub const ACTOR_ECHO_SECONDS: f64 = 0.4;
const RECORD_EVENT: i32 = 1006;
/// Level sound event a jukebox sends when its record stops or is ejected.
const RECORD_STOP_SOUND: &str = "record.null";
/// Jukebox records tracked at once; entries whose sound is no longer active are pruned first.
const MAX_RECORDS: usize = 64;

#[allow(clippy::too_many_arguments)]
pub fn ingest_audio_events(
    mut messages: MessageReader<SequencedAudioEvent>,
    mut cues: MessageReader<UiSoundCue>,
    world: crate::observations::WorldObservation<'_>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    mut engine: ResMut<AudioEngine>,
    mut state: Local<IngestState>,
) {
    if let Some(stream) = world.stream.as_ref() {
        state.bind(
            stream.authority().actor_session_id(),
            stream.form_dimension_epoch(),
            &mut engine,
        );
    } else if state.stream != 0 {
        state.stream = 0;
        state.records.clear();
        engine.stop_all();
        engine.clear_server_if_current();
    }
    for cue in cues.read() {
        engine.enqueue(SoundRequest::new(cue.0));
    }
    client_ui::sound_requests::drain_sounds(|name, volume, pitch| {
        engine.enqueue(SoundRequest::new(name).scaled(volume, pitch));
    });
    let Some(stream) = world.stream.as_ref() else {
        messages.clear();
        return;
    };
    let lookup = network_block_lookup(collisions, stream);
    for event in messages.read() {
        let synchronized_actor_alive = event.actor_synchronization.is_some_and(|owner| {
            owner.session_id == stream.authority().actor_session_id()
                && owner.dimension == stream.current_dimension()
                && stream
                    .authority()
                    .actor(owner.runtime_id)
                    .is_some_and(|actor| actor.spawn_revision == owner.spawn_revision)
        });
        if !state.admits(event, stream.current_dimension(), synchronized_actor_alive) {
            engine.stats.stale += 1;
            continue;
        }
        let request = match &event.event {
            protocol::AudioEvent::Play(play) => Some(route::play_request(play)),
            protocol::AudioEvent::Stop(stop) => {
                if stop.stop_music_legacy {
                    engine.stop_category(AudioCategory::Music);
                }
                if stop.stop_all_sounds {
                    engine.stop_all();
                } else {
                    engine.stop_named(&stop.name);
                }
                continue;
            }
            protocol::AudioEvent::Level(level) if level.sound_event.starts_with("record.") => {
                state.record_sound(level, &lookup, &mut engine);
                continue;
            }
            protocol::AudioEvent::Level(level) => {
                let request = engine
                    .bank()
                    .and_then(|bank| route::level_sound_request(bank.tables(), level, &lookup));
                if request.is_some() && !admit_level_echo(&mut engine, level) {
                    continue;
                }
                if level.sound_event.as_ref() == "thunder" {
                    engine.note_server_thunder();
                }
                request
            }
            protocol::AudioEvent::LevelEvent(level) if level.event_id == RECORD_EVENT => {
                let name = (level.data != 0)
                    .then(|| stream.authority().item_identifier(level.data))
                    .flatten()
                    .and_then(|identifier| route::record_sound_name(&identifier))
                    .filter(|name| {
                        engine
                            .bank()
                            .is_some_and(|bank| bank.definition(name).is_some())
                    });
                let request = name.map(|name| SoundRequest::new(name).at(level.position));
                state.start_record(level.position, request, &mut engine);
                continue;
            }
            protocol::AudioEvent::LevelEvent(level) => {
                let roll = engine.unit();
                engine
                    .bank()
                    .and_then(|bank| route::level_event_request(bank.tables(), level, roll))
            }
        };
        match request {
            Some(request) => engine.enqueue(request),
            None => engine.stats.unrouted += 1,
        }
    }
}

/// Whether a server level sound should play, or is the copy of a sound the client already voiced.
fn admit_level_echo(engine: &mut AudioEngine, level: &protocol::LevelAudioEvent) -> bool {
    let name = level.sound_event.as_ref();
    if ECHOED_BLOCK_EVENTS.contains(&name) {
        let cell = level.position.map(|axis| axis.floor() as i32);
        engine.admit_echo(
            EchoOrigin::Packet,
            name,
            EchoSubject::Cell(cell),
            BLOCK_ECHO_SECONDS,
        )
    } else if ECHOED_ACTOR_EVENTS.contains(&name) {
        engine.admit_echo(
            EchoOrigin::Packet,
            name,
            EchoSubject::Actor(level.actor_unique_id),
            ACTOR_ECHO_SECONDS,
        )
    } else {
        true
    }
}

#[allow(clippy::too_many_arguments)]
pub fn drive_local_motion(
    world: crate::observations::WorldObservation<'_>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    physics: &dyn crate::observations::PhysicsObservation,
    mut engine: ResMut<AudioEngine>,
    mut motion: Local<LocalMotion>,
    mut last_tick: Local<Option<u64>>,
) {
    let (Some(stream), Some(collisions), Some(state)) =
        (world.stream.as_ref(), collisions, physics.state())
    else {
        motion.reset();
        *last_tick = None;
        return;
    };
    if *last_tick == Some(state.tick) || !engine.has_bank() {
        return;
    }
    if last_tick.is_some_and(|tick| tick > state.tick) {
        motion.reset();
        *last_tick = None;
    }
    let mode = stream.network_id_mode();
    let palette = PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
    );
    let after = *last_tick;
    physics.visit_motion_ticks(after, &mut |tick, sample| {
        if last_tick.is_some_and(|previous| tick != previous + 1) {
            motion.reset();
        }
        *last_tick = Some(tick);
        let position = sample.position;
        let below = identifier_at(
            &palette,
            collisions,
            mode,
            [
                position[0].floor() as i32,
                (position[1] - FEET_PROBE_BELOW).floor() as i32,
                position[2].floor() as i32,
            ],
        );
        let cues = motion.advance(sample);
        for cue in cues {
            let request = {
                let Some(bank) = engine.bank() else { return };
                let tables = bank.tables();
                let material = below.as_deref().and_then(|name| tables.material_of(name));
                let (route, volume) = match cue {
                    LocalCue::Step => (
                        material.and_then(|material| tables.interactive(PLAYER, "step", material)),
                        None,
                    ),
                    LocalCue::Jump => (
                        material.and_then(|material| tables.interactive(PLAYER, "jump", material)),
                        None,
                    ),
                    LocalCue::Land { .. } => (
                        material.and_then(|material| tables.interactive(PLAYER, "land", material)),
                        None,
                    ),
                    LocalCue::Swim { volume } => {
                        (tables.entity(PLAYER, "swim", None), Some(volume))
                    }
                    LocalCue::Splash { volume } => {
                        (tables.entity(PLAYER, "splash", None), Some(volume))
                    }
                };
                route.map(|route| {
                    let volume = volume.map_or(route.volume, |value| assets::FloatRange {
                        min: value,
                        max: value,
                    });
                    SoundRequest::new(route.sound)
                        .with_ranges(volume, route.pitch)
                        .at(local_cue_position(cue, sample))
                })
            };
            if let Some(request) = request {
                engine.enqueue(request);
            }
        }
    });
}

/// Splashes originate at water sensing before travel; other cues use the completed position.
fn local_cue_position(cue: LocalCue, sample: MotionSample) -> [f32; 3] {
    std::array::from_fn(|axis| {
        let previous = if matches!(cue, LocalCue::Splash { .. }) {
            f64::from(sample.movement[axis])
        } else {
            0.0
        };
        (sample.position[axis] - previous) as f32
    })
}

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

#[allow(clippy::too_many_arguments)]
pub fn drive_ambience(
    time: Res<Time>,
    world: crate::observations::WorldObservation<'_>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    view: Res<LocalViewPose>,
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

    let eye = view.eye_translation();
    let underwater = match (stream, collisions) {
        (Some(stream), Some(collisions)) => {
            let mode = stream.network_id_mode();
            let palette = PaletteWorld::new(
                stream.collision_store(),
                collisions.registry(mode),
                stream.current_dimension(),
            );
            let block = [
                eye.x.floor() as i32,
                eye.y.floor() as i32,
                eye.z.floor() as i32,
            ];
            is_water(identifier_at(&palette, collisions, mode, block).as_deref())
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
    super::music::drive_music(&mut engine, &mut state.music, key, dt);
}

#[allow(clippy::too_many_arguments)]
pub fn drive_weather_and_particles(
    time: Res<Time>,
    world: crate::observations::WorldObservation<'_>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    mix: Option<Res<PrecipitationMix>>,
    particles: Option<ResMut<ParticleSimulation>>,
    inbox: Option<&mut dyn crate::observations::ParticleAudioObservation>,
    mut engine: ResMut<AudioEngine>,
    mut seen_bolts: Local<HashSet<i64>>,
    mut pending_bolts: Local<Vec<(f32, [f32; 3])>>,
) {
    let mut particles = particles;
    let mut inbox = inbox;
    let Some(stream) = world.stream.as_ref() else {
        engine.set_loop("rain", None);
        seen_bolts.clear();
        pending_bolts.clear();
        return;
    };
    if !engine.has_bank() {
        return;
    }
    let rain = mix.as_deref().map_or(0.0, |mix| mix.rain.clamp(0.0, 1.0));
    engine.set_loop(
        "rain",
        (rain > 0.01).then(|| LoopSpec {
            name: "ambient.weather.rain".into(),
            volume: rain,
        }),
    );

    let bolts = stream.authority().lightning_bolts();
    seen_bolts.retain(|id| bolts.iter().any(|bolt| bolt.unique_id == *id));
    for bolt in &bolts {
        if seen_bolts.insert(bolt.unique_id) {
            pending_bolts.push((THUNDER_GRACE_SECONDS, bolt.position));
        }
    }
    // The server usually voices bolts itself; only unvoiced ones get the local fallback.
    let dt = time.delta_secs();
    let mut due = Vec::new();
    pending_bolts.retain_mut(|(remaining, position)| {
        *remaining -= dt;
        if *remaining <= 0.0 {
            due.push(*position);
        }
        *remaining > 0.0
    });
    for position in due {
        if !engine.server_thundered_within(2.0) {
            engine.enqueue(SoundRequest::new("ambient.weather.lightning.impact").at(position));
            engine.enqueue(SoundRequest::new("ambient.weather.thunder"));
        }
    }

    let sounds = particles
        .as_mut()
        .map(|system| system.take_sounds())
        .unwrap_or_default();
    let destroyed = inbox
        .as_mut()
        .map(|inbox| inbox.take_level_audio())
        .unwrap_or_default();
    let lookup = block_lookup(collisions, stream.network_id_mode());
    let mut requests: Vec<(Option<[f32; 3]>, SoundRequest)> = Vec::new();
    if let Some(bank) = engine.bank() {
        let tables = bank.tables();
        for sound in &sounds {
            let request = match tables.individual_lookup(&sound.name) {
                assets::RouteLookup::Route(route) => {
                    SoundRequest::new(route.sound.clone()).with_ranges(route.volume, route.pitch)
                }
                assets::RouteLookup::Absent => SoundRequest::new(&*sound.name),
                assets::RouteLookup::Silent => continue,
            };
            requests.push((None, request.at(sound.position)));
        }
        for (id, position, data) in destroyed {
            requests.extend(
                route::destroy_block_request(tables, id, position, data, &lookup)
                    .map(|request| (Some(position), request)),
            );
        }
    }
    for (destroyed_at, request) in requests {
        if let Some(position) = destroyed_at {
            enqueue_destroy_sound(&mut engine, position, request);
        } else {
            engine.enqueue(request);
        }
    }
}

/// Enqueues the packet copy of a block-break sound.
fn enqueue_destroy_sound(engine: &mut AudioEngine, position: [f32; 3], request: SoundRequest) {
    let cell = position.map(|axis| axis.floor() as i32);
    if engine.admit_echo(
        EchoOrigin::Packet,
        "break",
        EchoSubject::Cell(cell),
        BLOCK_ECHO_SECONDS,
    ) {
        engine.enqueue(request);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn pump_audio(
    time: Res<Time>,
    view: Res<LocalViewPose>,
    camera: Query<&Transform, With<crate::camera::FlyCamera>>,
    server_camera: Option<Res<crate::camera::ServerCameraView>>,
    settings: Res<AudioSettings>,
    mut engine: ResMut<AudioEngine>,
    mut device: Option<NonSendMut<AudioDevice>>,
) {
    engine.poll_server();
    let listener = super::listener::camera_listener(
        &view,
        camera.single().ok(),
        server_camera
            .as_deref()
            .and_then(|camera| camera.active_listener())
            == Some(1),
    );
    let sources = engine.pump(Some(listener), time.delta_secs(), &settings);
    let Some(device) = device.as_mut() else {
        return;
    };
    for source in sources {
        if !device.play_source(source) {
            engine.stats.backend_failed += 1;
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::engine::Listener;
    use super::*;
    use std::sync::Arc;

    #[derive(Default)]
    struct BlockMaterials(sim::CollisionRegistry);

    impl PhysicsCollisionRegistries for BlockMaterials {
        fn registry(&self, _: assets::NetworkIdMode) -> &sim::CollisionRegistry {
            &self.0
        }

        fn block_canonical_state(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
            None
        }

        fn block_identifier(&self, _: assets::NetworkIdMode, runtime_id: u32) -> Option<&str> {
            match runtime_id {
                1 => Some("minecraft:dirt"),
                7 | 0x8000_0007 => Some("minecraft:stone"),
                _ => None,
            }
        }
    }

    #[test]
    fn splash_position_precedes_water_travel_and_other_cues_use_completed_feet() {
        let sample = MotionSample {
            position: [3.0, -2.0, 5.0],
            velocity_y: -1.6,
            entry_velocity: [0.0, -2.0, 0.0],
            movement: [0.5, -1.5, 0.25],
            on_ground: false,
            sneaking: false,
            in_water: true,
        };
        assert_eq!(
            local_cue_position(LocalCue::Splash { volume: 0.4 }, sample),
            [2.5, -0.5, 4.75]
        );
        for cue in [
            LocalCue::Step,
            LocalCue::Jump,
            LocalCue::Land { speed: 2.0 },
            LocalCue::Swim { volume: 0.4 },
        ] {
            assert_eq!(local_cue_position(cue, sample), [3.0, -2.0, 5.0]);
        }
    }

    #[test]
    fn server_block_sounds_use_the_session_palette_and_preserve_high_bit_hashes() {
        let tables = assets::SoundEventTables::from_json(
            &serde_json::json!({"block_sounds": {
                "stone": {"events": {"break": "dig.stone"}},
                "gravel": {"events": {"break": "dig.gravel"}}
            }}),
            &serde_json::json!({"stone": "stone", "dirt": "gravel"}),
        );
        let collisions = BlockMaterials::default();
        for (hashes, wire) in [(false, 1), (true, 0x8000_0007_u32)] {
            let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: hashes,
            });
            stream.set_sequential_id_remap(assets::SequentialIdRemap::from_palette(vec![0, 7], 8));
            let event = protocol::LevelAudioEvent {
                sound_event: Arc::from("break"),
                position: [1.5, 64.5, 2.5],
                data: i32::from_ne_bytes(wire.to_ne_bytes()),
                actor_identifier: Arc::from(""),
                is_baby: false,
                is_global: false,
                actor_unique_id: -1,
                fire_at_position: None,
            };
            let lookup = network_block_lookup(Some(&collisions), &stream);
            let request = route::level_sound_request(&tables, &event, &lookup).unwrap();
            assert_eq!(request.name.as_ref(), "dig.stone");
            assert_eq!(request.position, Some(event.position));
            assert_eq!(u32::from_ne_bytes(event.data.to_ne_bytes()), wire);
            assert_eq!(
                block_lookup(Some(&collisions), assets::NetworkIdMode::Sequential)(7),
                Some("minecraft:stone".to_owned()),
            );
        }
    }

    fn event(sequence: u64, dimension: i32, dimension_epoch: u64) -> SequencedAudioEvent {
        SequencedAudioEvent {
            actor_synchronization: None,
            origin_stream_session_id: 1,
            sequence,
            dimension,
            dimension_epoch,
            event: protocol::AudioEvent::Stop(protocol::StopAudioEvent {
                name: Arc::from("x"),
                stop_all_sounds: false,
                stop_music_legacy: false,
            }),
        }
    }

    // Record starts that never become (or stop being) voices must not stay tracked.
    #[test]
    fn review_destroy_block_packet_suppresses_the_predicted_break_echo() {
        let mut engine = crate::audio::engine::tests::engine(&[("dig.stone", "block")]);
        let cell = [1, 64, 2];
        assert!(engine.admit_echo(
            EchoOrigin::Client,
            "break",
            EchoSubject::Cell(cell),
            BLOCK_ECHO_SECONDS
        ));
        enqueue_destroy_sound(
            &mut engine,
            [1.5, 64.5, 2.5],
            SoundRequest::new("dig.stone"),
        );
        assert!(engine.pump(None, 0.0, &AudioSettings::default()).is_empty());
    }

    #[test]
    fn record_bookkeeping_follows_voice_lifetimes() {
        let mut engine = AudioEngine::default();
        let mut state = IngestState::default();
        let settings = AudioSettings::default();
        for x in 0..1000 {
            let position = [x as f32, 64.0, 0.0];
            let request = SoundRequest::new("record.cat").at(position);
            state.start_record(position, Some(request), &mut engine);
            engine.pump(None, 0.0, &settings);
        }
        assert!(state.records.len() <= 1, "{} retained", state.records.len());
    }

    // Stopping one jukebox stopped every jukebox playing the same disc.
    #[test]
    fn a_record_stop_silences_only_its_own_jukebox() {
        let mut engine = crate::audio::engine::tests::engine(&[("record.cat", "record")]);
        let mut state = IngestState::default();
        for x in [0.5, 2.5] {
            let position = [x, 64.0, 0.5];
            let request = SoundRequest::new("record.cat").at(position);
            state.start_record(position, Some(request), &mut engine);
        }
        let listener = Listener {
            position: [1.5, 64.0, 0.5],
            right: [1.0, 0.0, 0.0],
        };
        let mut sources = engine.pump(Some(listener), 0.05, &AudioSettings::default());
        assert_eq!(sources.len(), 2);
        let stop = protocol::LevelAudioEvent {
            sound_event: Arc::from(RECORD_STOP_SOUND),
            position: [0.5, 64.0, 0.5],
            data: -1,
            actor_identifier: Arc::from(""),
            is_baby: false,
            is_global: false,
            actor_unique_id: -1,
            fire_at_position: None,
        };
        state.record_sound(&stop, &|_| None, &mut engine);
        assert!(sources[0].next().is_none());
        assert!(sources[1].next().is_some());
    }

    // A sound committed before a 0 -> 1 -> 0 roundtrip must not play in the new visit.
    #[test]
    fn events_from_an_earlier_visit_to_the_same_dimension_are_stale() {
        let mut engine = AudioEngine::default();
        let mut state = IngestState::default();
        state.bind(1, 4, &mut engine);
        state.records.insert([0, 64, 0]);
        assert!(!state.admits(&event(10, 0, 2), 0, false));
        assert!(state.admits(&event(11, 0, 4), 0, false));
        state.bind(1, 9, &mut engine);
        assert!(state.records.is_empty(), "dimension-owned records reset");
        assert!(!state.admits(&event(12, 0, 4), 0, false));
        assert!(state.admits(&event(13, 0, 9), 0, false));
    }

    #[test]
    fn synchronized_actor_audio_retains_lifetime_and_epoch_fences_after_newer_packets() {
        let mut state = IngestState::default();
        state.bind(1, 4, &mut AudioEngine::default());
        assert!(state.admits(&event(20, 0, 4), 0, false));
        let mut delayed = event(2, 0, 4);
        delayed.actor_synchronization = Some(client_world::ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: 7,
            spawn_revision: 1,
        });
        delayed.event = protocol::AudioEvent::Level(protocol::LevelAudioEvent {
            sound_event: "death".into(),
            position: [1.0, 2.0, 3.0],
            data: -1,
            actor_identifier: "minecraft:ender_dragon".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: 17,
            fire_at_position: Some([1.0, 2.0, 3.0]),
        });
        assert!(state.admits(&delayed, 0, true));
        assert_eq!(
            state.last_sequence, 20,
            "delayed delivery preserves transport order"
        );
        assert!(
            !state.admits(&delayed, 0, false),
            "the actor was removed or replaced"
        );
        assert!(!state.admits(&delayed, 1, true), "the dimension changed");
        state.bind(1, 9, &mut AudioEngine::default());
        assert!(
            !state.admits(&delayed, 0, true),
            "a return visit has a new epoch"
        );
    }
}

#[cfg(test)]
mod ui_sound_tests;
