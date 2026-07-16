use super::*;
use crate::chunk::{
    gpu::upload::{
        PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR, PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR,
        packed_light_factor,
    },
    transparent::retirement::transparent_view_key_satisfies_witness,
};

fn sort_candidate(
    key: SubChunkKey,
    local_quad_index: u32,
    record: u32,
    subchunk_center: [f32; 3],
    quad_centroid: [f32; 3],
) -> TransparentSortCandidate {
    TransparentSortCandidate::new(
        key,
        local_quad_index,
        record,
        record + 100,
        subchunk_center,
        quad_centroid,
    )
}

#[test]
fn transparent_sort_is_grouped_back_to_front_stable_and_rotation_sensitive() {
    let near_key = SubChunkKey::new(0, 0, 0, -1);
    let far_key = SubChunkKey::new(0, 0, 0, -2);
    let candidates = Arc::from(vec![
        sort_candidate(near_key, 1, 11, [0.0, 0.0, -2.0], [0.0, 0.0, -2.5]),
        sort_candidate(far_key, 1, 21, [0.0, 0.0, -10.0], [0.0, 0.0, -10.0]),
        sort_candidate(far_key, 0, 20, [0.0, 0.0, -10.0], [0.0, 0.0, -12.0]),
        sort_candidate(far_key, 2, 22, [0.0, 0.0, -10.0], [0.0, 0.0, -10.0]),
    ]);
    let identity = sort_transparent_candidates(Mat4::IDENTITY, Arc::clone(&candidates));
    assert_eq!(
        identity
            .iter()
            .map(|draw_ref| draw_ref.liquid_record_index())
            .collect::<Vec<_>>(),
        vec![20, 21, 22, 11],
        "subchunks and their internal faces are back-to-front; ties use local index"
    );

    let rotated = sort_transparent_candidates(
        Mat4::from_quat(Quat::from_rotation_y(std::f32::consts::PI)),
        candidates,
    );
    assert_eq!(rotated[0].liquid_record_index(), 11);
}

fn resident_transparent_allocation(
    identity: &TransparentAllocationIdentity,
    tint_identity: ChunkBiomeTintIdentity,
) -> GpuChunkAllocation {
    GpuChunkAllocation {
        key: identity.key,
        generation: identity.mesh_generation,
        tint_identity,
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: None,
        model_lighting_range: None,
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: Some(identity.liquid_range.clone()),
        liquid_lighting_range: Some(identity.lighting_range.clone()),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_range: None,
        metadata_index: identity.metadata_index,
    }
}

#[test]
fn visibility_membership_churn_retains_resident_snapshot_until_ordered_swap() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let a = TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let b = TransparentAllocationIdentity::new(SubChunkKey::new(0, 1, 0, 0), 4, 16..24, 36..40, 2);
    let c = TransparentAllocationIdentity::new(SubChunkKey::new(0, 2, 0, 0), 5, 24..32, 40..44, 3);
    let old_key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![a.clone(), b.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let old_refs = vec![
        PackedTransparentDrawRef::new(2, 2),
        PackedTransparentDrawRef::new(1, 1),
    ];
    let mut state = committed_transparent_state(&old_key, old_refs.clone());
    let old_snapshot = state.committed().unwrap().clone();
    let resident = [
        resident_transparent_allocation(&a, tint_identity),
        resident_transparent_allocation(&b, tint_identity),
        resident_transparent_allocation(&c, tint_identity),
    ];
    assert!(transparent_snapshot_addresses_are_resident(
        &old_snapshot,
        resident.iter(),
        std::iter::empty(),
        texture_identity,
        tint_identity,
    ));

    // B leaves the frustum while C enters it. B's arena allocation remains
    // resident, so every absolute reference in the old ordered snapshot is
    // still safe to draw while the replacement sort runs.
    let next_key = ViewSortKey::try_new(
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        vec![a, c],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let next_generation = state.request_retaining_resident_snapshot(&next_key, true);
    assert_eq!(state.committed(), Some(&old_snapshot));
    let retained_draw = transparent_draw_args(
        state.committed().unwrap().buffer_slot(),
        state.committed().unwrap().refs().len(),
    )
    .unwrap();
    assert_eq!(retained_draw.instance_count, old_refs.len() as u32);
    let new_refs = vec![
        PackedTransparentDrawRef::new(3, 3),
        PackedTransparentDrawRef::new(1, 1),
    ];
    assert_eq!(
        state.complete(
            TransparentSortResult::new(next_generation, next_key, new_refs.clone()).unwrap()
        ),
        Ok(false)
    );
    assert_eq!(state.committed(), Some(&old_snapshot));
    assert_eq!(state.next_upload_batch().unwrap().refs(), new_refs);
    assert!(state.acknowledge_upload());
    let replacement = state.committed().unwrap();
    assert_eq!(replacement.generation(), next_generation);
    assert_eq!(replacement.refs(), new_refs);
    assert_ne!(replacement.buffer_slot(), old_snapshot.buffer_slot());
}

#[test]
fn missing_or_reallocated_snapshot_identity_clears_absolute_refs_immediately() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let changed_key = ViewSortKey::try_new(
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        texture_identity,
        tint_identity,
    )
    .unwrap();

    let exact = resident_transparent_allocation(&identity, tint_identity);
    let mut changed_liquid_range = exact.clone();
    changed_liquid_range.liquid_range = Some(12..20);
    let mut changed_metadata = exact;
    changed_metadata.metadata_index += 1;
    for resident in [
        Vec::new(),
        vec![changed_liquid_range],
        vec![changed_metadata],
    ] {
        let mut state =
            committed_transparent_state(&key, vec![PackedTransparentDrawRef::new(2, 1)]);
        assert!(!transparent_snapshot_addresses_are_resident(
            state.committed().unwrap(),
            resident.iter(),
            std::iter::empty(),
            texture_identity,
            tint_identity,
        ));
        state.request_retaining_resident_snapshot(&changed_key, false);
        assert!(state.committed().is_none());
    }
}

