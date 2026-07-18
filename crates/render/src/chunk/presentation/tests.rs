use super::*;
use crate::chunk::gpu::types::build_indexed_indirect_commands;

#[test]
fn gpu_write_does_not_complete_a_frame_probe() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let expectation = target_expectation(now, [(key, 7)]);
    let probe = CompletedFrameProbe {
        frame_sequence: 1,
        allocation_manifest: Arc::clone(&expectation.manifest),
        visible_allocation_manifest: Arc::clone(&expectation.manifest),
        drawn_manifest: Arc::clone(&expectation.manifest),
        expectation,
        missing_target_instances: 0,
        unexpected_target_instances: 0,
        source_instances: 0,
        foreign_instances: 0,
        stale_generation_instances: 0,
        orphan_allocations: 0,
        transparent_sort_generation: 0,
        model_witness: None,
    };

    assert!(
        build_presented_frame_ack(probe, FrameCompletionEvidence::default()).is_none(),
        "a PrepareResources write is not post-present GPU completion evidence"
    );
}

#[test]
fn visible_but_undrawn_target_manifest_is_not_exact_presented_evidence() {
    let render_ready_at = Instant::now();
    let present_returned_at = render_ready_at + std::time::Duration::from_millis(1);
    let gpu_completed_at = present_returned_at + std::time::Duration::from_millis(1);
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let probe = FrameProbe::begin(
        target_expectation(render_ready_at, [(key, 7)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        }],
        [allocation],
    );
    assert!(probe.record_visible(entity, allocation));
    let acknowledgement = build_presented_frame_ack(
        probe.complete(),
        FrameCompletionEvidence {
            present_returned_at: Some(present_returned_at),
            submitted_work_done_at: Some(gpu_completed_at),
        },
    )
    .expect("post-present GPU completion should publish diagnostic frame evidence");

    assert_eq!(acknowledgement.allocation_manifest.as_ref(), &[(key, 7)]);
    assert!(acknowledgement.drawn_manifest.is_empty());
    assert!(
        !acknowledgement.is_exact(),
        "a visible but undrawn target generation satisfied the presented-frame gate"
    );
}

#[test]
fn hidden_target_allocations_do_not_block_exact_presented_evidence() {
    let render_ready_at = Instant::now();
    let visible_key = SubChunkKey::new(0, 65, 0, 65);
    let hidden_key = SubChunkKey::new(0, 66, 0, 65);
    let visible_entity = Entity::from_bits(1);
    let hidden_entity = Entity::from_bits(2);
    let visible_allocation = FrameAllocationIdentity {
        entity: visible_entity,
        key: visible_key,
        generation: 7,
    };
    let hidden_allocation = FrameAllocationIdentity {
        entity: hidden_entity,
        key: hidden_key,
        generation: 8,
    };
    let expectation = target_expectation(render_ready_at, [(visible_key, 7), (hidden_key, 8)]);
    let completed_frame = |frame_sequence| {
        let probe = FrameProbe::begin(
            expectation.clone(),
            [
                FrameInstanceIdentity {
                    entity: visible_entity,
                    key: visible_key,
                    generation: 7,
                },
                FrameInstanceIdentity {
                    entity: hidden_entity,
                    key: hidden_key,
                    generation: 8,
                },
            ],
            [visible_allocation, hidden_allocation],
        );
        assert!(probe.record_visible(visible_entity, visible_allocation));
        assert!(probe.record_direct_draw(visible_entity, visible_allocation));
        let mut completed = probe.complete();
        completed.frame_sequence = frame_sequence;
        completed
    };
    let first = build_presented_frame_ack(
        completed_frame(10),
        FrameCompletionEvidence {
            present_returned_at: Some(render_ready_at + std::time::Duration::from_millis(1)),
            submitted_work_done_at: Some(render_ready_at + std::time::Duration::from_millis(2)),
        },
    )
    .expect("the first presented frame should publish evidence");
    let second = build_presented_frame_ack(
        completed_frame(11),
        FrameCompletionEvidence {
            present_returned_at: Some(render_ready_at + std::time::Duration::from_millis(3)),
            submitted_work_done_at: Some(render_ready_at + std::time::Duration::from_millis(4)),
        },
    )
    .expect("the second presented frame should publish evidence");

    assert_eq!(
        first.allocation_manifest.as_ref(),
        &[(visible_key, 7), (hidden_key, 8)]
    );
    assert_eq!(
        first.visible_allocation_manifest.as_ref(),
        &[(visible_key, 7)]
    );
    assert_eq!(first.drawn_manifest, first.visible_allocation_manifest);
    assert!(
        first.is_exact(),
        "a hidden but correctly allocated target blocked exact frame evidence"
    );
    assert!(first.forms_stable_exact_pair_with(&second));
}

