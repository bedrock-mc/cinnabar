//! What ingestion does when it has no room: changed columns wait dirty and are taken
//! later, final captures go ahead, and nothing a join settles is lost.

use std::{num::NonZeroUsize, sync::atomic::Ordering};

use super::*;
use crate::stream::distant_terrain::{
    MAX_LOD_BACKLOG, MAX_LOD_INBOX_COLUMNS,
    shared::{InFlight, LodShared, WriterInput, Writers},
};

/// Columns arriving together at a join, twice the backlog and within the test view.
const JOIN_COLUMNS: i32 = 2 * MAX_LOD_BACKLOG as i32;

#[test]
fn a_full_backlog_keeps_changed_columns_dirty_but_takes_final_captures() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_columns(&mut stream, 1, &[1, 2], 15);
    let shared = Arc::clone(&stream.lod.as_ref().expect("on").shared);
    shared
        .in_flight
        .fetch_add(MAX_LOD_BACKLOG, Ordering::SeqCst);
    settle(&mut stream);
    let full = stats(&stream);
    assert_eq!(full.dirty_columns, 2, "due columns wait for room");
    assert_eq!((full.jobs_queued, full.deferred), (0, 0));

    // The eviction's final capture goes ahead while the backlog stays full.
    stream.evict_column(ChunkKey::new(0, 1, 0));
    eventually("the final capture to be written", || {
        stats(&stream).merged_columns == 1
    });
    assert_eq!(
        stats(&stream).dirty_columns,
        1,
        "the evicted column left the queue"
    );
    shared
        .in_flight
        .fetch_sub(MAX_LOD_BACKLOG, Ordering::SeqCst);
    // The eviction relit the neighbour's border.
    settle_light(&mut stream, 2, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 2);
    let after = stats(&stream);
    assert_eq!((after.dirty_columns, after.dropped_final_captures), (0, 0));
    assert!(store.section(chain(1)[0]).is_some());
    assert!(
        store.chunk_hash(2).is_some(),
        "the waiting resident column is stored once there is room"
    );
}

#[test]
fn a_join_burst_behind_a_slow_writer_stores_every_column() {
    // A one-request store queue keeps the writers holding merges back until each nudge.
    let store = TestStore::with_config(ActorConfig {
        requests: NonZeroUsize::new(1).expect("non-zero"),
        batch: NonZeroUsize::new(1).expect("non-zero"),
        ..ActorConfig::default()
    });
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    // The Lod lane stays behind the frame's other work throughout the join and for a
    // whole settle period after every column is due.
    let shared = Arc::clone(&stream.lod.as_ref().expect("on").shared);
    shared
        .in_flight
        .fetch_add(MAX_LOD_BACKLOG, Ordering::SeqCst);
    let xs: Vec<i32> = (-JOIN_COLUMNS / 2..JOIN_COLUMNS / 2).collect();
    commit_columns(&mut stream, 1, &xs, 15);
    settle(&mut stream);
    settle(&mut stream);
    assert_eq!(stats(&stream).dirty_columns, xs.len(), "every column waits");
    shared
        .in_flight
        .fetch_sub(MAX_LOD_BACKLOG, Ordering::SeqCst);
    eventually("every joined column to be merged", || {
        stream.dispatch_lod_work();
        let stats = stats(&stream);
        stats.dirty_columns == 0 && stats.is_settled()
    });
    let done = stats(&stream);
    assert_eq!(done.dropped_final_captures, 0);
    for &x in &xs {
        assert!(store.chunk_hash(x).is_some(), "column {x} is stored");
    }
}

#[test]
fn a_full_writer_inbox_defers_changed_samples_but_takes_final_ones() {
    let store = TestStore::new();
    let mut stream = stream();
    commit_column(&mut stream, 1, 1, STONE, 15);
    // No writer thread runs for this state, as when the writer is busy for a long time.
    let shared = Arc::new(LodShared::new(store.client(), NOW, Writers::new(1)));
    let blocks = session_blocks(&mut None, &stream.authority);
    let capture = |seq, kind| Capture {
        seq,
        kind,
        input: stream
            .lod_column_input(ChunkKey::new(0, 1, 0), Arc::clone(&blocks))
            .expect("resident"),
        checked_hash: None,
        relit: false,
        job: Some(InFlight::start(&shared, InFlight::SAMPLE)),
        final_capture: (kind == CaptureKind::Final)
            .then(|| InFlight::start(&shared, InFlight::FINAL)),
    };
    for seq in 0..MAX_LOD_INBOX_COLUMNS as u64 {
        let check = WriterInput::Check(Box::new(capture(seq, CaptureKind::Changed)));
        assert!(shared.push(check).is_ok());
    }
    jobs::sample(&shared, capture(100, CaptureKind::Changed));
    jobs::sample(&shared, capture(101, CaptureKind::Final));
    let stats = shared.stats(0);
    assert_eq!((stats.deferred, stats.dropped_final_captures), (1, 0));
    assert_eq!(
        stats.dirty_columns, 1,
        "the deferred column counts as dirty"
    );
    assert_eq!(
        shared.take_deferred(),
        vec![(ChunkKey::new(0, 1, 0), false)],
        "the refused check goes back to the frame, keeping its relit flag"
    );
    assert_eq!(
        (stats.in_flight, stats.final_captures),
        (MAX_LOD_INBOX_COLUMNS + 1, 1),
        "the final capture waits in the inbox beyond its bound"
    );
    shared.disable();
    assert_eq!(shared.queued(), 0, "disabling takes queued work");
    // The frame never pays for releasing it.
    eventually("queued work to be released off the caller", || {
        let stopped = shared.stats(0);
        (stopped.in_flight, stopped.final_captures) == (0, 0)
    });
}

#[test]
fn a_deferred_resident_column_is_taken_again_and_stored() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);
    let shared = Arc::clone(&stream.lod.as_ref().expect("on").shared);
    let sampled = stats(&stream).sampled;

    // As if the inbox had refused a newer changed sample of the stored column.
    shared.defer(ChunkKey::new(0, 1, 0), true);
    // And of a column evicted since, whose final capture kept its last state.
    shared.defer(ChunkKey::new(0, 9, 0), true);
    stream.dispatch_lod_work();
    eventually("the deferred column to be sampled again", || {
        stream.dispatch_lod_work();
        let stats = stats(&stream);
        stats.sampled > sampled && stats.dirty_columns == 0 && stats.is_settled()
    });
    assert_eq!(
        stats(&stream).sampled,
        sampled + 1,
        "only the resident column"
    );
}
