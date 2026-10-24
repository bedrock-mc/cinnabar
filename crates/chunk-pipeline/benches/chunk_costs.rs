#![expect(
    clippy::print_stderr,
    reason = "the benchmark reports its preflight measurement lines on stderr"
)]

use std::{
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material,
    NO_ANIMATION, NO_MODEL_TEMPLATE, RuntimeAssets, TextureRef, VisualKind, VisualSupport,
};
use bytes::Bytes;
use chunk_pipeline::{WorldMeshChange, WorldStream, WorldStreamStats, benchmark_support};
use client_world::ingestion::{
    LevelChunkEvent, LevelChunkMode, WorldBootstrap, vanilla_dimension_range,
};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use protocol::{PublisherUpdateEvent, WorldEvent};

const CAMERA: [f32; 3] = [8.0, 8.0, 8.0];
const SECTIONS_PER_COLUMN: usize = 4;
const AIR: u32 = 0;
const SOLID: u32 = 1;
/// A second opaque block with the same light inputs as [`SOLID`], as a bulk replace sends.
const SOLID_SWAPPED: u32 = 2;

struct BurstFixture {
    stream: WorldStream,
    payloads: Arc<[Bytes; SECTIONS_PER_COLUMN]>,
    sections: usize,
    background_columns: usize,
    /// Columns already drained before this burst, whose sequences it follows.
    drained_columns: usize,
}

impl BurstFixture {
    fn columns(&self) -> usize {
        self.sections.div_ceil(SECTIONS_PER_COLUMN)
    }
}

fn inline_payload(sections: usize) -> Bytes {
    inline_payload_of(sections, SOLID)
}

/// An inline column whose lowest `sections` are uniform `block`.
fn inline_payload_of(sections: usize, block: u32) -> Bytes {
    let range = vanilla_dimension_range(0).unwrap();
    let mut payload = Vec::new();
    for offset in 0..sections {
        let y = range.base_sub_chunk_y + offset as i32;
        payload.extend([9, 1, y as i8 as u8, 1, (block << 1) as u8]);
    }
    payload.extend([1, 0]); // Uniform biome zero followed by extruded upper slots.
    payload.extend(std::iter::repeat_n(0xff, range.sub_chunk_count - 1));
    payload.push(0); // Border-block count; no block entities.
    Bytes::from(payload)
}

/// A two-entry packed palette in unchanged sections, optionally replacing only the top section.
fn inline_packed_payload(sections: usize, changed_top: bool) -> Bytes {
    let range = vanilla_dimension_range(0).unwrap();
    let bits = 2_u32;
    let indices_per_word = u32::BITS / bits;
    let word =
        (0..indices_per_word).fold(0_u32, |word, index| word | ((index % 2) << (index * bits)));
    let mut payload = Vec::new();
    for offset in 0..sections {
        let y = range.base_sub_chunk_y + offset as i32;
        if changed_top && offset + 1 == sections {
            payload.extend([9, 1, y as i8 as u8, 1, (SOLID_SWAPPED << 1) as u8]);
        } else {
            payload.extend([9, 1, y as i8 as u8, ((bits << 1) | 1) as u8]);
            for _ in 0..world::BLOCKS_PER_SUB_CHUNK.div_ceil(indices_per_word as usize) {
                payload.extend(word.to_le_bytes());
            }
            payload.extend([2 << 1, (SOLID << 1) as u8, (SOLID_SWAPPED << 1) as u8]);
        }
    }
    payload.extend(inline_payload(0));
    Bytes::from(payload)
}

fn terrain_assets() -> RuntimeAssets {
    let base = RuntimeAssets::diagnostic();
    let mut texture = base.texture_array().clone();
    texture.layers = 1;
    base.with_block_overlay(
        SOLID,
        &BlockOverlay {
            visuals: vec![
                BlockVisual {
                    native_material: assets::native_material::NativeMaterialFacts::Unresolved,
                    faces: [0; 6],
                    flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                    kind: VisualKind::Cube,
                    support: VisualSupport::VanillaFallback,
                    contributor_role: ContributorRole::Primary,
                    model_template: NO_MODEL_TEMPLATE,
                    animation: NO_ANIMATION,
                    variant: 0,
                    destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
                };
                2
            ],
            light_properties: vec![LightProperties::OPAQUE_DARK; 2],
            materials: vec![Material {
                texture: TextureRef::new(1, 0).unwrap(),
                flags: 0,
                ..Material::unvaried()
            }],
            textures: vec![texture],
            ..Default::default()
        },
    )
    .unwrap()
}