#[test]
fn generation_only_update_retains_physically_resident_snapshot_and_draw_args() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let old_identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let old_key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![old_identity.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let refs = vec![
        PackedTransparentDrawRef::new(2, 1),
        PackedTransparentDrawRef::new(3, 1),
    ];
    let mut state = committed_transparent_state(&old_key, refs.clone());
    let old_snapshot = state.committed().unwrap().clone();

    let mut resident = resident_transparent_allocation(&old_identity, tint_identity);
    resident.generation += 1;
    assert!(transparent_snapshot_addresses_are_resident(
        &old_snapshot,
        [&resident],
        std::iter::empty(),
        texture_identity,
        tint_identity,
    ));

    let next_identity = TransparentAllocationIdentity::new(
        old_identity.key,
        resident.generation,
        old_identity.liquid_range.clone(),
        old_identity.lighting_range.clone(),
        old_identity.metadata_index,
    );
    let next_key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![next_identity],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let generation = state.request_retaining_resident_snapshot(&next_key, true);
    assert_eq!(state.committed(), Some(&old_snapshot));
    let retained_args = transparent_draw_args(
        state.committed().unwrap().buffer_slot(),
        state.committed().unwrap().refs().len(),
    )
    .unwrap();
    assert_eq!(retained_args.instance_count, refs.len() as u32);

    let replacement_refs = refs.into_iter().rev().collect::<Vec<_>>();
    assert_eq!(
        state.complete(
            TransparentSortResult::new(generation, next_key, replacement_refs.clone()).unwrap()
        ),
        Ok(false)
    );
    assert_eq!(state.committed(), Some(&old_snapshot));
    assert!(state.acknowledge_upload());
    assert_eq!(state.committed().unwrap().refs(), replacement_refs);
    assert_ne!(
        state.committed().unwrap().buffer_slot(),
        old_snapshot.buffer_slot()
    );
}

#[test]
fn grown_same_start_liquid_range_keeps_old_refs_physically_resident() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let snapshot = committed_transparent_state(
        &key,
        vec![PackedTransparentDrawRef::new(2, identity.metadata_index)],
    )
    .committed()
    .unwrap()
    .clone();
    let mut resident = resident_transparent_allocation(&identity, tint_identity);
    resident.generation += 1;
    resident.liquid_range = Some(8..24);
    resident.liquid_lighting_range = Some(40..48);
    assert!(transparent_snapshot_addresses_are_resident(
        &snapshot,
        [&resident],
        std::iter::empty(),
        texture_identity,
        tint_identity,
    ));
}

