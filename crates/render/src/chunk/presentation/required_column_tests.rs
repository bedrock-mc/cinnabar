use super::*;

fn required_expectation(
    queue: &ChunkRenderQueue,
    source_cohort: Option<RenderViewCohort>,
    now: Instant,
) -> TargetRenderExpectation {
    queue
        .freeze_target_expectation_for_columns(
            RenderViewCohort::new(0, [65, 65], 16),
            source_cohort,
            [world::ChunkKey::new(0, 65, 65)],
            4,
            now,
        )
        .expect("required column belongs to the active target cohort")
}

fn acknowledged_pair(
    expectation: TargetRenderExpectation,
    instances: &[FrameInstanceIdentity],
    allocations: &[FrameAllocationIdentity],
) -> (PresentedFrameAck, PresentedFrameAck) {
    let render_ready_at = expectation.render_ready_at;
    let acknowledgement = |frame_sequence, elapsed_millis| {
        let mut probe = FrameProbe::begin(
            expectation.clone(),
            instances.iter().copied(),
            allocations.iter().copied(),
        );
        probe.frame_sequence = frame_sequence;
        let present_returned_at =
            render_ready_at + std::time::Duration::from_millis(elapsed_millis);
        build_presented_frame_ack(
            probe.complete(),
            FrameCompletionEvidence {
                present_returned_at: Some(present_returned_at),
                submitted_work_done_at: Some(
                    present_returned_at + std::time::Duration::from_millis(1),
                ),
            },
        )
        .expect("ordered present and GPU completion evidence")
    };
    (acknowledgement(1, 1), acknowledgement(2, 3))
}

#[test]
fn required_column_expectation_filters_manifest_to_announced_membership() {
    let now = Instant::now();
    let required_a = SubChunkKey::new(0, 65, 0, 65);
    let required_b = SubChunkKey::new(0, 65, 1, 65);
    let unannounced_retained = SubChunkKey::new(0, 66, 0, 65);
    let foreign_retained = SubChunkKey::new(0, 82, 0, 65);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.extend([
        (required_b, 8),
        (unannounced_retained, 99),
        (foreign_retained, 100),
        (required_a, 7),
    ]);

    let expectation = required_expectation(&queue, None, now);

    assert_eq!(
        expectation.manifest.as_ref(),
        &[(required_a, 7), (required_b, 8)]
    );
}

#[test]
fn stale_subchunk_in_a_required_column_blocks_exact_stable_proof() {
    let now = Instant::now();
    let required = SubChunkKey::new(0, 65, 0, 65);
    let stale_unfrozen = SubChunkKey::new(0, 65, 2, 65);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.insert(required, 7);
    let expectation = required_expectation(&queue, None, now);
    let required_entity = Entity::from_bits(1);
    let stale_entity = Entity::from_bits(2);
    let instances = [
        FrameInstanceIdentity {
            entity: required_entity,
            key: required,
            generation: 7,
        },
        FrameInstanceIdentity {
            entity: stale_entity,
            key: stale_unfrozen,
            generation: 99,
        },
    ];
    let allocations = [
        FrameAllocationIdentity {
            entity: required_entity,
            key: required,
            generation: 7,
        },
        FrameAllocationIdentity {
            entity: stale_entity,
            key: stale_unfrozen,
            generation: 99,
        },
    ];

    let (first, second) = acknowledged_pair(expectation, &instances, &allocations);

    assert_eq!(first.unexpected_target_instances, 1);
    assert!(!first.is_exact());
    assert!(!first.forms_stable_exact_pair_with(&second));
}

#[test]
fn source_allocation_blocks_required_column_exact_stable_proof() {
    let now = Instant::now();
    let required = SubChunkKey::new(0, 65, 0, 65);
    let source = SubChunkKey::new(0, 0, 0, 0);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.insert(required, 7);
    let expectation = required_expectation(&queue, Some(RenderViewCohort::new(0, [0, 0], 16)), now);
    let target_entity = Entity::from_bits(1);
    let source_entity = Entity::from_bits(2);
    let instances = [
        FrameInstanceIdentity {
            entity: target_entity,
            key: required,
            generation: 7,
        },
        FrameInstanceIdentity {
            entity: source_entity,
            key: source,
            generation: 8,
        },
    ];
    let allocations = [
        FrameAllocationIdentity {
            entity: target_entity,
            key: required,
            generation: 7,
        },
        FrameAllocationIdentity {
            entity: source_entity,
            key: source,
            generation: 8,
        },
    ];

    let (first, second) = acknowledged_pair(expectation, &instances, &allocations);

    assert_eq!(first.source_instances, 1);
    assert!(!first.is_exact());
    assert!(!first.forms_stable_exact_pair_with(&second));
}

#[test]
fn foreign_allocation_blocks_required_column_exact_stable_proof() {
    let now = Instant::now();
    let required = SubChunkKey::new(0, 65, 0, 65);
    let foreign = SubChunkKey::new(1, 65, 0, 65);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.insert(required, 7);
    let expectation = required_expectation(&queue, None, now);
    let target_entity = Entity::from_bits(1);
    let foreign_entity = Entity::from_bits(2);
    let instances = [
        FrameInstanceIdentity {
            entity: target_entity,
            key: required,
            generation: 7,
        },
        FrameInstanceIdentity {
            entity: foreign_entity,
            key: foreign,
            generation: 8,
        },
    ];
    let allocations = [
        FrameAllocationIdentity {
            entity: target_entity,
            key: required,
            generation: 7,
        },
        FrameAllocationIdentity {
            entity: foreign_entity,
            key: foreign,
            generation: 8,
        },
    ];

    let (first, second) = acknowledged_pair(expectation, &instances, &allocations);

    assert_eq!(first.foreign_instances, 1);
    assert!(!first.is_exact());
    assert!(!first.forms_stable_exact_pair_with(&second));
}
