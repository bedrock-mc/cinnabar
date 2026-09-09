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

fn required_expectation_for(
    queue: &ChunkRenderQueue,
    cohort: RenderViewCohort,
    source_cohort: Option<RenderViewCohort>,
    columns: impl IntoIterator<Item = world::ChunkKey>,
    now: Instant,
) -> Option<TargetRenderExpectation> {
    queue.freeze_target_expectation_for_columns(cohort, source_cohort, columns, 4, now)
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
fn explicit_required_columns_can_extend_beyond_raw_cohort_geometry() {
    let now = Instant::now();
    let cohort = RenderViewCohort::new(0, [65, 65], 2);
    let inside = SubChunkKey::new(0, 65, 0, 65);
    let announced_outside_a = SubChunkKey::new(0, 69, 0, 65);
    let announced_outside_b = SubChunkKey::new(0, 69, 1, 65);
    let unannounced_inside = SubChunkKey::new(0, 66, 0, 65);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.extend([
        (announced_outside_b, 9),
        (unannounced_inside, 99),
        (inside, 7),
        (announced_outside_a, 8),
    ]);

    let expectation = required_expectation_for(
        &queue,
        cohort,
        None,
        [inside.chunk(), announced_outside_a.chunk()],
        now,
    )
    .expect("same-dimension explicit columns define their own bounded membership");

    assert_eq!(expectation.cohort, cohort, "raw cohort identity changed");
    assert_eq!(
        expectation.target_columns.as_deref(),
        Some([inside.chunk(), announced_outside_a.chunk()].as_slice())
    );
    assert_eq!(
        expectation.manifest.as_ref(),
        &[
            (inside, 7),
            (announced_outside_a, 8),
            (announced_outside_b, 9)
        ]
    );

    let inside_entity = Entity::from_bits(1);
    let outside_a_entity = Entity::from_bits(2);
    let outside_b_entity = Entity::from_bits(3);
    let instances = [
        FrameInstanceIdentity {
            entity: outside_b_entity,
            key: announced_outside_b,
            generation: 9,
        },
        FrameInstanceIdentity {
            entity: inside_entity,
            key: inside,
            generation: 7,
        },
        FrameInstanceIdentity {
            entity: outside_a_entity,
            key: announced_outside_a,
            generation: 8,
        },
    ];
    let allocations = [
        FrameAllocationIdentity {
            entity: outside_b_entity,
            key: announced_outside_b,
            generation: 9,
        },
        FrameAllocationIdentity {
            entity: inside_entity,
            key: inside,
            generation: 7,
        },
        FrameAllocationIdentity {
            entity: outside_a_entity,
            key: announced_outside_a,
            generation: 8,
        },
    ];
    let (first, second) = acknowledged_pair(expectation, &instances, &allocations);

    assert_eq!(first.foreign_instances, 0);
    assert!(first.is_exact());
    assert!(first.forms_stable_exact_pair_with(&second));
}

#[test]
fn explicit_required_columns_reject_empty_and_wrong_dimension_membership() {
    let now = Instant::now();
    let cohort = RenderViewCohort::new(0, [65, 65], 2);
    let queue = ChunkRenderQueue::default();

    assert!(
        required_expectation_for(&queue, cohort, None, [], now).is_none(),
        "an empty explicit source set must fail closed"
    );
    assert!(
        required_expectation_for(&queue, cohort, None, [world::ChunkKey::new(1, 65, 65)], now,)
            .is_none(),
        "a cross-dimension explicit source set must fail closed"
    );
}

#[test]
fn unannounced_instances_inside_and_outside_raw_geometry_are_foreign() {
    let now = Instant::now();
    let cohort = RenderViewCohort::new(0, [65, 65], 2);
    let required = SubChunkKey::new(0, 65, 0, 65);
    let unannounced_inside = SubChunkKey::new(0, 66, 0, 65);
    let unannounced_outside = SubChunkKey::new(0, 69, 0, 65);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.insert(required, 7);
    let expectation = required_expectation_for(&queue, cohort, None, [required.chunk()], now)
        .expect("required column is valid");
    let instances = [
        FrameInstanceIdentity {
            entity: Entity::from_bits(1),
            key: required,
            generation: 7,
        },
        FrameInstanceIdentity {
            entity: Entity::from_bits(2),
            key: unannounced_inside,
            generation: 8,
        },
        FrameInstanceIdentity {
            entity: Entity::from_bits(3),
            key: unannounced_outside,
            generation: 9,
        },
    ];
    let allocations = instances.map(|instance| FrameAllocationIdentity {
        entity: instance.entity,
        key: instance.key,
        generation: instance.generation,
    });

    let (first, second) = acknowledged_pair(expectation, &instances, &allocations);

    assert_eq!(first.foreign_instances, 2);
    assert!(!first.is_exact());
    assert!(!first.forms_stable_exact_pair_with(&second));
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