fn new_stream(assets: &Arc<assets::RuntimeAssets>) -> WorldStream {
    WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: CAMERA,
            world_spawn_position: [8, 8, 8],
            air_network_id: AIR,
            block_network_ids_are_hashes: false,
        },
        Arc::clone(assets),
        CAMERA,
        None,
    )
}

fn poll_and_acknowledge(stream: &mut WorldStream) {
    black_box(stream.poll(CAMERA, 32));
    while let Some(change) = stream.pop_mesh_change() {
        match change {
            WorldMeshChange::Upsert {
                key,
                generation,
                dirty_since,
                mesh,
                biome,
                ..
            } => {
                black_box((&mesh, &biome));
                stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
            }
            WorldMeshChange::Remove {
                key,
                generation,
                dirty_since,
                ..
            } => {
                stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
            }
        }
    }
}

fn preload_air_boundary(stream: &mut WorldStream, sections: usize) -> usize {
    let columns = sections.div_ceil(SECTIONS_PER_COLUMN);
    let side = (columns as f64).sqrt().ceil() as i32;
    let mut boundary = Vec::new();
    for z in -1..=side {
        for x in -1..=side {
            if x >= 0 && x < side && z >= 0 && z < side && (z * side + x) < columns as i32 {
                continue;
            }
            boundary.push((x - side / 2, z - side / 2));
        }
    }
    let payload = inline_payload(0);
    let started = Instant::now();
    let mut submitted = 0;
    loop {
        stream.begin_frame_work();
        for _ in 0..stream
            .remaining_admission_capacity()
            .min(boundary.len() - submitted)
        {
            let (x, z) = boundary[submitted];
            stream
                .submit_level_chunk_bytes(
                    submitted as u64 + 1,
                    LevelChunkEvent {
                        dimension: 0,
                        x,
                        z,
                        mode: LevelChunkMode::Inline { count: 0 },
                        payload: Vec::new(),
                    },
                    payload.clone(),
                )
                .unwrap();
            submitted += 1;
        }
        poll_and_acknowledge(stream);
        if submitted == boundary.len()
            && stream.committed_sequence() == boundary.len() as u64
            && benchmark_support::work_is_idle(stream)
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "air boundary failed to settle: {:?}",
            stream.stats()
        );
        std::thread::yield_now();
    }
    boundary.len()
}

fn drain_burst(fixture: &mut BurstFixture, record_frames: bool) -> Vec<Duration> {
    let started = Instant::now();
    let mut frames = Vec::new();
    let mut submitted = 0;
    let columns = fixture.columns();
    let side = (columns as f64).sqrt().ceil() as usize;
    loop {
        let frame_started = record_frames.then(Instant::now);
        fixture.stream.begin_frame_work();
        let capacity = fixture.stream.remaining_admission_capacity();
        for _ in 0..capacity.min(columns - submitted) {
            let count =
                (fixture.sections - submitted * SECTIONS_PER_COLUMN).min(SECTIONS_PER_COLUMN);
            fixture
                .stream
                .submit_level_chunk_bytes(
                    (fixture.background_columns + fixture.drained_columns + submitted) as u64 + 1,
                    LevelChunkEvent {
                        dimension: 0,
                        x: (submitted % side) as i32 - (side / 2) as i32,
                        z: (submitted / side) as i32 - (side / 2) as i32,
                        mode: LevelChunkMode::Inline { count },
                        payload: Vec::new(),
                    },
                    fixture.payloads[count - 1].clone(),
                )
                .expect("bounded inline burst admission");
            submitted += 1;
        }
        poll_and_acknowledge(&mut fixture.stream);
        if let Some(frame_started) = frame_started {
            frames.push(frame_started.elapsed());
        }
        if submitted == columns
            && fixture.stream.committed_sequence()
                == (fixture.background_columns + fixture.drained_columns + columns) as u64
            && benchmark_support::work_is_idle(&fixture.stream)
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "streaming fixture failed to drain: {:?}",
            fixture.stream.stats()
        );
        std::thread::yield_now();
    }
    frames
}

