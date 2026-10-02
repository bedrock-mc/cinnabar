//! Frame systems feeding the audio engine: packets, local motion, ambience, weather and output.

use std::{
    collections::HashSet,
    sync::atomic::{AtomicU32, Ordering},
};

use bevy::prelude::{
    App, IntoScheduleConfigs, Local, Message, MessageReader, NonSendMut, Res, ResMut, Time, Update,
    Vec3,
};
use render::{ParticleSystem, PrecipitationMix};
use sim::PaletteWorld;

use super::{
    ambient::{
        ADDITIONS_INTERVAL, IntervalTimer, MOOD_INTERVAL, MusicScheduler, dimension_ambience,
        music_key,
    },
    echo::{EchoOrigin, EchoSubject},
    engine::{AudioEngine, Listener, LoopSpec, SoundRequest},
    local::{LocalCue, LocalMotion, MotionSample},
    predicted::{LocalBlockCue, drive_actor_audio, drive_block_cues, drive_consume_audio},
    route,
    settings::{AudioCategory, AudioSettings},
};
use crate::{
    local_player::LocalViewPose,
    movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    named_audio::AudioDevice,
    particles::ParticleInbox,
    runtime::{audio::SequencedAudioEvent, world::ClientWorld},
    ui_runtime::UiRuntime,
};

const PLAYER: &str = "minecraft:player";
const FEET_PROBE_BELOW: f64 = 0.2;
const WATER_IDENTIFIERS: [&str; 2] = ["minecraft:water", "minecraft:flowing_water"];
const THUNDER_GRACE_SECONDS: f32 = 0.3;
const UI_CLICK: &str = "random.click";
const AUDIO_STAGE: usize = render::RuntimeStage::Audio as usize;

static PENDING_UI_CLICKS: AtomicU32 = AtomicU32::new(0);

/// Requests the interface click sound from any code path (no ECS access needed); coalesced per frame.
pub(crate) fn ui_click() {
    let _ = PENDING_UI_CLICKS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
        Some(count.saturating_add(1))
    });
}

/// Interface sounds JSON-UI sound components asked for: name, volume, pitch.
static PENDING_UI_SOUNDS: std::sync::Mutex<Vec<(String, f32, f32)>> =
    std::sync::Mutex::new(Vec::new());
/// Bounds one frame's queued control sounds.
const MAX_PENDING_UI_SOUNDS: usize = 16;

/// Plays a pressed launcher control's sound, holding back a repeat inside its
/// `min_seconds_between_plays` (`SoundComponent`).
pub(crate) fn ui_control_sound(sound: &json_ui::ControlSound) {
    static LAST_PLAYED: std::sync::Mutex<Vec<(String, std::time::Instant)>> =
        std::sync::Mutex::new(Vec::new());
    if sound.min_seconds > 0.0 {
        let mut played = LAST_PLAYED
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let now = std::time::Instant::now();
        match played.iter().position(|(name, _)| *name == sound.name) {
            Some(index)
                if now.duration_since(played[index].1).as_secs_f32() < sound.min_seconds =>
            {
                return;
            }
            Some(index) => played[index].1 = now,
            None if played.len() < MAX_PENDING_UI_SOUNDS => played.push((sound.name.clone(), now)),
            None => {}
        }
    }
    ui_sound(&sound.name, sound.volume, sound.pitch);
}

/// Requests an interface sound a UI sound component names, at its volume and pitch.
pub(crate) fn ui_sound(name: &str, volume: f32, pitch: f32) {
    let mut pending = PENDING_UI_SOUNDS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if pending.len() < MAX_PENDING_UI_SOUNDS {
        pending.push((name.to_owned(), volume, pitch));
    }
}

/// A local interface sound request by sound definition name; ECS callers may send this instead of
/// calling [`ui_click`].
#[derive(Debug, Clone, PartialEq, Message)]
pub(crate) struct UiSoundCue(pub &'static str);

pub(crate) fn configure(app: &mut App) {
    app.init_resource::<AudioSettings>()
        .add_message::<UiSoundCue>()
        .add_message::<LocalBlockCue>()
        .add_systems(
            Update,
            (
                render::begin_stage_span::<AUDIO_STAGE>,
                ingest_audio_events,
                drive_local_motion,
                drive_ambience,
                drive_weather_and_particles,
                drive_block_cues,
                drive_consume_audio,
                drive_actor_audio,
                pump_audio,
                render::end_stage_span::<AUDIO_STAGE>,
            )
                .chain()
                .after(crate::named_audio::drain_live_named_audio),
        );
}

pub(super) fn block_lookup<'a>(
    collisions: Option<&'a PhysicsCollisionRegistries>,
    mode: assets::NetworkIdMode,
) -> impl Fn(u32) -> Option<String> + 'a {
    move |runtime_id| {
        collisions?
            .block_identifier(mode, runtime_id)
            .map(str::to_owned)
    }
}

