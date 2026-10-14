//! Distant terrain ingestion: change and eviction hooks, the content-hash skip, the
//! backlog, capture ordering, final captures and the writer's merges, against a real store.

mod backlog;
mod finals;
mod meshes;
mod relight;
mod sessions;

use lod::{
    codec::SectionCodec,
    geometry::{SectionKey, SectionPos},
    mip::LOCAL_MIP_TOP_LEVEL,
    section::Section,
};
use lod_store::{
    actor::{ActorConfig, ReadOutcome, ReadTarget, Request, StoreActor, StoreClient},
    identity::{Dimension, StoreKey},
    minute::Minute,
    root::{Quotas, StoreRoot},
    store::StoreOptions,
};
use test_time::eventually;

use super::{light_scheduler::install_current_light, *};
use crate::stream::{
    distant_terrain::{
        DistantTerrainLink, LOD_SETTLE_POLLS,
        book::Verdict,
        hooks::session_blocks,
        jobs,
        shared::{Capture, CaptureKind, Writers},
    },
    workers::LOD_JOBS_QUEUED,
};

const NOW: Minute = Minute::new(1);
const OVERWORLD: Dimension = Dimension(0);
/// The opaque cube of [`camera_medium_assets`].
const STONE: u32 = 3;
/// Water in [`camera_medium_assets`].
const WATER_ID: u32 = 1;
/// The only sub-chunk of [`inline_block_entity_event`] columns.
const SUB_CHUNK_Y: i32 = -4;

/// A store in a temporary directory with an overworld session begun.
struct TestStore {
    actor: StoreActor,
    _dir: tempfile::TempDir,
}

impl TestStore {
    fn new() -> Self {
        Self::with_config(ActorConfig::default())
    }

    /// A store whose thread runs with `config`.
    fn with_config(config: ActorConfig) -> Self {
        Self::in_dir(tempfile::tempdir().expect("temp dir"), config)
    }

    /// The test store kept in `dir`, opened with its thread running with `config`.
    fn in_dir(dir: tempfile::TempDir, config: ActorConfig) -> Self {
        let root = StoreRoot::new(dir.path());
        let store = root
            .open(&store_key(), NOW, StoreOptions::default())
            .expect("open store");
        let mut actor =
            StoreActor::spawn(store, root, Quotas::default(), config).expect("spawn store thread");
        actor
            .try_submit(Request::Begin {
                dimension: OVERWORLD,
                now: NOW,
            })
            .expect("begin session");
        Self { actor, _dir: dir }
    }

    fn client(&self) -> StoreClient {
        self.actor.client()
    }

    /// The stored section at `pos`, read after every earlier write.
    fn section(&self, pos: SectionPos) -> Option<Section> {
        match self
            .client()
            .read(OVERWORLD, ReadTarget::Section(pos.key()), NOW)
            .expect("read queued")
        {
            ReadOutcome::Found(record) => Some(
                SectionCodec::new()
                    .decode_expecting(&record, pos.key())
                    .expect("valid record"),
            ),
            _ => None,
        }
    }

    /// The stored content hash of chunk `(x, 0)`.
    fn chunk_hash(&self, x: i32) -> Option<u64> {
        match self
            .client()
            .read(OVERWORLD, ReadTarget::ChunkHash { x, z: 0 }, NOW)
            .expect("read queued")
        {
            ReadOutcome::ChunkHash(hash) => hash,
            outcome => panic!("chunk hash read failed: {outcome:?}"),
        }
    }
}

/// The key every test store is opened under.
fn store_key() -> StoreKey {
    StoreKey::new("remote:distant-terrain", "", 0)
}

fn stream() -> WorldStream {
    WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(camera_medium_assets()),
        [0.0; 3],
        None,
    )
}

/// Starts ingestion of `stream` into `store`.
fn enable(stream: &mut WorldStream, store: &TestStore) -> DistantTerrainLink {
    stream
        .enable_distant_terrain(store.client(), NOW)
        .expect("a writable store")
}

/// Starts ingestion of `stream` into `store` with `writers` writer threads.
fn enable_with(stream: &mut WorldStream, store: &TestStore, writers: usize) -> DistantTerrainLink {
    stream
        .enable_distant_terrain_with(store.client(), NOW, Writers::new(writers))
        .expect("a writable store")
}

