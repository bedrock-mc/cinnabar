use super::*;

fn exact_model_witness_ack(
    revision: u64,
    request_hash: [u8; 32],
    frame_sequence: u64,
    view_generation: u64,
    manifest: Arc<[ModelWitnessManifestRecord]>,
    now: Instant,
) -> ModelWitnessFrameAck {
    let total_model_ref_count = manifest.iter().map(|record| record.model_ref_count).sum();
    ModelWitnessFrameAck {
        revision,
        request_hash,
        frame_sequence,
        view_generation,
        present_returned_at: now,
        gpu_completed_at: now,
        total_model_ref_count,
        manifest,
        missing_key_count: 0,
        stale_generation_count: 0,
        wrong_stream_count: 0,
        zero_model_ref_count: 0,
        draw_mismatch_count: 0,
    }
}

#[test]
fn model_witness_request_is_exact_bounded_sorted_and_hashed() {
    let a = SubChunkKey::new(0, 1, 4, 5);
    let b = SubChunkKey::new(0, 2, 4, 5);
    let request = ModelWitnessRequest::try_new(7, [0xab; 32], vec![b, a]).unwrap();
    assert_eq!(request.revision(), 7);
    assert_eq!(request.request_hash(), &[0xab; 32]);
    assert_eq!(request.keys(), &[a, b]);
    assert!(ModelWitnessRequest::try_new(0, [0; 32], vec![a]).is_err());
    assert!(ModelWitnessRequest::try_new(1, [0; 32], Vec::new()).is_err());
    assert!(ModelWitnessRequest::try_new(1, [0; 32], vec![a, a]).is_err());
    assert!(
        ModelWitnessRequest::try_new(
            1,
            [0; 32],
            (0..=MAX_MODEL_WITNESS_KEYS)
                .map(|x| SubChunkKey::new(0, x as i32, 0, 0))
                .collect(),
        )
        .is_err()
    );
}

#[test]
fn model_witness_rejects_missing_stale_wrong_stream_zero_ref_and_draw_mismatch() {
    let key = SubChunkKey::new(0, 1, 4, 5);
    let request = ModelWitnessRequest::try_new(7, [0x11; 32], vec![key]).unwrap();
    let expected = [(key, 9)];

    let missing = evaluate_model_witness_frame(&request, 20, 3, &[], &[], &[]);
    assert_eq!(missing.missing_key_count, 1);
    let stale = evaluate_model_witness_frame(
        &request,
        20,
        3,
        &expected,
        &[(key, 8, ChunkStreamMask::MODEL, 2)],
        &[(key, 8, ChunkStreamMask::MODEL)],
    );
    assert_eq!(stale.stale_generation_count, 1);
    let cube_only = evaluate_model_witness_frame(
        &request,
        20,
        3,
        &expected,
        &[(key, 9, ChunkStreamMask::CUBE, 2)],
        &[(key, 9, ChunkStreamMask::CUBE)],
    );
    assert_eq!(cube_only.wrong_stream_count, 1);
    let zero_ref = evaluate_model_witness_frame(
        &request,
        20,
        3,
        &expected,
        &[(key, 9, ChunkStreamMask::MODEL, 0)],
        &[(key, 9, ChunkStreamMask::MODEL)],
    );
    assert_eq!(zero_ref.zero_model_ref_count, 1);
    let draw_mismatch = evaluate_model_witness_frame(
        &request,
        20,
        3,
        &expected,
        &[(key, 9, ChunkStreamMask::MODEL, 2)],
        &[],
    );
    assert_eq!(draw_mismatch.draw_mismatch_count, 1);
    assert!(!missing.is_exact());
    assert!(!stale.is_exact());
    assert!(!cube_only.is_exact());
    assert!(!zero_ref.is_exact());
    assert!(!draw_mismatch.is_exact());
}

