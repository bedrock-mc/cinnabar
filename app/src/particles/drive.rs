use bevy::prelude::IntoScheduleConfigs;
use std::sync::Arc;

use assets::{NetworkIdMode, RuntimeIconCatalog};
use bevy::camera::Projection;
use bevy::prelude::{
    App, DetectChangesMut, Local, MessageReader, Query, Res, ResMut, Resource, Time, Transform,
    Update, With, resource_exists,
};
use chunk_pipeline::WorldStream;
use client_world::{ActorStatusNotice, CommittedParticleEvent};
use particles::{
    ITEM_ICON_PARTICLES, LevelParticle, ParticleSystem, SpawnRequest, block_break_request,
    block_crack_request, burst_requests, classify_level_event, crack_cadence_due,
    critical_hit_request, dragon_egg_teleport_requests, eating_item_request, face_toward,
    item_icon_request, named_request, native_request, parse_molang_variables, terrain_request,
    tiles::item_tile, totem_activation_request,
};
use protocol::{ActorStatusKind, ParticleEvent, SpawnParticleEffectEvent};
use render_scene::{
    ParticleGpuFrame, ParticleSimulation, RainSplashQueue, particle_view, update_particle_frame,
};

use super::actors::{
    ActorParticleCommand, ActorParticleFrame, follow_actor_particles, queue_actor_particles,
    route_actor_particles,
};
use super::{
    ambient::AmbientParticles,
    cosmetics::CosmeticEmitters,
    tiles::{BlockTile, block_tile},
    world_adapter::StreamParticleWorld,
};
use {
    crate::runtime::world::ClientWorld,
    client_interaction::{
        movement::PhysicsCollisionRegistries, survival_mining::SurvivalMiningRuntime,
    },
    client_presentation::camera::FlyCamera,
};

#[cfg(test)]
#[path = "drive/bite_tests.rs"]
mod bite_tests;
#[cfg(test)]
#[path = "drive/snowball_tests.rs"]
mod snowball_tests;

#[cfg(test)]
#[path = "drive/totem_tests.rs"]
mod totem_tests;

#[cfg(test)]
#[path = "drive/break_tests.rs"]
mod break_tests;

/// Committed particle triggers and actor status notices waiting for the next frame's drive.
#[derive(Resource, Debug, Default)]
pub(crate) struct ParticleInbox {
    actor_commands: Vec<ActorParticleCommand>,
    events: Vec<CommittedParticleEvent>,
    notices: Vec<ActorStatusNotice>,
    /// Level events `(id, position, data)` the audio runtime drains; separate so particles can consume theirs.
    level_audio: Vec<(i32, [f32; 3], i32)>,
    /// Actor status notices copied for the audio runtime.
    status_audio: Vec<ActorStatusNotice>,
}

impl ParticleInbox {
    pub(crate) fn take_status_audio(&mut self) -> Vec<ActorStatusNotice> {
        std::mem::take(&mut self.status_audio)
    }

    pub(crate) fn take_level_audio(&mut self) -> Vec<(i32, [f32; 3], i32)> {
        std::mem::take(&mut self.level_audio)
    }
}

/// Item icons the particle system can turn into break/crumb pieces.
#[derive(Resource, Clone)]
pub(crate) struct ParticleIcons(pub(crate) Arc<RuntimeIconCatalog>);

const MAX_CRACKING_BLOCKS: usize = 8;
const MAX_QUEUED_INBOX: usize = 512;
const MAX_RAIN_SPLASHES_PER_FRAME: usize = 64;
const IDENTITY_BASIS: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

const BLOCK_BREAK_EFFECT: &str = assets::facts::ParticleEffect::BlockDestruct.identifier();
const RAIN_SPLASH_EFFECT: &str = assets::facts::ParticleEffect::RainSplashParticle.identifier();
/// Height fraction of an actor's box where head-level effects originate.
const HEAD_HEIGHT_FRACTION: f32 = 0.9;
/// Item pieces per icon-crack event.
const ITEM_ICON_PIECES: f32 = ITEM_ICON_PARTICLES as f32;

