//! Main-thread cost of streaming while the camera flies over realistic synthetic terrain.

use std::{
    collections::VecDeque,
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material,
    NO_ANIMATION, NO_MODEL_TEMPLATE, RuntimeAssets, TextureRef, VisualKind, VisualSupport,
};
use bytes::Bytes;
use chunk_pipeline::{
    CaveVisibilityScratch, CaveVisibleSet, WorldMeshChange, WorldStream, benchmark_support,
};
use client_world::ingestion::{
    LevelChunkEvent, LevelChunkMode, WorldBootstrap, vanilla_dimension_range,
};
use criterion::{BenchmarkId, Criterion, SamplingMode, criterion_group, criterion_main};
use protocol::{MovePlayerEvent, PublisherUpdateEvent, WorldEvent};
use world::{SubChunkKey, chunk_in_view};

#[path = "flight_costs/camera.rs"]
mod camera;

const AIR: u32 = 0;
const STONE: u32 = 1;
const DIRT: u32 = 2;
const GRASS: u32 = 3;
const ORE: u32 = 4;
const CUBE_IDS: u32 = 4;
/// Pacing between polls; per-frame budgets and deadlines see a 240 Hz client.
const FRAME: Duration = Duration::from_nanos(4_166_667);
const EYE_Y: f32 = 100.0;
const MESH_JOBS_PER_FRAME: usize = 32;

fn terrain_assets() -> RuntimeAssets {
    let base = RuntimeAssets::diagnostic();
    let mut texture = base.texture_array().clone();
    texture.layers = 1;
    let visual = |material| BlockVisual {
        faces: [material; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: VisualSupport::VanillaFallback,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    };
    let material = Material {
        texture: TextureRef::new(1, 0).unwrap(),
        flags: 0,
        ..Material::unvaried()
    };
    base.with_block_overlay(
        STONE,
        &BlockOverlay {
            visuals: (0..CUBE_IDS).map(visual).collect(),
            light_properties: vec![LightProperties::OPAQUE_DARK; CUBE_IDS as usize],
            materials: vec![material; CUBE_IDS as usize],
            texture: Some(texture),
            ..Default::default()
        },
    )
    .unwrap()
}

fn surface(x: i32, z: i32) -> i32 {
    let (x, z) = (x as f32, z as f32);
    (64.0 + 10.0 * (x / 23.0).sin() * (z / 19.0).cos() + 4.0 * ((x + z) / 7.0).sin()) as i32
}

fn hash(x: i32, y: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e37_79b1)
        ^ (y as u32).wrapping_mul(0x85eb_ca77)
        ^ (z as u32).wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

fn cave(x: i32, y: i32, z: i32) -> bool {
    y > -56 && (x as f32 / 9.0).sin() + (y as f32 / 7.0).sin() + (z as f32 / 11.0).sin() > 2.2
}

/// Grass over dirt over ore-flecked stone with sealed cave pockets.
fn block(x: i32, y: i32, z: i32, top: i32) -> u32 {
    if y >= top {
        AIR
    } else if y == top - 1 {
        GRASS
    } else if y >= top - 4 {
        DIRT
    } else if cave(x, y, z) {
        AIR
    } else if hash(x, y, z) % 23 == 0 {
        ORE
    } else {
        STONE
    }
}

fn push_var_i32(bytes: &mut Vec<u8>, value: i32) {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            bytes.push(byte);
            break;
        }
        bytes.push(byte | 0x80);
    }
}

/// Bedrock v9 network section: one storage, x-z-y index order, smallest legal index width.
fn push_section(bytes: &mut Vec<u8>, y_index: i32, id: impl Fn(i32, i32, i32) -> u32) {
    let mut palette = Vec::new();
    let mut indices = vec![0_u32; 4096];
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..16 {
                let value = id(x, y, z);
                let index = match palette.iter().position(|&entry| entry == value) {
                    Some(index) => index,
                    None => {
                        palette.push(value);
                        palette.len() - 1
                    }
                };
                indices[((x << 8) | (z << 4) | y) as usize] = index as u32;
            }
        }
    }
    bytes.extend([9, 1, y_index as i8 as u8]);
    if palette.len() == 1 {
        bytes.push(1);
        push_var_i32(bytes, palette[0] as i32);
        return;
    }
    let bits = [1_u32, 2, 3, 4, 5, 6, 8, 16]
        .into_iter()
        .find(|bits| 1_usize << bits >= palette.len())
        .unwrap();
    bytes.push(((bits << 1) | 1) as u8);
    let per_word = (32 / bits) as usize;
    for word in indices.chunks(per_word) {
        let packed = word
            .iter()
            .enumerate()
            .fold(0_u32, |packed, (slot, &index)| {
                packed | (index << (slot as u32 * bits))
            });
        bytes.extend_from_slice(&packed.to_le_bytes());
    }
    push_var_i32(bytes, palette.len() as i32);
    for entry in palette {
        push_var_i32(bytes, entry as i32);
    }
}