#[test]
fn model_witness_accepts_direct_and_mdi_model_stream_evidence() {
    let key = SubChunkKey::new(0, 1, 4, 5);
    let request = ModelWitnessRequest::try_new(7, [0x22; 32], vec![key]).unwrap();
    let expected = [(key, 9)];
    let allocations = [(key, 9, ChunkStreamMask::CUBE | ChunkStreamMask::MODEL, 3)];
    for drawn in [
        vec![(key, 9, ChunkStreamMask::MODEL)],
        vec![(key, 9, ChunkStreamMask::CUBE | ChunkStreamMask::MODEL)],
    ] {
        let frame = evaluate_model_witness_frame(&request, 20, 3, &expected, &allocations, &drawn);
        assert!(frame.is_exact());
        assert_eq!(frame.total_model_ref_count, 3);
        assert_eq!(frame.manifest.len(), 1);
    }
}

#[test]
fn model_witness_pair_requires_adjacent_identical_gpu_completed_frames() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 1, 4, 5);
    let manifest: Arc<[ModelWitnessManifestRecord]> = Arc::from([ModelWitnessManifestRecord {
        key,
        generation: 9,
        model_ref_count: 3,
    }]);
    let first = exact_model_witness_ack(7, [0x33; 32], 40, 3, Arc::clone(&manifest), now);
    let adjacent = exact_model_witness_ack(7, [0x33; 32], 41, 3, Arc::clone(&manifest), now);
    let skipped = exact_model_witness_ack(7, [0x33; 32], 42, 3, manifest, now);
    assert!(first.forms_stable_exact_pair_with(&adjacent));
    assert!(!first.forms_stable_exact_pair_with(&skipped));
}

fn presented_model_witness_ack(
    request: &ModelWitnessRequest,
    key: SubChunkKey,
    frame_sequence: u64,
    stale_generation_instances: usize,
    unexpected_target_instances: usize,
) -> PresentedFrameAck {
    let now = Instant::now();
    let manifest: Arc<[ModelWitnessManifestRecord]> = Arc::from([ModelWitnessManifestRecord {
        key,
        generation: 9,
        model_ref_count: 3,
    }]);
    PresentedFrameAck {
        cohort: RenderViewCohort::new(key.dimension, [key.x, key.z], 0),
        frame_sequence,
        allocation_manifest: Arc::from([(key, 9)]),
        visible_allocation_manifest: Arc::from([(key, 9)]),
        drawn_manifest: Arc::from([(key, 9)]),
        view_generation: 3,
        render_ready_at: now,
        present_returned_at: now,
        gpu_completed_at: now,
        missing_target_instances: 0,
        unexpected_target_instances,
        source_instances: 0,
        foreign_instances: 0,
        stale_generation_instances,
        orphan_allocations: 0,
        transparent_sort_generation: 0,
        model_witness: Some(exact_model_witness_ack(
            request.revision(),
            *request.request_hash(),
            frame_sequence,
            3,
            manifest,
            now,
        )),
    }
}

fn probed_model_witness_ack(
    request: &ModelWitnessRequest,
    render_ready_at: Instant,
    frame_sequence: u64,
    unrelated_visible: bool,
) -> PresentedFrameAck {
    let requested_key = request.keys()[0];
    let unrelated_key =
        SubChunkKey::new(0, requested_key.y + 1, requested_key.x + 1, requested_key.z);
    let requested_entity = Entity::from_bits(91);
    let unrelated_entity = Entity::from_bits(92);
    let requested_allocation = FrameAllocationIdentity {
        entity: requested_entity,
        key: requested_key,
        generation: 9,
    };
    let unrelated_allocation = FrameAllocationIdentity {
        entity: unrelated_entity,
        key: unrelated_key,
        generation: 4,
    };
    let mut probe = FrameProbe::begin_with_model_witness(
        target_expectation(render_ready_at, [(requested_key, 9)]),
        [
            FrameInstanceIdentity {
                entity: requested_entity,
                key: requested_key,
                generation: 9,
            },
            FrameInstanceIdentity {
                entity: unrelated_entity,
                key: unrelated_key,
                generation: 4,
            },
        ],
        [
            (
                requested_allocation,
                ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                3,
            ),
            (
                unrelated_allocation,
                ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                2,
            ),
        ],
        request.clone(),
    );
    probe.frame_sequence = frame_sequence;
    assert!(probe.record_visible(requested_entity, requested_allocation));
    if unrelated_visible {
        assert!(probe.record_visible(unrelated_entity, unrelated_allocation));
    }
    assert!(probe.record_direct_streams(
        requested_entity,
        requested_allocation,
        ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
    ));
    build_presented_frame_ack(
        probe.complete(),
        FrameCompletionEvidence {
            present_returned_at: Some(
                render_ready_at + std::time::Duration::from_millis(frame_sequence),
            ),
            submitted_work_done_at: Some(
                render_ready_at + std::time::Duration::from_millis(frame_sequence + 1),
            ),
        },
    )
    .unwrap()
}