pub(crate) fn drain_committed_particles(stream: &mut WorldStream, inbox: &mut ParticleInbox) {
    let committed = stream.take_committed_particles();
    for committed in &committed {
        if let ParticleEvent::Level(level) = &committed.event
            && inbox.level_audio.len() < MAX_QUEUED_INBOX
        {
            inbox
                .level_audio
                .push((level.event_id, level.position, level.data));
        }
    }
    inbox.events.extend(committed);
    let notices = stream.take_actor_status_notices();
    let room = MAX_QUEUED_INBOX.saturating_sub(inbox.status_audio.len());
    inbox
        .status_audio
        .extend(notices.iter().take(room).copied());
    inbox.notices.extend(notices);
    let events_excess = inbox.events.len().saturating_sub(MAX_QUEUED_INBOX);
    inbox.events.drain(..events_excess);
    let notices_excess = inbox.notices.len().saturating_sub(MAX_QUEUED_INBOX);
    inbox.notices.drain(..notices_excess);
}

pub(crate) fn configure_particles(app: &mut App) {
    app.init_resource::<ParticleInbox>().add_systems(
        Update,
        drive_particles
            .run_if(resource_exists::<PhysicsCollisionRegistries>)
            .after(client_presentation::camera::FlyCameraUpdateSet)
            .after(client_state::ClientFrameSet::ActorPreparation)
            // Cosmetic emitters follow the attachments this frame's actor finalization placed.
            .after(client_state::ClientFrameSet::ActorFinalization)
            .after(crate::environment::update_seasonal_foliage),
    );
}

fn floor_cell(position: [f32; 3]) -> [i32; 3] {
    position.map(|c| c.floor() as i32)
}

fn spawn_spawn_packet(
    system: &mut ParticleSystem,
    stream: &WorldStream,
    event: &SpawnParticleEffectEvent,
) {
    let mut position = event.position;
    let mut bound = None;
    if let Some(unique_id) = event.actor_unique_id {
        // The position is relative to the attached actor, which the emitter then follows.
        let Some(actor) = stream.authority().actor_by_unique_id(unique_id) else {
            return;
        };
        bound = Some((actor.runtime_id, event.position));
        for (component, base) in position.iter_mut().zip(actor.position) {
            *component += base;
        }
    }
    let variables = event
        .molang_variables
        .as_deref()
        .map(parse_molang_variables)
        .unwrap_or_default();
    system.spawn(&SpawnRequest {
        effect: event.effect.to_string(),
        position,
        variables,
        bound,
        ..SpawnRequest::default()
    });
}

/// Everything a trigger needs to resolve blocks and items.
struct Routing<'a> {
    world: &'a StreamParticleWorld<'a>,
    stream: &'a WorldStream,
    mode: NetworkIdMode,
    icons: Option<&'a RuntimeIconCatalog>,
}

/// Emits destruction particles using an already-resolved block tile.
fn spawn_block_break(system: &mut ParticleSystem, block: [i32; 3], found: BlockTile) {
    system.spawn_terrain(&block_break_request(
        BLOCK_BREAK_EFFECT,
        block,
        found.tile,
        found.tint,
    ));
}

/// Records a break echo only after its texture has been resolved for rendering.
///
/// A local break can be predicted before the server's `LevelEventPacket` arrives. If its
/// texture is unavailable, the local particle spawn is skipped and the packet must remain
/// eligible to provide the authoritative effect.
fn route_block_break(
    system: &mut ParticleSystem,
    routing: &Routing<'_>,
    echoes: &mut client_audio::audio::EchoLedger,
    origin: client_audio::audio::EchoOrigin,
    runtime_id: i32,
    block: [i32; 3],
    now: f64,
) {
    let found = block_tile(routing.stream, routing.mode, runtime_id as u32, block);
    if !admit_break_echo(echoes, origin, block, now, found.is_some()) {
        return;
    }
    spawn_block_break(
        system,
        block,
        found.expect("renderable break echo has a resolved block tile"),
    );
}

/// Pairs only destruction effects that have a renderable texture with their server echo.
fn admit_break_echo(
    echoes: &mut client_audio::audio::EchoLedger,
    origin: client_audio::audio::EchoOrigin,
    block: [i32; 3],
    now: f64,
    renderable: bool,
) -> bool {
    renderable
        && echoes.admit(
            origin,
            "break",
            client_audio::audio::EchoSubject::Cell(block),
            client_audio::audio::BLOCK_ECHO_SECONDS,
            now,
        )
}

