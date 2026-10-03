//! Env-gated lobby frame benchmark: replays a captured server session into a world stream and
//! times the real actor publication system per frame.
//! Run: `CINNABAR_LOBBY_CAPTURE=<raw.bin> CINNABAR_RENDER_PACK=<uuid_version.zip> cargo test -p
//! bedrock-client --lib lobby_frame_bench -- --ignored --nocapture`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::{
    math::Vec3,
    prelude::World,
    time::{Real, Time},
};
use client_world::WorldStream;
use protocol::{ActorKind, BedrockSession, WorldBootstrap, WorldEvent};
use render::{ActorRenderFrame, RuntimeStage, RuntimeStageProfiler};

use crate::runtime::network::{
    HandRigBuilder, prepare_actor_render_frame, publish_actor_render_frame,
};

mod gpu_replay;
mod join_setup;
mod player_report;

const FRAME: Duration = Duration::from_nanos(16_666_667);
const COMPILED: &str = "../.local/assets/compiled";

/// Packets that only build terrain; the actor path never reads them.
const TERRAIN_PACKETS: [u32; 2] = [58, 174];
const START_GAME: u32 = 11;

struct Capture {
    bootstrap: WorldBootstrap,
    /// `(packet id, body)` after StartGame, in arrival order.
    packets: Vec<(u32, Vec<u8>)>,
}

fn read_capture(path: &Path) -> Capture {
    let bytes = std::fs::read(path).unwrap();
    let mut records = Vec::new();
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let id = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        records.push((id, bytes[at + 8..at + 8 + len].to_vec()));
        at += 8 + len;
    }
    let start = records
        .iter()
        .position(|(id, _)| *id == START_GAME)
        .unwrap();
    let bootstrap = start_game_bootstrap(&records[start].1);
    Capture {
        bootstrap,
        packets: records.split_off(start + 1),
    }
}

fn read_varint(bytes: &[u8], at: &mut usize) -> u64 {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = bytes[*at];
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            break;
        }
    }
    value
}

/// StartGame opens with the unique id, runtime id, game mode and feet position.
fn start_game_bootstrap(body: &[u8]) -> WorldBootstrap {
    let mut at = 0;
    let unique = read_varint(body, &mut at);
    let unique_id = ((unique >> 1) as i64) ^ -((unique & 1) as i64);
    let runtime_id = read_varint(body, &mut at);
    let _game_mode = read_varint(body, &mut at);
    let position: [f32; 3] = std::array::from_fn(|axis| {
        f32::from_le_bytes(body[at + axis * 4..at + axis * 4 + 4].try_into().unwrap())
    });
    WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: runtime_id,
        local_player_unique_id: unique_id,
        player_position: position,
        world_spawn_position: position.map(|value| value as i32),
        air_network_id: 0,
        block_network_ids_are_hashes: true,
    }
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Feeds one captured packet through the client's decoder into the stream.
struct Replay {
    session: BedrockSession,
    sequence: u64,
    local_position: [f32; 3],
    local_runtime_id: u64,
    rejected: u64,
    /// Rejections by packet id, and the first error of each.
    rejections: std::collections::BTreeMap<u32, (u64, String)>,
}

impl Replay {
    fn apply(&mut self, stream: &mut WorldStream, id: u32, body: &[u8]) {
        if TERRAIN_PACKETS.contains(&id) {
            return;
        }
        let mut header = Vec::new();
        write_varint(&mut header, u64::from(id));
        let mut batch = vec![0xfe];
        write_varint(&mut batch, (header.len() + body.len()) as u64);
        batch.extend_from_slice(&header);
        batch.extend_from_slice(body);
        let packets = match protocol::decode_batch(batch.into(), &self.session) {
            Ok(packets) => packets,
            Err(error) => {
                self.reject(id, error.to_string());
                return;
            }
        };
        for packet in packets {
            let event = match protocol::into_world_event(packet, 0) {
                Ok(Some(event)) => event,
                Ok(None) => continue,
                Err(error) => {
                    self.reject(id, error.to_string());
                    continue;
                }
            };
            if let WorldEvent::MovePlayer(movement) = &event
                && movement.runtime_id == self.local_runtime_id
            {
                self.local_position = movement.position;
            }
            self.sequence += 1;
            // The stream admits a bounded backlog; the frame loop's poll applies it.
            if self.sequence.is_multiple_of(32) {
                drain_through(stream, self.local_position, self.sequence - 1);
            }
            if let Err(error) = stream.submit(self.sequence, event) {
                self.reject(id, format!("{error:?}"));
            }
        }
    }