/// Inline LevelChunk payload holding every section up to the column's highest surface.
fn column_payload(cx: i32, cz: i32) -> (usize, Bytes) {
    let range = vanilla_dimension_range(0).unwrap();
    let tops: [[i32; 16]; 16] = std::array::from_fn(|x| {
        std::array::from_fn(|z| surface(cx * 16 + x as i32, cz * 16 + z as i32))
    });
    let highest = tops.iter().flatten().copied().max().unwrap();
    let count = ((highest - 1).div_euclid(16) - range.base_sub_chunk_y + 1) as usize;
    let mut payload = Vec::new();
    for offset in 0..count {
        let y_index = range.base_sub_chunk_y + offset as i32;
        push_section(&mut payload, y_index, |x, y, z| {
            let (wx, wy, wz) = (cx * 16 + x, y_index * 16 + y, cz * 16 + z);
            block(wx, wy, wz, tops[x as usize][z as usize])
        });
    }
    payload.extend([1, 0]); // Uniform biome zero followed by extruded upper slots.
    payload.extend(std::iter::repeat_n(0xff, range.sub_chunk_count - 1));
    payload.push(0); // Border-block count; no block entities.
    (count, Bytes::from(payload))
}

/// What a server sends while the local player flies: its position, view centre and new columns.
enum Ingress {
    Radius(i32),
    Position([f32; 3]),
    Publisher([i32; 3], u32),
    Column(i32, i32, usize, Bytes),
}

struct Flight {
    stream: WorldStream,
    radius: i32,
    blocks_per_second: f32,
    camera_x: f32,
    center_x: i32,
    sequence: u64,
    backlog: VecDeque<Ingress>,
    next_frame: Instant,
}

