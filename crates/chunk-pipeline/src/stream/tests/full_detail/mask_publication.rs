//! Outgoing masks outlive scheduling membership until current removals publish.

use super::*;

/// Observes the mask membership consumed by the far-terrain scene.
fn masked(stream: &WorldStream, column: ChunkKey) -> bool {
    stream.near_mask_columns().1.any(|masked| masked == column)
}

/// Starts a real delivered handoff and captures its exact pending removal identities.
fn withdrawing() -> (WorldStream, Vec<(SubChunkKey, u64, Instant)>) {
    let mut stream = super::handoffs::delivered_handoff();
    let column = ChunkKey::new(0, 5, 0);
    stream.finish_far_handoff(column);
    assert!(!drawn(&stream, 5), "scheduling membership has ended");
    assert!(
        masked(&stream, column),
        "the mesh withdrawal has not published"
    );
    let removals = stream
        .resident
        .column(column)
        .map(|&key| {
            let dirty = stream.revisions.dirty(key).unwrap();
            (key, dirty.revision, dirty.since)
        })
        .collect::<Vec<_>>();
    assert!(!removals.is_empty());
    (stream, removals)
}

#[test]
fn a_reentered_column_keeps_its_mask_across_stale_removal_acknowledgements() {
    for move_centre in [false, true] {
        let (mut stream, removals) = withdrawing();
        let column = ChunkKey::new(0, 5, 0);
        stream.between_frames_service = false;
        stream.service_yield = None;
        if move_centre {
            stream.local_player_chunk = Some(column);
            stream.reevaluate_full_detail();
        } else {
            stream.set_full_detail_radius(Some(6));
        }
        assert!(stream.wants_mesh(column));
        let revision = stream.near_mask_columns().0;
        for (key, generation, since) in removals {
            stream.acknowledge_mesh_upload(key, generation, since, Instant::now());
        }
        assert!(masked(&stream, column));
        assert_eq!(stream.near_mask_columns().0, revision);
        assert!(
            !drawn(&stream, 5),
            "stale receipts cannot complete restoration"
        );
        assert!(!upload(&mut stream, 5).is_empty());
        assert!(drawn(&stream, 5));
        assert_eq!(
            stream
                .near_mask_columns()
                .1
                .filter(|key| *key == column)
                .count(),
            1
        );
    }
}

#[test]
fn newer_dirty_removals_keep_the_mask_until_every_current_section_acknowledges() {
    let (mut stream, removals) = withdrawing();
    let column = ChunkKey::new(0, 5, 0);
    for &(key, _, _) in &removals {
        stream.mark_dirty_exact_ungated(key, Instant::now(), false);
    }
    let revision = stream.near_mask_columns().0;
    for (key, generation, since) in removals {
        stream.acknowledge_mesh_upload(key, generation, since, Instant::now());
    }
    assert!(masked(&stream, column));
    assert_eq!(stream.near_mask_columns().0, revision);
    let keys: Vec<_> = stream.resident.column(column).copied().collect();
    for (index, key) in keys.iter().copied().enumerate() {
        let dirty = stream.revisions.dirty(key).unwrap();
        let (generation, since) = (dirty.revision, dirty.since);
        stream.acknowledge_mesh_upload(key, generation, since, Instant::now());
        assert_eq!(masked(&stream, column), index + 1 != keys.len());
    }
    assert!(!masked(&stream, column));
}

#[test]
fn clearing_or_replacing_a_stream_does_not_keep_old_withdrawal_masks() {
    let (mut outgoing, removals) = withdrawing();
    let column = ChunkKey::new(0, 5, 0);
    outgoing.set_far_terrain_enabled(false);
    assert!(!masked(&outgoing, column));
    assert_eq!(outgoing.near_mask_columns().1.count(), 0);
    let mut replacement = stream();
    for (key, generation, since) in removals {
        replacement.acknowledge_mesh_upload(key, generation, since, Instant::now());
    }
    assert!(!masked(&replacement, column));
    assert_eq!(replacement.near_mask_columns().1.count(), 0);
}