    fn reject(&mut self, id: u32, error: String) {
        self.rejected += 1;
        self.rejections.entry(id).or_insert((0, error)).0 += 1;
    }
}

/// Applies admitted events and discards the queues the app's other systems consume.
fn drain(stream: &mut WorldStream, camera: [f32; 3]) {
    stream.poll(camera, 0);
    let _ = stream.take_committed_controls();
    let _ = stream.take_committed_ui();
    let _ = stream.take_committed_audio();
    let _ = stream.take_committed_particles();
    let _ = stream.take_committed_camera();
    let _ = stream.take_actor_status_notices();
    let _ = stream.take_equipment_notices();
}

/// Finishes the fixture's submitted FIFO work before advancing its synthetic frame clock.
fn drain_through(stream: &mut WorldStream, camera: [f32; 3], sequence: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        drain(stream, camera);
        let committed = stream
            .inventory_committed_through()
            .expect("captured replay must retain a valid commit frontier");
        if committed >= sequence {
            return;
        }
        assert!(Instant::now() < deadline, "captured replay commit stalled");
        std::thread::yield_now();
    }
}

struct Population {
    players: usize,
    entities: usize,
    by_identifier: std::collections::BTreeMap<String, usize>,
}

fn population(stream: &WorldStream) -> Population {
    let mut result = Population {
        players: 0,
        entities: 0,
        by_identifier: Default::default(),
    };
    for rig in stream.actor_rigs() {
        let Some(actor) = stream.actor(rig.actor.runtime_id) else {
            continue;
        };
        match &actor.kind {
            ActorKind::Player { .. } => result.players += 1,
            ActorKind::Entity { identifier } => {
                result.entities += 1;
                *result
                    .by_identifier
                    .entry(identifier.to_string())
                    .or_default() += 1;
            }
        }
    }
    result
}

/// Centroid of the non-player actors, which the lobby camera faces.
fn entity_centroid(stream: &WorldStream) -> Option<Vec3> {
    let points: Vec<Vec3> = stream
        .actor_rigs()
        .filter_map(|rig| stream.actor(rig.actor.runtime_id))
        .filter(|actor| matches!(actor.kind, ActorKind::Entity { .. }))
        .map(|actor| Vec3::from_array(actor.position))
        .collect();
    (!points.is_empty()).then(|| points.iter().sum::<Vec3>() / points.len() as f32)
}