#[test]
fn exact_model_manifest_pairs_with_unrelated_non_visible_allocation_undrawn() {
    let render_ready_at = Instant::now();
    let key = SubChunkKey::new(0, 65, 65, 65);
    let request = ModelWitnessRequest::try_new(7, [0x43; 32], vec![key]).unwrap();
    let evidence = ModelWitnessEvidence::default();
    evidence.set_authoritative_request(&request);

    for frame_sequence in [40, 41] {
        let acknowledgement =
            probed_model_witness_ack(&request, render_ready_at, frame_sequence, false);
        assert!(acknowledgement.is_exact());
        assert_eq!(acknowledgement.allocation_manifest.len(), 1);
        assert_eq!(acknowledgement.visible_allocation_manifest.len(), 1);
        assert_eq!(
            acknowledgement.visible_allocation_manifest,
            acknowledgement.drawn_manifest
        );
        assert!(
            acknowledgement
                .model_witness
                .as_ref()
                .is_some_and(ModelWitnessFrameAck::is_exact)
        );
        evidence.observe_presented_frame(&request, &acknowledgement);
    }

    let events = evidence.drain_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].consecutive, 1);
    assert_eq!(events[1].consecutive, 2);
    assert!(evidence.is_complete_for(&request));

    let next = ModelWitnessRequest::try_new(8, [0x44; 32], vec![key]).unwrap();
    evidence.set_authoritative_request(&next);
    assert!(!evidence.is_complete_for(&request));
    assert!(!evidence.is_complete_for(&next));
}

#[test]
fn exact_model_manifest_ignores_unrelated_visible_allocation() {
    let render_ready_at = Instant::now();
    let key = SubChunkKey::new(0, 65, 65, 65);
    let request = ModelWitnessRequest::try_new(7, [0x45; 32], vec![key]).unwrap();
    let evidence = ModelWitnessEvidence::default();
    evidence.set_authoritative_request(&request);

    for frame_sequence in [40, 41] {
        let acknowledgement =
            probed_model_witness_ack(&request, render_ready_at, frame_sequence, true);
        assert_eq!(acknowledgement.visible_allocation_manifest.len(), 1);
        assert_eq!(acknowledgement.drawn_manifest.len(), 1);
        assert!(acknowledgement.is_model_witness_compatible());
        assert!(
            acknowledgement
                .model_witness
                .as_ref()
                .is_some_and(ModelWitnessFrameAck::is_exact)
        );
        evidence.observe_presented_frame(&request, &acknowledgement);
    }

    let events = evidence.drain_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].consecutive, 1);
    assert_eq!(events[1].consecutive, 2);
}

