use std::sync::Mutex;

use super::{
    admission::{Admission, JOB_OVERHEAD, Policy},
    producer::{Executor, Job},
    *,
};

#[derive(Clone, Default)]
struct Manual(Arc<Mutex<VecDeque<Job>>>);

impl Manual {
    /// Holds accepted work until a test explicitly executes or rejects it.
    fn executor(&self) -> Executor {
        let pending = Arc::clone(&self.0);
        Arc::new(move |job| pending.lock().unwrap().push_back(job))
    }

    /// Executes the oldest accepted job after releasing the manual queue lock.
    fn run(&self) -> bool {
        let job = self.0.lock().unwrap().pop_front();
        if let Some(job) = job {
            job();
            true
        } else {
            false
        }
    }

    /// Observes accepted closures independently from retained source backlog.
    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// Uses one admission owner across every manual inbox in a fixture.
fn owner(jobs: usize) -> Arc<Admission> {
    let slice_bytes = far_terrain::SurfaceSlice::MIN_BYTES;
    Arc::new(Admission::new(Policy {
        jobs,
        bytes: jobs * (slice_bytes + JOB_OVERHEAD),
        slice_bytes,
    }))
}

/// Attaches a new active or retiring inbox to the fixture's shared owner and executor.
fn inbox(owner: &Arc<Admission>, manual: &Manual) -> FarSummaryInbox {
    FarSummaryInbox::new(Producer::new(Arc::clone(owner), manual.executor()))
}

/// Captures registry handles exactly as an owner stream does, with replaceable asset identity.
fn context(assets: &Arc<RuntimeAssets>) -> PendingContext {
    let tints = Arc::new(assets.biome_assets().resolve_live(&[]).unwrap());
    PendingContext {
        prepared: PreparedAssets::shared(Arc::clone(assets)),
        air: 0,
        assets: Arc::clone(assets),
        mode: NetworkIdMode::Sequential,
        tints,
        default_biome: 0,
    }
}

/// Builds immutable uniform sections without any session or wire-cache dependency.
fn snapshot(id: u8, count: i32) -> ColumnSnapshot {
    let section = Arc::new(SubChunk::decode(
        &[9, 1, 0, 1, id << 1],
        &world::RawBlockIds { air: 0 },
    ));
    ColumnSnapshot::new(
        Arc::new((0..count).map(|y| (y, Arc::clone(&section))).collect()),
        Vec::new(),
    )
}

/// Keeps every summary visible to the fixture while all inboxes make bounded progress.
fn settle(
    inboxes: &mut [FarSummaryInbox],
    manual: &Manual,
    owner: &Admission,
) -> Vec<FarColumnSummary> {
    let mut out = Vec::new();
    for _ in 0..40_000 {
        for inbox in inboxes.iter_mut() {
            inbox.drain(&mut out);
        }
        let usage = owner.usage();
        assert!(usage.jobs <= owner.policy().jobs);
        assert!(usage.bytes <= owner.policy().bytes);
        if inboxes.iter().all(FarSummaryInbox::is_idle) {
            assert_eq!(owner.usage().jobs, 0);
            return out;
        }
        manual.run();
    }
    panic!("finite source backlog did not drain");
}

/// Thousands of retired sources never become thousands of worker closures or uncharged results.
#[test]
fn active_and_retired_feeds_share_one_job_and_byte_ceiling() {
    let owner = owner(4);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let mut feeds: Vec<_> = (0..5).map(|_| inbox(&owner, &manual)).collect();
    for (session, feed) in feeds.iter_mut().enumerate() {
        let count = if session == 2 { 4096 } else { 17 };
        for x in 0..count {
            feed.enqueue(
                ChunkKey::new(session as i32, x, 0),
                snapshot(0, 1 + x % 3),
                context(&assets),
            );
        }
        feed.pump();
    }
    assert_eq!(
        manual.len(),
        1,
        "one shared preparation, no waiting consumers on workers"
    );
    assert!(feeds.iter().all(|feed| !feed.is_idle()));
    assert!(feeds[2].producer.source_bytes() > owner.usage().bytes);
    manual.run();
    for feed in &mut feeds {
        feed.pump();
    }
    assert_eq!(manual.len(), 4);
    let sources: usize = feeds.iter().map(|feed| feed.producer.sources.len()).sum();
    assert_eq!(sources, 4096 + 4 * 17);
    let out = settle(&mut feeds, &manual, &owner);
    assert_eq!(out.len(), sources);
    assert_eq!(
        out.iter()
            .map(|summary| summary.column)
            .collect::<HashSet<_>>()
            .len(),
        sources
    );
}

/// Byte denial leaves the final source intact; mixed copied sizes reuse the same capacity.
#[test]
fn byte_pressure_defers_sources_and_delivery_retains_its_reservation() {
    let owner = owner(4);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let mut feeds = [inbox(&owner, &manual), inbox(&owner, &manual)];
    feeds[0].enqueue(ChunkKey::new(0, 1, 0), snapshot(0, 1), context(&assets));
    feeds[0].pump();
    manual.run();
    let held = owner
        .acquire(3 * (owner.slice_bytes() + JOB_OVERHEAD))
        .unwrap();
    feeds[0].pump();
    manual.run();
    let before = owner.usage();
    assert_eq!(before.jobs, 2);
    feeds[1].enqueue(ChunkKey::new(1, 1, 0), snapshot(0, 3), context(&assets));
    // Fill the remaining bytes, while leaving spare job-count capacity.
    let spare = owner.policy().bytes - before.bytes;
    let bytes = owner.acquire(spare).unwrap();
    feeds[1].pump();
    assert_eq!(manual.len(), 0);
    assert_eq!(feeds[1].producer.sources.len(), 1);
    let mut out = Vec::new();
    feeds[0].drain_budgeted(&mut out, &mut SummaryBudget::new(0, 0));
    assert!(out.is_empty());
    assert!(feeds[0].has_finished());
    assert_eq!(owner.usage().jobs, 3);
    drop(bytes);
    drop(held);
    feeds[0].drain(&mut out);
    assert_eq!(out.len(), 1);
    let rest = settle(&mut feeds, &manual, &owner);
    assert_eq!(rest.len(), 1);
}

/// An oversized raw column advances by bounded section copies, including through a deadline drain.
#[test]
fn tall_retired_source_progresses_without_retaining_it_in_a_worker() {
    let owner = owner(1);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    // Uniform air forces the scanner to visit every section instead of stopping at a solid top.
    let source = snapshot(0, 4096);
    let mut feeds = [inbox(&owner, &manual)];
    for x in 0..3 {
        feeds[0].enqueue(ChunkKey::new(0, x, 0), source.clone(), context(&assets));
    }
    assert!(feeds[0].producer.source_bytes() > owner.slice_bytes());
    let mut out = Vec::new();
    feeds[0].drain_waiting(&mut out, Instant::now());
    assert!(!feeds[0].is_idle());
    assert!(out.is_empty());
    let out = settle(&mut feeds, &manual, &owner);
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|finished| {
        far_terrain::ColumnSummary::decode(&finished.summary.bytes)
            .unwrap()
            .is_empty()
    }));
}