fn build_world(
    capture: &Capture,
    pack_path: &Path,
    away: bool,
) -> (World, Vec<(u32, Vec<u8>)>, Replay) {
    let compiled = PathBuf::from(
        std::env::var_os("CINNABAR_RENDER_CARRIERS").unwrap_or_else(|| COMPILED.into()),
    );
    let loaded = crate::asset_startup::load_runtime_assets(crate::asset_startup::AssetSelection {
        path: compiled.join("vanilla-v2193.mcbea"),
        source: crate::asset_startup::AssetPathSource::CommandLine,
    })
    .unwrap();
    let entity_runtime = Arc::clone(loaded.entities.runtime());
    let artwork =
        crate::asset_startup::require_actor_artwork(&loaded.selected_path, &loaded.entities)
            .unwrap();
    let icons = crate::asset_startup::require_icon_assets(
        &loaded.selected_path,
        crate::asset_startup::vanilla_source_manifest_json(),
    )
    .unwrap();
    let equipment_catalog = crate::asset_startup::load_optional_equipment_assets(
        &loaded.selected_path,
        &loaded.entities,
    );
    let (equipment, artwork, equipment_geometries) =
        crate::presentation::equipment::EquipmentRuntime::build(
            Arc::clone(&entity_runtime),
            equipment_catalog,
            Arc::clone(icons.runtime()),
            Some(Arc::clone(&loaded.runtime)),
            crate::asset_startup::load_optional_block_entity_assets(&loaded.selected_path),
            artwork,
        );
    let mut scene = render::ActorRenderScene::with_runtime_entity_assets_and_equipment(
        &entity_runtime,
        &equipment_geometries,
    )
    .unwrap();
    scene.configure_artwork(artwork.clone());
    let hand = HandRigBuilder::from_runtime_assets(&entity_runtime).unwrap();
    if let Some(refs) = std::fs::read(compiled.join("vanilla-v1.vanillarefs.json"))
        .ok()
        .and_then(|bytes| assets::VanillaEntityRefs::from_json(&bytes))
    {
        super::set_vanilla_refs(refs);
    }
    let view = super::super::local_pack::local_pack_view_at(pack_path).unwrap();
    let pack = super::compile(&view).expect("the pack defines entities");

    let mut stream = WorldStream::new_with_asset_sets(
        capture.bootstrap,
        Arc::clone(&loaded.runtime),
        Arc::clone(&entity_runtime),
        capture.bootstrap.player_position,
        None,
    );
    stream.set_pack_entities(Some((
        Arc::clone(&pack.assets),
        pack.bindings
            .iter()
            .map(|binding| binding.geometry_candidate)
            .collect(),
    )));
    stream.seed_property_defaults(&super::pack_property_defaults(&view));
    let mut replay = Replay {
        session: BedrockSession { shield_item_id: 0 },
        sequence: 0,
        local_position: capture.bootstrap.player_position,
        local_runtime_id: capture.bootstrap.local_player_runtime_id,
        rejected: 0,
        rejections: Default::default(),
    };
    // The first 40% of the capture builds the lobby; the rest plays out during the frames.
    let split = capture.packets.len() * 2 / 5;
    for (id, body) in &capture.packets[..split] {
        replay.apply(&mut stream, *id, body);
    }
    drain_through(&mut stream, replay.local_position, replay.sequence);
    // The camera stands at the local player's eye facing the NPCs, or directly away.
    let eye = Vec3::from_array(replay.local_position) + Vec3::Y * 1.62;
    let target = entity_centroid(&stream).unwrap_or(eye + Vec3::NEG_Z);
    let target = if away { eye * 2.0 - target } else { target };
    let mut client_world = crate::runtime::world::ClientWorld::new_with_entity_assets(
        Arc::clone(&loaded.runtime),
        entity_runtime,
    );
    client_world.pack_entities = Some(pack);
    client_world.stream = Some(stream);
    let mut world = crate::tests::actor_frame_allocations::actor_frame_world(
        client_world,
        scene,
        artwork,
        hand,
        (eye, target),
    );
    world.insert_resource(equipment);
    world.insert_resource(RuntimeStageProfiler::new(true));
    let rest = capture.packets[split..].to_vec();
    (world, rest, replay)
}

/// Draws and vertex shader invocations as the actor pass issues them: one draw per run of
/// instances sharing a page and geometry, each with that geometry's vertex count.
fn gpu_draws(frame: &ActorRenderFrame) -> (usize, u64) {
    let rig = &frame.rig;
    let (mut draws, mut invocations, mut last) = (0, 0, None);
    for (page, instance) in frame.instance_pages().iter().zip(rig.instances.iter()) {
        let vertex_count = rig
            .geometry_spans
            .get(instance.geometry_id as usize)
            .map_or(0, |span| span.vertex_count);
        if last != Some((*page, instance.geometry_id)) {
            draws += 1;
            last = Some((*page, instance.geometry_id));
        }
        invocations += u64::from(vertex_count);
    }
    (draws, invocations)
}

/// Order-independent digest of everything each drawn instance sends the GPU: identity, layer,
/// transform, texture, tint, light, uv_anim, geometry size and both bone palettes.
fn frame_digest(frame: &ActorRenderFrame) -> u64 {
    use std::hash::{Hash, Hasher};
    let rig = &frame.rig;
    let skin = render::STANDARD_SKIN_BYTES;
    let mut records: Vec<(u64, u8, u64)> = rig
        .instances
        .iter()
        .zip(rig.manifest.iter())
        .zip(frame.instance_pages())
        .map(|((instance, entry), page)| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let bits = |values: &[f32], hasher: &mut std::collections::hash_map::DefaultHasher| {
                for value in values {
                    value.to_bits().hash(hasher);
                }
            };
            bits(instance.world_from_actor.as_flattened(), &mut hasher);
            bits(&instance.uv_anim, &mut hasher);
            // Skin slots and skin rig ids are allocation order; their pixels are what draws.
            if *page == 0 {
                let layer = instance.texture_layer as usize;
                frame
                    .skins_rgba8
                    .get(layer * skin..(layer + 1) * skin)
                    .hash(&mut hasher);
            } else {
                (page, instance.texture_layer).hash(&mut hasher);
            }
            (
                instance.tint,
                instance.overlay_rgba8,
                instance.light,
                entry.bone_count,
            )
                .hash(&mut hasher);
            rig.geometry_spans[instance.geometry_id as usize]
                .vertex_count
                .hash(&mut hasher);
            let bones = entry.bone_count as usize;
            for palette in [
                &rig.previous_bones[entry.previous_bone_base as usize..][..bones],
                &rig.current_bones[entry.current_bone_base as usize..][..bones],
            ] {
                for matrix in palette {
                    bits(matrix.as_flattened(), &mut hasher);
                }
            }
            (
                entry.identity.runtime_id,
                entry.identity.layer,
                hasher.finish(),
            )
        })
        .collect();
    records.sort_unstable();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    records.hash(&mut hasher);
    hasher.finish()
}