#[test]
fn physical_residency_rejects_moved_shrunk_or_structurally_invalid_streams() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let snapshot = committed_transparent_state(
        &key,
        vec![PackedTransparentDrawRef::new(2, identity.metadata_index)],
    )
    .committed()
    .unwrap()
    .clone();
    let exact = resident_transparent_allocation(&identity, tint_identity);
    let mut moved = exact.clone();
    moved.liquid_range = Some(4..16);
    let mut shrunk = exact.clone();
    shrunk.liquid_range = Some(8..12);
    let mut missing_lighting = exact.clone();
    missing_lighting.liquid_lighting_range = None;
    let mut invalid_lighting_count = exact.clone();
    invalid_lighting_count.liquid_lighting_range = Some(32..34);
    let mut changed_tint = exact.clone();
    changed_tint.tint_identity = ChunkBiomeTintIdentity::new(9, 9);
    let mut changed_key = exact;
    changed_key.key = SubChunkKey::new(0, 1, 0, 0);

    for resident in [
        moved,
        shrunk,
        missing_lighting,
        invalid_lighting_count,
        changed_tint,
        changed_key,
    ] {
        assert!(!transparent_snapshot_addresses_are_resident(
            &snapshot,
            [&resident],
            std::iter::empty(),
            texture_identity,
            tint_identity,
        ));
    }
    assert!(!transparent_snapshot_addresses_are_resident(
        &snapshot,
        [&resident_transparent_allocation(&identity, tint_identity)],
        std::iter::empty(),
        ChunkTextureAssetIdentity::new(9, 9),
        tint_identity,
    ));
}

#[test]
fn retirement_fence_uses_monotonic_epochs_and_ignores_stale_callbacks() {
    let fence = TransparentRetirementFence::default();
    let first = fence.try_reserve().expect("reserve first retirement epoch");
    assert_eq!(first, 1);
    assert!(!fence.complete(first + 9));
    assert_eq!(fence.completed_epoch(), 0);
    assert!(fence.complete(first));
    assert_eq!(fence.completed_epoch(), first);
    let second = fence
        .try_reserve()
        .expect("reserve second retirement epoch");
    assert!(second > first);
    assert!(!fence.complete(first));
    assert_eq!(fence.completed_epoch(), first);
    assert!(fence.complete(second));
    assert_eq!(fence.completed_epoch(), second);
}

#[test]
fn retirement_budget_backpressures_without_overcommit_and_recovers_after_release() {
    let mut budget = TransparentRetirementBudget::with_limits(2, 64);
    assert!(budget.try_reserve(1, 32));
    assert!(budget.try_reserve(1, 32));
    assert!(!budget.try_reserve(1, 1));
    assert!(!budget.try_reserve(0, 1));
    budget.release(1, 32);
    assert!(budget.try_reserve(1, 32));
    assert_eq!(budget.items(), 2);
    assert_eq!(budget.bytes(), 64);
}

#[test]
fn moved_or_shrunk_liquid_contract_requires_copy_on_write_but_containment_does_not() {
    let old = retirement_test_allocation();
    let same_start_growth = GeometryStreamCounts {
        model: 2,
        liquid: 3,
        liquid_lighting: 3,
        ..default()
    };
    assert!(!transparent_geometry_update_requires_cow(
        &old,
        same_start_growth
    ));
    let moved_by_model = GeometryStreamCounts {
        model: 2,
        model_lighting: 1,
        model_draw: 2,
        liquid: 2,
        liquid_lighting: 2,
        ..default()
    };
    assert!(transparent_geometry_update_requires_cow(
        &old,
        moved_by_model
    ));
    assert!(transparent_geometry_update_requires_cow(
        &old,
        GeometryStreamCounts::default()
    ));

    let mut lava_only = old;
    lava_only.gpu.has_transparent_liquid = false;
    assert!(!transparent_geometry_update_requires_cow(
        &lava_only,
        moved_by_model,
    ));
}