/// A resend replaces a denied source, while already-issued older output remains stale work.
#[test]
fn duplicate_sources_and_reverse_completions_deliver_only_the_latest() {
    let owner = owner(4);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let mut feed = inbox(&owner, &manual);
    let column = ChunkKey::new(0, 3, 0);
    feed.enqueue(column, snapshot(1, 1), context(&assets));
    feed.enqueue(column, snapshot(2, 1), context(&assets));
    assert_eq!(feed.producer.sources.len(), 1);
    feed.pump();
    manual.run();
    feed.pump();
    feed.enqueue(column, snapshot(0, 1), context(&assets));
    let generation = feed.issued_generation();
    feed.pump();
    let newest = manual.0.lock().unwrap().pop_back().unwrap();
    newest();
    manual.run();
    let mut out = Vec::new();
    feed.drain_budgeted(&mut out, &mut SummaryBudget::new(1, usize::MAX));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].generation, generation);
    assert!(!feed.is_idle(), "stale output still consumes delivery work");
    feed.drain(&mut out);
    assert_eq!(out.len(), 1);
    assert!(feed.is_idle());
    assert!(
        far_terrain::ColumnSummary::decode(&out[0].summary.bytes)
            .unwrap()
            .is_empty()
    );
    assert_eq!(owner.usage().jobs, 0);
}