fn spawn_crack(system: &mut ParticleSystem, routing: &Routing<'_>, block: [i32; 3], face: u8) {
    let Some(runtime_id) = routing.world.block_runtime_id(block) else {
        return;
    };
    if let Some(found) = block_tile(routing.stream, routing.mode, runtime_id, block) {
        system.spawn_terrain(&block_crack_request(
            BLOCK_BREAK_EFFECT,
            block,
            face,
            found.tile,
            found.tint,
        ));
    }
}

fn spawn_item_icon(
    system: &mut ParticleSystem,
    routing: &Routing<'_>,
    identifier: &str,
    aux: i32,
    position: [f32; 3],
    count: f32,
) {
    let Some(icons) = routing.icons else {
        return;
    };
    if let Some(tile) = item_tile(icons, identifier, aux.max(0) as u32) {
        system.spawn(&item_icon_request(position, tile, count));
    }
}

fn spawn_item_icon_by_id(
    system: &mut ParticleSystem,
    routing: &Routing<'_>,
    network_id: i32,
    aux: i32,
    position: [f32; 3],
) {
    if let Some(identifier) = routing.stream.authority().item_identifier(network_id) {
        spawn_item_icon(
            system,
            routing,
            &identifier,
            aux,
            position,
            ITEM_ICON_PIECES,
        );
    }
}

fn route_level_event(
    system: &mut ParticleSystem,
    routing: &Routing<'_>,
    break_echoes: &mut client_audio::audio::EchoLedger,
    event: &protocol::LevelParticleEvent,
    now: f64,
    sequence: u64,
) {
    let protocol::LevelParticleEvent {
        event_id,
        position,
        data,
    } = *event;
    match classify_level_event(event_id, data) {
        Some(LevelParticle::DragonEggTeleport { displacement }) => {
            for request in dragon_egg_teleport_requests(position, displacement, sequence) {
                system.spawn(&request);
            }
        }
        Some(LevelParticle::Named {
            effect,
            spell_color,
        }) => {
            system.spawn(&native_request(effect, position, spell_color));
        }
        Some(LevelParticle::BlockBreak { runtime_id }) => {
            route_block_break(
                system,
                routing,
                break_echoes,
                client_audio::audio::EchoOrigin::Packet,
                runtime_id,
                floor_cell(position),
                now,
            );
        }
        Some(LevelParticle::Terrain { runtime_id }) => {
            let block = floor_cell(position);
            if let Some(found) = block_tile(routing.stream, routing.mode, runtime_id as u32, block)
            {
                system.spawn(&terrain_request(
                    BLOCK_BREAK_EFFECT,
                    block,
                    found.tile,
                    found.tint,
                ));
            }
        }
        Some(LevelParticle::BlockCrack { face, .. }) => {
            spawn_crack(system, routing, floor_cell(position), face);
        }
        Some(LevelParticle::ItemIcon { network_id, aux }) => {
            spawn_item_icon_by_id(system, routing, network_id, aux, position);
        }
        Some(LevelParticle::FixedItemIcon { identifier, count }) => {
            spawn_item_icon(system, routing, identifier, 0, position, count as f32);
        }
        None => {}
    }
}

fn burst(system: &mut ParticleSystem, effect: &str, origin: [f32; 3], pieces: u64, seed: u64) {
    for request in burst_requests(effect, origin, pieces, seed) {
        system.spawn(&request);
    }
}

