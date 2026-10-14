//! Measures one distant terrain sample job without the store or the writer, the writer
//! threads' merges into a store, and mesh jobs from a store end to end.

use std::{
    sync::{RwLock, atomic::Ordering},
    time::{Duration, Instant},
};

use lod::{
    epoch::ChunkSurface,
    fixtures::{biome_column, fixture_classifier, terrain_column},
    geometry::SectionPos,
    registry::MaterialRegistry,
    sampler::{ColumnSample, ColumnSnapshot, sample_column},
};
use lod_store::{actor::StoreClient, identity::Dimension, minute::Minute, store::SharedRegistry};

use super::*;
use crate::stream::distant_terrain::{
    blocks::SessionBlocks,
    hooks::session_blocks,
    shared::{CaptureKind, LodShared, SampledColumn, WriterInput, Writers},
    writer,
};

/// One resident column of mixed-palette sub-chunks with solved light, sampled as a Lod-lane
/// job samples it.
pub struct LodSampleFixture {
    stream: WorldStream,
    column: ChunkKey,
    registry: SharedRegistry,
}

impl LodSampleFixture {
    /// Installs a column of `sub_chunks` stacked sub-chunks from Y 0; setup is outside the
    /// timed region.
    pub fn new(sub_chunks: usize) -> Self {
        assert!(sub_chunks > 0);
        let mut stream = WorldStream::new(client_world::ingestion::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [8.0; 3],
            world_spawn_position: [8; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let mut bytes = vec![9, 1, 0, 3];
        for _ in 0..128 {
            bytes.extend_from_slice(&0xaaaa_aaaa_u32.to_le_bytes());
        }
        bytes.extend([4, 0, 2]);
        let section = SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 });
        let column = ChunkKey::new(0, 0, 0);
        for y in 0..sub_chunks as i32 {
            let key = SubChunkKey::from_chunk(column, y);
            stream
                .authority
                .commit_sub_chunk(key, section.clone())
                .unwrap();
            stream.resident.insert(key);
            let mut light = SubChunkLight::uniform(0, 15, 1).unwrap();
            light.set(LightChannel::Block, 8, 8, 8, 7).unwrap();
            stream.lighting.store.insert_resident(key, light);
        }
        Self {
            stream,
            column,
            registry: Arc::new(RwLock::new(MaterialRegistry::new())),
        }
    }

    /// Captures, hashes and samples the column with its surface signature; returns the
    /// content hash and the sampled sub-chunk count.
    pub fn sample(&self) -> (u64, usize) {
        self.stream
            .sample_lod_column_for_bench(self.column, &self.registry)
            .expect("the fixture column is keyed and resident")
    }
}

/// Sub-chunk Ys of every writer fixture column: the terrain, its caves and the air above.
const WRITER_SUB_CHUNK_YS: std::ops::Range<i32> = 0..8;

/// The `lod` fixture terrain over a square of chunk columns, sampled ahead of time, for
/// timing the writer threads' merges into a store.
pub struct LodWriterFixture {
    columns: Vec<(ChunkKey, ColumnSample, ChunkSurface)>,
    blocks: Arc<SessionBlocks>,
}