#[test]
fn copy_on_write_plan_keeps_old_extent_out_of_free_lists_and_growth_copy() {
    let old = retirement_test_allocation();
    let required = GeometryStreamCounts {
        model: 2,
        model_lighting: 1,
        liquid: 2,
        liquid_lighting: 2,
        ..default()
    };
    let plan = plan_chunk_range_update(
        2,
        &[],
        30,
        &[],
        6,
        &[],
        required,
        4,
        Some(&old),
        true,
        ArenaLimits {
            max_quad_items: 128,
            max_geometry_stream_words: 128,
            max_origin_items: 16,
            max_biome_words: 128,
        },
    )
    .unwrap();
    assert_eq!(plan.geometry_stream_start, 32);
    assert_eq!(plan.free_geometry_stream_words, vec![30..32]);
    assert!(plan.geometry_stream_len > old.geometry_stream_capacity as usize);
    assert_eq!(
        plan_arena_growth(1, plan.geometry_stream_len, 4, 128)
            .unwrap()
            .unwrap()
            .gpu_copy_bytes,
        4,
        "growth copies the prior high-water buffer, including retired bytes"
    );
}

#[test]
fn partial_and_full_retirement_transfer_only_the_owned_spans() {
    let entity = Entity::from_bits(1 << 32 | 11);
    let old = retirement_test_allocation();
    let partial = RetiredArenaAllocation::geometry_only(entity, &old).unwrap();
    assert!(partial.quad.is_none());
    assert!(partial.geometry.is_some());
    assert!(partial.biome.is_none());
    assert!(partial.origin.is_none());

    let full = RetiredArenaAllocation::full(entity, old);
    assert!(full.quad.is_some());
    assert!(full.geometry.is_some());
    assert!(full.biome.is_some());
    assert_eq!(full.origin, Some(3));
}

#[test]
fn retired_range_is_fence_delayed_then_coalesced_for_eventual_reuse() {
    let entity = Entity::from_bits(1 << 32 | 11);
    let old = retirement_test_allocation();
    let mut retirement = RetiredArenaAllocation::geometry_only(entity, &old).unwrap();
    retirement.release_epoch = Some(2);
    let mut len = 60;
    let mut free = Vec::new();
    assert!(!retirement.can_release(1));
    assert_eq!(allocate_quad_range(&mut len, &mut free, 30, 90), Some(60));
    release_quad_range(&mut len, &mut free, 60..90);

    assert!(retirement.can_release(2));
    let (range, capacity) = retirement.geometry.clone().unwrap();
    release_quad_range(&mut len, &mut free, range.start..range.start + capacity);
    assert_eq!(allocate_quad_range(&mut len, &mut free, 30, 90), Some(0));
}

#[test]
fn transparent_witness_requires_the_exact_bounded_key_set() {
    let a = SubChunkKey::new(0, 0, 4, 5);
    let b = SubChunkKey::new(0, 1, 4, 5);
    let request = TransparentWitnessRequest::try_new(7, vec![b, a]).unwrap();
    assert_eq!(request.revision(), 7);
    assert_eq!(request.keys(), &[a, b]);
    assert!(TransparentWitnessRequest::try_new(0, vec![a]).is_err());
    assert!(TransparentWitnessRequest::try_new(1, Vec::new()).is_err());
    assert!(TransparentWitnessRequest::try_new(1, vec![a, a]).is_err());
    assert!(
        TransparentWitnessRequest::try_new(
            1,
            (0..=MAX_TRANSPARENT_WITNESS_KEYS)
                .map(|x| SubChunkKey::new(0, x as i32, 4, 5))
                .collect(),
        )
        .is_err()
    );
}

#[test]
fn transparent_witness_snapshot_rejects_gen193_missing_key_then_accepts_gen194_complete() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let a = SubChunkKey::new(0, 0, 4, 5);
    let b = SubChunkKey::new(0, 1, 4, 5);
    let request = TransparentWitnessRequest::try_new(11, vec![a, b]).unwrap();
    let allocation = |key, generation, start| {
        TransparentAllocationIdentity::new(key, generation, start..start + 4, 40..42, 1)
    };
    let missing = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![allocation(a, 193, 8)],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let complete = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![allocation(a, 194, 8), allocation(b, 194, 12)],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    assert!(!transparent_view_key_satisfies_witness(&missing, &request));
    assert!(transparent_view_key_satisfies_witness(&complete, &request));
}