fn route_notice(system: &mut ParticleSystem, routing: &Routing<'_>, notice: &ActorStatusNotice) {
    let height = notice.height.unwrap_or(1.0);
    let mut head = notice.position;
    head[1] += height * HEAD_HEIGHT_FRACTION;
    let seed = notice.runtime_id;
    // Piece counts follow the vanilla effects but need independent measurement.
    match notice.kind {
        ActorStatusKind::TamingSucceeded | ActorStatusKind::LoveHearts => {
            burst(
                system,
                assets::facts::ParticleEffect::HeartParticle.identifier(),
                head,
                7,
                seed,
            );
        }
        ActorStatusKind::TamingFailed => {
            burst(
                system,
                assets::facts::ParticleEffect::BasicSmokeParticle.identifier(),
                head,
                7,
                seed,
            );
        }
        ActorStatusKind::VillagerAngry => {
            burst(
                system,
                assets::facts::ParticleEffect::VillagerAngry.identifier(),
                head,
                3,
                seed,
            );
        }
        ActorStatusKind::VillagerHappy => {
            burst(
                system,
                assets::facts::ParticleEffect::VillagerHappy.identifier(),
                head,
                5,
                seed,
            );
        }
        ActorStatusKind::WitchHatMagic => {
            burst(
                system,
                assets::facts::ParticleEffect::WitchspellEmitter.identifier(),
                head,
                1,
                seed,
            );
        }
        ActorStatusKind::TotemActivate => {
            if notice.runtime_id == routing.stream.authority().local_player_runtime_id()
                && let Some(actor) = routing.stream.authority().actor(notice.runtime_id)
            {
                system.spawn(&totem_activation_request(
                    actor.runtime_id,
                    actor.position,
                    actor.native_position(),
                ));
            }
        }
        ActorStatusKind::Feed => {
            let consumption = routing
                .stream
                .authority()
                .item_use_profile(notice.data >> 16)
                .consumption;
            // Local admitted food uses scatter from their predicted eating cues.
            if client_audio::audio::predicted::predicts_bites(consumption)
                && notice.runtime_id != routing.stream.local_player_runtime_id()
                && let Some(identifier) = routing
                    .stream
                    .authority()
                    .item_identifier(notice.data >> 16)
                && let Some(icons) = routing.icons
                && let Some(tile) = item_tile(icons, &identifier, (notice.data & 0xffff) as u32)
            {
                system.spawn(&eating_item_request(notice.eating_position, tile));
            }
        }
        _ => {}
    }
}

/// How far ahead of and below the first-person camera the local player's crumbs scatter, so
/// they show around the raised food as they do in vanilla's first person. Provisional: no
/// version-matched witness of the native first-person spawn point is recorded yet.
const FIRST_PERSON_BITE_AHEAD: f32 = 0.45;
const FIRST_PERSON_BITE_BELOW: f32 = 0.15;

/// Scatters crumbs of the local player's food for one bite.
fn spawn_local_bite(
    system: &mut ParticleSystem,
    routing: &Routing<'_>,
    camera: &Transform,
    first_person: bool,
    cue: &client_audio::audio::predicted::LocalEatingCue,
) {
    let Some(tile) = routing
        .icons
        .and_then(|icons| item_tile(icons, &cue.identifier, cue.metadata))
    else {
        return;
    };
    if let Some(position) = local_bite_position(routing.stream, camera, first_person) {
        system.spawn(&eating_item_request(position.to_array(), tile));
    }
}

/// In view of the camera when it resolves to first person, else the local player's mouth as
/// other players see it; `None` without a local actor in third person.
fn local_bite_position(
    stream: &WorldStream,
    camera: &Transform,
    first_person: bool,
) -> Option<bevy::math::Vec3> {
    if first_person {
        return Some(
            camera.translation + camera.forward() * FIRST_PERSON_BITE_AHEAD
                - camera.up() * FIRST_PERSON_BITE_BELOW,
        );
    }
    let actor = stream.authority().actor(stream.local_player_runtime_id())?;
    Some(bevy::math::Vec3::from_array(actor.eating_position()))
}

/// Hit pieces on the local player's target and on the nearest server-reported cracks.
fn spawn_mining_cracks(
    system: &mut ParticleSystem,
    routing: &Routing<'_>,
    local_target: Option<([i32; 3], u8)>,
    camera: [f32; 3],
) {
    if let Some((block, face)) = local_target {
        spawn_crack(system, routing, block, face);
    }
    let snapshot = routing.stream.block_crack_snapshot();
    let mut blocks: Vec<[i32; 3]> = snapshot
        .entries
        .iter()
        .map(|crack| crack.position)
        .filter(|block| local_target.is_none_or(|(local, _)| local != *block))
        .collect();
    let distance = |block: &[i32; 3]| -> f32 {
        (0..3)
            .map(|i| (block[i] as f32 + 0.5 - camera[i]).powi(2))
            .sum()
    };
    blocks.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    for block in blocks.into_iter().take(MAX_CRACKING_BLOCKS) {
        spawn_crack(system, routing, block, face_toward(block, camera));
    }
}

