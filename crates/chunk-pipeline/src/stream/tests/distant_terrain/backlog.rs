//! What ingestion drops when it has no room: changed columns only, never final captures.

use std::sync::atomic::Ordering;

use super::*;
use crate::stream::distant_terrain::{
    MAX_LOD_BACKLOG, MAX_LOD_INBOX_COLUMNS,
    shared::{InFlight, LodShared, WriterInput},
};

#[test]
fn a_full_backlog_drops_changed_columns_but_never_final_captures() {
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
    assert_eq!(full.dirty_columns, 0, "due columns are taken");
    assert_eq!(full.dropped, 2, "and dropped for the full backlog");
    assert_eq!(full.jobs_queued, 0);

    // The eviction's final capture goes ahead while the backlog stays full.
    stream.evict_column(ChunkKey::new(0, 1, 0));
    eventually("the final capture to be written", || {
        stats(&stream).merged_columns == 1
    });
    shared
        .in_flight
        .fetch_sub(MAX_LOD_BACKLOG, Ordering::SeqCst);
    eventually("ingestion to settle", || stats(&stream).is_settled());
    let after = stats(&stream);
    assert_eq!((after.dropped, after.dropped_final_captures), (2, 0));
    assert!(store.section(chain(1)[0]).is_some());
    assert_eq!(
        store.chunk_hash(2),
        None,
        "the dropped resident column waits"
    );
}

#[test]
fn a_full_writer_inbox_drops_changed_samples_but_takes_final_ones() {
    let store = TestStore::new();
    let mut stream = stream();
    commit_column(&mut stream, 1, 1, STONE, 15);
    // No writer thread runs for this state, as when the writer is busy for a long time.
    let shared = Arc::new(LodShared::new(store.client(), NOW));
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
    assert_eq!((stats.dropped, stats.dropped_final_captures), (1, 0));
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