/// Rejected closures return capacity and retry from the original immutable source.
#[test]
fn cancelled_work_and_disconnected_receivers_release_permits() {
    let owner = owner(2);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let mut feeds = [inbox(&owner, &manual)];
    feeds[0].enqueue(ChunkKey::new(0, 0, 0), snapshot(1, 1), context(&assets));
    feeds[0].pump();
    manual.run();
    feeds[0].pump();
    let cancelled = manual.0.lock().unwrap().pop_front().unwrap();
    assert_eq!(owner.usage().jobs, 1);
    drop(cancelled);
    assert_eq!(owner.usage().jobs, 0);
    assert_eq!(settle(&mut feeds, &manual, &owner).len(), 1);
    let mut disconnected = inbox(&owner, &manual);
    disconnected.enqueue(ChunkKey::new(0, 1, 0), snapshot(2, 1), context(&assets));
    disconnected.pump();
    manual.run();
    disconnected.pump();
    drop(disconnected);
    while manual.run() {}
    assert_eq!(owner.usage().jobs, 0);
}

/// Each retained asset allocation prepares once, including simultaneous dimensions and replacement.
#[test]
fn preparation_is_shared_nonblocking_and_invalidated_by_asset_identity() {
    let owner = owner(4);
    let manual = Manual::default();
    let old = Arc::new(RuntimeAssets::diagnostic());
    let replacement = Arc::new(RuntimeAssets::diagnostic());
    let mut feeds = [inbox(&owner, &manual), inbox(&owner, &manual)];
    for (dimension, feed) in feeds.iter_mut().enumerate() {
        feed.enqueue(
            ChunkKey::new(dimension as i32, 0, 0),
            snapshot(0, 1),
            context(&old),
        );
        feed.pump();
    }
    assert_eq!(manual.len(), 1);
    assert!(PreparedAssets::shared(Arc::clone(&old)).colours().is_none());
    manual.run();
    let prepared = PreparedAssets::shared(Arc::clone(&old));
    assert_eq!(
        prepared.colours().unwrap().as_ref(),
        &SurfaceColours::new(&old)
    );
    feeds[0].enqueue(
        ChunkKey::new(0, 1, 0),
        snapshot(0, 1),
        context(&replacement),
    );
    feeds[0].pump();
    assert!(
        PreparedAssets::shared(Arc::clone(&replacement))
            .colours()
            .is_none()
    );
    let out = settle(&mut feeds, &manual, &owner);
    assert_eq!(out.len(), 3);
    assert_eq!(
        out.iter()
            .map(|summary| summary.summary.hash)
            .collect::<HashSet<_>>()
            .len(),
        1
    );
}

/// Dropping a stream's inbox releases raw sources, while admitted copies retain valid context.
#[test]
fn source_and_registry_lifetimes_follow_delivery_and_cancellation() {
    let owner = owner(2);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let weak_assets = Arc::downgrade(&assets);
    let source = snapshot(1, 1);
    let mut feed = inbox(&owner, &manual);
    feed.enqueue(ChunkKey::new(0, 0, 0), source, context(&assets));
    drop(assets);
    feed.pump();
    manual.run();
    feed.pump();
    drop(feed);
    assert!(
        weak_assets.upgrade().is_some(),
        "admitted work retains the old registry"
    );
    while manual.run() {}
    assert!(weak_assets.upgrade().is_none());
    assert_eq!(owner.usage().jobs, 0);
}