#[repr(C)]
#[cfg(any(target_os = "macos", target_os = "linux"))]
struct Timespec {
    seconds: i64,
    nanoseconds: i64,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
unsafe extern "C" {
    fn clock_gettime(clock: i32, time: *mut Timespec) -> i32;
}

#[cfg(target_os = "macos")]
const THREAD_CPU_CLOCK: i32 = 16;
#[cfg(target_os = "linux")]
const THREAD_CPU_CLOCK: i32 = 3;

/// CPU time this thread has run, which preemption by other processes does not inflate.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn thread_cpu_time() -> Option<Duration> {
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: `time` is a valid out pointer for the duration of the call.
    let result = unsafe { clock_gettime(THREAD_CPU_CLOCK, &raw mut time) };
    (result == 0).then(|| Duration::new(time.seconds as u64, time.nanoseconds as u32))
}

/// Keeps the benchmark's other measurements available without a native thread CPU clock.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn thread_cpu_time() -> Option<Duration> {
    None
}

#[derive(Default)]
struct Series(Vec<f64>);

impl Series {
    fn summary(&mut self) -> String {
        if self.0.is_empty() {
            return "-".into();
        }
        self.0.sort_by(f64::total_cmp);
        let mean = self.0.iter().sum::<f64>() / self.0.len() as f64;
        let at = |q: f64| self.0[((self.0.len() - 1) as f64 * q) as usize];
        format!(
            "mean={mean:.3} p50={:.3} p95={:.3} max={:.3}",
            at(0.5),
            at(0.95),
            at(1.0)
        )
    }
}