impl Flight {
    fn new(assets: &Arc<RuntimeAssets>, radius: i32) -> Self {
        let camera = [8.0, EYE_Y, 8.0];
        let stream = WorldStream::new_with_assets(
            WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: camera,
                world_spawn_position: [8, 64, 8],
                air_network_id: AIR,
                block_network_ids_are_hashes: false,
            },
            Arc::clone(assets),
            camera,
            None,
        );
        let mut flight = Self {
            stream,
            radius,
            blocks_per_second: 0.0,
            camera_x: 8.0,
            center_x: 0,
            sequence: 0,
            backlog: VecDeque::new(),
            next_frame: Instant::now(),
        };
        flight.backlog.push_back(Ingress::Radius(radius));
        flight
            .backlog
            .push_back(Ingress::Position(flight.server_position()));
        flight.publish_view();
        flight.enqueue_entering_columns(None);
        flight.settle();
        flight
    }

    fn camera(&self) -> [f32; 3] {
        [self.camera_x, EYE_Y, 8.0]
    }

    fn server_position(&self) -> [f32; 3] {
        [(self.center_x * 16 + 8) as f32, EYE_Y, 8.0]
    }

    fn publish_view(&mut self) {
        self.backlog.push_back(Ingress::Publisher(
            [self.center_x * 16 + 8, EYE_Y as i32, 8],
            (self.radius * 16) as u32,
        ));
    }

    /// Columns the client grid retains around `center_x` but not around `previous`, nearest first.
    fn enqueue_entering_columns(&mut self, previous: Option<i32>) {
        let (radius, center) = (self.radius, self.center_x);
        let reach = radius + world::CHUNK_VIEW_SLACK;
        let retained = |middle: i32, x: i32, z: i32| chunk_in_view(radius, [x, z], [middle, 0]);
        let mut columns: Vec<_> = (center - reach..=center + reach)
            .flat_map(|x| (-reach..=reach).map(move |z| (x, z)))
            .filter(|&(x, z)| {
                retained(center, x, z) && previous.is_none_or(|old| !retained(old, x, z))
            })
            .collect();
        columns.sort_by_key(|&(x, z)| (x - center).pow(2) + z * z);
        for (x, z) in columns {
            self.enqueue_column(x, z);
        }
    }

    fn enqueue_column(&mut self, x: i32, z: i32) {
        let (count, payload) = column_payload(x, z);
        self.backlog
            .push_back(Ingress::Column(x, z, count, payload));
    }

    fn submit(&mut self, ingress: Ingress) {
        self.sequence += 1;
        let sequence = self.sequence;
        match ingress {
            Ingress::Radius(radius) => self
                .stream
                .submit(sequence, WorldEvent::ChunkRadiusUpdated(radius)),
            // Client retention follows the server-confirmed player position.
            Ingress::Position(position) => self.stream.submit(
                sequence,
                WorldEvent::MovePlayer(MovePlayerEvent {
                    runtime_id: 1,
                    position,
                    ..MovePlayerEvent::default()
                }),
            ),
            Ingress::Publisher(center, radius_blocks) => self.stream.submit(
                sequence,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center,
                    radius_blocks,
                }),
            ),
            Ingress::Column(x, z, count, payload) => self.stream.submit_level_chunk_bytes(
                sequence,
                LevelChunkEvent {
                    dimension: 0,
                    x,
                    z,
                    mode: LevelChunkMode::Inline { count },
                    payload: Vec::new(),
                },
                payload,
            ),
        }
        .expect("bounded flight admission");
    }

    /// Drains queued ingress and stream work with a stationary camera; never measured.
    fn settle(&mut self) {
        let speed = std::mem::take(&mut self.blocks_per_second);
        let started = Instant::now();
        while !(self.backlog.is_empty()
            && self.stream.committed_sequence() == self.sequence
            && benchmark_support::work_is_idle(&self.stream))
        {
            self.frame();
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "flight fixture failed to settle: {:?}",
                self.stream.stats()
            );
        }
        self.blocks_per_second = speed;
        let stats = self.stream.stats();
        assert_eq!(stats.decode_errors, 0);
        assert_eq!(stats.normalization_errors, 0);
        assert_eq!(stats.light_solve_failures, 0);
    }

    /// One paced client frame; returns only the time spent in stream calls.
    fn frame(&mut self) -> Duration {
        self.camera_x += self.blocks_per_second * FRAME.as_secs_f32();
        let camera_chunk = (self.camera_x.floor() as i32).div_euclid(16);
        while self.center_x < camera_chunk {
            self.center_x += 1;
            self.backlog
                .push_back(Ingress::Position(self.server_position()));
            self.publish_view();
            self.enqueue_entering_columns(Some(self.center_x - 1));
        }
        let camera = self.camera();
        let started = Instant::now();
        self.stream.begin_frame_work();
        for _ in 0..self.stream.remaining_admission_capacity() {
            let Some(ingress) = self.backlog.pop_front() else {
                break;
            };
            self.submit(ingress);
        }
        self.stream.set_view_forward([1.0, 0.0, 0.0]);
        black_box(self.stream.poll(camera, MESH_JOBS_PER_FRAME));
        while let Some(change) = self.stream.pop_mesh_change() {
            let (key, generation, dirty_since) = match change {
                WorldMeshChange::Upsert {
                    key,
                    generation,
                    dirty_since,
                    mesh,
                    biome,
                    ..
                } => {
                    black_box((&mesh, &biome));
                    (key, generation, dirty_since)
                }
                WorldMeshChange::Remove {
                    key,
                    generation,
                    dirty_since,
                    ..
                } => (key, generation, dirty_since),
            };
            self.stream
                .acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
        }
        let elapsed = started.elapsed();
        let now = Instant::now();
        self.next_frame = (self.next_frame + FRAME).max(now);
        while Instant::now() < self.next_frame {
            std::hint::spin_loop();
        }
        elapsed
    }

    /// Times only the server position update that retires the trailing row, then
    /// streams the leading row and settles so every crossing starts from a full view.
    fn cross_one_chunk(&mut self) -> Duration {
        self.center_x += 1;
        self.camera_x = self.server_position()[0];
        let position = Ingress::Position(self.server_position());
        let started = Instant::now();
        self.submit(position);
        let elapsed = started.elapsed();
        self.publish_view();
        self.enqueue_entering_columns(Some(self.center_x - 1));
        self.settle();
        elapsed
    }
}

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    sorted[(sorted.len() - 1) * percent / 100]
}