/// Interleaved dimensions retain their own air, network mode and biome routing.
#[test]
fn interleaved_contexts_match_their_synchronous_builder() {
    let owner = owner(4);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let source = snapshot(1, 1);
    let mut feeds = [inbox(&owner, &manual), inbox(&owner, &manual)];
    let mut expected = HashMap::new();
    for (dimension, feed) in feeds.iter_mut().enumerate() {
        let mut captured = context(&assets);
        captured.air = dimension as u32;
        captured.mode = if dimension == 0 {
            NetworkIdMode::Sequential
        } else {
            NetworkIdMode::Hashed
        };
        captured.default_biome = dimension as u32 + 7;
        let synchronous = SurfaceContext {
            air: captured.air,
            assets: Arc::clone(&captured.assets),
            mode: captured.mode,
            colours: Arc::new(SurfaceColours::new(&assets)),
            tints: Arc::clone(&captured.tints),
            default_biome: captured.default_biome,
        };
        let column = ChunkKey::new(dimension as i32, 0, 0);
        expected.insert(
            column,
            far_terrain::summarise_column(&synchronous, &source).encode(),
        );
        feed.enqueue(column, source.clone(), captured);
    }
    let out = settle(&mut feeds, &manual, &owner);
    assert_eq!(out.len(), 2);
    for finished in out {
        assert_eq!(finished.summary, expected[&finished.column]);
    }
}

/// Installs the manual executor in a live stream without changing its admission owner on disable.
fn manual_stream(owner: &Arc<Admission>, manual: &Manual) -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream.far_terrain.inbox = inbox(owner, manual);
    stream.set_far_terrain_enabled(true);
    stream
}

/// A mass eviction captures bounded batches and delivers every column without worker scheduling.
#[test]
fn retiring_columns_are_admitted_a_bounded_batch_per_poll() {
    let owner = owner(1);
    let manual = Manual::default();
    let mut live = manual_stream(&owner, &manual);
    let columns: HashSet<_> = (0..100).map(|x| ChunkKey::new(0, x, 0)).collect();
    live.far_terrain.retiring.extend(
        columns
            .iter()
            .map(|&column| (column, ColumnSnapshot::new(Arc::default(), Vec::new()))),
    );
    live.dispatch_far_summaries();
    let left = live.far_terrain.retiring.len();
    assert!(left < columns.len());
    assert!(columns.len() - left <= JOBS_PER_POLL);
    live.dispatch_far_summaries();
    assert_eq!(
        live.far_terrain.retiring.len(),
        left,
        "unfinished work defers further source capture"
    );
    let mut summaries = Vec::new();
    assert!(manual.run(), "prepare the shared surface colours");
    for expected in 1..=columns.len() {
        let before = live.far_terrain.retiring.len();
        live.dispatch_far_summaries();
        assert!(before - live.far_terrain.retiring.len() <= JOBS_PER_POLL);
        assert!(manual.run(), "available capacity must advance a source");
        live.drain_far_summaries(&mut summaries);
        assert_eq!(summaries.len(), expected, "completed sources are delivered");
    }
    assert!(live.far_summaries_idle());
    assert_eq!(manual.len(), 0);
    assert_eq!(owner.usage().jobs, 0);
    assert_eq!(owner.usage().bytes, 0);
    assert_eq!(
        summaries
            .iter()
            .map(|summary| summary.column)
            .collect::<HashSet<_>>(),
        columns
    );
}