#[test]
fn visibility_change_cannot_form_a_stable_exact_presented_pair() {
    let render_ready_at = Instant::now();
    let first_key = SubChunkKey::new(0, 65, 0, 65);
    let second_key = SubChunkKey::new(0, 66, 0, 65);
    let first_entity = Entity::from_bits(1);
    let second_entity = Entity::from_bits(2);
    let first_allocation = FrameAllocationIdentity {
        entity: first_entity,
        key: first_key,
        generation: 7,
    };
    let second_allocation = FrameAllocationIdentity {
        entity: second_entity,
        key: second_key,
        generation: 8,
    };
    let expectation = target_expectation(render_ready_at, [(first_key, 7), (second_key, 8)]);
    let completed_frame = |frame_sequence, visible_entity, visible_allocation| {
        let probe = FrameProbe::begin(
            expectation.clone(),
            [
                FrameInstanceIdentity {
                    entity: first_entity,
                    key: first_key,
                    generation: 7,
                },
                FrameInstanceIdentity {
                    entity: second_entity,
                    key: second_key,
                    generation: 8,
                },
            ],
            [first_allocation, second_allocation],
        );
        assert!(probe.record_visible(visible_entity, visible_allocation));
        assert!(probe.record_direct_draw(visible_entity, visible_allocation));
        let mut completed = probe.complete();
        completed.frame_sequence = frame_sequence;
        completed
    };
    let acknowledgement = |probe, present_offset, gpu_offset| {
        build_presented_frame_ack(
            probe,
            FrameCompletionEvidence {
                present_returned_at: Some(
                    render_ready_at + std::time::Duration::from_millis(present_offset),
                ),
                submitted_work_done_at: Some(
                    render_ready_at + std::time::Duration::from_millis(gpu_offset),
                ),
            },
        )
        .expect("the presented frame should publish evidence")
    };
    let first = acknowledgement(completed_frame(10, first_entity, first_allocation), 1, 2);
    let second = acknowledgement(completed_frame(11, second_entity, second_allocation), 3, 4);

    assert!(first.is_exact());
    assert!(second.is_exact());
    assert_eq!(first.allocation_manifest, second.allocation_manifest);
    assert_ne!(
        first.visible_allocation_manifest,
        second.visible_allocation_manifest
    );
    assert!(
        !first.forms_stable_exact_pair_with(&second),
        "adjacent exact frames with different visibility formed a stable pair"
    );
}

#[test]
fn frame_probe_rejects_stale_allocation_generation() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let instance = FrameInstanceIdentity {
        entity,
        key,
        generation: 7,
    };
    let stale_allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let probe = FrameProbe::begin(
        target_expectation(now, [(key, 8)]),
        [instance],
        [stale_allocation],
    );

    assert!(probe.record_direct_draw(entity, stale_allocation));
    let completed = probe.complete();
    assert_eq!(completed.stale_generation_instances, 1);
    assert_eq!(completed.missing_target_instances, 1);
    assert_eq!(completed.unexpected_target_instances, 1);
}