pub(super) fn identifier_at(
    world: &PaletteWorld<'_>,
    collisions: &PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    block: [i32; 3],
) -> Option<String> {
    let runtime_id = world.primary_runtime_id(block).ok()?;
    collisions
        .block_identifier(mode, runtime_id)
        .map(str::to_owned)
}

pub(super) fn is_water(identifier: Option<&str>) -> bool {
    identifier.is_some_and(|name| WATER_IDENTIFIERS.contains(&name))
}

#[derive(Default)]
pub(super) struct IngestState {
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

    /// Whether `event` belongs to the bound session and current dimension, in order.
    fn admits(&mut self, event: &SequencedAudioEvent, dimension: i32) -> bool {
        let fresh = event.origin_stream_session_id == self.stream
            && event.dimension == dimension
            && event.dimension_epoch == self.epoch
            && event.sequence > self.last_sequence;
        if fresh {
            self.last_sequence = event.sequence;
        }
        fresh
    }
}

/// Level sound events the client also voices itself: block events by cell, actor events by actor.
const ECHOED_BLOCK_EVENTS: [&str; 2] = ["place", "break"];
const ECHOED_ACTOR_EVENTS: [&str; 2] = ["hurt", "death"];
pub(super) const BLOCK_ECHO_SECONDS: f64 = 0.6;
pub(super) const ACTOR_ECHO_SECONDS: f64 = 0.4;
const RECORD_EVENT: i32 = 1006;
/// Level sound event a jukebox sends when its record stops or is ejected.
const RECORD_STOP_SOUND: &str = "record.null";
/// Jukebox records tracked at once; entries whose sound is no longer active are pruned first.
const MAX_RECORDS: usize = 64;

