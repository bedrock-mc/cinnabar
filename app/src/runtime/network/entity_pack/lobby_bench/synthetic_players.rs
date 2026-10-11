//! Fixed offline player populations through the production actor animation and publication path.

use protocol::{
    ActorEvent, ActorSpawnEvent, MovePlayerEvent, MovePlayerMode, PlayerListEntry,
    PlayerListUpdateEvent, PlayerSkin,
};
use render_api::StandardSkin;

use {super::*, client_presentation::actor_publication::publish_actor_render_frame};

const WARM_UP_FRAMES: u64 = 60;
const SAMPLE_FRAMES: u64 = 600;

/// Adds anonymous, visible vanilla player rigs without a socket, account, or captured session.
fn add_players(world: &mut World, players: u64) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let mut sequence = 0;
    for index in 0..players {
        let mut pixels = vec![200; render_model::STANDARD_SKIN_BYTES];
        pixels[..3].copy_from_slice(&index.to_le_bytes()[..3]);
        pixels[3] = 255;
        let runtime_id = index + 100;
        let mut uuid = [0; 16];
        uuid[..8].copy_from_slice(&runtime_id.to_le_bytes());
        sequence += 1;
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
                    entries: Arc::from([PlayerListEntry::Add {
                        uuid,
                        unique_id: -(runtime_id as i64),
                        username: "offline benchmark".into(),
                        verified: true,
                        skin: PlayerSkin::Standard(StandardSkin {
                            width: render_model::STANDARD_SKIN_SIDE as u32,
                            height: render_model::STANDARD_SKIN_SIDE as u32,
                            rgba8: pixels.into(),
                            cape: None,
                            geometry: None,
                        }),
                    }]),
                })),
            )
            .unwrap();
        sequence += 1;
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: -(runtime_id as i64),
                    runtime_id,
                    kind: ActorKind::Player {
                        uuid,
                        username: "offline benchmark".into(),
                    },
                    position: [
                        (index % 16) as f32 * 0.8 - 6.0,
                        64.0,
                        (index / 16) as f32 * 0.8,
                    ],
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
        drain_through(stream, [0.0, 64.0, -12.0], sequence);
    }
}

/// Moves every player at the simulation cadence while keeping the same camera and population.
fn move_players(world: &mut World, mut sequence: u64, players: u64, tick: u64) -> u64 {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    for index in 0..players {
        sequence += 1;
        stream
            .submit(
                sequence,
                WorldEvent::MovePlayer(MovePlayerEvent {
                    runtime_id: index + 100,
                    position: [
                        (index % 16) as f32 * 0.8 - 6.0 + ((tick + index) as f32 * 0.1).sin(),
                        64.0 + protocol::PLAYER_NETWORK_OFFSET,
                        (index / 16) as f32 * 0.8,
                    ],
                    pitch: 0.0,
                    yaw: (tick as f32 * 2.0) % 360.0,
                    head_yaw: (tick as f32 * 2.0) % 360.0,
                    mode: MovePlayerMode::Normal,
                    on_ground: true,
                    teleported: false,
                    source_tick: tick,
                }),
            )
            .unwrap();
    }
    drain_through(stream, [0.0, 64.0, -12.0], sequence);
    sequence
}

/// Vanilla mobs cycled through the mixed crowd, so entity rigs and controllers share the tick.
const MOBS: [&str; 10] = [
    "minecraft:zombie",
    "minecraft:skeleton",
    "minecraft:creeper",
    "minecraft:cow",
    "minecraft:pig",
    "minecraft:sheep",
    "minecraft:chicken",
    "minecraft:spider",
    "minecraft:wolf",
    "minecraft:villager_v2",
];
const MOB_RUNTIME_BASE: u64 = 10_000;

fn mob_position(index: u64, tick: u64) -> [f32; 3] {
    [
        (index % 12) as f32 * 1.2 - 7.0 + ((tick + index) as f32 * 0.1).sin(),
        64.0,
        6.0 + (index / 12) as f32 * 1.2,
    ]
}

/// Adds walking vanilla mobs alongside the players.
fn add_mobs(world: &mut World, first_sequence: u64, mobs: u64) -> u64 {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let mut sequence = first_sequence;
    for index in 0..mobs {
        sequence += 1;
        let runtime_id = MOB_RUNTIME_BASE + index;
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: runtime_id as i64,
                    runtime_id,
                    kind: ActorKind::Entity {
                        identifier: MOBS[index as usize % MOBS.len()].into(),
                    },
                    position: mob_position(index, 0),
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
    }
    drain_through(stream, [0.0, 64.0, -12.0], sequence);
    sequence
}

fn move_mobs(world: &mut World, mut sequence: u64, mobs: u64, tick: u64) -> u64 {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    for index in 0..mobs {
        sequence += 1;
        let yaw = (tick as f32 * 3.0 + index as f32 * 17.0) % 360.0;
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                    dimension: 0,
                    runtime_id: MOB_RUNTIME_BASE + index,
                    position: mob_position(index, tick).map(Some),
                    position_origin: protocol::ActorPositionOrigin::Feet,
                    pitch: Some(0.0),
                    yaw: Some(yaw),
                    head_yaw: Some(yaw),
                    on_ground: Some(true),
                    teleported: false,
                    player_mode: None,
                    source_tick: None,
                    interpolation: Default::default(),
                })),
            )
            .unwrap();
    }
    drain_through(stream, [0.0, 64.0, -12.0], sequence);
    sequence
}