#[test]
fn frame_probe_rejects_source_foreign_and_orphan_allocations() {
    let now = Instant::now();
    let target_key = SubChunkKey::new(0, 65, 0, 65);
    let source_key = SubChunkKey::new(0, 0, 0, 0);
    let foreign_key = SubChunkKey::new(1, 65, 0, 65);
    let target = Entity::from_bits(1);
    let source = Entity::from_bits(2);
    let foreign = Entity::from_bits(3);
    let orphan = Entity::from_bits(4);
    let instances = [
        FrameInstanceIdentity {
            entity: target,
            key: target_key,
            generation: 7,
        },
        FrameInstanceIdentity {
            entity: source,
            key: source_key,
            generation: 1,
        },
        FrameInstanceIdentity {
            entity: foreign,
            key: foreign_key,
            generation: 1,
        },
    ];
    let allocations = [
        FrameAllocationIdentity {
            entity: target,
            key: target_key,
            generation: 7,
        },
        FrameAllocationIdentity {
            entity: source,
            key: source_key,
            generation: 1,
        },
        FrameAllocationIdentity {
            entity: foreign,
            key: foreign_key,
            generation: 1,
        },
        FrameAllocationIdentity {
            entity: orphan,
            key: target_key,
            generation: 7,
        },
    ];

    let completed = FrameProbe::begin(
        target_expectation(now, [(target_key, 7)]),
        instances,
        allocations,
    )
    .complete();

    assert_eq!(completed.source_instances, 1);
    assert_eq!(completed.foreign_instances, 1);
    assert_eq!(completed.orphan_allocations, 1);
    assert_eq!(
        completed.missing_target_instances, 0,
        "the hidden target allocation belongs to the complete allocation manifest"
    );
}

#[test]
fn newly_prepared_generation_is_not_acknowledged_until_a_later_draw_frame() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let instance = FrameInstanceIdentity {
        entity,
        key,
        generation: 9,
    };
    let prepared = FrameAllocationIdentity {
        entity,
        key,
        generation: 9,
    };

    let same_frame = FrameProbe::begin(
        target_expectation(now, [(key, 9)]),
        [instance],
        std::iter::empty::<FrameAllocationIdentity>(),
    );
    assert!(
        !same_frame.record_direct_draw(entity, prepared),
        "an allocation created after Queue was eligible in its PrepareResources frame"
    );
    assert_eq!(same_frame.complete().missing_target_instances, 1);

    let later_frame =
        FrameProbe::begin(target_expectation(now, [(key, 9)]), [instance], [prepared]);
    assert!(later_frame.record_direct_draw(entity, prepared));
    assert_eq!(later_frame.complete().missing_target_instances, 0);
}

#[test]
fn direct_and_mdi_draws_publish_the_same_exact_manifest() {
    let now = Instant::now();
    let key_a = SubChunkKey::new(0, 64, 0, 65);
    let key_b = SubChunkKey::new(0, 65, 0, 65);
    let entity_a = Entity::from_bits(1);
    let entity_b = Entity::from_bits(2);
    let instances = [
        FrameInstanceIdentity {
            entity: entity_b,
            key: key_b,
            generation: 8,
        },
        FrameInstanceIdentity {
            entity: entity_a,
            key: key_a,
            generation: 7,
        },
    ];
    let allocations = [
        FrameAllocationIdentity {
            entity: entity_b,
            key: key_b,
            generation: 8,
        },
        FrameAllocationIdentity {
            entity: entity_a,
            key: key_a,
            generation: 7,
        },
    ];
    let expectation = target_expectation(now, [(key_a, 7), (key_b, 8)]);

    let direct = FrameProbe::begin(expectation.clone(), instances, allocations);
    assert!(direct.record_direct_draw(entity_b, allocations[0]));
    assert!(direct.record_direct_draw(entity_a, allocations[1]));
    let direct_manifest = direct.complete().drawn_manifest;

    let mdi = FrameProbe::begin(expectation, instances, allocations);
    assert_eq!(
        mdi.record_mdi_draws([(entity_b, allocations[0]), (entity_a, allocations[1]),]),
        2
    );
    let mdi_manifest = mdi.complete().drawn_manifest;

    assert_eq!(direct_manifest.as_ref(), &[(key_a, 7), (key_b, 8)]);
    assert_eq!(mdi_manifest, direct_manifest);
}