/// Every publication/cancellation ordering releases cancelled output while the live stream stays off.
#[test]
fn disabling_live_far_terrain_releases_results_without_polling_it() {
    #[derive(Clone, Copy)]
    enum CancelAt {
        Preparing,
        Queued,
        Returned,
        Deferred,
    }
    for at in [
        CancelAt::Preparing,
        CancelAt::Queued,
        CancelAt::Returned,
        CancelAt::Deferred,
    ] {
        let owner = owner(1);
        let manual = Manual::default();
        let mut live = manual_stream(&owner, &manual);
        let assets = Arc::clone(live.runtime_assets());
        live.spawn_far_summaries(vec![(ChunkKey::new(0, 1, 0), snapshot(1, 1))]);
        live.far_terrain.inbox.pump();
        // The retiring session shares admission but has an independent delivery lifetime.
        let mut retiring = [inbox(&owner, &manual)];
        retiring[0].enqueue(ChunkKey::new(-1, 2, 0), snapshot(2, 1), context(&assets));
        if !matches!(at, CancelAt::Preparing) {
            assert!(manual.run());
            live.far_terrain.inbox.pump();
        }
        if matches!(at, CancelAt::Returned | CancelAt::Deferred) {
            assert!(manual.run());
        }
        if matches!(at, CancelAt::Deferred) {
            live.far_terrain
                .inbox
                .drain_budgeted(&mut Vec::new(), &mut SummaryBudget::new(0, 0));
            assert!(live.far_terrain.inbox.deferred.is_some());
        }
        assert_eq!(owner.usage().jobs, 1);
        retiring[0].pump();
        live.set_far_terrain_enabled(false);
        if matches!(at, CancelAt::Preparing | CancelAt::Queued) {
            assert_eq!(
                owner.usage().jobs,
                1,
                "accepted work retains capacity until it returns"
            );
            assert!(manual.run());
        }
        assert_eq!(owner.usage().jobs, 0);
        assert_eq!(owner.usage().bytes, 0);
        let out = settle(&mut retiring, &manual, &owner);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].column, ChunkKey::new(-1, 2, 0));
        assert!(!live.far_terrain_enabled());
        assert!(live.far_summaries_idle());
        assert!(!live.has_far_summaries());
    }
}

/// Returning cancelled work cannot acknowledge a newer issue after far terrain is enabled again.
#[test]
fn cancelled_delivery_is_disconnected_from_reenabled_generation() {
    let owner = owner(2);
    let manual = Manual::default();
    let mut live = manual_stream(&owner, &manual);
    let column = ChunkKey::new(0, 1, 0);
    live.spawn_far_summaries(vec![(column, snapshot(1, 1))]);
    live.far_terrain.inbox.pump();
    assert!(manual.run());
    live.far_terrain.inbox.pump();
    let old_generation = live.far_terrain_issued_generation();
    live.set_far_terrain_enabled(false);
    live.set_far_terrain_enabled(true);
    live.spawn_far_summaries(vec![(column, snapshot(0, 1))]);
    assert!(live.far_terrain_issued_generation() > old_generation);
    // Publish new work before allowing the cancelled job to return.
    live.far_terrain.inbox.pump();
    let preparation = manual.0.lock().unwrap().pop_back().unwrap();
    preparation();
    live.far_terrain.inbox.pump();
    let newest = manual.0.lock().unwrap().pop_back().unwrap();
    newest();
    assert!(manual.run());
    let mut out = Vec::new();
    live.far_terrain.inbox.drain(&mut out);
    assert_eq!(out.len(), 1);
    assert!(
        far_terrain::ColumnSummary::decode(&out[0].summary.bytes)
            .unwrap()
            .is_empty()
    );
    assert!(live.far_summaries_idle());
    assert_eq!(owner.usage().jobs, 0);
}