#[test]
fn witness_publishes_two_consecutive_gpu_completions_and_resets_fail_closed() {
    let evidence = TransparentWitnessEvidence::default();
    let key = SubChunkKey::new(0, 0, 4, 5);
    let request = TransparentWitnessRequest::try_new(9, vec![key]).unwrap();
    evidence.set_authoritative_request(&request);
    let first = evidence.try_reserve(&request, 193, true).unwrap();
    assert!(evidence.complete(first));
    let second = evidence.try_reserve(&request, 193, true).unwrap();
    assert!(evidence.complete(second));
    let events = evidence.drain_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].consecutive, 1);
    assert_eq!(events[1].consecutive, 2);
    assert_eq!(events[1].revision, 9);
    assert_eq!(events[1].key_count, 1);

    let reset_evidence = TransparentWitnessEvidence::default();
    reset_evidence.set_authoritative_request(&request);
    let complete = reset_evidence.try_reserve(&request, 194, true).unwrap();
    assert!(reset_evidence.complete(complete));
    let incomplete = reset_evidence.try_reserve(&request, 194, false).unwrap();
    assert!(reset_evidence.complete(incomplete));
    let complete = reset_evidence.try_reserve(&request, 194, true).unwrap();
    assert!(reset_evidence.complete(complete));
    assert_eq!(reset_evidence.drain_events()[0].consecutive, 1);

    let stale = reset_evidence.try_reserve(&request, 194, true).unwrap();
    reset_evidence.reset();
    assert!(!reset_evidence.complete(stale));
    assert!(reset_evidence.drain_events().is_empty());
}

#[test]
fn stale_extracted_witness_request_cannot_reactivate_after_authoritative_reset() {
    let evidence = TransparentWitnessEvidence::default();
    let request =
        TransparentWitnessRequest::try_new(9, vec![SubChunkKey::new(0, 0, 4, 5)]).unwrap();
    evidence.set_authoritative_request(&request);
    assert!(evidence.try_reserve(&request, 193, true).is_some());

    evidence.reset();
    assert!(evidence.try_reserve(&request, 194, true).is_none());
    assert!(evidence.drain_events().is_empty());
}

#[test]
fn incomplete_witness_diagnostic_is_exact_deduplicated_and_reset_bounded() {
    let evidence = TransparentWitnessEvidence::default();
    let a = SubChunkKey::new(0, 0, 4, 5);
    let b = SubChunkKey::new(0, 1, 4, 5);
    let request = TransparentWitnessRequest::try_new(9, vec![a, b]).unwrap();
    evidence.set_authoritative_request(&request);

    for generation in [193, 194] {
        let token = evidence
            .try_reserve_missing(&request, generation, vec![b])
            .unwrap();
        assert!(evidence.complete(token));
    }
    let diagnostics = evidence.drain_incomplete_events();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].revision, 9);
    assert_eq!(diagnostics[0].generation, 193);
    assert_eq!(&*diagnostics[0].missing_keys, &[b]);

    evidence.reset();
    assert!(evidence.drain_incomplete_events().is_empty());
}

#[test]
fn witness_stage_diagnostics_are_change_deduplicated_and_request_bounded() {
    let evidence = TransparentWitnessEvidence::default();
    let key = SubChunkKey::new(0, 0, 4, 5);
    let request = TransparentWitnessRequest::try_new(9, vec![key]).unwrap();
    evidence.set_authoritative_request(&request);
    let mut record = TransparentWitnessStageRecord {
        key,
        extracted_visible: false,
        instance_present: true,
        liquid_quad_count: 5,
        instance_generation: 7,
        allocation_present: false,
        liquid_range_len: 0,
        lighting_range_len: 0,
        allocation_matches: false,
        committed_member: false,
    };
    assert!(evidence.record_stage_snapshot(9, 193, vec![record]));
    assert!(!evidence.record_stage_snapshot(9, 194, vec![record]));
    for generation in 195..=203 {
        record.extracted_visible = !record.extracted_visible;
        let _ = evidence.record_stage_snapshot(9, generation, vec![record]);
    }
    let events = evidence.drain_stage_events();
    assert_eq!(events.len(), 8);
    assert_eq!(events[0].committed_generation, 193);
    assert_eq!(
        events[0].records.as_ref(),
        &[TransparentWitnessStageRecord {
            extracted_visible: false,
            ..record
        }]
    );

    evidence.reset();
    assert!(evidence.drain_stage_events().is_empty());
}