#[test]
fn model_frame_probe_scopes_manifests_and_contamination_to_requested_keys() {
    let now = Instant::now();
    let requested_key = SubChunkKey::new(0, 65, 65, 65);
    let unrelated_key = SubChunkKey::new(0, 66, 66, 65);
    let request = ModelWitnessRequest::try_new(7, [0x46; 32], vec![requested_key]).unwrap();
    let requested_entity = Entity::from_bits(93);
    let unrelated_entity = Entity::from_bits(94);
    let requested_allocation = FrameAllocationIdentity {
        entity: requested_entity,
        key: requested_key,
        generation: 9,
    };
    let unrelated_allocation = FrameAllocationIdentity {
        entity: unrelated_entity,
        key: unrelated_key,
        generation: 4,
    };
    let probe = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(requested_key, 9)]),
        [
            FrameInstanceIdentity {
                entity: requested_entity,
                key: requested_key,
                generation: 9,
            },
            FrameInstanceIdentity {
                entity: unrelated_entity,
                key: unrelated_key,
                generation: 4,
            },
        ],
        [
            (
                requested_allocation,
                ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                3,
            ),
            (
                unrelated_allocation,
                ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
                2,
            ),
        ],
        request,
    );

    assert!(probe.record_visible(requested_entity, requested_allocation));
    assert!(probe.record_visible(unrelated_entity, unrelated_allocation));
    assert!(probe.record_direct_streams(
        requested_entity,
        requested_allocation,
        ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
    ));
    assert!(probe.record_direct_streams(
        unrelated_entity,
        unrelated_allocation,
        ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
    ));
    let completed = probe.complete();

    assert_eq!(
        completed.allocation_manifest.as_ref(),
        &[(requested_key, 9)]
    );
    assert_eq!(
        completed.visible_allocation_manifest.as_ref(),
        &[(requested_key, 9)]
    );
    assert_eq!(completed.drawn_manifest.as_ref(), &[(requested_key, 9)]);
    assert_eq!(completed.missing_target_instances, 0);
    assert_eq!(completed.unexpected_target_instances, 0);
    assert_eq!(completed.source_instances, 0);
    assert_eq!(completed.foreign_instances, 0);
    assert_eq!(completed.stale_generation_instances, 0);
    assert_eq!(completed.orphan_allocations, 0);
    assert!(
        completed
            .model_witness
            .is_some_and(|model| model.is_exact())
    );
}

#[test]
fn model_frame_probe_still_rejects_duplicate_and_stale_requested_targets() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 65, 65);
    let request = ModelWitnessRequest::try_new(7, [0x47; 32], vec![key]).unwrap();
    let first = Entity::from_bits(95);
    let duplicate = Entity::from_bits(96);
    let duplicate_probe = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(key, 9)]),
        [
            FrameInstanceIdentity {
                entity: first,
                key,
                generation: 9,
            },
            FrameInstanceIdentity {
                entity: duplicate,
                key,
                generation: 9,
            },
        ],
        [
            (
                FrameAllocationIdentity {
                    entity: first,
                    key,
                    generation: 9,
                },
                ChunkStreamMask::MODEL,
                3,
            ),
            (
                FrameAllocationIdentity {
                    entity: duplicate,
                    key,
                    generation: 9,
                },
                ChunkStreamMask::MODEL,
                3,
            ),
        ],
        request.clone(),
    )
    .complete();
    assert_eq!(duplicate_probe.unexpected_target_instances, 1);

    let stale_entity = Entity::from_bits(97);
    let stale_probe = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(key, 9)]),
        [FrameInstanceIdentity {
            entity: stale_entity,
            key,
            generation: 8,
        }],
        [(
            FrameAllocationIdentity {
                entity: stale_entity,
                key,
                generation: 8,
            },
            ChunkStreamMask::MODEL,
            3,
        )],
        request,
    )
    .complete();
    assert_eq!(stale_probe.stale_generation_instances, 1);
    assert_eq!(stale_probe.missing_target_instances, 1);
    assert_eq!(stale_probe.unexpected_target_instances, 1);
    assert_eq!(
        stale_probe
            .model_witness
            .as_ref()
            .expect("model evaluation must be retained")
            .stale_generation_count,
        1
    );
}

