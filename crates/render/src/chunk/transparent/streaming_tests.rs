//! Streaming water in and out patches the committed sort instead of re-uploading it.
use super::transparent_strafe::{Fixture, SURFACE_SUBCHUNK_Y, fixture_with};
use super::*;

const CAMERA: Vec3 = Vec3::new(8.5, 64.6, 8.5);

/// Reads the fixture sort counters after streaming updates.
fn metrics(fixture: &Fixture) -> TransparentSortMetricsSnapshot {
    fixture
        .app
        .world()
        .resource::<TransparentSortMetrics>()
        .snapshot()
}

/// Returns the active snapshot slot so the fixture can detect unnecessary replacement.
fn committed_slot(fixture: &Fixture) -> u8 {
    fixture
        .app
        .world()
        .resource::<TransparentSortRuntime>()
        .state
        .committed()
        .unwrap()
        .buffer_slot()
}

/// Advances enough fixture frames to finish the initial bounded sort upload.
fn settle(fixture: &mut Fixture) {
    for _ in 0..8 {
        fixture.frame_looking(CAMERA, Vec3::Z);
    }
}

/// A new shore costs one sort job and its own refs: it is sorted in the frame after it
/// arrives, and nothing else of the committed slot is uploaded again.
#[test]
fn a_new_shore_uploads_only_its_own_refs() {
    let mut fixture = fixture_with(5);
    settle(&mut fixture);
    let (before, slot, sorted) = (
        metrics(&fixture),
        committed_slot(&fixture),
        fixture.sorted_refs,
    );
    let shore = SubChunkKey::new(0, 0, SURFACE_SUBCHUNK_Y + 1, 10);
    assert_eq!(fixture.stream(&[], &[(shore, true)]), 1);
    // The arrival frame sorts it on a worker and draws it unsorted meanwhile.
    fixture.frame_looking(CAMERA, Vec3::Z);
    fixture.frame_looking(CAMERA, Vec3::Z);
    let after = metrics(&fixture);
    let refs = 272;
    println!(
        "new shore: sorted_refs={} upload_bytes={} committed_refs={}",
        fixture.sorted_refs - sorted,
        after.upload_bytes - before.upload_bytes,
        after.ref_count
    );
    assert_eq!(fixture.sorted_refs - sorted, refs);
    assert_eq!(
        after.upload_bytes - before.upload_bytes,
        (refs * size_of::<PackedTransparentDrawRef>()) as u64
    );
    assert_eq!(
        committed_slot(&fixture),
        slot,
        "the committed slot was patched"
    );
    assert_eq!(after.ref_count, before.ref_count + refs);
}

/// Streaming rows of shores in ahead and out behind never blocks an upload: each removed
/// sub-chunk's allocation is released as soon as the patched sort stops reading it.
#[test]
fn streaming_an_ocean_through_never_blocks_uploads() {
    const ROWS: i32 = 24;
    let mut fixture = fixture_with(1);
    settle(&mut fixture);
    {
        let world = fixture.app.world_mut();
        // Bounded uploads make any whole-slot restage span many frames.
        world
            .resource_mut::<TransparentSortRuntime>()
            .state
            .upload_cap = 8_192;
        world.resource_mut::<ChunkGpuArena>().retirement_budget =
            TransparentRetirementBudget::with_limits(128, u64::MAX);
    }
    let (mut blocked, mut most_retired) = (0, 0);
    let before = metrics(&fixture);
    for row in 0..ROWS {
        let out = (-8..=8)
            .map(|x| SubChunkKey::new(0, x, SURFACE_SUBCHUNK_Y, row - 8))
            .collect::<Vec<_>>();
        let into = (-8..=8)
            .map(|x| (SubChunkKey::new(0, x, SURFACE_SUBCHUNK_Y, row + 9), true))
            .collect::<Vec<_>>();
        blocked += into.len() - fixture.stream(&out, &into);
        fixture.frame_looking(CAMERA, Vec3::Z);
        fixture.complete_gpu_frame();
        most_retired = most_retired.max(
            fixture
                .app
                .world()
                .resource::<ChunkGpuArena>()
                .retired_allocations
                .len(),
        );
    }
    let after = metrics(&fixture);
    println!(
        "streamed {ROWS} rows: blocked_uploads={blocked} most_retired={most_retired} \
         upload_bytes={} committed_refs={}",
        after.upload_bytes - before.upload_bytes,
        after.ref_count
    );
    assert_eq!(blocked, 0, "retirement pressure blocked fresh uploads");
    assert!(
        most_retired <= 3 * 17,
        "{most_retired} retirements were held"
    );
    for _ in 0..4 {
        fixture.stream(&[], &[]);
        fixture.frame_looking(CAMERA, Vec3::Z);
        fixture.complete_gpu_frame();
    }
    fixture.stream(&[], &[]);
    let arena = fixture.app.world().resource::<ChunkGpuArena>();
    assert!(arena.retired_allocations.is_empty());
}