#[test]
fn retired_identity_matches_exact_old_snapshot_and_not_unrelated_active_address() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let old = retirement_test_allocation();
    let identity = TransparentAllocationIdentity::new(
        old.gpu.key,
        old.gpu.generation,
        old.gpu.liquid_range.clone().unwrap(),
        old.gpu.liquid_lighting_range.clone().unwrap(),
        old.gpu.metadata_index,
    );
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let snapshot = committed_transparent_state(
        &key,
        vec![PackedTransparentDrawRef::new(2, old.gpu.metadata_index)],
    )
    .committed()
    .unwrap()
    .clone();
    assert!(transparent_snapshot_addresses_are_resident(
        &snapshot,
        std::iter::empty(),
        [&old.gpu],
        texture_identity,
        tint_identity,
    ));
    let mut unrelated = old.gpu;
    unrelated.generation += 1;
    assert!(!transparent_snapshot_addresses_are_resident(
        &snapshot,
        std::iter::empty(),
        [&unrelated],
        texture_identity,
        tint_identity,
    ));
}

#[test]
fn removal_to_empty_arms_only_after_snapshot_no_longer_references_retired_identity() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let old = retirement_test_allocation();
    let identity = TransparentAllocationIdentity::new(
        old.gpu.key,
        old.gpu.generation,
        old.gpu.liquid_range.clone().unwrap(),
        old.gpu.liquid_lighting_range.clone().unwrap(),
        old.gpu.metadata_index,
    );
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let snapshot = committed_transparent_state(
        &key,
        vec![PackedTransparentDrawRef::new(2, old.gpu.metadata_index)],
    )
    .committed()
    .unwrap()
    .clone();
    assert!(!transparent_retirement_can_arm(Some(&snapshot), &old.gpu));
    assert!(transparent_retirement_can_arm(None, &old.gpu));
}

#[test]
fn asset_or_tint_identity_change_clears_even_resident_snapshot() {
    let texture_identity = ChunkTextureAssetIdentity::new(1, 1);
    let tint_identity = ChunkBiomeTintIdentity::new(2, 2);
    let identity =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 3, 8..16, 32..36, 1);
    let old_key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![identity.clone()],
        texture_identity,
        tint_identity,
    )
    .unwrap();
    let resident = [resident_transparent_allocation(&identity, tint_identity)];
    for (next_texture, next_tint) in [
        (ChunkTextureAssetIdentity::new(9, 9), tint_identity),
        (texture_identity, ChunkBiomeTintIdentity::new(9, 9)),
    ] {
        let mut state =
            committed_transparent_state(&old_key, vec![PackedTransparentDrawRef::new(2, 1)]);
        assert!(!transparent_snapshot_addresses_are_resident(
            state.committed().unwrap(),
            resident.iter(),
            std::iter::empty(),
            next_texture,
            next_tint,
        ));
        let next_key = ViewSortKey::try_new(
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            vec![identity.clone()],
            next_texture,
            next_tint,
        )
        .unwrap();
        state.request_retaining_resident_snapshot(&next_key, false);
        assert!(state.committed().is_none());
    }
}

#[test]
fn conflicting_manifest_fail_closes_every_absolute_ref_owner_and_active_metric() {
    let metrics = TransparentSortMetrics::default();
    metrics.update(|snapshot| {
        *snapshot = TransparentSortMetricsSnapshot {
            request_generation: 7,
            result_generation: 7,
            committed_generation: 7,
            encoded_generation: 7,
            presented_generation: 7,
            ref_count: 2,
            staged_bytes: 16,
            upload_bytes: 16,
            active_slot_age_frames: 3,
            transparent_water_distinct_tint_count: 2,
            ..Default::default()
        }
    });
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let mut runtime = TransparentSortRuntime {
        view_entity: Some(Entity::from_bits(1)),
        ..Default::default()
    };
    let committed_generation = runtime.state.request(&key);
    assert_eq!(
        runtime.state.complete(
            TransparentSortResult::new(committed_generation, key.clone(), vec![]).unwrap()
        ),
        Ok(true)
    );
    let pending_generation = ViewSortGeneration::new(committed_generation.get() + 1);
    let work = TransparentSortWork {
        generation: pending_generation,
        requested_at: Instant::now(),
        key: key.clone(),
        view_from_world: Mat4::IDENTITY,
        candidates: Arc::from([]),
        distinct_tint_count: 0,
    };
    assert!(runtime.gate.submit(pending_generation, work).is_some());
    runtime
        .requested_at
        .insert(pending_generation, Instant::now());
    runtime
        .staged_distinct_tint_counts
        .insert(pending_generation, 2);

    runtime.fail_closed_conflicting_manifest(&metrics);

    assert!(runtime.state.committed().is_none());
    assert_eq!(runtime.state.staged_ref_count(), 0);
    assert_eq!(runtime.gate.in_flight_generation(), None);
    assert_eq!(runtime.gate.pending_generation(), None);
    assert!(runtime.requested_at.is_empty());
    assert!(runtime.staged_distinct_tint_counts.is_empty());
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.committed_generation, 0);
    assert_eq!(snapshot.encoded_generation, 0);
    assert_eq!(snapshot.presented_generation, 0);
    assert_eq!(snapshot.ref_count, 0);
    assert_eq!(
        snapshot.upload_bytes, 16,
        "cumulative accounting survives fail-close"
    );
    assert!(runtime.state.request(&key) > committed_generation);
}