#[test]
fn model_frame_probe_rejects_visible_requested_allocation_missing_an_expected_stream_draw() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 65, 65);
    let request = ModelWitnessRequest::try_new(7, [0x48; 32], vec![key]).unwrap();
    let entity = Entity::from_bits(98);
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 9,
    };
    let mut probe = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(key, 9)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 9,
        }],
        [(
            allocation,
            ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
            3,
        )],
        request,
    );
    probe.frame_sequence = 40;
    assert!(probe.record_visible(entity, allocation));
    assert!(probe.record_direct_streams(entity, allocation, ChunkStreamMask::MODEL));

    let acknowledgement = build_presented_frame_ack(
        probe.complete(),
        FrameCompletionEvidence {
            present_returned_at: Some(now + std::time::Duration::from_millis(1)),
            submitted_work_done_at: Some(now + std::time::Duration::from_millis(2)),
        },
    )
    .expect("post-present GPU completion should publish model frame evidence");

    assert_eq!(
        acknowledgement.visible_allocation_manifest.as_ref(),
        &[(key, 9)]
    );
    assert!(acknowledgement.drawn_manifest.is_empty());
    assert!(
        acknowledgement
            .model_witness
            .as_ref()
            .is_some_and(ModelWitnessFrameAck::is_exact),
        "the requested model stream itself was drawn exactly"
    );
    assert!(
        !acknowledgement.is_model_witness_compatible(),
        "a visible requested allocation missing its expected cube draw passed outer evidence"
    );
}

#[test]
fn model_frame_probe_counts_only_requested_orphan_allocations() {
    let now = Instant::now();
    let requested_key = SubChunkKey::new(0, 65, 65, 65);
    let unrelated_key = SubChunkKey::new(0, 66, 66, 65);
    let request = ModelWitnessRequest::try_new(7, [0x49; 32], vec![requested_key]).unwrap();
    let probe = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(requested_key, 9)]),
        std::iter::empty::<FrameInstanceIdentity>(),
        [
            (
                FrameAllocationIdentity {
                    entity: Entity::from_bits(99),
                    key: requested_key,
                    generation: 9,
                },
                ChunkStreamMask::MODEL,
                3,
            ),
            (
                FrameAllocationIdentity {
                    entity: Entity::from_bits(100),
                    key: unrelated_key,
                    generation: 4,
                },
                ChunkStreamMask::MODEL,
                2,
            ),
        ],
        request,
    );
    let acknowledgement = build_presented_frame_ack(
        probe.complete(),
        FrameCompletionEvidence {
            present_returned_at: Some(now + std::time::Duration::from_millis(1)),
            submitted_work_done_at: Some(now + std::time::Duration::from_millis(2)),
        },
    )
    .expect("post-present GPU completion should publish orphan diagnostics");

    assert_eq!(acknowledgement.orphan_allocations, 1);
    assert!(
        !acknowledgement.is_model_witness_compatible(),
        "a requested orphan allocation passed outer model evidence"
    );
}

#[test]
fn model_witness_outer_compatibility_rejects_every_contamination_counter() {
    let key = SubChunkKey::new(0, 1, 4, 5);
    let request = ModelWitnessRequest::try_new(7, [0x44; 32], vec![key]).unwrap();
    let clean = presented_model_witness_ack(&request, key, 40, 0, 0);
    assert!(clean.is_model_witness_compatible());

    let contaminate: [fn(&mut PresentedFrameAck); 6] = [
        |acknowledgement| acknowledgement.missing_target_instances = 1,
        |acknowledgement| acknowledgement.unexpected_target_instances = 1,
        |acknowledgement| acknowledgement.source_instances = 1,
        |acknowledgement| acknowledgement.foreign_instances = 1,
        |acknowledgement| acknowledgement.stale_generation_instances = 1,
        |acknowledgement| acknowledgement.orphan_allocations = 1,
    ];
    for contaminate in contaminate {
        let mut acknowledgement = clean.clone();
        contaminate(&mut acknowledgement);
        assert!(!acknowledgement.is_model_witness_compatible());
    }
}

#[test]
fn exact_model_manifest_cannot_pair_across_stale_outer_frame_contamination() {
    let key = SubChunkKey::new(0, 1, 4, 5);
    let request = ModelWitnessRequest::try_new(7, [0x44; 32], vec![key]).unwrap();
    let evidence = ModelWitnessEvidence::default();
    evidence.set_authoritative_request(&request);

    evidence.observe_presented_frame(
        &request,
        &presented_model_witness_ack(&request, key, 40, 0, 0),
    );
    evidence.observe_presented_frame(
        &request,
        &presented_model_witness_ack(&request, key, 41, 1, 0),
    );
    evidence.observe_presented_frame(
        &request,
        &presented_model_witness_ack(&request, key, 42, 0, 0),
    );

    assert!(evidence.drain_events().is_empty());
}