#[test]
fn frame_ack_requires_present_return_and_submitted_work_done_callback() {
    let render_ready_at = Instant::now();
    let present_returned_at = render_ready_at + std::time::Duration::from_millis(1);
    let gpu_completed_at = present_returned_at + std::time::Duration::from_millis(1);
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let probe = FrameProbe::begin(
        target_expectation(render_ready_at, [(key, 7)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        }],
        [allocation],
    );
    assert!(probe.record_direct_draw(entity, allocation));
    let completed = probe.complete();

    assert!(
        build_presented_frame_ack(
            completed.clone(),
            FrameCompletionEvidence {
                present_returned_at: Some(present_returned_at),
                submitted_work_done_at: None,
            },
        )
        .is_none()
    );
    assert!(
        build_presented_frame_ack(
            completed.clone(),
            FrameCompletionEvidence {
                present_returned_at: None,
                submitted_work_done_at: Some(gpu_completed_at),
            },
        )
        .is_none()
    );
    assert!(
        build_presented_frame_ack(
            completed.clone(),
            FrameCompletionEvidence {
                present_returned_at: Some(gpu_completed_at),
                submitted_work_done_at: Some(present_returned_at),
            },
        )
        .is_none(),
        "GPU completion cannot precede present return"
    );
    let acknowledgement = build_presented_frame_ack(
        completed,
        FrameCompletionEvidence {
            present_returned_at: Some(present_returned_at),
            submitted_work_done_at: Some(gpu_completed_at),
        },
    )
    .expect("both post-render signals should publish the frame");
    assert_eq!(acknowledgement.render_ready_at, render_ready_at);
    assert_eq!(acknowledgement.present_returned_at, present_returned_at);
    assert_eq!(acknowledgement.gpu_completed_at, gpu_completed_at);
}

#[test]
fn transparent_generation_is_published_only_from_actual_liquid_draw_evidence() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let probe = FrameProbe::begin(
        target_expectation(now, [(key, 7)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        }],
        [(allocation, ChunkStreamMask::LIQUID)],
    );
    assert_eq!(
        probe.record_transparent_draw(ViewSortGeneration::new(23), [(entity, allocation)]),
        1
    );
    let completed = probe.complete();
    assert_eq!(completed.transparent_sort_generation, 23);
    assert_eq!(completed.drawn_manifest.as_ref(), &[(key, 7)]);
}

#[test]
fn retired_backed_liquid_draw_still_attributes_the_encoded_sort_generation() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let current = FrameAllocationIdentity {
        entity,
        key,
        generation: 8,
    };
    let retired = FrameAllocationIdentity {
        generation: 7,
        ..current
    };
    let probe = FrameProbe::begin(
        target_expectation(now, [(key, 8)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 8,
        }],
        [(current, ChunkStreamMask::LIQUID)],
    );
    assert_eq!(
        probe.record_transparent_draw(ViewSortGeneration::new(24), [(entity, retired)]),
        0,
        "retired geometry is not the current opaque allocation manifest"
    );
    assert_eq!(probe.complete().transparent_sort_generation, 24);
}

#[test]
fn shared_frame_gate_publishes_only_the_current_expectation_callback() {
    let render_ready_at = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let expectation = target_expectation(render_ready_at, [(key, 7)]);
    let gate = PresentedFrameGate::default();
    gate.set_expectation(expectation.clone());
    assert_eq!(gate.expectation(), Some(expectation.clone()));

    let probe = FrameProbe::begin(
        expectation.clone(),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        }],
        [allocation],
    );
    assert!(probe.record_direct_draw(entity, allocation));
    let completed = probe.complete();
    assert!(gate.try_reserve_callback(&expectation));
    assert!(gate.publish_reserved_probe(
        completed.clone(),
        render_ready_at + std::time::Duration::from_millis(1),
        render_ready_at + std::time::Duration::from_millis(2),
    ));
    assert_eq!(gate.drain().len(), 1);

    let mut replacement = expectation;
    replacement.view_generation = 2;
    assert!(gate.try_reserve_callback(&completed.expectation));
    gate.set_expectation(replacement);
    assert!(!gate.publish_reserved_probe(
        completed,
        render_ready_at + std::time::Duration::from_millis(3),
        render_ready_at + std::time::Duration::from_millis(4),
    ));
    assert!(gate.drain().is_empty());
}

#[test]
fn frame_gate_bounds_in_flight_gpu_callbacks() {
    let render_ready_at = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let expectation = target_expectation(render_ready_at, [(key, 7)]);
    let gate = PresentedFrameGate::default();
    gate.set_expectation(expectation.clone());
    let completed = FrameProbe::begin(
        expectation.clone(),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        }],
        [allocation],
    )
    .complete();

    for _ in 0..DEFAULT_PRESENTED_FRAME_ACK_CAPACITY {
        assert!(gate.try_reserve_callback(&expectation));
    }
    assert!(
        !gate.try_reserve_callback(&expectation),
        "a stalled GPU allowed an unbounded callback reservation"
    );

    assert!(gate.publish_reserved_probe(
        completed,
        render_ready_at + std::time::Duration::from_millis(1),
        render_ready_at + std::time::Duration::from_millis(2),
    ));
    assert!(gate.try_reserve_callback(&expectation));
}