/// Submits chunk `(x, 0)` as one sub-chunk of `runtime_id` and decodes it, leaving its
/// light pending.
fn submit_column(stream: &mut WorldStream, sequence: u64, x: i32, runtime_id: u32) {
    stream
        .submit(
            sequence,
            inline_block_entity_event(x, runtime_id, Vec::new()),
        )
        .expect("admit column");
    complete_pending_decode_jobs(stream);
}

/// Commits chunk `(x, 0)` as one sub-chunk of `runtime_id` with uniform sky light `sky`,
/// settling the light of the column's air above it unsolved.
fn commit_column(stream: &mut WorldStream, sequence: u64, x: i32, runtime_id: u32, sky: u8) {
    submit_column(stream, sequence, x, runtime_id);
    settle_light(stream, x, sky);
}

/// Commits chunks `xs` as one stone sub-chunk each from `sequence` on, then settles their
/// light at sky `sky`, as neighbours arriving together relight each other.
fn commit_columns(stream: &mut WorldStream, sequence: u64, xs: &[i32], sky: u8) {
    for (offset, &x) in xs.iter().enumerate() {
        submit_column(stream, sequence + offset as u64, x, STONE);
    }
    for &x in xs {
        settle_light(stream, x, sky);
    }
}

/// Installs uniform sky light `sky` in chunk `(x, 0)`'s stored sub-chunk and drops the
/// column's other pending light work.
fn settle_light(stream: &mut WorldStream, x: i32, sky: u8) {
    install_current_light(
        stream,
        SubChunkKey::new(0, x, SUB_CHUNK_Y, 0),
        0,
        sky,
        false,
    );
    let column = ChunkKey::new(0, x, 0);
    stream
        .lighting
        .jobs
        .pending
        .retain(|key, _| key.chunk() != column);
}

fn stats(stream: &WorldStream) -> DistantTerrainStats {
    stream.distant_terrain_stats().expect("ingestion is on")
}

/// Runs polls until a column dirtied before them is due, and one more dispatch.
fn settle(stream: &mut WorldStream) {
    for _ in 0..LOD_SETTLE_POLLS {
        stream.dispatch_lod_work();
    }
}

/// Waits until `merged` columns were written and no sample is in flight or queued.
fn wait_for_merges(stream: &WorldStream, merged: u64) {
    eventually("the writer to merge every sample", || {
        let stats = stats(stream);
        stats.merged_columns >= merged && stats.is_settled()
    });
}

/// The level-0 section of chunk `(x, 0)`'s sub-chunk and its ancestors up to the top
/// local level.
fn chain(x: i32) -> Vec<SectionPos> {
    let base = SectionPos::of_sub_chunk(x, SUB_CHUNK_Y, 0)
        .expect("keyed")
        .0;
    (0..=LOCAL_MIP_TOP_LEVEL)
        .map(|level| base.ancestor(level).expect("local level"))
        .collect()
}

/// The stored sections of chunks `xs`' chains.
fn stored_chains(store: &TestStore, xs: &[i32]) -> Vec<(SectionKey, Option<Section>)> {
    let keys: BTreeSet<SectionKey> = xs
        .iter()
        .flat_map(|&x| chain(x))
        .map(SectionPos::key)
        .collect();
    keys.into_iter()
        .map(|key| (key, store.section(key.pos())))
        .collect()
}

/// The sky light stored at the X = 16 face of chunk 1's level-0 section, which borders an
/// unsampled column, so the stone there is not hidden and carries the sampled light.
fn chunk_one_face_sky(store: &TestStore) -> u8 {
    let section = store.section(chain(1)[0]).expect("level-0 section");
    let mut dense = lod::mip::unknown_section();
    section.expand_into(&mut dense);
    let face = dense.get(16, 0, 0);
    assert!(!face.is_hidden());
    face.light().sky()
}