impl LodWriterFixture {
    /// Samples the `side` × `side` chunk columns from `(min, min)`; setup is outside the
    /// timed region.
    pub fn new(min: i32, side: i32) -> Self {
        let stream = WorldStream::new(client_world::ingestion::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [8.0; 3],
            world_spawn_position: [8; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let blocks = session_blocks(&mut None, &stream.authority);
        let biomes = biome_column(0, &[1, 1, 2, 2, 3, 3, 4, 4]);
        let mut classifier = fixture_classifier();
        let mut columns = Vec::new();
        for z in min..min + side {
            for x in min..min + side {
                let chunk = ChunkKey::new(0, x, z);
                let sub_chunks = terrain_column(x, z, WRITER_SUB_CHUNK_YS);
                let snapshot = ColumnSnapshot {
                    chunk,
                    sub_chunks: &sub_chunks,
                    biomes: &biomes,
                    default_biome: 0,
                    light: None,
                };
                let sample =
                    sample_column(&snapshot, &mut classifier).expect("fixture columns have keys");
                let surface = ChunkSurface::from_sample(&sample, &classifier);
                columns.push((chunk, sample, surface));
            }
        }
        Self { columns, blocks }
    }

    /// Fixture columns.
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    /// True without columns.
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Hands every column to `writers` writer threads merging into the store behind
    /// `client`, whose overworld session has begun, and returns the time until all are
    /// merged and their writes queued in the store. Starting the threads and copying the
    /// samples are not timed; the wait reads the merge counter without taking the inbox
    /// lock the writers need, and checks that ingestion settled only once all are merged.
    pub fn ingest(&self, client: StoreClient, writers: usize) -> Duration {
        let shared = Arc::new(LodShared::new(
            client,
            Minute::new(1),
            Writers::new(writers),
        ));
        let inputs: Vec<WriterInput> = self
            .columns
            .iter()
            .enumerate()
            .map(|(index, (chunk, sample, surface))| {
                WriterInput::Sampled(Box::new(SampledColumn {
                    seq: index as u64 + 1,
                    kind: CaptureKind::Final,
                    dimension: Dimension(chunk.dimension),
                    chunk: *chunk,
                    sample: sample.clone(),
                    surface: Box::new(surface.clone()),
                    hash: index as u64,
                    blocks: Arc::clone(&self.blocks),
                    _final: None,
                }))
            })
            .collect();
        writer::spawn(&shared).expect("writer threads start");
        let started = Instant::now();
        for input in inputs {
            if shared.push(input).is_err() {
                panic!("final captures are never refused while ingesting");
            }
        }
        let expected = self.columns.len() as u64;
        loop {
            // Takes the inbox lock only for a writer waiting on the store's full queue.
            shared.nudge_writer();
            if shared.counters.merged_columns.load(Ordering::Acquire) >= expected
                && shared.stats(0).is_settled()
            {
                break;
            }
            std::thread::yield_now();
        }
        let elapsed = started.elapsed();
        shared.disable();
        elapsed
    }
}

/// Wanting the level-0 sections of a [`LodWriterFixture`]'s columns from a store that holds
/// them, for timing mesh jobs end to end: snapshot reads through the store's queue, decoding
/// and meshing on the Lod lane, and handing the meshes to the frame.
pub struct LodMeshFixture {
    sections: Vec<SectionPos>,
}

/// One timed run of [`LodMeshFixture::mesh`].
pub struct LodMeshRun {
    /// From wanting every section until the last mesh was taken.
    pub elapsed: Duration,
    /// Per section, from wanting it until its mesh was taken, in completion order.
    pub latencies: Vec<Duration>,
}

impl LodMeshFixture {
    /// The level-0 sections `columns` write.
    pub fn new(columns: &LodWriterFixture) -> Self {
        let mut sections: Vec<SectionPos> = columns
            .columns
            .iter()
            .flat_map(|(chunk, _, _)| {
                WRITER_SUB_CHUNK_YS.map(|y| {
                    SectionPos::of_sub_chunk(chunk.x, y, chunk.z)
                        .expect("fixture sub-chunks are keyed")
                        .0
                })
            })
            .collect();
        sections.sort();
        sections.dedup();
        Self { sections }
    }

    /// Sections wanted per run.
    pub fn len(&self) -> usize {
        self.sections.len()
    }

    /// True without sections.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Starts a mesher reading through `client`, whose overworld session holds the
    /// sections, wants every section and polls as the frame does until each mesh was
    /// taken. With `load`, its columns are merged by [`MAX_LOD_WRITERS`] writers into the
    /// store behind its client meanwhile, sharing the store's queue with the mesher's reads.
    /// Starting the mesher thread is not timed.
    pub fn mesh(
        &self,
        client: StoreClient,
        load: Option<(&LodWriterFixture, StoreClient)>,
    ) -> LodMeshRun {
        let mut stream = WorldStream::new(client_world::ingestion::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [8.0; 3],
            world_spawn_position: [8; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        stream
            .enable_distant_terrain_meshes(client, Minute::new(1))
            .expect("the mesher thread starts");
        let mut waiting: std::collections::HashSet<_> =
            self.sections.iter().map(|pos| pos.key()).collect();
        let run = std::thread::scope(|scope| {
            let loader = load.map(|(columns, client)| {
                scope.spawn(move || columns.ingest(client, crate::MAX_LOD_WRITERS))
            });
            let started = Instant::now();
            stream.want_distant_terrain_meshes(self.sections.iter().copied());
            let mut latencies = Vec::with_capacity(self.sections.len());
            let mut fresh = Vec::new();
            while !waiting.is_empty() {
                stream.sync_distant_terrain_meshes();
                stream.take_distant_terrain_meshes(&mut fresh);
                for (key, _) in fresh.drain(..) {
                    if waiting.remove(&key) {
                        latencies.push(started.elapsed());
                    }
                }
                std::thread::yield_now();
            }
            let elapsed = started.elapsed();
            if let Some(loader) = loader {
                loader.join().expect("the load ingests");
            }
            LodMeshRun { elapsed, latencies }
        });
        stream.disable_distant_terrain_meshes();
        run
    }
}
