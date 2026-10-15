//! Deferred target changes must preserve published near coverage.

use super::*;

/// Publishes near geometry, then delivers the summary for its outgoing handoff.
pub(super) fn delivered_handoff() -> WorldStream {
    let mut stream = stream();
    for (sequence, x) in [(1, 0), (2, 5)] {
        stream
            .submit(sequence, inline_block_entity_event(x, 7, Vec::new()))
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    assert!(!upload(&mut stream, 5).is_empty());
    stream.set_full_detail_radius(Some(1));
    test_time::eventually("the outgoing summary", || {
        stream.dispatch_far_summaries();
        stream.drain_far_summaries(&mut Vec::new());
        stream.far_handoffs().any(|column| column.x == 5)
    });
    stream.mesh_jobs.pending.clear();
    stream.between_frames_service = true;
    stream.service_yield = Some(Arc::new(AtomicBool::new(true)));
    stream
}

/// Checks actual retained and published membership without allowing a replacement upload.
fn assert_near_coverage(stream: &WorldStream, revision: u64) {
    let column = ChunkKey::new(0, 5, 0);
    let keys: Vec<_> = stream.resident.column(column).copied().collect();
    assert!(!keys.is_empty(), "the column remains resident");
    assert!(drawn(stream, 5), "published near coverage must remain");
    assert_eq!(stream.near_drawn_columns().0, revision);
    assert!(keys.iter().all(|key| {
        stream.applied_mesh_generations.contains_key(key)
            && !stream.withdrawn_meshes.contains(key)
            && stream.revisions.dirty(*key).is_none()
    }));
    assert!(!meshing(stream, 5), "no obsolete removal may be queued");
}

/// The consumer cannot withdraw a published column newly inside the radius or its band.
#[test]
fn deferred_detail_target_filters_obsolete_handoffs() {
    for radius in [Some(6), Some(3), None] {
        let mut stream = delivered_handoff();
        let revision = stream.near_drawn_columns().0;
        stream.set_full_detail_radius(radius);
        stream.reevaluate_full_detail();
        assert!(stream.has_pending_full_detail());
        let ready: Vec<_> = stream.far_handoffs().collect();
        for column in &ready {
            stream.finish_far_handoff(*column);
        }
        assert_near_coverage(&stream, revision);
        assert!(ready.is_empty(), "the superseded demotion is not offered");
    }
}

/// A consumer's earlier ready list cannot bypass a target change before acknowledgement.
#[test]
fn deferred_detail_target_rejects_stale_handoff_acknowledgements() {
    for radius in [Some(6), Some(3), None] {
        let mut stream = delivered_handoff();
        let ready: Vec<_> = stream.far_handoffs().collect();
        assert_eq!(ready, [ChunkKey::new(0, 5, 0)]);
        let revision = stream.near_drawn_columns().0;
        stream.set_full_detail_radius(radius);
        for column in ready {
            stream.finish_far_handoff(column);
        }
        assert_near_coverage(&stream, revision);
    }
}

/// Eligibility follows the latest local centre even before its maintenance scan starts.
#[test]
fn moving_detail_target_preserves_reentered_handoff_coverage() {
    for x in [2, 4, 5] {
        let mut stream = delivered_handoff();
        let column = ChunkKey::new(0, 5, 0);
        let revision = stream.near_drawn_columns().0;
        stream.local_player_chunk = Some(ChunkKey::new(0, x, 0));
        stream.finish_far_handoff(column);
        assert_near_coverage(&stream, revision);
        assert!(!stream.far_handoffs().any(|ready| ready == column));

        stream.local_player_chunk = Some(ChunkKey::new(0, 0, 0));
        assert!(stream.far_handoffs().any(|ready| ready == column));
        stream.finish_far_handoff(column);
        assert!(!drawn(&stream, 5));
        assert!(!stream.wants_mesh(column));
        assert!(meshing(&stream, 5), "a truly outgoing mesh is removed");
    }
}

/// A target change leaves genuinely outgoing handoffs able to finish while work is paused.
#[test]
fn deferred_detail_target_still_completes_outgoing_handoffs() {
    let mut stream = delivered_handoff();
    let column = ChunkKey::new(0, 5, 0);
    stream.set_full_detail_radius(Some(2));
    assert!(stream.has_pending_full_detail());
    assert!(stream.far_handoffs().any(|ready| ready == column));
    stream.finish_far_handoff(column);
    assert!(!drawn(&stream, 5));
    assert!(!stream.wants_mesh(column));
    assert!(meshing(&stream, 5));
    assert!(stream.resident.column(column).next().is_some());
}