fn flight_benches(c: &mut Criterion) {
    let assets = Arc::new(terrain_assets());
    let mut group = c.benchmark_group("pipeline/flight_main_thread_per_frame");
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));
    for (radius, blocks_per_second) in [
        (10, 0.0_f32),
        (10, 32.0),
        (10, 128.0),
        (16, 0.0),
        (16, 128.0),
    ] {
        let id = BenchmarkId::new(
            format!("radius_{radius}"),
            format!("{blocks_per_second}_blocks_per_s"),
        );
        let mut fixture = None;
        group.bench_function(id, |b| {
            let flight = fixture.get_or_insert_with(|| {
                let mut flight = Flight::new(&assets, radius);
                flight.blocks_per_second = blocks_per_second;
                let resident_before = flight.stream.loaded_column_count();
                let acknowledged_before = flight
                    .stream
                    .stats()
                    .phase2_stages
                    .mesh_uploads_acknowledged;
                // Two seconds of flight: per-frame distribution and work witnesses.
                let mut frames: Vec<_> = (0..480).map(|_| flight.frame()).collect();
                frames.sort_unstable();
                let stats = flight.stream.stats();
                assert_eq!(stats.decode_errors, 0);
                assert_eq!(stats.light_solve_failures, 0);
                let grid = (2 * (radius + world::CHUNK_VIEW_SLACK) + 1) as usize;
                assert!(
                    flight.stream.loaded_column_count() <= grid * grid,
                    "retention bounds residency"
                );
                let published = stats.phase2_stages.mesh_uploads_acknowledged - acknowledged_before;
                if blocks_per_second > 0.0 {
                    assert!(published > 0, "flight published no meshes");
                }
                let total: Duration = frames.iter().sum();
                eprintln!(
                    "FLIGHT_PREFLIGHT radius={radius} blocks_per_second={blocks_per_second} resident_columns={resident_before}->{} meshes_published={published} backlog={} main_ms_per_frame_mean={:.3} p50_us={} p99_us={} max_us={}",
                    flight.stream.loaded_column_count(),
                    flight.backlog.len(),
                    total.as_secs_f64() * 1000.0 / frames.len() as f64,
                    percentile(&frames, 50).as_micros(),
                    percentile(&frames, 99).as_micros(),
                    frames.last().unwrap().as_micros(),
                );
                flight
            });
            b.iter_custom(|frames| (0..frames).map(|_| flight.frame()).sum());
        });
    }
    group.finish();
}

fn eviction_benches(c: &mut Criterion) {
    let assets = Arc::new(terrain_assets());
    let mut group = c.benchmark_group("pipeline/row_eviction_on_chunk_crossing");
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));
    group.measurement_time(Duration::from_millis(300));
    for radius in [10, 16] {
        let mut fixture = None;
        group.bench_function(BenchmarkId::new("radius", radius), |b| {
            let flight = fixture.get_or_insert_with(|| {
                let mut flight = Flight::new(&assets, radius);
                let columns = flight.stream.loaded_column_count();
                let sub_chunks = flight.stream.stats().resident_sub_chunks;
                let elapsed = flight.cross_one_chunk();
                assert_eq!(flight.stream.loaded_column_count(), columns);
                assert_eq!(flight.stream.stats().resident_sub_chunks, sub_chunks);
                eprintln!(
                    "EVICTION_PREFLIGHT radius={radius} resident_columns={columns} resident_sub_chunks={sub_chunks} retire_row_us={}",
                    elapsed.as_micros()
                );
                flight
            });
            b.iter_custom(|crossings| (0..crossings).map(|_| flight.cross_one_chunk()).sum());
        });
    }
    group.finish();
}

fn cave_visibility_benches(c: &mut Criterion) {
    let assets = Arc::new(terrain_assets());
    let mut group = c.benchmark_group("pipeline/cave_visibility_full_search");
    for radius in [10, 16] {
        let flight = Flight::new(&assets, radius);
        // A sealed pocket near the origin, found from the same deterministic terrain.
        let pocket = (-50..0)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .find(|&(x, y)| cave(x, y, 8))
            .expect("terrain has a cave pocket near the origin");
        for (name, camera) in [
            (
                "surface",
                SubChunkKey::new(0, 0, (EYE_Y as i32).div_euclid(16), 0),
            ),
            (
                "cave",
                SubChunkKey::new(0, pocket.0.div_euclid(16), pocket.1.div_euclid(16), 0),
            ),
        ] {
            let mut scratch = CaveVisibilityScratch::default();
            let mut visible = CaveVisibleSet::default();
            flight
                .stream
                .cave_visible_sub_chunks_into(camera, &mut scratch, &mut visible);
            let reached = visible.iter().count();
            eprintln!(
                "CAVE_PREFLIGHT radius={radius} camera={name} resident_sub_chunks={} visible={reached}",
                flight.stream.stats().resident_sub_chunks
            );
            assert!(reached > 0);
            group.bench_with_input(
                BenchmarkId::new(format!("radius_{radius}"), name),
                &camera,
                |b, &camera| {
                    b.iter(|| {
                        flight.stream.cave_visible_sub_chunks_into(
                            camera,
                            &mut scratch,
                            &mut visible,
                        );
                        black_box(&visible);
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    flight_benches,
    eviction_benches,
    cave_visibility_benches,
    camera::camera_benches
);
criterion_main!(benches);
