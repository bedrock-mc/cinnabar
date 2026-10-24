//! Frame systems feeding the audio engine: packets, local motion, ambience, weather and output.

use crate::{
    audio_ingress::SequencedAudioEvent, named_audio::AudioDevice,
    observations::CollisionLookup as PhysicsCollisionRegistries,
};
use assets::audio_category::AudioCategory;

use std::collections::HashSet;

use bevy::prelude::{Local, Message, MessageReader, NonSendMut, Res, ResMut, Time};
use sim::PaletteWorld;

use super::{
    echo::{EchoOrigin, EchoSubject},
    engine::{AudioEngine, SoundRequest},
    local::{LocalCue, LocalMotion, MotionSample},
    route,
    settings::AudioSettings,
};

pub mod ambience;

const PLAYER: &str = assets::facts::VanillaEntity::Player.identifier();
const FEET_PROBE_BELOW: f64 = 0.2;
const THUNDER_GRACE_SECONDS: f32 = 0.3;

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

/// Resolves a packet block's sound component in the current session's ID space.
fn network_block_material<'a>(
    collisions: Option<&'a dyn PhysicsCollisionRegistries>,
    stream: &chunk_pipeline::WorldStream,
    wire_id: u32,
) -> Option<&'a str> {
    collisions?.block_sound_material(
        stream.network_id_mode(),
        stream.resolve_block_network_id(wire_id),
    )
}

/// The primary block identifier at `block`, borrowed from the registries so per-frame and
/// per-tick probes allocate nothing.
pub(super) fn identifier_ref<'a>(
    world: &PaletteWorld<'_>,
    collisions: &'a dyn PhysicsCollisionRegistries,
    mode: assets::NetworkIdMode,
    block: [i32; 3],
) -> Option<&'a str> {
    let runtime_id = world.primary_runtime_id(block).ok()?;
    collisions.block_identifier(mode, runtime_id)
}