/// Moves actor-bound emitters with their actors; an emitter whose actor vanished stops.
fn follow_bound_emitters(system: &mut ParticleSystem, stream: &WorldStream) {
    system.update_bound_emitters(|runtime_id, offset| {
        let actor = stream.authority().actor(runtime_id)?;
        let position = std::array::from_fn(|i| actor.position[i] + offset[i]);
        Some((position, IDENTITY_BASIS))
    });
}

/// Audio cues and camera authority that place local break and eating particles.
type ParticleCues<'w, 's> = (
    MessageReader<'w, 's, client_audio::audio::LocalBlockCue>,
    MessageReader<'w, 's, client_audio::audio::predicted::LocalEatingCue>,
    Option<Res<'w, client_presentation::camera::CameraSettingsAuthority>>,
    Option<Res<'w, client_presentation::camera::ServerCameraView>>,
);

/// Cave culling, profiling and the cosmetic emitters placed by this frame's actor finalization.
type ParticleFrameContext<'w, 's> = (
    Res<'w, crate::runtime::visibility::CaveVisibilityCache>,
    Option<Res<'w, render_scene::RuntimeStageProfiler>>,
    Option<Res<'w, client_presentation::actor_publication::CosmeticEmitterPlacements>>,
    Local<'s, CosmeticEmitters>,
    Option<Res<'w, client_interaction::movement::LocalPhysicsController>>,
    Local<'s, super::ground_effects::GroundEffects>,
    Option<Res<'w, client_presentation::actor_publication::GroundEffectWearers>>,
    Option<Res<'w, client_interaction::movement::MovementTicker>>,
);

