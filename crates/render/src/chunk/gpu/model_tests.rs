use super::*;
use crate::chunk::gpu::upload::{absolutize_model_draw_refs, validate_local_model_streams};

fn owned_model_stream_fixture() -> (
    Vec<PackedModelRef>,
    Vec<PackedQuadLighting>,
    Vec<PackedModelDrawRef>,
    Vec<assets::ModelTemplate>,
) {
    (
        vec![
            PackedModelRef::new(0x111, 0, 0, 0b10_1101),
            PackedModelRef::new(0x222, 1, 6, 0b10),
        ],
        vec![PackedQuadLighting::new([0; 4]); 8],
        vec![
            PackedModelDrawRef::new(0, 0),
            PackedModelDrawRef::new(0, 2),
            PackedModelDrawRef::new(0, 3),
            PackedModelDrawRef::new(0, 5),
            PackedModelDrawRef::new(1, 1),
        ],
        vec![
            assets::ModelTemplate {
                quad_start: 0,
                quad_count: 6,
                flags: 0,
            },
            assets::ModelTemplate {
                quad_start: 6,
                quad_count: 2,
                flags: 0,
            },
        ],
    )
}

#[test]
fn partial_masks_own_full_contiguous_template_lighting() {
    let (refs, lighting, draws, templates) = owned_model_stream_fixture();
    assert!(validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_upload_rejects_duplicate_lighting_base() {
    let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
    refs[1] = PackedModelRef::new(0x222, 1, 0, 0b10);
    assert!(!validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_upload_rejects_first_lighting_base_gap() {
    let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
    refs[0] = PackedModelRef::new(0x111, 0, 1, 0b10_1101);
    assert!(!validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_upload_rejects_middle_lighting_gap() {
    let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
    refs[1] = PackedModelRef::new(0x222, 1, 7, 0b10);
    assert!(!validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_upload_rejects_trailing_unreachable_lighting() {
    let (refs, mut lighting, draws, templates) = owned_model_stream_fixture();
    lighting.push(PackedQuadLighting::new([0; 4]));
    assert!(!validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_upload_rejects_cross_model_lighting_overlap() {
    let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
    refs[1] = PackedModelRef::new(0x222, 1, 5, 0b10);
    assert!(!validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_upload_rejects_cross_model_draw_read() {
    let (refs, lighting, mut draws, templates) = owned_model_stream_fixture();
    draws[3] = PackedModelDrawRef::new(1, 1);
    assert!(!validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_upload_rejects_zero_mask_ref_beside_drawable_ref() {
    let (mut refs, lighting, draws, templates) = owned_model_stream_fixture();
    refs[0] = PackedModelRef::new(0x111, 0, 0, 0);
    assert!(!validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
}

#[test]
fn model_draw_absolutization_changes_only_the_model_index() {
    let mut draws = vec![[0, 2], [3, 31]];
    absolutize_model_draw_refs(&mut draws, 20).expect("aligned model record base");
    assert_eq!(draws, [[5, 2], [8, 31]]);
    assert!(absolutize_model_draw_refs(&mut draws, 22).is_none());
}

#[test]
fn partitioned_model_draw_absolutization_preserves_both_exact_routes() {
    let mut opaque = vec![[0, 0], [2, 7]];
    let mut blend = vec![[0, 1], [2, 8]];

    absolutize_partitioned_model_draw_refs(&mut opaque, &mut blend, 20)
        .expect("aligned shared model base");

    assert_eq!(opaque, [[5, 0], [7, 7]]);
    assert_eq!(blend, [[5, 1], [7, 8]]);
    assert!(absolutize_partitioned_model_draw_refs(&mut opaque, &mut blend, 22).is_none());
}

#[test]
fn production_model_witness_counts_refs_not_exact_draw_records() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1 << 32 | 81);
    let mut allocation = retirement_test_allocation();
    allocation.gpu.key = key;
    allocation.gpu.generation = 9;
    allocation.gpu.model_range = Some(0..8);
    allocation.gpu.model_lighting_range = Some(8..20);
    allocation.gpu.model_draw_range = Some(20..40);
    allocation.model_range = allocation.gpu.model_range.clone();
    allocation.model_lighting_range = allocation.gpu.model_lighting_range.clone();
    allocation.model_draw_range = allocation.gpu.model_draw_range.clone();

    let model_ref_count = model_ref_count_for_witness(&allocation.gpu);
    let model_draw_count = allocation.gpu.model_draw_range.as_ref().unwrap().len() / 2;
    assert_eq!(model_ref_count, 2);
    assert_eq!(model_draw_count, 10);

    let identity = FrameAllocationIdentity {
        entity,
        key,
        generation: 9,
    };
    let request = ModelWitnessRequest::try_new(7, [0x81; 32], vec![key]).unwrap();
    let probe = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(key, 9)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 9,
        }],
        [(identity, allocation.expected_streams(), model_ref_count)],
        request,
    );
    assert!(probe.record_direct_streams(entity, identity, ChunkStreamMask::MODEL));
    let witness = probe.complete().model_witness.unwrap();
    assert_eq!(witness.total_model_ref_count, 2);
    assert_eq!(witness.manifest[0].model_ref_count, 2);
}

#[test]
fn model_workload_counts_refs_exact_draws_and_avoided_fixed_slots() {
    let mut first = retirement_test_allocation().gpu;
    first.model_range = Some(0..8);
    first.model_lighting_range = Some(8..20);
    first.model_draw_range = Some(20..40);

    let mut second = retirement_test_allocation().gpu;
    second.model_range = Some(64..68);
    second.model_lighting_range = Some(68..74);
    second.model_draw_range = Some(74..82);

    let snapshot = summarize_model_workload([&first, &second]);
    assert_eq!(snapshot.model_ref_count, 3);
    assert_eq!(snapshot.model_draw_ref_count, 14);
    assert_eq!(snapshot.legacy_fixed_slot_quad_invocations_avoided, 82);
}

#[test]
fn model_workload_ignores_allocations_without_a_complete_model_stream() {
    let mut missing_draws = retirement_test_allocation().gpu;
    missing_draws.model_range = Some(0..4);
    missing_draws.model_lighting_range = Some(4..10);
    missing_draws.model_draw_range = None;

    let empty = retirement_test_allocation().gpu;
    assert_eq!(
        summarize_model_workload([&missing_draws, &empty]),
        ModelWorkloadCount::default()
    );
}

#[test]
fn model_workload_counts_transparent_only_model_draws() {
    let mut allocation = retirement_test_allocation().gpu;
    allocation.model_range = Some(0..4);
    allocation.model_lighting_range = Some(4..8);
    allocation.model_draw_range = None;
    allocation.transparent_model_draw_range = Some(8..12);

    assert_eq!(
        summarize_model_workload([&allocation]),
        ModelWorkloadCount {
            model_ref_count: 1,
            model_draw_ref_count: 2,
            legacy_fixed_slot_quad_invocations_avoided: 30,
        }
    );
}

#[test]
fn model_mdi_batch_emits_one_command_per_eligible_allocation() {
    let tint = ChunkBiomeTintIdentity::new(4, 5);
    let allocations = [
        (Entity::from_bits(201), 0_u32, 1_u32),
        (Entity::from_bits(202), 20_u32, 3_u32),
        (Entity::from_bits(203), 44_u32, 5_u32),
    ]
    .map(|(entity, start, draw_count)| {
        (
            entity,
            GpuChunkAllocation {
                key: SubChunkKey::new(0, start as i32, 0, 0),
                generation: 1,
                tint_identity: tint,
                quad_range: 0..0,
                cube_lighting_range: None,
                model_range: Some(start..start + 4),
                model_lighting_range: Some(start + 4..start + 6),
                model_draw_range: Some(start + 6..start + 6 + draw_count * 2),
                transparent_model_draw_range: None,
                liquid_range: None,
                liquid_lighting_range: None,
                has_depth_liquid: false,
                has_transparent_liquid: false,
                depth_liquid_range: None,
                metadata_index: start / 4,
            },
        )
    });
    let frame_probe = ActiveFrameProbe::default();
    let (commands, drawn) = prepare_model_indirect_batch_draws(
        allocations
            .iter()
            .map(|(entity, allocation)| (*entity, allocation)),
        &frame_probe,
        tint,
    );

    assert_eq!(commands.len(), allocations.len());
    assert_eq!(drawn.len(), allocations.len());
    assert_eq!(
        commands
            .iter()
            .map(|command| command.instance_count)
            .collect::<Vec<_>>(),
        [1, 3, 5]
    );
    assert_eq!(
        drawn.iter().map(|(entity, _)| *entity).collect::<Vec<_>>(),
        allocations.map(|(entity, _)| entity)
    );
}

#[test]
fn any_partial_model_triplet_is_expected_as_model_but_cannot_draw() {
    for (model, lighting, draw) in [
        (Some(0..4), None, None),
        (None, Some(4..6), None),
        (None, None, Some(6..8)),
        (Some(0..4), Some(4..6), None),
        (Some(0..4), None, Some(4..6)),
        (None, Some(0..2), Some(2..4)),
    ] {
        let mut allocation = retirement_test_allocation();
        allocation.model_range = model.clone();
        allocation.model_lighting_range = lighting.clone();
        allocation.model_draw_range = draw.clone();
        allocation.gpu.model_range = model;
        allocation.gpu.model_lighting_range = lighting;
        allocation.gpu.model_draw_range = draw;

        assert!(
            allocation
                .expected_streams()
                .contains(ChunkStreamMask::MODEL)
        );
        assert!(model_direct_draw_command(&allocation.gpu).is_none());
        assert!(model_mdi_draw_command(&allocation.gpu).is_none());
    }
}

#[test]
fn liquid_lighting_index_patches_to_shared_arena_records_without_mutating_other_words() {
    let mut quads = vec![[0x432, 7, 0b101, 0], [0x765, 8, 0b110, 2]];
    absolutize_liquid_lighting_indices(&mut quads, 20);
    assert_eq!(quads, [[0x432, 7, 0b101, 10], [0x765, 8, 0b110, 12]]);
}

#[test]
fn transparent_refs_require_exact_instance_identity_and_aligned_stream_ranges() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let tint = ChunkBiomeTintIdentity::new(4, 5);
    let instance = ChunkRenderInstance {
        key,
        cube_quads: Arc::from([]),
        cube_lighting: Arc::from([]),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([PackedLiquidQuad::try_pack(
            [0, 0, 0],
            Face::PositiveY,
            [255; 4],
            1,
            0,
            [0, 0],
            false,
        )
        .unwrap()]),
        liquid_lighting: Arc::from([PackedQuadLighting::new([0; 4])]),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: tint,
        generation: 6,
        token: None,
        origin: [16, 32, 48],
    };
    let allocation = GpuChunkAllocation {
        key,
        generation: 6,
        tint_identity: tint,
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: None,
        model_lighting_range: None,
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: Some(8..12),
        liquid_lighting_range: Some(20..22),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_range: None,
        metadata_index: 7,
    };
    assert!(transparent_allocation_matches(&instance, &allocation, tint));

    let mut mismatches = Vec::new();
    let mut mismatch = allocation.clone();
    mismatch.generation += 1;
    mismatches.push(mismatch);
    let mut mismatch = allocation.clone();
    mismatch.tint_identity = ChunkBiomeTintIdentity::new(4, 6);
    mismatches.push(mismatch);
    let mut mismatch = allocation.clone();
    mismatch.liquid_range = Some(9..13);
    mismatches.push(mismatch);
    let mut mismatch = allocation.clone();
    mismatch.liquid_range = Some(8..16);
    mismatches.push(mismatch);
    let mut mismatch = allocation.clone();
    mismatch.liquid_lighting_range = Some(21..23);
    mismatches.push(mismatch);
    let mut mismatch = allocation.clone();
    mismatch.liquid_lighting_range = Some(20..24);
    mismatches.push(mismatch);
    assert!(
        mismatches
            .into_iter()
            .all(|allocation| { !transparent_allocation_matches(&instance, &allocation, tint) })
    );
    assert!(!transparent_allocation_matches(
        &instance,
        &allocation,
        ChunkBiomeTintIdentity::new(9, 9),
    ));
}

#[test]
fn transparent_model_refs_require_the_exact_gpu_generation_and_stream_ranges() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let instance = ChunkRenderInstance {
        key,
        cube_quads: Arc::from([]),
        cube_lighting: Arc::from([]),
        model_refs: Arc::from([PackedModelRef::new(0, 0, 0, 1)]),
        model_lighting: Arc::from([PackedQuadLighting::new([0; 4])]),
        model_draw_refs: Arc::from([PackedModelDrawRef::new(0, 0)]),
        transparent_model_draw_refs: Arc::from([PackedModelDrawRef::new(0, 0)]),
        liquid_quads: Arc::from([]),
        liquid_lighting: Arc::from([]),
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::new(4, 5),
        generation: 6,
        token: None,
        origin: [16, 32, 48],
    };
    let allocation = GpuChunkAllocation {
        key,
        generation: 6,
        tint_identity: instance.tint_identity,
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: Some(0..4),
        model_lighting_range: Some(4..6),
        model_draw_range: Some(6..8),
        transparent_model_draw_range: Some(8..10),
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        metadata_index: 7,
    };
    assert!(transparent_model_allocation_matches(&instance, &allocation));

    let mut stale = allocation.clone();
    stale.generation += 1;
    assert!(!transparent_model_allocation_matches(&instance, &stale));
    let mut wrong_key = allocation.clone();
    wrong_key.key = SubChunkKey::new(0, 1, 2, 4);
    assert!(!transparent_model_allocation_matches(&instance, &wrong_key));
    let mut wrong_model = allocation.clone();
    wrong_model.model_range = Some(0..8);
    assert!(!transparent_model_allocation_matches(
        &instance,
        &wrong_model
    ));
    let mut wrong_draw = allocation;
    wrong_draw.transparent_model_draw_range = Some(9..11);
    assert!(!transparent_model_allocation_matches(
        &instance,
        &wrong_draw
    ));
}

#[test]
fn water_and_models_share_one_transparent_upload_allowance() {
    let mut budget = TransparentUploadBudget::default();
    assert!(budget.consume(DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME - 3));
    assert_eq!(budget.remaining(), 3);
    assert!(!budget.consume(4));
    assert!(budget.consume(3));
    assert_eq!(budget.remaining(), 0);

    budget.reset();
    assert_eq!(
        budget.remaining(),
        DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME
    );
}