#[test]
fn invalid_camera_transform_fail_closes_committed_staged_gate_and_metadata() {
    let metrics = TransparentSortMetrics::default();
    metrics.update(|snapshot| {
        *snapshot = TransparentSortMetricsSnapshot {
            committed_generation: 7,
            encoded_generation: 7,
            presented_generation: 7,
            ref_count: 2,
            upload_bytes: 16,
            ..Default::default()
        }
    });
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let mut runtime = TransparentSortRuntime {
        view_entity: Some(Entity::from_bits(1)),
        ..Default::default()
    };
    let committed = runtime.state.request(&key);
    assert_eq!(
        runtime
            .state
            .complete(TransparentSortResult::new(committed, key.clone(), vec![]).unwrap()),
        Ok(true)
    );
    let moved = ViewSortKey::try_new(
        [f32::from_bits(1), 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let staged = runtime.state.request(&moved);
    assert_eq!(
        runtime.state.complete(
            TransparentSortResult::new(
                staged,
                moved.clone(),
                vec![PackedTransparentDrawRef::new(1, 2)],
            )
            .unwrap(),
        ),
        Ok(false)
    );
    let pending = ViewSortGeneration::new(staged.get() + 1);
    let work = TransparentSortWork {
        generation: pending,
        requested_at: Instant::now(),
        key: moved,
        view_from_world: Mat4::IDENTITY,
        candidates: Arc::from([]),
        distinct_tint_count: 0,
    };
    assert!(runtime.gate.submit(pending, work).is_some());
    runtime.requested_at.insert(staged, Instant::now());
    runtime.requested_at.insert(pending, Instant::now());
    runtime.staged_distinct_tint_counts.insert(staged, 2);

    fail_closed_transparent_sort_key_error(
        &mut runtime,
        &metrics,
        TransparentSortError::InvalidCameraTransform,
    );

    assert!(runtime.state.committed().is_none());
    assert_eq!(runtime.state.staged_ref_count(), 0);
    assert_eq!(runtime.gate.in_flight_generation(), None);
    assert!(runtime.requested_at.is_empty());
    assert!(runtime.staged_distinct_tint_counts.is_empty());
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.committed_generation, 0);
    assert_eq!(snapshot.encoded_generation, 0);
    assert_eq!(snapshot.presented_generation, 0);
    assert_eq!(snapshot.ref_count, 0);
    assert_eq!(snapshot.upload_bytes, 16);
    assert!(runtime.state.request(&key) > staged);
}

#[test]
fn staged_generation_is_not_resubmitted_and_retains_causal_latency_origin() {
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let mut runtime = TransparentSortRuntime::default();
    let generation = runtime.state.request(&key);
    assert_eq!(
        runtime.state.complete(
            TransparentSortResult::new(generation, key, vec![PackedTransparentDrawRef::new(1, 2)],)
                .unwrap(),
        ),
        Ok(false)
    );
    assert!(!runtime.generation_needs_sort_job(generation));

    let requested_at = Instant::now();
    assert_eq!(
        transparent_request_to_commit_latency(
            requested_at,
            requested_at + Duration::from_millis(5),
        ),
        Duration::from_millis(5)
    );
}

#[test]
fn candidate_cache_reuses_camera_only_arc_rebuilds_identity_and_clears_on_failure() {
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let camera_only = ViewSortKey::try_new(
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let candidate =
        TransparentSortCandidate::new(SubChunkKey::new(0, 0, 0, 0), 0, 4, 5, [8.0; 3], [0.5; 3]);
    let mut runtime = TransparentSortRuntime::default();
    let (first, first_tints) = runtime
        .resolve_candidate_cache(&key, || Ok((vec![candidate.clone()], 2)))
        .unwrap();
    let (camera_reuse, camera_tints) = runtime
        .resolve_candidate_cache(&camera_only, || {
            panic!("camera-only key rebuilt candidates")
        })
        .unwrap();
    assert!(Arc::ptr_eq(&first, &camera_reuse));
    assert_eq!((first_tints, camera_tints), (2, 2));

    let changed_identity = ViewSortKey::try_new(
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(2, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let (rebuilt, _) = runtime
        .resolve_candidate_cache(&changed_identity, || Ok((vec![candidate], 3)))
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &rebuilt));

    let failed_identity = ViewSortKey::try_new(
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        vec![],
        ChunkTextureAssetIdentity::new(3, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    assert_eq!(
        runtime.resolve_candidate_cache(&failed_identity, || {
            Err(TransparentSortError::ReferenceCeiling {
                requested: MAX_TRANSPARENT_DRAW_REFS + 1,
                ceiling: MAX_TRANSPARENT_DRAW_REFS,
            })
        }),
        Err(TransparentSortError::ReferenceCeiling {
            requested: MAX_TRANSPARENT_DRAW_REFS + 1,
            ceiling: MAX_TRANSPARENT_DRAW_REFS,
        })
    );
    assert!(runtime.candidate_cache.is_none());
}

#[test]
fn encoded_liquid_draw_is_not_presented_until_submitted_work_completes() {
    let metrics = TransparentSortMetrics::default();
    metrics.update(|snapshot| {
        *snapshot = TransparentSortMetricsSnapshot {
            committed_generation: 12,
            ref_count: 1,
            ..Default::default()
        }
    });
    record_encoded_transparent_generation(&metrics, ViewSortGeneration::new(12));
    assert_eq!(metrics.snapshot().encoded_generation, 12);
    assert_eq!(metrics.snapshot().presented_generation, 0);

    record_gpu_completed_transparent_generation(&metrics, 12);
    assert_eq!(metrics.snapshot().presented_generation, 12);
}

#[test]
fn transparent_completion_fence_is_bounded_and_stale_callbacks_cannot_regress() {
    let fence = TransparentPresentationFence::default();
    assert!(fence.try_reserve(12));
    assert!(!fence.try_reserve(13));
    assert!(!fence.complete(13));
    assert!(fence.complete(12));
    assert!(fence.try_reserve(13));

    let metrics = TransparentSortMetrics::default();
    metrics.update(|snapshot| {
        *snapshot = TransparentSortMetricsSnapshot {
            committed_generation: 13,
            encoded_generation: 13,
            presented_generation: 11,
            ref_count: 1,
            ..Default::default()
        }
    });
    record_gpu_completed_transparent_generation(&metrics, 12);
    assert_eq!(metrics.snapshot().presented_generation, 11);
    assert!(fence.complete(13));
    record_gpu_completed_transparent_generation(&metrics, 13);
    assert_eq!(metrics.snapshot().presented_generation, 13);
}

#[test]
fn daylight_changes_only_sky_light_without_rebaking() {
    let block_only = 0x000f;
    let sky_only = 0x00f0;
    assert_eq!(
        packed_light_factor(0, 0.0),
        PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR,
        "light level zero retains the named vanilla-like ambient visibility floor"
    );
    assert!(
        packed_light_factor(0x0001, 0.0) > packed_light_factor(0, 0.0),
        "the ambient floor must not flatten the first emitted-light step"
    );
    assert_eq!(
        packed_light_factor(block_only, 0.0),
        packed_light_factor(block_only, 1.0),
        "daylight transfer must never alter independent block light"
    );
    assert_eq!(
        packed_light_factor(sky_only, 0.0),
        PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR
            + (1.0 - PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR) * PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR,
        "true-night skylight retains its transfer before the independent ambient remap"
    );
    assert_eq!(
        packed_light_factor(sky_only, 1.0),
        1.0,
        "the ambient remap must preserve full daylight exactly"
    );
    assert!(packed_light_factor(0x03f0, 1.0) < packed_light_factor(sky_only, 1.0));
}
