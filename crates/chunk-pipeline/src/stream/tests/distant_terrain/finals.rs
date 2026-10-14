//! Final captures: the pending bound counts only captures that still need writing, and a
//! stream that stops ingesting captures its resident columns a bounded number per poll.

use std::sync::atomic::Ordering;

use super::*;
use crate::stream::distant_terrain::{
    MAX_LOD_FINAL_CAPTURES_PER_POLL, MAX_LOD_PENDING_FINAL_CAPTURES,
};

#[test]
fn final_captures_needing_nothing_never_count_against_the_pending_bound() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    // Chunks 1 and 3 are not neighbours, so neither relights the other.
    commit_columns(&mut stream, 1, &[1, 3], 15);
    settle(&mut stream);
    wait_for_merges(&stream, 2);
    // The next poll keeps the fingerprints, so the eviction skips chunk 1 on the frame.
    stream.dispatch_lod_work();
    commit_column(&mut stream, 3, 3, WATER_ID, 15);
    let shared = Arc::clone(&stream.lod.as_ref().expect("on").shared);
    let others = MAX_LOD_PENDING_FINAL_CAPTURES - 1;
    shared.final_captures.fetch_add(others, Ordering::SeqCst);

    // One place is left: the unchanged chunk 1 needs none, so the changed chunk 3 takes it.
    stream.evict_columns(BTreeSet::from([
        ChunkKey::new(0, 1, 0),
        ChunkKey::new(0, 3, 0),
    ]));
    let evicted = stats(&stream);
    assert_eq!(evicted.skipped_unchanged, 1);
    assert_eq!(evicted.dropped_final_captures, 0);
    assert_eq!(evicted.final_captures, others + 1);

    shared.final_captures.fetch_sub(others, Ordering::SeqCst);
    eventually("the eviction to be written", || stats(&stream).is_settled());
    assert_eq!(stats(&stream).merged_columns, 3);
}

#[test]
fn a_stream_that_stops_ingesting_captures_a_bounded_number_of_columns_per_poll() {
    let store = TestStore::new();
    let mut stream = stream();
    let link = enable(&mut stream, &store);
    let columns = MAX_LOD_FINAL_CAPTURES_PER_POLL as i32 + 1;
    // Around the player, where the stream keeps every column.
    let xs: Vec<i32> = (-columns / 2..=columns / 2).collect();
    commit_columns(&mut stream, 1, &xs, 15);

    stream.finish_distant_terrain();
    assert!(!stream.distant_terrain_enabled());
    assert!(!link.is_released(), "one column waits for the next poll");
    assert_eq!(stats(&stream).dirty_columns, 1);
    stream.dispatch_lod_work();
    assert!(link.is_released());
    eventually("the final captures to be written", || link.poll_written());
    assert_eq!(link.stats().merged_columns, columns as u64);
}