#[test]
fn exact_model_manifest_cannot_pair_across_duplicate_outer_frame_contamination() {
    let key = SubChunkKey::new(0, 1, 4, 5);
    let request = ModelWitnessRequest::try_new(7, [0x55; 32], vec![key]).unwrap();
    let evidence = ModelWitnessEvidence::default();
    evidence.set_authoritative_request(&request);

    evidence.observe_presented_frame(
        &request,
        &presented_model_witness_ack(&request, key, 50, 0, 0),
    );
    evidence.observe_presented_frame(
        &request,
        &presented_model_witness_ack(&request, key, 51, 0, 1),
    );
    evidence.observe_presented_frame(
        &request,
        &presented_model_witness_ack(&request, key, 52, 0, 0),
    );

    assert!(evidence.drain_events().is_empty());
}

#[test]
fn model_witness_uses_actual_direct_and_mdi_frame_probe_recording_paths() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 4, 65);
    let entity = Entity::from_bits(91);
    let instance = FrameInstanceIdentity {
        entity,
        key,
        generation: 9,
    };
    let allocation = FrameAllocationIdentity {
        entity,
        key,
        generation: 9,
    };
    let request = ModelWitnessRequest::try_new(7, [0x66; 32], vec![key]).unwrap();

    let direct = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(key, 9)]),
        [instance],
        [(
            allocation,
            ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
            3,
        )],
        request.clone(),
    );
    assert!(direct.record_direct_streams(entity, allocation, ChunkStreamMask::MODEL));
    let direct = direct.complete().model_witness.unwrap();
    assert!(direct.is_exact());
    assert_eq!(direct.total_model_ref_count, 3);

    let mdi = FrameProbe::begin_with_model_witness(
        target_expectation(now, [(key, 9)]),
        [instance],
        [(
            allocation,
            ChunkStreamMask::CUBE | ChunkStreamMask::MODEL,
            3,
        )],
        request,
    );
    assert_eq!(
        mdi.record_mdi_streams([(entity, allocation)], ChunkStreamMask::MODEL),
        1
    );
    let mdi = mdi.complete().model_witness.unwrap();
    assert!(mdi.is_exact());
    assert_eq!(mdi.manifest, direct.manifest);
}

#[test]
fn depth_liquid_direct_and_mdi_draws_share_exact_addresses() {
    let allocation = GpuChunkAllocation {
        key: SubChunkKey::new(0, 1, 2, 3),
        generation: 4,
        tint_identity: ChunkBiomeTintIdentity::default(),
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: None,
        model_lighting_range: None,
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: Some(40..64),
        liquid_lighting_range: Some(64..76),
        has_depth_liquid: true,
        has_transparent_liquid: true,
        depth_liquid_range: Some(10..16),
        metadata_index: 7,
    };
    let direct = depth_liquid_direct_draw_command(&allocation).unwrap();
    let mdi = depth_liquid_mdi_draw_command(&allocation).unwrap();
    assert_eq!(direct.index_count, mdi.index_count);
    assert_eq!(direct.instance_count, mdi.instance_count);
    assert_eq!(direct.first_index, mdi.first_index);
    assert_eq!(direct.base_vertex, mdi.base_vertex);
    assert_eq!(direct.first_instance, mdi.first_instance);
    assert_eq!(direct.index_count, 6);
    assert_eq!(direct.instance_count, 6);
    assert_eq!(direct.base_vertex, 28);
    assert_eq!(direct.first_instance, 10);

    let mut water_only = allocation;
    water_only.has_depth_liquid = false;
    assert!(depth_liquid_direct_draw_command(&water_only).is_none());
    assert!(depth_liquid_mdi_draw_command(&water_only).is_none());
}