fn validate_burst(fixture: &BurstFixture) -> WorldStreamStats {
    let stats = fixture.stream.stats();
    assert_eq!(stats.decode_errors, 0);
    assert_eq!(stats.normalization_errors, 0);
    assert_eq!(stats.light_solve_failures, 0);
    let range = vanilla_dimension_range(0).unwrap();
    assert_eq!(
        stats.resident_sub_chunks,
        (fixture.background_columns + fixture.columns()) * range.sub_chunk_count
    );
    let side = (fixture.columns() as f64).sqrt().ceil() as usize;
    let mut stored_sections = 0;
    for index in 0..fixture.columns() {
        let column = world::ChunkKey::new(
            0,
            (index % side) as i32 - (side / 2) as i32,
            (index / side) as i32 - (side / 2) as i32,
        );
        let count = (fixture.sections - index * SECTIONS_PER_COLUMN).min(SECTIONS_PER_COLUMN);
        for offset in 0..range.sub_chunk_count {
            let key =
                world::SubChunkKey::from_chunk(column, range.base_sub_chunk_y + offset as i32);
            let source = fixture.stream.authority().terrain().sub_chunk(key);
            if offset < count {
                let source = source.expect("submitted inline section is stored");
                assert_eq!(source.runtime_id(0, 0, 0, 0), Some(SOLID));
                assert_eq!(source.runtime_id(0, 15, 15, 15), Some(SOLID));
                stored_sections += 1;
            } else {
                assert!(source.is_none(), "omitted slots remain implicit air");
            }
        }
    }
    assert_eq!(stored_sections, fixture.sections);
    assert_eq!(
        stats.publication_stages.decode_jobs_completed,
        (fixture.background_columns + fixture.columns()) as u64
    );
    assert_eq!(stats.publication_stages.mesh_uploads_unacknowledged, 0);
    assert_eq!(
        stats.publication_stages.mesh_uploads_acknowledged,
        stats.publication_stages.mesh_changes_dequeued
    );
    assert!(stats.publication_stages.mesh_jobs_completed >= fixture.sections as u64);
    stats
}

fn streaming_benches(c: &mut Criterion) {
    let assets = Arc::new(terrain_assets());
    let payloads = Arc::new(std::array::from_fn(|index| inline_payload(index + 1)));
    let make_fixture = |sections| {
        let mut stream = new_stream(&assets);
        let background_columns = preload_air_boundary(&mut stream, sections);
        BurstFixture {
            stream,
            payloads: Arc::clone(&payloads),
            sections,
            background_columns,
            drained_columns: 0,
        }
    };
    let mut group = c.benchmark_group("pipeline/inline_burst_cpu_drain");
    for sections in [4_usize, 16, 64, 871] {
        group.throughput(Throughput::Elements(sections as u64));
        group.bench_with_input(BenchmarkId::new("stored_sections", sections), &sections, |b, &sections| {
            let mut witness = make_fixture(sections);
            let started = Instant::now();
            let mut frames = drain_burst(&mut witness, true);
            let elapsed = started.elapsed();
            let stats = validate_burst(&witness);
            frames.sort_unstable();
            let p99 = frames[(frames.len() - 1) * 99 / 100];
            eprintln!("STREAM_PREFLIGHT columns={} background_columns={} stored_sections={sections} resident_slots={} drain_ms={:.3} cpu_step_p99_us={} cpu_step_max_us={} meshes={} stale_meshes={} stale_lights={}", witness.columns(), witness.background_columns, stats.resident_sub_chunks, elapsed.as_secs_f64() * 1000.0, p99.as_micros(), frames.last().unwrap().as_micros(), stats.publication_stages.mesh_jobs_completed, stats.stale_mesh_jobs, stats.stale_light_jobs);
            // Setup/teardown are excluded; bounded submission, worker waits, polling,
            // CPU publication and acknowledgements are included. No GPU upload occurs.
            b.iter_batched_ref(
                || make_fixture(sections),
                |fixture| { black_box(drain_burst(fixture, false)); },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();

    c.bench_function("pipeline/settled_poll/64_stored_sections", |b| {
        let mut settled = make_fixture(64);
        drain_burst(&mut settled, false);
        validate_burst(&settled);
        b.iter(|| {
            settled.stream.begin_frame_work();
            black_box(settled.stream.poll(black_box(CAMERA), 32));
        });
    });
}

fn cohort_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline/cohort_status_metadata");
    for radius in [4_i32, 16] {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.5, 70.0, 0.5],
            world_spawn_position: [0, 70, 0],
            air_network_id: AIR,
            block_network_ids_are_hashes: false,
        });
        stream
            .submit(
                1,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: [0, 70, 0],
                    radius_blocks: (radius * world::SUB_CHUNK_SIDE as i32) as u32,
                }),
            )
            .unwrap();
        let stream = benchmark_support::cohort_fixture(stream, radius);
        let target = stream.committed_view_cohort().unwrap();
        let status = stream.cohort_status(target);
        let range = vanilla_dimension_range(0).unwrap();
        assert_eq!(
            status.resident_count,
            ((2 * radius + 1).pow(2) as usize) * range.sub_chunk_count
        );
        group.throughput(Throughput::Elements(status.resident_count as u64));
        group.bench_with_input(BenchmarkId::new("radius", radius), &target, |b, target| {
            b.iter(|| black_box(stream.cohort_status(black_box(*target))));
        });
    }
    group.finish();
}