/// Latest completed actor tick, which advances only on frames that cross a simulation tick.
fn completed_tick(world: &World) -> u64 {
    world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_rigs()
        .map(|rig| rig.completed_tick)
        .max()
        .unwrap_or(0)
}

/// Measures a fixed synthetic lobby; installed carriers are optional local test inputs.
/// `CINNABAR_LOBBY_MOBS` adds walking vanilla mobs; `CINNABAR_RENDER_PACK` layers a server pack.
#[test]
#[ignore = "offline performance evidence; needs installed carriers"]
fn synthetic_player_lobby_bench() {
    let compiled = PathBuf::from(
        std::env::var_os("CINNABAR_RENDER_CARRIERS").unwrap_or_else(|| COMPILED.into()),
    );
    if !world_carrier(&compiled).exists() {
        eprintln!("PLAYER_LOBBY_BENCH skipped: installed carriers absent");
        return;
    }
    let pack = std::env::var_os("CINNABAR_RENDER_PACK").map(PathBuf::from);
    let env_count = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let players = env_count("CINNABAR_LOBBY_PLAYERS", 64);
    let mobs = env_count("CINNABAR_LOBBY_MOBS", 0);
    let moving = std::env::var_os("CINNABAR_LOBBY_MOVING").is_some();
    let capture = Capture {
        bootstrap: WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, -12.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        packets: Vec::new(),
    };
    let (mut world, _, _) = build_world(&capture, pack.as_deref(), false);
    add_players(&mut world, players);
    let mut sequence = add_mobs(&mut world, players * 2, mobs);
    let mut camera = world.query_filtered::<
        &mut bevy::prelude::Transform,
        bevy::prelude::With<client_presentation::camera::FlyCamera>,
    >();
    *camera.single_mut(&mut world).unwrap() =
        bevy::prelude::Transform::from_translation(Vec3::new(0.0, 66.0, -12.0))
            .looking_at(Vec3::new(0.0, 65.0, 4.0), Vec3::Y);
    let mut clock = Instant::now();
    let tracked = [
        RuntimeStage::ActorAnimation,
        RuntimeStage::ActorPreparation,
        RuntimeStage::ActorRigBuild,
        RuntimeStage::ActorPublication,
    ];
    // Index 0 holds frames that crossed a simulation tick, index 1 the frames between ticks.
    let mut wall: [Series; 2] = Default::default();
    let mut cpu: [Series; 2] = Default::default();
    let mut stages: [[Series; 4]; 2] = Default::default();
    for index in 0..WARM_UP_FRAMES + SAMPLE_FRAMES {
        if moving && index.is_multiple_of(3) {
            let tick = index / 3 + 1;
            sequence = move_players(&mut world, sequence, players, tick);
            sequence = move_mobs(&mut world, sequence, mobs, tick);
        }
        clock += FRAME;
        world
            .resource_mut::<Time<Real>>()
            .update_with_instant(clock);
        let tick_before = completed_tick(&world);
        let start = Instant::now();
        let cpu_start = thread_cpu_time();
        prepare_offline_actor_frame(&mut world);
        world.run_system_cached(publish_actor_render_frame).unwrap();
        let wall_time = start.elapsed();
        let cpu_time = thread_cpu_time()
            .zip(cpu_start)
            .map(|(end, start)| end - start);
        let snapshot = world
            .resource::<RuntimeStageProfiler>()
            .take_snapshot_if_due(Duration::ZERO)
            .unwrap();
        if index < WARM_UP_FRAMES {
            continue;
        }
        let class = usize::from(completed_tick(&world) == tick_before);
        wall[class].0.push(wall_time.as_secs_f64() * 1e3);
        if let Some(time) = cpu_time {
            cpu[class].0.push(time.as_secs_f64() * 1e3);
        }
        for (series, stage) in stages[class].iter_mut().zip(tracked) {
            series
                .0
                .push(snapshot.samples[stage as usize].total.as_secs_f64() * 1e3);
        }
    }
    let drawn = world.resource::<ActorRenderFrame>().rig.instances.len();
    eprintln!(
        "PLAYER_LOBBY_BENCH players={players} mobs={mobs} moving={moving} frames={SAMPLE_FRAMES} drawn={drawn} tick_frames={} other_frames={}",
        wall[0].0.len(),
        wall[1].0.len(),
    );
    for (class, label) in ["tick", "between"].into_iter().enumerate() {
        eprintln!(
            "PLAYER_LOBBY_BENCH {label} wall_ms {}",
            wall[class].summary()
        );
        eprintln!("PLAYER_LOBBY_BENCH {label} cpu_ms {}", cpu[class].summary());
        for (series, stage) in stages[class].iter_mut().zip(tracked) {
            eprintln!(
                "PLAYER_LOBBY_BENCH {label} {}_ms {}",
                stage.name(),
                series.summary()
            );
        }
    }
    eprintln!(
        "PLAYER_LOBBY_BENCH animation {:?}",
        world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor_animation_stats()
    );
}
