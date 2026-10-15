//! Cancelled restorations must not grant unscheduled arrivals an older membership.

use super::*;

/// Completes a limited disk before servicing later changes between frames.
fn limited_service() -> WorldStream {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(3));
    stream
        .submit(1, inline_block_entity_event(0, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(!stream.has_pending_full_detail());
    assert!(!upload(&mut stream, 0).is_empty());
    stream.mesh_jobs.pending.clear();
    stream.lighting.jobs.pending.clear();
    stream.between_frames_service = true;
    stream
}

/// Finishes detail maintenance while granting at most one column of work per slice.
fn finish_detail(stream: &mut WorldStream) {
    stream.service_yield = Some(Arc::new(AtomicBool::new(false)));
    test_time::eventually("the latest full-detail target", || {
        stream.poll_deadline = Some(Instant::now());
        stream.maintenance_guarantee = true;
        let before = stream.maintenance_items;
        stream.reevaluate_full_detail();
        assert!(
            stream.maintenance_items - before <= 1,
            "maintenance must remain resumable"
        );
        !stream.has_pending_full_detail()
    });
    stream.poll_deadline = None;
    stream.service_yield = None;
}

/// Admits a previously empty disk column after the first restoration was cancelled.
fn cancelled_arrival(repetitions: usize) -> (WorldStream, SubChunkKey) {
    let mut stream = limited_service();
    stream.set_full_detail_radius(None);
    stream.set_full_detail_radius(Some(3));
    stream
        .submit(2, inline_block_entity_event(3, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let column = ChunkKey::new(0, 3, 0);
    let key = stream
        .resident
        .column(column)
        .copied()
        .find(|key| stream.authority.terrain().sub_chunk(*key).is_some())
        .expect("the newly resident block section");
    assert!(!meshing(&stream, 3), "arrival waits for promotion");
    assert!(
        !stream.lighting.jobs.pending.contains_key(&key),
        "arrival lighting waits for promotion"
    );
    for _ in 0..repetitions {
        stream.set_full_detail_radius(None);
        stream.set_full_detail_radius(Some(3));
    }
    assert!(stream.has_pending_full_detail());
    (stream, key)
}

/// Every repeated cancellation still refreshes the arrival's actual mesh work.
#[test]
fn repeated_cancellation_schedules_new_resident_meshes() {
    let results: Vec<_> = [1, 2, 4]
        .into_iter()
        .map(|repetitions| {
            let (mut stream, key) = cancelled_arrival(repetitions);
            finish_detail(&mut stream);
            assert!(stream.resident.contains(&key));
            assert!(stream.wants_mesh(key.chunk()));
            (repetitions, meshing(&stream, 3))
        })
        .collect();
    assert!(
        results.iter().all(|(_, queued)| *queued),
        "new resident mesh work: {results:?}"
    );
}

/// Every repeated cancellation still queues a light solve for the new block section.
#[test]
fn repeated_cancellation_schedules_new_resident_lighting() {
    let results: Vec<_> = [1, 2, 4]
        .into_iter()
        .map(|repetitions| {
            let (mut stream, key) = cancelled_arrival(repetitions);
            finish_detail(&mut stream);
            (repetitions, stream.lighting.jobs.pending.contains_key(&key))
        })
        .collect();
    assert!(
        results.iter().all(|(_, queued)| *queued),
        "new resident lighting work: {results:?}"
    );
}

/// Columns that were already published stay live across repeated cancellation.
#[test]
fn repeated_cancellation_preserves_existing_members() {
    let mut stream = limited_service();
    let key = *stream
        .resident
        .column(ChunkKey::new(0, 0, 0))
        .next()
        .unwrap();
    let applied = stream.applied_mesh_generations[&key];
    for _ in 0..4 {
        stream.set_full_detail_radius(None);
        stream.set_full_detail_radius(Some(3));
    }
    finish_detail(&mut stream);
    assert!(drawn(&stream, 0));
    assert_eq!(stream.applied_mesh_generations[&key], applied);
    assert!(!stream.withdrawn_meshes.contains(&key));
    assert!(
        !meshing(&stream, 0),
        "unchanged membership needs no refresh"
    );
    stream.mark_changed_sources([key], Instant::now());
    assert!(meshing(&stream, 0));
    assert!(stream.lighting.jobs.pending.contains_key(&key));
}

/// Completing unlimited detail retires the old checkpoint before the next limited target.
#[test]
fn repeated_cancellation_allows_a_completed_restoration() {
    let mut stream = limited_service();
    stream.set_full_detail_radius(None);
    stream.set_full_detail_radius(Some(3));
    stream.set_full_detail_radius(None);
    finish_detail(&mut stream);
    assert_eq!(stream.full_detail_radius(), None);
    stream
        .submit(2, inline_block_entity_event(3, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(
        meshing(&stream, 3),
        "unlimited arrivals are scheduled immediately"
    );
    assert!(!upload(&mut stream, 3).is_empty());
    assert!(drawn(&stream, 3));
    stream.mesh_jobs.pending.clear();
    stream.set_full_detail_radius(Some(1));
    finish_detail(&mut stream);
    assert!(
        stream.wants_mesh(ChunkKey::new(0, 3, 0)),
        "a genuinely restored column stays live in the band"
    );
    assert!(drawn(&stream, 3));
    assert!(
        !meshing(&stream, 3),
        "a completed restoration does not inherit the cancelled checkpoint"
    );
}

/// A new column published during a later restoration still receives intervening edits.
#[test]
fn repeated_cancellation_preserves_a_newly_published_member() {
    let (mut stream, key) = cancelled_arrival(1);
    stream.set_full_detail_radius(None);
    let keys: Vec<_> = stream.resident.column(key.chunk()).copied().collect();
    stream.mark_changed_sources(keys, Instant::now());
    assert!(!upload(&mut stream, 3).is_empty());
    assert!(drawn(&stream, 3));
    stream.mesh_jobs.pending.clear();
    stream.lighting.jobs.pending.clear();
    stream.set_full_detail_radius(Some(3));
    stream.mark_changed_sources([key], Instant::now());
    finish_detail(&mut stream);
    assert!(drawn(&stream, 3), "the published mesh remains covered");
    assert!(
        stream.mesh_jobs.pending.contains_key(&key),
        "the intervening edit must reach the mesh"
    );
    assert!(
        stream.lighting.jobs.pending.contains_key(&key),
        "the intervening edit must reach lighting"
    );
}

/// Finishing unlimited detail also schedules work deferred by earlier cancellations.
#[test]
fn repeated_cancellation_finishes_pending_restoration_work() {
    let (mut stream, key) = cancelled_arrival(2);
    stream.set_full_detail_radius(None);
    finish_detail(&mut stream);
    assert_eq!(stream.full_detail_radius(), None);
    assert!(
        stream.mesh_jobs.pending.contains_key(&key),
        "completed restoration must schedule the deferred mesh"
    );
    assert!(
        stream.lighting.jobs.pending.contains_key(&key),
        "completed restoration must schedule deferred lighting"
    );
}