/// Checks ordinary water from numeric movement facts; bubble-column cues keep their own route.
pub fn is_water(registry: &sim::CollisionRegistry, runtime_id: u32) -> bool {
    registry.movement_facts(runtime_id).is_some_and(|fact| {
        fact.flags.contains(sim::BlockPhysicsFlags::WATER)
            && !matches!(
                fact.surface_response,
                sim::SurfaceResponse::BubbleUp | sim::SurfaceResponse::BubbleDown
            )
    })
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
        component_material: Option<&str>,
        engine: &mut AudioEngine,
    ) {
        let stop = &*level.sound_event == RECORD_STOP_SOUND;
        let request = (!stop).then(|| engine.bank()).flatten().and_then(|bank| {
            route::level_sound_request_with_component_material(
                bank.tables(),
                level,
                block_identifier,
                component_material,
            )
        });
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
    for cue in cues.read() {
        if client_ui::sound_requests::interface_sound_enabled(cue.0) {
            engine.enqueue(SoundRequest::new(cue.0));
        }
    }
    let sounds = client_ui::sound_requests::take_sounds();
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
        stream.authority().actor_session_id(),
        stream.form_dimension_epoch(),
        &mut engine,
    );
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
                state.record_sound(
                    level,
                    &lookup,
                    network_block_material(collisions, stream, level.data as u32),
                    &mut engine,
                );
                continue;
            }
            protocol::AudioEvent::Level(level) => {
                let request = engine.bank().and_then(|bank| {
                    route::level_sound_request_with_component_material(
                        bank.tables(),
                        level,
                        &lookup,
                        network_block_material(collisions, stream, level.data as u32),
                    )
                });
                let admitted = request.is_none()
                    || if event.client_generated {
                        engine.admit_echo(
                            EchoOrigin::Client,
                            level.sound_event.as_ref(),
                            launch_subject(level),
                            ACTOR_ECHO_SECONDS,
                        )
                    } else {
                        admit_level_echo(&mut engine, level)
                    };
                if request.is_some() && !admitted {
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
    } else if name == client_world::ROCKET_LAUNCH_EVENT
        && (level.actor_identifier.is_empty()
            || assets::facts::entity::NativeEntityClass::FireworksRocket
                .identifier()
                .is_some_and(|identifier| level.actor_identifier.as_ref() == identifier))
    {
        engine.admit_echo(
            EchoOrigin::Packet,
            name,
            launch_subject(level),
            ACTOR_ECHO_SECONDS,
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

/// Keeps launch echoes tied to a named actor, or exact coordinates for anonymous packets.
fn launch_subject(level: &protocol::LevelAudioEvent) -> EchoSubject {
    EchoSubject::ActorAt {
        unique_id: level.actor_unique_id,
        position: level
            .position
            .map(|axis| if axis == 0.0 { 0 } else { axis.to_bits() }),
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
        Default::default(),
    );
    let after = *last_tick;
    physics.visit_motion_ticks(after, &mut |tick, sample| {
        if last_tick.is_some_and(|previous| tick != previous + 1) {
            motion.reset();
        }
        *last_tick = Some(tick);
        let position = sample.position;
        let below_cell = [
            position[0].floor() as i32,
            (position[1] - FEET_PROBE_BELOW).floor() as i32,
            position[2].floor() as i32,
        ];
        let below_runtime_id = palette.primary_runtime_id(below_cell).ok();
        let below =
            below_runtime_id.and_then(|runtime_id| collisions.block_identifier(mode, runtime_id));
        let component_material = below_runtime_id
            .and_then(|runtime_id| collisions.block_sound_material(mode, runtime_id));
        let cues = motion.advance(sample);
        for cue in cues {
            let request = {
                let Some(bank) = engine.bank() else { return };
                let tables = bank.tables();
                let material =
                    below.map(|name| tables.material_of_with_component(name, component_material));
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

#[allow(clippy::too_many_arguments)]
pub fn drive_weather_and_particles(
    time: Res<Time>,
    world: crate::observations::WorldObservation<'_>,
    collisions: Option<&dyn crate::observations::CollisionLookup>,
    particles: Option<&mut particles::ParticleSystem>,
    inbox: Option<&mut dyn crate::observations::ParticleAudioObservation>,
    mut engine: ResMut<AudioEngine>,
    mut seen_bolts: Local<HashSet<i64>>,
    mut pending_bolts: Local<Vec<(f32, [f32; 3])>>,
) {
    let mut particles = particles;
    let mut inbox = inbox;
    let Some(stream) = world.stream.as_ref() else {
        seen_bolts.clear();
        pending_bolts.clear();
        return;
    };
    if !engine.has_bank() {
        return;
    }
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
    let mode = stream.network_id_mode();
    let lookup = block_lookup(collisions, mode);
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
                route::destroy_block_request_with_component_material(
                    tables,
                    id,
                    position,
                    data,
                    &lookup,
                    collisions
                        .and_then(|collisions| collisions.block_sound_material(mode, data as u32)),
                )
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

/// Pumps pending sounds at the sampled listener and submits them to the optional device.
pub fn pump_audio(
    time: Res<Time>,
    listener: super::engine::Listener,
    settings: Res<AudioSettings>,
    mut engine: ResMut<AudioEngine>,
    mut device: Option<NonSendMut<AudioDevice>>,
) {
    engine.poll_server();
    let sources = engine.pump(Some(listener), time.delta_secs(), &settings);
    engine.submit_sources(sources, device.as_deref_mut());
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

        fn block_identifier(&self, _: assets::NetworkIdMode, runtime_id: u32) -> Option<&str> {
            match runtime_id {
                1 => Some("minecraft:dirt"),
                7 | 0x8000_0007 => Some("minecraft:stone"),
                _ => None,
            }
        }

        fn block_sound_material(&self, _: assets::NetworkIdMode, runtime_id: u32) -> Option<&str> {
            match runtime_id {
                1 => Some("metal"),
                7 | 0x8000_0007 => Some("wood"),
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
        let component_tables = assets::SoundEventTables::from_json(
            &serde_json::json!({"block_sounds": {
                "wood": {"events": {"break": "dig.wood"}},
                "metal": {"events": {"break": "dig.metal"}}
            }}),
            &serde_json::json!({}),
        );
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
            let request = route::level_sound_request_with_component_material(
                &component_tables,
                &event,
                &lookup,
                network_block_material(Some(&collisions), &stream, wire),
            )
            .unwrap();
            assert_eq!(request.name.as_ref(), "dig.wood");
        }
    }

    fn event(sequence: u64, dimension: i32, dimension_epoch: u64) -> SequencedAudioEvent {
        SequencedAudioEvent {
            client_generated: false,
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

    #[test]
    fn rocket_launch_audio_pairs_client_and_packet_without_suppressing_packet_repeats() {
        let mut engine = AudioEngine::default();
        let mut launch = protocol::LevelAudioEvent {
            sound_event: "launch".into(),
            position: [1.0, 2.0, 3.0],
            data: -1,
            actor_identifier: "minecraft:fireworks_rocket".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: 17,
            fire_at_position: None,
        };
        assert!(engine.admit_echo(
            EchoOrigin::Client,
            "launch",
            EchoSubject::Actor(17),
            ACTOR_ECHO_SECONDS
        ));
        assert!(
            !admit_level_echo(&mut engine, &launch),
            "the packet copy must not replay the actor cue"
        );
        assert!(
            admit_level_echo(&mut engine, &launch),
            "the consumed echo must preserve packet repeats"
        );
        assert!(
            !engine.admit_echo(
                EchoOrigin::Client,
                "launch",
                EchoSubject::Actor(17),
                ACTOR_ECHO_SECONDS
            ),
            "packet-first delivery also pairs"
        );
        launch.actor_unique_id = 18;
        assert!(admit_level_echo(&mut engine, &launch));
        assert!(admit_level_echo(&mut engine, &launch));
    }

    #[test]
    fn rocket_launch_audio_pairs_anonymous_packets_at_exact_positions_in_either_order() {
        for packet_first in [false, true] {
            let mut engine = AudioEngine::default();
            let launch = protocol::LevelAudioEvent {
                sound_event: client_world::ROCKET_LAUNCH_EVENT.into(),
                position: [1.0, 2.0, 3.0],
                data: -1,
                actor_identifier: "".into(),
                is_baby: false,
                is_global: false,
                actor_unique_id: -1,
                fire_at_position: None,
            };
            let mut client = launch.clone();
            client.actor_unique_id = 17;
            let mut nearby = launch.clone();
            nearby.position[0] += 0.125;
            assert!(admit_level_echo(&mut engine, &nearby));
            if packet_first {
                assert!(admit_level_echo(&mut engine, &launch));
                assert!(!engine.admit_echo(
                    EchoOrigin::Client,
                    client_world::ROCKET_LAUNCH_EVENT,
                    launch_subject(&client),
                    ACTOR_ECHO_SECONDS
                ));
            } else {
                assert!(engine.admit_echo(
                    EchoOrigin::Client,
                    client_world::ROCKET_LAUNCH_EVENT,
                    launch_subject(&client),
                    ACTOR_ECHO_SECONDS
                ));
                assert!(!admit_level_echo(&mut engine, &launch));
            }
            assert!(admit_level_echo(&mut engine, &launch));
            assert!(admit_level_echo(&mut engine, &launch));
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
        state.record_sound(&stop, &|_| None, None, &mut engine);
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
        delayed.actor_synchronization = Some(actor_animation::ActorLifetimeId {
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
mod open_tests;