#[allow(clippy::too_many_arguments)]
fn drive_particles(
    frame_time: (
        Res<Time>,
        Option<Res<client_presentation::actor_publication::ActorFramePartialTick>>,
    ),
    mut inbox: ResMut<ParticleInbox>,
    mut system: ResMut<ParticleSimulation>,
    mut frame: ResMut<ParticleGpuFrame>,
    mut client_world: ResMut<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    cameras: Query<(&Transform, &Projection), With<FlyCamera>>,
    icons: Option<Res<ParticleIcons>>,
    splashes: Option<ResMut<RainSplashQueue>>,
    mining: Option<Res<SurvivalMiningRuntime>>,
    mut session: Local<(u64, i32)>,
    mut crack_timer: Local<f32>,
    (mut block_cues, mut eating_cues, camera_settings, server_camera): ParticleCues<'_, '_>,
    mut break_echoes: Local<client_audio::audio::EchoLedger>,
    mut ambient: Local<AmbientParticles>,
    (
        cave,
        profiler,
        cosmetic_placements,
        mut cosmetics,
        physics,
        mut ground,
        ground_wearers,
        movement,
    ): ParticleFrameContext<'_, '_>,
) {
    let (time, actor_alpha) = frame_time;
    // Timed inside the system: a span around it would also count systems ordered in between.
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::Particles));
    let Some(stream) = client_world.stream.as_mut() else {
        if system.emitter_count() > 0 {
            system.clear();
        }
        inbox.events.clear();
        inbox.notices.clear();
        inbox.actor_commands.clear();
        block_cues.clear();
        eating_cues.clear();
        ambient.reset();
        cosmetics.reset();
        return;
    };
    drain_committed_particles(stream, &mut inbox);
    let Ok((transform, projection)) = cameras.single() else {
        return;
    };
    let identity = (
        stream.authority().actor_session_id(),
        stream.current_dimension(),
    );
    if *session != identity {
        *session = identity;
        *break_echoes = client_audio::audio::EchoLedger::default();
        system.clear();
        inbox.actor_commands.clear();
        ambient.reset();
        cosmetics.reset();
        inbox.events.retain(|event| event.dimension == identity.1);
    }
    let mode = stream.network_id_mode();
    let world = StreamParticleWorld::new(stream, collisions.registry(mode), Some(&collisions));
    let routing = Routing {
        world: &world,
        stream,
        mode,
        icons: icons.as_ref().map(|icons| &*icons.0),
    };
    let view = particle_view(&(*transform).into(), projection);
    let first_person = client_presentation::camera::hand_first_person(
        camera_settings.map_or(semantic_input::PerspectiveMode::FirstPerson, |settings| {
            settings.perspective()
        }),
        server_camera.as_deref(),
    );
    system.set_camera(view.position);
    ambient.drive(
        time.delta(),
        &view,
        stream,
        &world,
        &collisions,
        &mut system,
    );

    // Vanilla emits local destruction effects before a server echo. The cue carries the
    // destroyed id because the world already predicts air; a missing local texture must not
    // consume the server packet's echo slot.
    for cue in block_cues.read() {
        if let client_audio::audio::LocalBlockCue::Break {
            position,
            block_runtime_id,
        } = *cue
        {
            route_block_break(
                &mut system,
                &routing,
                &mut break_echoes,
                client_audio::audio::EchoOrigin::Client,
                block_runtime_id,
                position,
                time.elapsed_secs_f64(),
            );
        }
    }
    for committed in inbox.events.drain(..) {
        match &committed.event {
            ParticleEvent::Level(level) => {
                route_level_event(
                    &mut system,
                    &routing,
                    &mut break_echoes,
                    level,
                    time.elapsed_secs_f64(),
                    committed.sequence,
                );
            }
            ParticleEvent::Spawn(spawn) => spawn_spawn_packet(&mut system, stream, spawn),
            ParticleEvent::ActorCritical {
                actor_runtime_id,
                magic,
                particle_count,
            } => route_critical(
                &mut system,
                stream,
                *actor_runtime_id,
                *magic,
                *particle_count,
            ),
        }
    }
    for notice in inbox.notices.drain(..) {
        route_notice(&mut system, &routing, &notice);
    }
    for cue in eating_cues.read() {
        spawn_local_bite(&mut system, &routing, transform, first_person, cue);
    }
    if let Some(mut splashes) = splashes {
        for position in splashes
            .positions
            .drain(..)
            .take(MAX_RAIN_SPLASHES_PER_FRAME)
        {
            system.spawn(&named_request(RAIN_SPLASH_EFFECT, position, None));
        }
    }
    if crack_cadence_due(&mut crack_timer, time.delta_secs()) {
        let local_target = mining
            .as_ref()
            .and_then(|mining| mining.destroying_target());
        spawn_mining_cracks(&mut system, &routing, local_target, view.position);
    }
    let mut actor_frame =
        ActorParticleFrame::new(stream, actor_alpha.as_ref().map_or(0.0, |alpha| alpha.0));
    queue_actor_particles(&mut actor_frame, &mut system, &mut inbox.actor_commands);
    route_actor_particles(&mut system, &mut inbox.actor_commands);
    follow_bound_emitters(&mut system, stream);
    follow_actor_particles(&mut system, &mut actor_frame);
    cosmetics.sync(
        &mut system,
        cosmetic_placements
            .as_deref()
            .map_or(&[], |placements| &placements.0),
    );
    ground.drive(
        &mut system,
        physics
            .as_deref()
            .zip(movement.as_deref())
            .map(|(physics, movement)| {
                (physics, movement.interaction_authority_identity().1.get())
            }),
        &world,
        stream,
        ground_wearers.as_deref().map_or(&[], |wearers| &wearers.0),
        time.delta_secs(),
        actor_alpha.as_ref().map_or(0.0, |alpha| alpha.0),
    );
    let camera = crate::runtime::telemetry::camera_sub_chunk_key(
        stream.current_dimension(),
        bevy::math::Vec3::from_array(view.position),
    );
    let generation = stream.connectivity_generation();
    // Mark the frame for extraction only when its published contents changed.
    let changed = update_particle_frame(
        &mut system,
        frame.bypass_change_detection(),
        time.delta_secs(),
        &view,
        &world,
        |low, high| {
            cave.hides_box(
                camera,
                generation,
                |key| stream.has_sub_chunk_connectivity(key),
                low,
                high,
            )
        },
    );
    if changed {
        frame.set_changed();
    }
}

fn route_critical(
    system: &mut ParticleSystem,
    stream: &WorldStream,
    runtime_id: u64,
    magic: bool,
    particle_count: f32,
) {
    let Some(actor) = stream.authority().actor(runtime_id) else {
        return;
    };
    let height = actor
        .bounding_box()
        .map_or(1.8, |(min, max)| max[1] - min[1]);
    let mut position = actor.position;
    position[1] += height * HEAD_HEIGHT_FRACTION;
    system.spawn(&critical_hit_request(magic, position, particle_count));
}