#[test]
fn duplicate_target_allocations_cannot_satisfy_exact_manifest() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let first = Entity::from_bits(1);
    let duplicate = Entity::from_bits(2);
    let completed = FrameProbe::begin(
        target_expectation(now, [(key, 7)]),
        [
            FrameInstanceIdentity {
                entity: first,
                key,
                generation: 7,
            },
            FrameInstanceIdentity {
                entity: duplicate,
                key,
                generation: 7,
            },
        ],
        [
            FrameAllocationIdentity {
                entity: first,
                key,
                generation: 7,
            },
            FrameAllocationIdentity {
                entity: duplicate,
                key,
                generation: 7,
            },
        ],
    )
    .complete();

    assert_eq!(completed.allocation_manifest.as_ref(), &[(key, 7)]);
    assert_eq!(completed.missing_target_instances, 0);
    assert_eq!(
        completed.unexpected_target_instances, 1,
        "duplicate target allocation multiplicity was collapsed into an exact set"
    );
}

#[test]
fn two_identical_partial_manifests_do_not_satisfy_the_expected_target_manifest() {
    let render_ready_at = Instant::now();
    let key_a = SubChunkKey::new(0, 64, 0, 65);
    let key_b = SubChunkKey::new(0, 65, 0, 65);
    let entity_a = Entity::from_bits(1);
    let allocation_a = FrameAllocationIdentity {
        entity: entity_a,
        key: key_a,
        generation: 7,
    };
    let expectation = target_expectation(render_ready_at, [(key_a, 7), (key_b, 7)]);
    let probe = FrameProbe::begin(
        expectation,
        [FrameInstanceIdentity {
            entity: entity_a,
            key: key_a,
            generation: 7,
        }],
        [allocation_a],
    );
    assert!(probe.record_direct_draw(entity_a, allocation_a));
    let completed = probe.complete();
    let first = build_presented_frame_ack(
        completed.clone(),
        FrameCompletionEvidence {
            present_returned_at: Some(render_ready_at + std::time::Duration::from_millis(1)),
            submitted_work_done_at: Some(render_ready_at + std::time::Duration::from_millis(2)),
        },
    )
    .unwrap();
    let second = build_presented_frame_ack(
        completed,
        FrameCompletionEvidence {
            present_returned_at: Some(render_ready_at + std::time::Duration::from_millis(3)),
            submitted_work_done_at: Some(render_ready_at + std::time::Duration::from_millis(4)),
        },
    )
    .unwrap();

    assert_eq!(first.allocation_manifest.as_ref(), &[(key_a, 7)]);
    assert_eq!(second.allocation_manifest, first.allocation_manifest);
    assert_eq!(first.missing_target_instances, 1);
    assert_eq!(second.missing_target_instances, 1);
    assert!(!first.is_exact());
    assert!(!first.forms_stable_exact_pair_with(&second));
}

#[test]
fn skipped_render_frame_sequence_cannot_form_a_stable_pair() {
    let render_ready_at = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1);
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let frame_probe = ActiveFrameProbe::default();
    let completed_frame = || {
        frame_probe.begin(FrameProbe::begin(
            target_expectation(render_ready_at, [(key, 7)]),
            [FrameInstanceIdentity {
                entity,
                key,
                generation: 7,
            }],
            [allocation],
        ));
        assert!(frame_probe.record_visible(entity, allocation));
        assert!(frame_probe.record_direct_draw(entity, allocation));
        frame_probe.take_completed().unwrap()
    };
    let acknowledgement = |probe, present_offset, gpu_offset| {
        build_presented_frame_ack(
            probe,
            FrameCompletionEvidence {
                present_returned_at: Some(
                    render_ready_at + std::time::Duration::from_millis(present_offset),
                ),
                submitted_work_done_at: Some(
                    render_ready_at + std::time::Duration::from_millis(gpu_offset),
                ),
            },
        )
        .unwrap()
    };
    let first_probe = completed_frame();
    let adjacent_probe = completed_frame();
    let skipped_probe = completed_frame();
    assert_eq!(first_probe.frame_sequence, 1);
    assert_eq!(adjacent_probe.frame_sequence, 2);
    assert_eq!(skipped_probe.frame_sequence, 3);
    let first = acknowledgement(first_probe, 1, 2);
    let adjacent = acknowledgement(adjacent_probe, 3, 4);
    let skipped = acknowledgement(skipped_probe, 5, 6);

    assert!(first.forms_stable_exact_pair_with(&adjacent));
    assert!(
        !first.forms_stable_exact_pair_with(&skipped),
        "non-adjacent target render frames formed a consecutive stable pair"
    );
}

