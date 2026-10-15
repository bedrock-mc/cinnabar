//! Resumable detail changes keep publication and lighting consistent with residency.

use super::super::light_scheduler::{install_current_light, lit_stream};
use super::*;

/// Captures an actual worker result before changing the detail target.
fn pending_outer_mesh(published: bool) -> (WorldStream, MeshCompletion) {
    let mut stream = lit_stream(1);
    stream.set_far_terrain_enabled(true);
    stream.set_full_detail_radius(Some(1));
    let key = SubChunkKey::new(1, 5, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(2))
        .unwrap();
    install_current_light(&mut stream, key, 0, 0, false);
    stream.between_frames_service = true;
    stream.set_full_detail_radius(None);
    let revision = stream.mark_dirty_exact(key, Instant::now());
    if published {
        let now = Instant::now();
        let since = stream.revisions.dirty(key).unwrap().since;
        stream.acknowledge_mesh_upload(key, revision, since, now);
        assert!(stream.applied_mesh_generations.contains_key(&key));
        stream.mark_dirty_exact(key, now);
    }
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 1);
    let mut result = None;
    test_time::eventually("the captured outer mesh", || {
        result = stream.mesh_rx.try_recv().ok();
        result.is_some()
    });
    (stream, result.unwrap())
}

/// An unpublished outer worker result cannot outlive an interrupted restoration.
#[test]
fn transition_rejects_unpublished_outer_completion() {
    let (mut stream, completion) = pending_outer_mesh(false);
    stream.set_full_detail_radius(Some(1));
    stream.between_frames_service = false;
    stream.reevaluate_full_detail();
    assert!(!stream.wants_mesh(completion.key.chunk()));
    stream.accept_mesh_completion(completion);
    assert!(
        !stream
            .mesh_changes
            .iter()
            .any(|change| matches!(change, WorldMeshChange::Upsert { .. })),
        "an obsolete near mesh must not overlap far terrain"
    );
}

/// A valid replacement remains publishable while its drawn column waits for far coverage.
#[test]
fn transition_keeps_published_outer_handoff_completion() {
    let (mut stream, completion) = pending_outer_mesh(true);
    stream.set_full_detail_radius(Some(1));
    stream.between_frames_service = false;
    stream.reevaluate_full_detail();
    assert!(stream.wants_mesh(completion.key.chunk()));
    stream.accept_mesh_completion(completion);
    assert!(
        stream
            .mesh_changes
            .iter()
            .any(|change| matches!(change, WorldMeshChange::Upsert { .. }))
    );
}

/// A protected handoff regains ordinary lighting, including a resident neighbour halo.
#[test]
fn transition_relights_hysteresis_handoff_and_halo() {
    let mut stream = super::handoffs::delivered_handoff();
    let column = ChunkKey::new(0, 5, 0);
    let key = *stream.resident.column(column).next().unwrap();
    let halo = SubChunkKey::new(0, 6, key.y, 0);
    install_current_light(&mut stream, halo, 0, 15, false);
    stream.set_full_detail_radius(Some(3));
    stream
        .service_yield
        .as_ref()
        .unwrap()
        .store(false, Ordering::Release);
    stream.reevaluate_full_detail();
    assert!(!stream.has_pending_full_detail());
    assert!(drawn(&stream, 5), "maintenance must keep existing coverage");
    stream.lighting.jobs.pending.clear();
    stream.mark_changed_sources([key, halo], Instant::now());
    assert!(
        stream.lighting.jobs.pending.contains_key(&key),
        "a later update must relight the retained mesh"
    );
    assert!(
        stream.lighting.jobs.pending.contains_key(&halo),
        "the retained mesh's halo must also relight"
    );
}

/// A recreated column behind the checkpoint refreshes after its old removal publishes.
#[test]
fn transition_refreshes_recreated_column_behind_cursor() {
    let mut stream = stream();
    for (sequence, x) in [(1, -2), (2, 0), (3, 1)] {
        stream
            .submit(sequence, inline_block_entity_event(x, 7, Vec::new()))
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    stream.set_full_detail_radius(Some(2));
    assert!(!upload(&mut stream, -2).is_empty());
    stream.between_frames_service = true;
    stream.set_full_detail_radius(None);
    stream.set_full_detail_radius(Some(2));
    stream.service_yield = Some(Arc::new(AtomicBool::new(false)));
    stream.poll_deadline = Some(Instant::now());
    stream.maintenance_guarantee = true;
    let items = stream.maintenance_items;
    stream.reevaluate_full_detail();
    let column = ChunkKey::new(0, -2, 0);
    assert_eq!(stream.maintenance_items - items, 1);
    assert!(stream.has_pending_full_detail());
    stream.mesh_jobs.pending.clear();
    stream.evict_column(column);
    stream.poll_deadline = None;
    stream.dispatch_mesh_jobs([0.0; 3], usize::MAX);
    let mut removed = 0;
    while let Some(change) = stream.pop_mesh_change() {
        if let WorldMeshChange::Remove {
            key,
            generation,
            dirty_since,
            ..
        } = change
            && key.chunk() == column
        {
            removed += 1;
            stream.acknowledge_mesh_upload(key, generation, dirty_since, Instant::now());
        }
    }
    assert!(removed > 0, "the old geometry was actually withdrawn");
    stream
        .submit(4, inline_block_entity_event(-2, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(!drawn(&stream, -2));
    assert!(!meshing(&stream, -2), "arrival was gated by the checkpoint");
    stream.reevaluate_full_detail();
    assert!(!stream.has_pending_full_detail());
    assert!(stream.wants_mesh(column));
    assert!(
        meshing(&stream, -2),
        "replacement terrain must be refreshed after the checkpoint"
    );
}

/// Eligibility changes before the resumable scan reaches an obsolete worker result.
#[test]
fn transition_rejects_completion_before_detail_scan() {
    let (mut stream, completion) = pending_outer_mesh(false);
    stream.set_full_detail_radius(Some(1));
    assert!(stream.has_pending_full_detail());
    assert!(!stream.wants_mesh(completion.key.chunk()));
    stream.accept_mesh_completion(completion);
    assert!(
        !stream
            .mesh_changes
            .iter()
            .any(|change| matches!(change, WorldMeshChange::Upsert { .. }))
    );
}

/// Discarded detail also retires geometry queued before the target changed.
#[test]
fn transition_withdraws_unacknowledged_outer_publication() {
    let (mut stream, completion) = pending_outer_mesh(false);
    let key = completion.key;
    stream.accept_mesh_completion(completion);
    assert!(
        stream
            .mesh_changes
            .iter()
            .any(|change| matches!(change, WorldMeshChange::Upsert { .. }))
    );
    stream.set_full_detail_radius(Some(1));
    stream.between_frames_service = false;
    stream.reevaluate_full_detail();
    stream.dispatch_mesh_jobs([0.0; 3], usize::MAX);
    let change = stream
        .pop_mesh_change()
        .expect("obsolete publication is withdrawn");
    assert!(matches!(change, WorldMeshChange::Remove { key: removed, .. } if removed == key));
}