#[cfg(not(feature = "runtime-profile"))]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

#[cfg(feature = "benchmark-mimalloc")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[path = "chunk_costs/dispatch.rs"]
mod dispatch;

/// A loaded block of columns resent with every section swapped for a block of the same light
/// inputs, as a server applying a bulk replace sends them; measures the resend's drain alone.
fn resend_benches(c: &mut Criterion) {
    const COLUMNS: usize = 500;
    let sections = COLUMNS * SECTIONS_PER_COLUMN;
    let assets = Arc::new(terrain_assets());
    let loaded = Arc::new(std::array::from_fn(|index| inline_payload(index + 1)));
    let swapped = Arc::new(std::array::from_fn(|index| {
        inline_payload_of(index + 1, SOLID_SWAPPED)
    }));
    let mostly_identical = Arc::new(std::array::from_fn(|index| {
        let sections = index + 1;
        let mut payload = inline_payload(sections).to_vec();
        payload[(sections - 1) * 5 + 4] = (SOLID_SWAPPED << 1) as u8;
        Bytes::from(payload)
    }));
    let packed_loaded = Arc::new(std::array::from_fn(|index| {
        inline_packed_payload(index + 1, false)
    }));
    let packed_changed = Arc::new(std::array::from_fn(|index| {
        inline_packed_payload(index + 1, true)
    }));
    let mut group = c.benchmark_group("pipeline/inline_resend_cpu_drain");
    group.sample_size(10);
    group.throughput(Throughput::Elements(sections as u64));
    for (name, initial, replacement) in [
        ("light_neutral_columns", Arc::clone(&loaded), swapped),
        (
            "mostly_identical_columns",
            Arc::clone(&loaded),
            mostly_identical,
        ),
        (
            "identical_columns",
            Arc::clone(&loaded),
            Arc::clone(&loaded),
        ),
        (
            "packed_mostly_identical_columns",
            Arc::clone(&packed_loaded),
            packed_changed,
        ),
        (
            "packed_identical_columns",
            Arc::clone(&packed_loaded),
            packed_loaded,
        ),
    ] {
        let loaded_fixture = || {
            let mut stream = new_stream(&assets);
            let background_columns = preload_air_boundary(&mut stream, sections);
            let mut fixture = BurstFixture {
                stream,
                payloads: Arc::clone(&initial),
                sections,
                background_columns,
                drained_columns: 0,
            };
            drain_burst(&mut fixture, false);
            fixture.payloads = Arc::clone(&replacement);
            fixture.drained_columns = COLUMNS;
            fixture
        };
        let mut witness = loaded_fixture();
        let before = witness.stream.stats();
        let started = Instant::now();
        drain_burst(&mut witness, false);
        let drain_ms = started.elapsed().as_secs_f64() * 1000.0;
        let after = witness.stream.stats();
        if Arc::ptr_eq(&initial, &replacement) {
            assert_eq!(
                after.publication_stages.mesh_jobs_completed
                    - before.publication_stages.mesh_jobs_completed,
                0,
                "identical resends must not build meshes",
            );
        }
        eprintln!(
            "RESEND_PREFLIGHT case={name} columns={COLUMNS} sections={sections} decoded={} reused={} worker_cpu_ms={:.3} drain_ms={drain_ms:.3} light_jobs={} meshes={}",
            after.publication_stages.inline_sections_decoded
                - before.publication_stages.inline_sections_decoded,
            after.publication_stages.inline_sections_reused
                - before.publication_stages.inline_sections_reused,
            (after.publication_stages.inline_decode_worker_time
                - before.publication_stages.inline_decode_worker_time)
                .as_secs_f64()
                * 1000.0,
            after.accepted_light_jobs - before.accepted_light_jobs,
            after.publication_stages.mesh_jobs_completed
                - before.publication_stages.mesh_jobs_completed,
        );
        group.bench_function(BenchmarkId::new(name, COLUMNS), |b| {
            b.iter_batched_ref(
                loaded_fixture,
                |fixture| {
                    black_box(drain_burst(fixture, false));
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    streaming_benches,
    resend_benches,
    cohort_benches,
    dispatch::benches,
    dispatch::light_column_benches,
    subchunk_updates::benches
);
criterion_main!(benches);

#[path = "chunk_costs/subchunk_updates.rs"]
mod subchunk_updates;