#[allow(clippy::too_many_arguments)]
pub(super) fn ingest_audio_events(
    mut messages: MessageReader<SequencedAudioEvent>,
    mut cues: MessageReader<UiSoundCue>,
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    mut engine: ResMut<AudioEngine>,
    mut state: Local<IngestState>,
) {
    for cue in cues.read() {
        engine.enqueue(SoundRequest::new(cue.0));
    }
    if PENDING_UI_CLICKS.swap(0, Ordering::Relaxed) > 0 {
        engine.enqueue(SoundRequest::new(UI_CLICK));
    }
    let sounds = std::mem::take(
        &mut *PENDING_UI_SOUNDS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()),
    );
    for (name, volume, pitch) in sounds {
        engine.enqueue(SoundRequest::new(name).scaled(volume, pitch));
    }
    let Some(stream) = world.stream.as_ref() else {
        messages.clear();
        if state.stream != 0 {
            state.stream = 0;
            state.records.clear();
            engine.stop_all();
            engine.clear_server_if_current();
        }
        return;
    };
    state.bind(
        stream.actor_session_id(),
        stream.form_dimension_epoch(),
        &mut engine,
    );
    let dimension = stream.current_dimension();
    let lookup = block_lookup(collisions.as_deref(), stream.network_id_mode());
    for event in messages.read() {
        if !state.admits(event, dimension) {
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
                    .then(|| stream.item_identifier(level.data))
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
pub(super) fn drive_local_motion(
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    physics: Res<LocalPhysicsController>,
    mut engine: ResMut<AudioEngine>,
    mut motion: Local<LocalMotion>,
    mut last_tick: Local<Option<u64>>,
) {
    let (Some(stream), Some(collisions), Some(state)) = (
        world.stream.as_ref(),
        collisions.as_deref(),
        physics.state(),
    ) else {
        motion.reset();
        *last_tick = None;
        return;
    };
    if *last_tick == Some(state.tick) || !engine.has_bank() {
        return;
    }
    *last_tick = Some(state.tick);
    let mode = stream.network_id_mode();
    let palette = PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
    );
    let position = [state.position.x, state.position.y, state.position.z];
    let cell = |dy: f64| {
        [
            position[0].floor() as i32,
            (position[1] + dy).floor() as i32,
            position[2].floor() as i32,
        ]
    };
    let in_water = is_water(identifier_at(&palette, collisions, mode, cell(0.5)).as_deref());
    let below = identifier_at(&palette, collisions, mode, cell(-FEET_PROBE_BELOW));
    let sneaking = physics
        .latest_sneak_sprint()
        .is_some_and(|(sneak, _)| sneak);
    let cues = motion.advance(MotionSample {
        position,
        velocity_y: state.velocity.y,
        on_ground: state.on_ground,
        sneaking,
        in_water,
    });
    let feet = [position[0] as f32, position[1] as f32, position[2] as f32];
    let requests: Vec<SoundRequest> = {
        let Some(bank) = engine.bank() else { return };
        let tables = bank.tables();
        let material = below.as_deref().and_then(|name| tables.material_of(name));
        let interactive = |event: &str| {
            tables
                .interactive(PLAYER, event, material?)
                .map(|route| (route.sound, route.volume, route.pitch))
        };
        let entity = |event: &str| {
            tables
                .entity(PLAYER, event, None)
                .map(|route| (route.sound, route.volume, route.pitch))
        };
        cues.iter()
            .filter_map(|cue| match cue {
                LocalCue::Step => interactive("step"),
                LocalCue::Jump => interactive("jump"),
                LocalCue::Land { .. } => interactive("land"),
                LocalCue::Swim => entity("swim"),
                LocalCue::Splash => entity("splash"),
            })
            .map(|(sound, volume, pitch)| {
                SoundRequest::new(sound).with_ranges(volume, pitch).at(feet)
            })
            .collect()
    };
    for request in requests {
        engine.enqueue(request);
    }
}

pub(super) struct AmbientState {
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
    stream: &client_world::WorldStream,
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
pub(super) fn drive_ambience(
    time: Res<Time>,
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    view: Res<LocalViewPose>,
    ui: Option<Res<UiRuntime>>,
    mut engine: ResMut<AudioEngine>,
    mut state: Local<AmbientState>,
) {
    if !engine.has_bank() {
        return;
    }
    let dt = time.delta_secs();
    let stream = world.stream.as_ref();
    let dimension = stream.map_or(0, |stream| stream.current_dimension());
    let creative = ui
        .as_deref()
        .and_then(UiRuntime::player_game_mode)
        .is_some_and(|mode| matches!(mode, protocol::PlayerGameMode::Creative));

    let eye = view.eye_translation();
    let underwater = match (stream, collisions.as_deref()) {
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

    let key = music_key(stream.is_some(), dimension, creative);
    let entry = engine
        .bank()
        .and_then(|bank| bank.music(key))
        .map(|entry| (entry.event_name.clone(), (entry.min_delay, entry.max_delay)));
    if let Some((event_name, delay)) = entry {
        let playing = engine.is_playing_category(AudioCategory::Music);
        let mut rolls = [engine.unit(), engine.unit()].into_iter();
        let start = state
            .music
            .update(key, delay, playing, dt, || rolls.next().unwrap_or(0.5));
        if start {
            engine.enqueue(SoundRequest::new(&*event_name));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn drive_weather_and_particles(
    time: Res<Time>,
    world: Res<ClientWorld>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    mix: Option<Res<PrecipitationMix>>,
    particles: Option<ResMut<ParticleSystem>>,
    inbox: Option<ResMut<ParticleInbox>>,
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

    let bolts = stream.lightning_bolts();
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
    let lookup = block_lookup(collisions.as_deref(), stream.network_id_mode());
    let mut requests: Vec<(Option<[f32; 3]>, SoundRequest)> = Vec::new();
    if let Some(bank) = engine.bank() {
        let tables = bank.tables();
        for sound in &sounds {
            let request = match tables.individual(&sound.name) {
                Some(route) => {
                    SoundRequest::new(route.sound.clone()).with_ranges(route.volume, route.pitch)
                }
                None => SoundRequest::new(&*sound.name),
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
pub(super) fn pump_audio(
    time: Res<Time>,
    view: Res<LocalViewPose>,
    settings: Res<AudioSettings>,
    mut engine: ResMut<AudioEngine>,
    mut device: Option<NonSendMut<AudioDevice>>,
) {
    engine.poll_server();
    let eye = view.eye_translation();
    let right = view.rotation() * Vec3::X;
    let listener = Listener {
        position: [eye.x, eye.y, eye.z],
        right: [right.x, right.y, right.z],
    };
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
    use super::*;
    use std::sync::Arc;

    fn event(sequence: u64, dimension: i32, dimension_epoch: u64) -> SequencedAudioEvent {
        SequencedAudioEvent {
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
        assert!(!state.admits(&event(10, 0, 2), 0));
        assert!(state.admits(&event(11, 0, 4), 0));
        state.bind(1, 9, &mut engine);
        assert!(state.records.is_empty(), "dimension-owned records reset");
        assert!(!state.admits(&event(12, 0, 4), 0));
        assert!(state.admits(&event(13, 0, 9), 0));
    }
}