#[test]
fn changed_columns_are_sampled_only_after_settling_for_whole_polls() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    for _ in 1..LOD_SETTLE_POLLS {
        stream.dispatch_lod_work();
    }
    // A further change restarts the settle count.
    commit_column(&mut stream, 2, 1, WATER_ID, 15);
    for _ in 1..LOD_SETTLE_POLLS {
        stream.dispatch_lod_work();
    }
    assert_eq!(stats(&stream).dirty_columns, 1, "still settling");
    assert_eq!(stats(&stream).jobs_queued, 0);
    stream.dispatch_lod_work();
    assert_eq!(stats(&stream).dirty_columns, 0);
    wait_for_merges(&stream, 1);
    assert_eq!(stats(&stream).sampled, 1, "one sample for both changes");
    for pos in chain(1) {
        assert!(store.section(pos).is_some(), "{pos:?} is stored");
    }
    assert!(store.chunk_hash(1).is_some());
}

#[test]
fn pending_light_holds_a_settled_column_back() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    submit_column(&mut stream, 1, 2, STONE);
    let key = SubChunkKey::new(0, 2, SUB_CHUNK_Y, 0);
    assert!(stream.lighting.jobs.pending.contains_key(&key));
    settle(&mut stream);
    assert_eq!(stats(&stream).dirty_columns, 1, "waits for its light");
    settle_light(&mut stream, 2, 15);
    stream.dispatch_lod_work();
    assert_eq!(stats(&stream).dirty_columns, 0);
    wait_for_merges(&stream, 1);
}

#[test]
fn evicted_columns_are_sampled_with_the_light_they_had() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 7);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    let key = SubChunkKey::new(0, 1, SUB_CHUNK_Y, 0);
    assert!(stream.lighting.store.light(key).is_none(), "light is gone");
    assert_eq!(
        stats(&stream).dirty_columns,
        0,
        "the eviction took the change"
    );
    wait_for_merges(&stream, 1);
    assert_eq!(
        chunk_one_face_sky(&store),
        7,
        "the solved light, not the fallback"
    );
}

#[test]
fn an_eviction_keeps_ingestion_unsettled_until_its_columns_are_written() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    assert_eq!(stats(&stream).final_captures, 1);
    assert!(!stats(&stream).is_settled());
    eventually("ingestion to settle", || stats(&stream).is_settled());
    // Settled means the store's queue holds the column's writes.
    assert_eq!(stats(&stream).merged_columns, 1);
    assert!(store.chunk_hash(1).is_some());
}

#[test]
fn unchanged_columns_queue_no_job() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);
    // The next poll keeps the fingerprint of the job's hash.
    stream.dispatch_lod_work();
    let queued = stats(&stream).jobs_queued;

    // An equal resend keeps the resident sections, so nothing is marked.
    submit_column(&mut stream, 2, 1, STONE);
    assert_eq!(stats(&stream).dirty_columns, 0);
    // Evicting the unchanged column finds its content stored without a job.
    stream.evict_column(ChunkKey::new(0, 1, 0));
    assert_eq!(stats(&stream).jobs_queued, queued);
    assert_eq!(stats(&stream).skipped_unchanged, 1);
    eventually("the skipped capture to be released", || {
        stats(&stream).is_settled()
    });
    assert_eq!(stats(&stream).merged_columns, 1);
}

#[test]
fn a_new_stream_skips_columns_stored_in_its_session() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);

    // A rejoin hashes the column once and finds the stored content hash.
    let mut rejoined = self::stream();
    let _rejoined_link = enable(&mut rejoined, &store);
    commit_column(&mut rejoined, 1, 1, STONE, 15);
    settle(&mut rejoined);
    eventually("the hash check", || {
        let stats = stats(&rejoined);
        stats.skipped_unchanged == 1 && stats.is_settled()
    });
    assert_eq!(stats(&rejoined).sampled, 0);
    assert_eq!(stats(&rejoined).merged_columns, 0);
}

#[test]
fn an_eviction_after_a_light_only_change_keeps_the_stored_sample() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);
    let first_hash = store.chunk_hash(1);

    // A neighbour's arrival relights the column without touching its blocks; light that
    // depends on loaded neighbours never makes a column count as changed.
    settle_light(&mut stream, 1, 4);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    eventually("the eviction to be checked", || stats(&stream).is_settled());
    assert_eq!(stats(&stream).skipped_unchanged, 1);
    assert_eq!(stats(&stream).merged_columns, 1);
    assert_eq!(store.chunk_hash(1), first_hash);
    assert_eq!(chunk_one_face_sky(&store), 15);
}