/// A group wait observes a deferred result even when every channel has already been emptied.
#[test]
fn group_wait_sees_deferred_delivery_in_another_inbox() {
    let owner = owner(1);
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let mut pending = inbox(&owner, &manual);
    let mut ready = inbox(&owner, &manual);
    ready.enqueue(ChunkKey::new(0, 1, 0), snapshot(1, 1), context(&assets));
    ready.pump();
    assert!(manual.run());
    ready.pump();
    assert!(manual.run());
    pending.enqueue(ChunkKey::new(1, 2, 0), snapshot(2, 1), context(&assets));
    let mut out = Vec::new();
    pending.drain_budgeted(&mut out, &mut SummaryBudget::new(0, 0));
    ready.drain_budgeted(&mut out, &mut SummaryBudget::new(0, 0));
    assert!(pending.rx.is_empty());
    assert!(ready.rx.is_empty());
    assert!(ready.deferred.is_some());
    assert!(FarSummaryInbox::wait_for_any(
        [&pending, &ready],
        Instant::now()
    ));
    assert_eq!(owner.usage().jobs, 1);
    ready.drain(&mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(settle(&mut [pending, ready], &manual, &owner).len(), 1);
}

/// Retains sixteen-bit physical words and old slots after every cell returns to one value.
fn shrunk_physical_snapshot() -> ColumnSnapshot {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut store = world::ChunkStore::new();
    store
        .commit_sub_chunk(
            key,
            SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 }),
        )
        .unwrap();
    let updates = (0..257)
        .map(|cell| {
            world::BlockUpdate::new(
                (cell >> 8) as u8,
                (cell & 15) as u8,
                ((cell >> 4) & 15) as u8,
                0,
                cell as u32 + 2,
            )
        })
        .collect::<Vec<_>>();
    store.update_sub_chunk_blocks(key, &updates, 0).unwrap();
    for update in updates {
        store
            .update_block(
                key,
                world::BlockUpdate {
                    runtime_id: 1,
                    ..update
                },
                0,
            )
            .unwrap();
    }
    let section = store.sub_chunk(key).unwrap();
    assert_eq!(section.storages()[0].bits_per_index(), 0);
    ColumnSnapshot::new(
        Arc::new((0..64).map(|y| (y, Arc::clone(&section))).collect()),
        Vec::new(),
    )
}

/// Shrunk logical palettes still consume the unchanged process-wide copied-work byte budget.
#[test]
fn physical_palette_retention_limits_slice_and_global_job_admission() {
    let policy = admission::POLICY;
    let owner = Arc::new(Admission::new(policy));
    let manual = Manual::default();
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let source = shrunk_physical_snapshot();
    let slice = source.surface_slice(None, policy.slice_bytes);
    assert!(
        slice.next().is_some(),
        "physical inputs must split across slices"
    );
    assert!(slice.retained_bytes() <= policy.slice_bytes);
    assert!(source.retained_payload_bytes() > policy.slice_bytes);
    let mut feeds = (0..policy.jobs + 1)
        .map(|_| inbox(&owner, &manual))
        .collect::<Vec<_>>();
    for (index, feed) in feeds.iter_mut().enumerate() {
        feed.enqueue(
            ChunkKey::new(index as i32, 0, 0),
            source.clone(),
            context(&assets),
        );
    }
    feeds[0].pump();
    assert!(manual.run(), "prepare the shared immutable assets first");
    for feed in &mut feeds {
        feed.pump();
    }
    let reservation = policy.slice_bytes + JOB_OVERHEAD;
    let admitted = policy.jobs.min(policy.bytes / reservation);
    assert_eq!(manual.len(), admitted);
    assert_eq!(owner.usage().jobs, admitted);
    assert_eq!(owner.usage().bytes, admitted * reservation);
    assert!(owner.usage().bytes <= policy.bytes);
    assert!(admitted < feeds.len());
    assert!(feeds.iter().all(|feed| feed.producer.sources.len() == 1));
    drop(feeds);
    manual.0.lock().unwrap().clear();
    assert_eq!(owner.usage().bytes, 0);
    assert_eq!(owner.usage().jobs, 0);
}