/// A later commit keeps lagging ranges of an earlier one until they are written.
#[test]
fn lagging_ranges_survive_the_next_in_place_commit() {
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 1, 0..16, 16..24, 0);
    let key = |x: f32| {
        ViewSortKey::try_new(
            [x, 0.0, 0.0],
            vec![identity.clone()],
            ChunkTextureAssetIdentity::new(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap()
    };
    let refs = |order: [u32; 4]| order.map(|record| PackedTransparentDrawRef::new(record, 0));
    let mut state = TransparentSortState::with_upload_cap(64);
    let first = key(0.0);
    let generation = state.request(&first);
    state
        .complete(
            TransparentSortResult::new(generation, first, refs([0, 1, 2, 3]).to_vec()).unwrap(),
        )
        .unwrap();
    assert!(state.acknowledge_upload());

    let second = key(20.0);
    let generation = state.request(&second);
    assert_eq!(
        state.complete(
            TransparentSortResult::new(generation, second, refs([3, 2, 1, 0]).to_vec()).unwrap()
        ),
        Ok(true)
    );
    assert!(state.take_urgent_patch().is_empty());
    assert_eq!(state.take_patch_within(1), [0..1]);

    let third = key(40.0);
    let generation = state.request(&third);
    assert_eq!(
        state.complete(
            TransparentSortResult::new(generation, third, refs([3, 2, 1, 0]).to_vec()).unwrap()
        ),
        Ok(true)
    );
    // Nothing changed in the third order, but the second's unwritten refs still lag.
    assert_eq!(state.take_patch_within(usize::MAX), [1..4]);
    assert_eq!(state.committed().unwrap().refs(), refs([3, 2, 1, 0]));
}

/// Removing a slot's tail preserves valid deferred writes and drops obsolete ones.
#[test]
fn deferred_uploads_stay_within_a_shrinking_committed_slot() {
    let identities = [
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 1, 0..16, 0..8, 0),
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 1, 0, 0), 1, 16..32, 8..16, 1),
    ];
    let key = |x, allocations| {
        ViewSortKey::try_new(
            [x, 0.0, 0.0],
            allocations,
            ChunkTextureAssetIdentity::new(1, 1),
            ChunkBiomeTintIdentity::new(1, 1),
        )
        .unwrap()
    };
    let first = (0..8)
        .map(|record| PackedTransparentDrawRef::new(record, record / 4))
        .collect::<Vec<_>>();
    let reordered =
        [3, 2, 1, 0, 7, 6, 5, 4].map(|record| PackedTransparentDrawRef::new(record, record / 4));
    for remaining in [4, 0] {
        for uploaded in [1, 5] {
            let mut state = TransparentSortState::with_upload_cap(64);
            let initial = key(0.0, identities.to_vec());
            let generation = state.request(&initial);
            state
                .complete(TransparentSortResult::new(generation, initial, first.clone()).unwrap())
                .unwrap();
            assert!(state.acknowledge_upload());

            let moved = key(20.0, identities.to_vec());
            let generation = state.request(&moved);
            assert_eq!(
                state.complete(
                    TransparentSortResult::new(generation, moved, reordered.to_vec()).unwrap()
                ),
                Ok(true)
            );
            assert_eq!(state.take_patch_within(uploaded), [0..uploaded]);
            let before = state.committed().unwrap().clone();
            let next = key(20.0, identities[..remaining / 4].to_vec());
            let generation = state.request_retaining_resident_snapshot(&next, true, false);
            let patch = TransparentRefPatch {
                base: Arc::clone(&before.refs),
                urgent: Vec::new(),
                deferred: Vec::new(),
            };
            assert_eq!(
                state.complete(
                    TransparentSortResult::with_patch(
                        generation,
                        next,
                        reordered[..remaining].to_vec().into(),
                        Some(patch),
                    )
                    .unwrap()
                ),
                Ok(true)
            );
            assert!(state.take_urgent_patch().is_empty());
            let snapshot = state.committed().unwrap().clone();
            assert_eq!(snapshot.buffer_slot(), before.buffer_slot());
            let mut written = Vec::new();
            for span in state.take_patch_within(usize::MAX) {
                written.extend_from_slice(&snapshot.refs()[span]);
            }
            let expected = &reordered[uploaded.min(remaining)..remaining];
            assert_eq!(
                written, expected,
                "uploaded={uploaded}, remaining={remaining}"
            );
        }
    }
}