#[test]
fn a_column_sampled_before_its_light_was_solved_is_sampled_again_once_lit() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    // Evicted while its light is still pending, the column is sampled with the fallback.
    submit_column(&mut stream, 1, 1, STONE);
    assert!(
        stream
            .lighting
            .jobs
            .pending
            .contains_key(&SubChunkKey::new(0, 1, SUB_CHUNK_Y, 0))
    );
    stream.evict_column(ChunkKey::new(0, 1, 0));
    wait_for_merges(&stream, 1);
    let unlit = store.chunk_hash(1);

    commit_column(&mut stream, 2, 1, STONE, 6);
    settle(&mut stream);
    wait_for_merges(&stream, 2);
    assert_eq!(stats(&stream).skipped_unchanged, 0);
    assert_ne!(store.chunk_hash(1), unlit);
    assert_eq!(chunk_one_face_sky(&store), 6);
}

#[test]
fn an_older_capture_finishing_last_never_replaces_a_newer_one() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    let column = ChunkKey::new(0, 1, 0);
    let capture = |stream: &mut WorldStream| {
        let lod = stream.lod.as_deref_mut().expect("on");
        let seq = lod.next_seq();
        let shared = Arc::clone(&lod.shared);
        let blocks = session_blocks(&mut None, &stream.authority);
        let input = stream.lod_column_input(column, blocks).expect("resident");
        // Uncounted, so ingestion settles while the older capture waits.
        (
            shared,
            Capture {
                seq,
                kind: CaptureKind::Changed,
                input,
                checked_hash: None,
                relit: false,
                job: None,
                final_capture: None,
            },
        )
    };
    let (shared, older) = capture(&mut stream);
    commit_column(&mut stream, 2, 1, WATER_ID, 15);
    let (_, newer) = capture(&mut stream);

    jobs::sample(&shared, newer);
    wait_for_merges(&stream, 1);
    let newer_hash = store.chunk_hash(1).expect("newer content stored");
    let newer_section = store.section(chain(1)[0]);
    jobs::sample(&shared, older);
    eventually("the older capture to be dropped", || {
        let stats = stats(&stream);
        stats.superseded == 1 && stats.is_settled()
    });
    assert_eq!(stats(&stream).merged_columns, 1);
    assert_eq!(store.chunk_hash(1), Some(newer_hash));
    assert_eq!(store.section(chain(1)[0]), newer_section);
}

#[test]
fn neighbouring_columns_store_the_same_sections_in_either_order_or_together() {
    // Chunks 1 and 2 lie in neighbouring level-0 sections.
    let mut stored = Vec::new();
    for order in [[1, 2], [2, 1]] {
        let store = TestStore::new();
        let mut stream = stream();
        let _link = enable(&mut stream, &store);
        for (merged, x) in order.into_iter().enumerate() {
            commit_column(&mut stream, merged as u64 + 1, x, STONE, 15);
            settle(&mut stream);
            wait_for_merges(&stream, merged as u64 + 1);
        }
        stored.push(stored_chains(&store, &[1, 2]));
    }
    // Both settle in one poll, so their jobs run at once and the writer takes them as
    // they land.
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_columns(&mut stream, 1, &[1, 2], 15);
    LOD_JOBS_QUEUED.set(0);
    settle(&mut stream);
    // Only the frame's own jobs: the writer queues more after asking the store.
    assert_eq!(LOD_JOBS_QUEUED.get(), 2, "dispatched together");
    wait_for_merges(&stream, 2);
    stored.push(stored_chains(&store, &[1, 2]));

    assert!(stored[0].iter().all(|(_, section)| section.is_some()));
    assert_eq!(stored[0], stored[1]);
    assert_eq!(stored[0], stored[2]);
}

