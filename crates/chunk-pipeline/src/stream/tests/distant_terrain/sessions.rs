//! Ingestion across store session changes and a full store queue: nothing is skipped for
//! another epoch's content, merged from another epoch's sections, or lost while it waits.

use std::num::NonZeroUsize;

use lod::voxel::Voxel;
use lod_store::actor::{EpochWrite, ReadOutcome};

use super::*;
use crate::stream::distant_terrain::{
    shared::{LodShared, WriterInput},
    writer::Writer,
};

/// Ends the overworld session and begins it again, as leaving and coming back does.
fn begin_again(store: &mut TestStore) {
    for request in [
        Request::End {
            dimension: OVERWORLD,
        },
        Request::Begin {
            dimension: OVERWORLD,
            now: NOW,
        },
    ] {
        store
            .actor
            .try_submit(request)
            .expect("session request queued");
    }
}

#[test]
fn writes_a_full_store_queue_refused_keep_ingestion_unsettled_until_sent() {
    let mut store = TestStore::with_config(ActorConfig {
        requests: NonZeroUsize::new(2).expect("non-zero"),
        ..ActorConfig::default()
    });
    let mut stream = stream();
    let link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);
    let first = store.chunk_hash(1);

    // The change merges from cached sections alone, so only its writes meet the full queue.
    let paused = store.actor.pause();
    while store.actor.try_submit(Request::Flush { now: NOW }).is_ok() {}
    commit_column(&mut stream, 2, 1, WATER_ID, 15);
    settle(&mut stream);
    eventually("the writes to meet the full queue", || {
        stats(&stream).store_queue_full > 0
    });
    for _ in 0..3 {
        stream.dispatch_lod_work();
    }
    assert!(
        !stats(&stream).is_settled(),
        "held writes keep ingestion unsettled"
    );
    assert!(!link.poll_written());

    drop(paused);
    eventually("the held writes to be sent", || {
        stream.dispatch_lod_work();
        stats(&stream).is_settled()
    });
    assert_eq!(stats(&stream).merged_columns, 2);
    let changed = store.chunk_hash(1);
    assert!(changed.is_some());
    assert_ne!(changed, first, "the change reached the store");
}

#[test]
fn a_session_begun_again_samples_columns_its_epoch_never_stored() {
    let mut store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);
    // The next poll keeps the fingerprint, so the eviction is checked on the frame.
    stream.dispatch_lod_work();

    // The fresh session is a candidate of the stored epoch: it needs the column's sample
    // as a vote, though the column book saw its hash stored a moment ago.
    begin_again(&mut store);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    wait_for_merges(&stream, 2);
    assert_eq!(stats(&stream).skipped_unchanged, 0);
    assert!(
        store.chunk_hash(1).is_some(),
        "the candidate holds the column"
    );
}

/// Samples resident chunk `(x, 0)` of `stream` as a changed capture and hands the sample
/// to `shared`'s writer inbox.
fn sample_into(stream: &mut WorldStream, shared: &Arc<LodShared>, x: i32, seq: u64) {
    let blocks = session_blocks(&mut None, &stream.authority);
    let input = stream
        .lod_column_input(ChunkKey::new(0, x, 0), blocks)
        .expect("resident");
    jobs::sample(
        shared,
        Capture {
            seq,
            kind: CaptureKind::Changed,
            input,
            checked_hash: Some(seq),
            relit: false,
            job: None,
            final_capture: None,
        },
    );
}

/// The voxel at `(x, 0, 0)` of the level-1 section above chunk `(1, 0)`'s sub-chunk, read
/// in the overworld session's own epoch.
fn own_level_one_voxel(store: &TestStore, x: usize) -> Voxel {
    let key = chain(1)[1].key();
    let record = match store
        .client()
        .read(OVERWORLD, ReadTarget::OwnSection(key), NOW)
        .expect("read queued")
    {
        ReadOutcome::Found(record) => record,
        outcome => panic!("level-1 section not stored: {outcome:?}"),
    };
    let section = SectionCodec::new()
        .decode_expecting(&record, key)
        .expect("valid record");
    let mut dense = lod::mip::unknown_section();
    section.expand_into(&mut dense);
    dense.get(x, 0, 0)
}

#[test]
fn a_merge_prepared_before_a_session_change_is_merged_again_in_the_new_epoch() {
    let mut store = TestStore::new();
    let mut stream = stream();
    commit_columns(&mut stream, 1, &[1, 2], 15);
    // No writer thread: the test steps the writer itself.
    let shared = Arc::new(LodShared::new(store.client(), NOW));
    let mut writer = Writer::default();
    sample_into(&mut stream, &shared, 1, 1);
    let batch = shared.wait_for_work(8, false).expect("ingesting");
    writer.step(&shared, batch);
    assert_eq!(shared.stats(0).merged_columns, 1);

    // Chunk 2 shares chunk 1's level-1 section, which the writer has cached.
    sample_into(&mut stream, &shared, 2, 2);
    let Some(WriterInput::Sampled(column)) =
        shared.wait_for_work(8, false).expect("ingesting").pop()
    else {
        panic!("a sampled column");
    };
    let Ok(prepared) = writer.prepare(&shared, OVERWORLD, vec![*column]) else {
        panic!("merged from the cache");
    };
    // The session begins again before the merge reaches the store.
    begin_again(&mut store);
    assert!(writer.send(&shared, prepared), "the store answered");
    assert!(
        writer.is_blocked(),
        "the refused column waits to merge again"
    );
    writer.step(&shared, Vec::new());
    assert!(!writer.is_blocked());
    let stats = shared.stats(0);
    assert_eq!((stats.stale_writes, stats.merged_columns), (1, 2));

    // The new candidate holds chunk 2, and none of the old epoch's chunk 1.
    assert_eq!(
        own_level_one_voxel(&store, 12),
        lod::mip::UNKNOWN_CHILD_FILL
    );
    assert_ne!(
        own_level_one_voxel(&store, 20),
        lod::mip::UNKNOWN_CHILD_FILL
    );
    assert!(store.chunk_hash(2).is_some());
    assert_eq!(store.chunk_hash(1), None);
    shared.disable();
}

#[test]
fn writes_sent_in_the_current_epoch_are_taken() {
    let store = TestStore::new();
    let client = store.client();
    let written = client.write_in_epoch(
        client.session_epoch(),
        vec![Request::ChunkHash {
            dimension: OVERWORLD,
            x: 5,
            z: 0,
            hash: Some(9),
        }],
    );
    assert!(matches!(written, Ok(EpochWrite::Taken)));
    assert_eq!(store.chunk_hash(5), Some(9));
}