#[test]
fn target_expectation_freezes_a_sorted_independent_complete_manifest() {
    let now = Instant::now();
    let target_a = SubChunkKey::new(0, 64, 0, 65);
    let target_b = SubChunkKey::new(0, 65, 0, 65);
    let foreign = SubChunkKey::new(0, 100, 0, 100);
    let mut queue = ChunkRenderQueue::default();
    queue
        .render_manifest
        .extend([(target_b, 8), (foreign, 99), (target_a, 7)]);

    let expectation = queue.freeze_target_expectation(
        RenderViewCohort::new(0, [65, 65], 16),
        Some(RenderViewCohort::new(0, [0, 0], 16)),
        4,
        now,
    );

    assert_eq!(
        expectation.manifest.as_ref(),
        &[(target_a, 7), (target_b, 8)]
    );
    assert_eq!(expectation.view_generation, 4);
    assert_eq!(expectation.render_ready_at, now);
}

#[test]
fn required_column_expectation_excludes_unannounced_retained_columns() {
    let now = Instant::now();
    let required_a = SubChunkKey::new(0, 65, 0, 65);
    let required_b = SubChunkKey::new(0, 65, 1, 65);
    let unannounced_retained = SubChunkKey::new(0, 66, 0, 65);
    let foreign_retained = SubChunkKey::new(0, 82, 0, 65);
    let cohort = RenderViewCohort::new(0, [65, 65], 16);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.extend([
        (required_b, 8),
        (unannounced_retained, 99),
        (foreign_retained, 100),
        (required_a, 7),
    ]);

    let expectation = queue
        .freeze_target_expectation_for_columns(
            cohort,
            None,
            [world::ChunkKey::new(0, 65, 65)],
            4,
            now,
        )
        .expect("announced required column belongs to the active cohort");

    assert_eq!(
        expectation.manifest.as_ref(),
        &[(required_a, 7), (required_b, 8)]
    );
    assert!(
        expectation.manifest.iter().all(|(key, _)| {
            key.chunk() != unannounced_retained.chunk() && key.chunk() != foreign_retained.chunk()
        }),
        "an unannounced retained column entered required presentation proof"
    );

    let required_entity = Entity::from_bits(1);
    let unannounced_entity = Entity::from_bits(2);
    let foreign_entity = Entity::from_bits(3);
    let completed = FrameProbe::begin(
        expectation,
        [
            FrameInstanceIdentity {
                entity: required_entity,
                key: required_a,
                generation: 7,
            },
            FrameInstanceIdentity {
                entity: unannounced_entity,
                key: unannounced_retained,
                generation: 99,
            },
            FrameInstanceIdentity {
                entity: foreign_entity,
                key: foreign_retained,
                generation: 100,
            },
        ],
        [
            FrameAllocationIdentity {
                entity: required_entity,
                key: required_a,
                generation: 7,
            },
            FrameAllocationIdentity {
                entity: unannounced_entity,
                key: unannounced_retained,
                generation: 99,
            },
            FrameAllocationIdentity {
                entity: foreign_entity,
                key: foreign_retained,
                generation: 100,
            },
        ],
    )
    .complete();
    assert_eq!(completed.allocation_manifest.as_ref(), &[(required_a, 7)]);
    assert_eq!(completed.unexpected_target_instances, 0);
    assert_eq!(completed.foreign_instances, 0);
}

