//! Fixed offline player populations through the production actor animation and publication path.

use protocol::{
    ActorEvent, ActorSpawnEvent, MovePlayerEvent, MovePlayerMode, PlayerListEntry,
    PlayerListUpdateEvent, PlayerSkin, StandardSkin,
};

use super::*;

const WARM_UP_FRAMES: u64 = 60;
const SAMPLE_FRAMES: u64 = 600;

/// Adds anonymous, visible vanilla player rigs without a socket, account, or captured session.
fn add_players(world: &mut World, players: u64) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let mut sequence = 0;
    for index in 0..players {
        let mut pixels = vec![200; render::STANDARD_SKIN_BYTES];
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
                            width: render::STANDARD_SKIN_SIDE as u32,
                            height: render::STANDARD_SKIN_SIDE as u32,
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
fn move_players(world: &mut World, players: u64, tick: u64) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let mut sequence = players * 2 + (tick - 1) * players;
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
}

/// Measures a fixed synthetic lobby; installed carriers are optional local test inputs.
#[test]
#[ignore = "offline performance evidence; needs installed carriers and CINNABAR_RENDER_PACK"]
fn synthetic_player_lobby_bench() {
    let compiled = PathBuf::from(
        std::env::var_os("CINNABAR_RENDER_CARRIERS").unwrap_or_else(|| COMPILED.into()),
    );
    let Some(pack) = std::env::var_os("CINNABAR_RENDER_PACK") else {
        eprintln!("PLAYER_LOBBY_BENCH skipped: set CINNABAR_RENDER_PACK");
        return;
    };
    if !world_carrier(&compiled).exists() {
        eprintln!("PLAYER_LOBBY_BENCH skipped: installed carriers absent");
        return;
    }
    let players = std::env::var("CINNABAR_LOBBY_PLAYERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(64);
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
    let (mut world, _, _) = build_world(&capture, Path::new(&pack), false);
    add_players(&mut world, players);
    let mut camera = world.query_filtered::<
        &mut bevy::prelude::Transform,
        bevy::prelude::With<crate::camera::FlyCamera>,
    >();
    *camera.single_mut(&mut world).unwrap() =
        bevy::prelude::Transform::from_translation(Vec3::new(0.0, 66.0, -12.0))
            .looking_at(Vec3::new(0.0, 65.0, 4.0), Vec3::Y);
    let mut clock = Instant::now();
    let mut elapsed = Series::default();
    let mut cpu = Series::default();
    let mut stages: [Series; 3] = Default::default();
    let tracked = [
        RuntimeStage::ActorAnimation,
        RuntimeStage::ActorPreparation,
        RuntimeStage::ActorRigBuild,
    ];
    for index in 0..WARM_UP_FRAMES + SAMPLE_FRAMES {
        if moving && index.is_multiple_of(3) {
            move_players(&mut world, players, index / 3 + 1);
        }
        clock += FRAME;
        world
            .resource_mut::<Time<Real>>()
            .update_with_instant(clock);
        let start = Instant::now();
        let cpu_start = thread_cpu_time();
        world.run_system_cached(prepare_actor_render_frame).unwrap();
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
        elapsed.0.push(wall_time.as_secs_f64() * 1e3);
        if let Some(time) = cpu_time {
            cpu.0.push(time.as_secs_f64() * 1e3);
        }
        for (series, stage) in stages.iter_mut().zip(tracked) {
            series
                .0
                .push(snapshot.samples[stage as usize].total.as_secs_f64() * 1e3);
        }
    }
    let drawn = world.resource::<ActorRenderFrame>().rig.instances.len();
    assert_eq!(drawn, (players as usize).min(render::MAX_RENDERED_PLAYERS));
    eprintln!(
        "PLAYER_LOBBY_BENCH players={players} moving={moving} frames={SAMPLE_FRAMES} drawn={}",
        drawn
    );
    eprintln!("PLAYER_LOBBY_BENCH wall_ms {}", elapsed.summary());
    eprintln!("PLAYER_LOBBY_BENCH cpu_ms {}", cpu.summary());
    for (series, stage) in stages.iter_mut().zip(tracked) {
        eprintln!(
            "PLAYER_LOBBY_BENCH {}_ms {}",
            stage.name(),
            series.summary()
        );
    }
    eprintln!(
        "PLAYER_LOBBY_BENCH animation {:?}",
        world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .actor_animation_stats()
    );
}