#[test]
#[ignore = "benchmark; needs a lobby capture and its cached pack"]
fn lobby_frame_bench() {
    let capture = std::env::var_os("CINNABAR_LOBBY_CAPTURE")
        .expect("offline lobby timing requires CINNABAR_LOBBY_CAPTURE");
    let pack = std::env::var_os("CINNABAR_RENDER_PACK")
        .expect("offline lobby timing requires CINNABAR_RENDER_PACK");
    let frames: usize = std::env::var("CINNABAR_LOBBY_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(900);
    let away = std::env::var_os("CINNABAR_LOBBY_LOOK_AWAY").is_some();
    let digest = std::env::var_os("CINNABAR_LOBBY_DIGEST").is_some();
    let capture = read_capture(Path::new(&capture));
    let (mut world, rest, mut replay) = build_world(&capture, Path::new(&pack), away);

    let started = Instant::now();
    let mut clock = started;
    let (mut total, mut cpu, mut allocations) =
        (Series::default(), Series::default(), Series::default());
    let mut stages: [Series; 4] = Default::default();
    let tracked = [
        RuntimeStage::ActorPublication,
        RuntimeStage::ActorAnimation,
        RuntimeStage::ActorPreparation,
        RuntimeStage::ActorRigBuild,
    ];
    let (mut instances, mut vertices, mut vertex_invocations, mut draw_calls) = (
        Series::default(),
        Series::default(),
        Series::default(),
        Series::default(),
    );
    let mut next_packet = 0usize;
    for frame_index in 0..frames {
        let frame = frame_index;
        let due = rest.len() * (frame + 1) / frames;
        {
            let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
            let stream = client_world.stream.as_mut().unwrap();
            while next_packet < due {
                let (id, body) = &rest[next_packet];
                replay.apply(stream, *id, body);
                next_packet += 1;
            }
            drain_through(stream, replay.local_position, replay.sequence);
        }
        clock += FRAME;
        world
            .resource_mut::<Time<Real>>()
            .update_with_instant(clock);
        let before = crate::tests::alloc_count::thread_allocations();
        let (timer, cpu_timer) = (Instant::now(), thread_cpu_time());
        world.run_system_cached(prepare_actor_render_frame).unwrap();
        world.run_system_cached(publish_actor_render_frame).unwrap();
        let elapsed = timer.elapsed();
        let cpu_elapsed = thread_cpu_time()
            .zip(cpu_timer)
            .map(|(after, before)| after - before);
        let allocated = crate::tests::alloc_count::thread_allocations() - before;
        let snapshot = world
            .resource::<RuntimeStageProfiler>()
            .take_snapshot_if_due(Duration::ZERO)
            .unwrap();
        // The first second warms caches and registers skin models.
        if frame < 60 {
            continue;
        }
        if let Some(cpu_elapsed) = cpu_elapsed
            && cpu_elapsed > Duration::from_millis(4)
        {
            let stage =
                |stage: RuntimeStage| snapshot.samples[stage as usize].total.as_secs_f64() * 1e3;
            eprintln!(
                "LOBBY_BENCH spike frame={frame} cpu={:.3} total={:.3} animation={:.3} preparation={:.3} rig_build={:.3} allocations={allocated}",
                cpu_elapsed.as_secs_f64() * 1e3,
                elapsed.as_secs_f64() * 1e3,
                stage(RuntimeStage::ActorAnimation),
                stage(RuntimeStage::ActorPreparation),
                stage(RuntimeStage::ActorRigBuild),
            );
        }
        total.0.push(elapsed.as_secs_f64() * 1e3);
        if let Some(cpu_elapsed) = cpu_elapsed {
            cpu.0.push(cpu_elapsed.as_secs_f64() * 1e3);
        }
        allocations.0.push(allocated as f64);
        for (series, stage) in stages.iter_mut().zip(tracked) {
            series
                .0
                .push(snapshot.samples[stage as usize].total.as_secs_f64() * 1e3);
        }
        let frame = world.resource::<ActorRenderFrame>();
        if digest {
            eprintln!(
                "LOBBY_DIGEST frame={frame_index} hash={:016x}",
                frame_digest(frame)
            );
        }
        let rig = &frame.rig;
        instances.0.push(rig.instances.len() as f64);
        vertices.0.push(f64::from(rig.maximum_vertex_count));
        let (draws, invocations) = gpu_draws(frame);
        draw_calls.0.push(draws as f64);
        vertex_invocations.0.push(invocations as f64);
    }
    let stream = world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap();
    let population = population(stream);
    eprintln!(
        "LOBBY_BENCH frames={} look_away={away} players={} entities={} rejected_packets={} {:?}",
        frames - 60,
        population.players,
        population.entities,
        replay.rejected,
        population.by_identifier
    );
    for (id, (count, error)) in &replay.rejections {
        eprintln!("LOBBY_BENCH rejected packet {id} x{count}: {error}");
    }
    eprintln!("LOBBY_BENCH system_ms {}", total.summary());
    if cpu.0.is_empty() {
        eprintln!("LOBBY_BENCH system_cpu_ms unavailable");
    } else {
        eprintln!("LOBBY_BENCH system_cpu_ms {}", cpu.summary());
    }
    for (series, stage) in stages.iter_mut().zip(tracked) {
        eprintln!("LOBBY_BENCH {}_ms {}", stage.name(), series.summary());
    }
    eprintln!("LOBBY_BENCH allocations {}", allocations.summary());
    eprintln!("LOBBY_BENCH instances {}", instances.summary());
    eprintln!("LOBBY_BENCH max_vertices {}", vertices.summary());
    eprintln!("LOBBY_BENCH draw_calls {}", draw_calls.summary());
    eprintln!(
        "LOBBY_BENCH gpu_vertex_invocations {}",
        vertex_invocations.summary()
    );
    eprintln!(
        "LOBBY_BENCH catalog_vertices={} geometries={}",
        world
            .resource::<ActorRenderFrame>()
            .rig
            .geometry_vertices
            .len(),
        world
            .resource::<ActorRenderFrame>()
            .rig
            .geometry_spans
            .len()
    );
    eprintln!("LOBBY_BENCH animation {:?}", stream.actor_animation_stats());
}