#[test]
fn region_writers_store_what_one_writer_stores_across_a_region_border_in_any_order() {
    // Chunks -1 and 0 lie in neighbouring writer regions at every local level, which
    // different writers own.
    let xs = [-1, 0];
    let four = Writers::new(4);
    assert_ne!(
        four.of_chunk(ChunkKey::new(0, -1, 0)),
        four.of_chunk(ChunkKey::new(0, 0, 0))
    );
    let mut stored = Vec::new();
    for writers in [1, 4] {
        for order in [xs, [xs[1], xs[0]]] {
            let store = TestStore::new();
            let mut stream = stream();
            let _link = enable_with(&mut stream, &store, writers);
            for (merged, x) in order.into_iter().enumerate() {
                commit_column(&mut stream, merged as u64 + 1, x, STONE, 15);
                settle(&mut stream);
                wait_for_merges(&stream, merged as u64 + 1);
            }
            stored.push((writers, order, stored_chains(&store, &xs)));
        }
        let store = TestStore::new();
        let mut stream = stream();
        let _link = enable_with(&mut stream, &store, writers);
        commit_columns(&mut stream, 1, &xs, 15);
        settle(&mut stream);
        wait_for_merges(&stream, 2);
        assert_eq!(
            stats(&stream).handed_slabs > 0,
            writers > 1,
            "only owners of the neighbour's region merge its slabs"
        );
        stored.push((writers, xs, stored_chains(&store, &xs)));
    }
    let (_, _, expected) = &stored[0];
    assert!(expected.iter().all(|(_, section)| section.is_some()));
    for (writers, order, sections) in &stored {
        assert_eq!(sections, expected, "{writers} writers, {order:?}");
    }
}

#[test]
fn disabled_ingestion_queues_no_distant_terrain_work() {
    let store = TestStore::new();
    let mut stream = stream();
    LOD_JOBS_QUEUED.set(0);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    assert_eq!(stream.distant_terrain_stats(), None);
    assert_eq!(LOD_JOBS_QUEUED.get(), 0, "nothing reached the Lod lane");
    assert_eq!(store.chunk_hash(1), None, "nothing reached the store");

    let link = enable(&mut stream, &store);
    stream.disable_distant_terrain();
    commit_column(&mut stream, 2, 2, STONE, 15);
    settle(&mut stream);
    stream.evict_column(ChunkKey::new(0, 2, 0));
    drop(stream);
    assert_eq!(LOD_JOBS_QUEUED.get(), 0);
    assert!(
        link.poll_written(),
        "the stopped stream left nothing behind"
    );
    assert_eq!(store.chunk_hash(2), None);
}

#[test]
fn a_stream_that_stops_ingesting_writes_its_resident_columns() {
    let store = TestStore::new();
    let mut stream = stream();
    let link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    // Still settling: only the final capture takes the column.
    stream.finish_distant_terrain();
    assert!(!stream.distant_terrain_enabled());
    assert!(link.is_released());
    eventually("the final captures to be written", || link.poll_written());
    assert_eq!(link.stats().merged_columns, 1);
    assert!(store.chunk_hash(1).is_some());
}

#[test]
fn dropping_an_ingesting_stream_writes_its_resident_columns() {
    let store = TestStore::new();
    let mut stream = stream();
    let link = enable(&mut stream, &store);
    commit_columns(&mut stream, 1, &[1, 2], 15);
    assert!(!link.poll_written(), "the stream still captures");
    drop(stream);
    eventually("the final captures to be written", || link.poll_written());
    assert_eq!(link.stats().merged_columns, 2);
    assert!(store.chunk_hash(1).is_some() && store.chunk_hash(2).is_some());
}

#[test]
fn a_stopped_store_leaves_no_hash_vouching_for_unwritten_content() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);
    let TestStore { actor, _dir } = store;
    actor.shutdown(NOW);

    // The change merges from cached sections alone, so only its submits meet the
    // stopped store.
    commit_column(&mut stream, 2, 1, WATER_ID, 15);
    settle(&mut stream);
    eventually("the refused writes", || {
        let stats = stats(&stream);
        stats.store_errors > 0 && stats.is_settled()
    });
    let shared = Arc::clone(&stream.lod.as_ref().expect("on").shared);
    assert_eq!(
        shared.lock_book().check(
            (OVERWORLD, ChunkKey::new(0, 1, 0)),
            u64::MAX,
            0,
            shared.generation()
        ),
        Verdict::Unknown,
        "the refused hash is forgotten"
    );
}