#[test]
fn requested_key_expectation_is_sorted_and_ignores_unrelated_manifest_churn() {
    let now = Instant::now();
    let target_a = SubChunkKey::new(0, 64, 0, 65);
    let target_b = SubChunkKey::new(0, 65, 0, 65);
    let unrelated = SubChunkKey::new(0, 66, 0, 65);
    let cohort = RenderViewCohort::new(0, [65, 65], 16);
    let mut queue = ChunkRenderQueue::default();
    queue
        .render_manifest
        .extend([(target_b, 8), (unrelated, 99), (target_a, 7)]);

    let first = queue
        .freeze_target_expectation_for_keys(cohort, None, [target_b, target_a], 4, now)
        .expect("requested keys belong to the active cohort");
    queue.render_manifest.insert(unrelated, 100);
    let after_unrelated_churn = queue
        .freeze_target_expectation_for_keys(cohort, None, [target_b, target_a], 4, now)
        .expect("requested keys still belong to the active cohort");
    queue.render_manifest.insert(target_b, 9);
    let after_requested_generation_change = queue
        .freeze_target_expectation_for_keys(cohort, None, [target_b, target_a], 4, now)
        .expect("updated requested keys still belong to the active cohort");

    assert_eq!(first.manifest.as_ref(), &[(target_a, 7), (target_b, 8)]);
    assert_eq!(after_unrelated_churn, first);
    assert_eq!(
        after_requested_generation_change.manifest.as_ref(),
        &[(target_a, 7), (target_b, 9)]
    );
    assert_ne!(after_requested_generation_change, first);
}

#[test]
fn requested_key_expectation_rejects_a_key_outside_the_active_cohort() {
    let now = Instant::now();
    let target = SubChunkKey::new(0, 65, 0, 65);
    let outside = SubChunkKey::new(0, 100, 0, 100);
    let cohort = RenderViewCohort::new(0, [65, 65], 16);
    let mut queue = ChunkRenderQueue::default();
    queue.render_manifest.extend([(target, 7), (outside, 8)]);

    assert!(
        queue
            .freeze_target_expectation_for_keys(cohort, None, [target, outside], 4, now)
            .is_none(),
        "an out-of-cohort request must fail closed"
    );
}

#[test]
fn accepted_tracked_queue_changes_maintain_the_expected_generation_manifest() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let cohort = RenderViewCohort::new(0, [65, 65], 16);
    let mut queue = ChunkRenderQueue::default();
    queue
        .try_update_tracked(
            key,
            solid_test_mesh(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 7,
                dirty_since: now,
            },
        )
        .unwrap();

    assert_eq!(
        queue
            .freeze_target_expectation(cohort, None, 1, now)
            .manifest
            .as_ref(),
        &[(key, 7)]
    );

    queue
        .try_remove_tracked(
            key,
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 8,
                dirty_since: now,
            },
        )
        .unwrap();
    assert!(
        queue
            .freeze_target_expectation(cohort, None, 1, now)
            .manifest
            .is_empty()
    );
}

#[test]
fn indexed_indirect_commands_preserve_order_and_encode_quad_and_origin_ranges() {
    let allocations = [
        GpuChunkAllocation {
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 17..23,
            cube_lighting_range: Some(100..112),
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            metadata_index: 4,
        },
        GpuChunkAllocation {
            key: SubChunkKey::new(0, 1, 0, 0),
            generation: 2,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 4..9,
            cube_lighting_range: Some(120..130),
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            metadata_index: 1,
        },
    ];

    let commands = build_indexed_indirect_commands(allocations.iter());

    assert_eq!(size_of::<DrawIndexedIndirectArgs>(), 20);
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].index_count, 6);
    assert_eq!(commands[0].instance_count, 6);
    assert_eq!(commands[0].first_index, 0);
    assert_eq!(commands[0].base_vertex, 16);
    assert_eq!(commands[0].first_instance, 17);
    assert_eq!(commands[1].index_count, 6);
    assert_eq!(commands[1].instance_count, 5);
    assert_eq!(commands[1].first_index, 0);
    assert_eq!(commands[1].base_vertex, 4);
    assert_eq!(commands[1].first_instance, 4);
    assert_eq!(
        bytemuck::cast_slice::<DrawIndexedIndirectArgs, u32>(&commands),
        &[6, 6, 0, 16, 17, 6, 5, 0, 4, 4],
    );
    assert_eq!(metadata_base_vertex(4), Some(commands[0].base_vertex));
    assert_eq!(metadata_base_vertex(1), Some(commands[1].base_vertex));
}

#[test]
fn origin_metadata_preserves_the_palette_record_pointer() {
    assert_eq!(
        gpu_chunk_origin([16, -64, 32], 27, 7, 22),
        Some(GpuChunkOrigin {
            value: [16, -64, 32, 27],
            cube_bases: [7, 11, 0, 0],
        })
    );
    assert_eq!(
        gpu_chunk_origin([0, 0, 0], 0, 0, 0),
        Some(GpuChunkOrigin {
            value: [0, 0, 0, 0],
            cube_bases: [0, 0, 0, 0],
        })
    );
}

#[test]
fn multi_draw_requires_indirect_execution_and_indirect_first_instance() {
    let indirect = DownlevelFlags::INDIRECT_EXECUTION | DownlevelFlags::BASE_VERTEX;
    let first_instance = WgpuFeatures::INDIRECT_FIRST_INSTANCE
        | WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;

    assert_eq!(
        select_chunk_draw_mode(indirect, first_instance, false, true),
        ChunkDrawMode::MultiDrawIndirect,
    );
    assert_eq!(
        select_chunk_draw_mode(DownlevelFlags::BASE_VERTEX, first_instance, false, true,),
        ChunkDrawMode::Direct,
    );
    assert_eq!(
        select_chunk_draw_mode(
            indirect,
            WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            false,
            true,
        ),
        ChunkDrawMode::Direct,
    );
    assert_eq!(
        select_chunk_draw_mode(DownlevelFlags::empty(), WgpuFeatures::empty(), false, true,),
        ChunkDrawMode::Unsupported,
    );
    assert_eq!(
        select_chunk_draw_mode(
            DownlevelFlags::INDIRECT_EXECUTION,
            WgpuFeatures::INDIRECT_FIRST_INSTANCE,
            false,
            true,
        ),
        ChunkDrawMode::Unsupported,
    );
}

#[cfg(debug_assertions)]
#[test]
fn debug_build_uses_direct_draws_when_indirect_validation_needs_extended_commands() {
    let indirect = DownlevelFlags::INDIRECT_EXECUTION | DownlevelFlags::BASE_VERTEX;
    let first_instance = WgpuFeatures::INDIRECT_FIRST_INSTANCE;

    assert_eq!(
        select_chunk_draw_mode(indirect, first_instance, true, true),
        ChunkDrawMode::Direct,
        "debug DX12 validation expands indexed commands from 20 to 32 bytes, so wgpu 27 cannot batch them safely"
    );
    assert_eq!(
        select_chunk_draw_mode(indirect, first_instance, true, false),
        ChunkDrawMode::MultiDrawIndirect,
        "release DX12 keeps the required multi-draw path after debug validation is compiled out"
    );
}

#[test]
fn indirect_batch_collection_keeps_entities_before_gpu_allocation() {
    let mut world = World::new();
    let first = world.spawn_empty().id();
    let second = world.spawn_empty().id();

    let visible = sorted_visible_entities([(second, ()), (first, ())]);
    let mut expected = vec![first, second];
    expected.sort_unstable();

    assert_eq!(
        visible
            .into_iter()
            .map(|(render_entity, ())| render_entity)
            .collect::<Vec<_>>(),
        expected,
    );
}

#[test]
fn blank_adapter_fields_are_recorded_as_unavailable() {
    assert_eq!(adapter_metadata_field(String::new()), "unavailable");
    assert_eq!(adapter_metadata_field("  ".to_owned()), "unavailable");
    assert_eq!(
        adapter_metadata_field("driver 1.2".to_owned()),
        "driver 1.2"
    );
}

#[test]
fn explicit_present_mode_evidence_comes_from_surface_capabilities() {
    use bevy::window::PresentMode as WindowPresentMode;
    use wgpu::PresentMode as WgpuPresentMode;

    assert_eq!(
        resolve_surface_present_mode(
            WindowPresentMode::Immediate,
            &[WgpuPresentMode::Fifo, WgpuPresentMode::Immediate],
        ),
        Some(WgpuPresentMode::Immediate)
    );
    assert_eq!(
        resolve_surface_present_mode(WindowPresentMode::Immediate, &[WgpuPresentMode::Fifo],),
        Some(WgpuPresentMode::Fifo)
    );
    assert_eq!(
        resolve_surface_present_mode(WindowPresentMode::Immediate, &[]),
        None
    );
    assert_eq!(
        resolve_surface_present_mode(WindowPresentMode::Fifo, &[WgpuPresentMode::Fifo]),
        Some(WgpuPresentMode::Fifo)
    );
}
