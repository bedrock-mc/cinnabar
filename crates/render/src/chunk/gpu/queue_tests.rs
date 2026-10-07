use super::*;

#[test]
fn adjacent_quad_frees_coalesce_and_reuse_the_lowest_range_under_churn() {
    let mut free = Vec::new();

    insert_free_quad_range(&mut free, 12..16);
    insert_free_quad_range(&mut free, 0..4);
    insert_free_quad_range(&mut free, 8..12);
    insert_free_quad_range(&mut free, 4..8);
    assert_eq!(free.len(), 1);
    assert_eq!(free[0], 0..16);

    assert_eq!(take_free_quad_range(&mut free, 3), Some(0));
    assert_eq!(take_free_quad_range(&mut free, 5), Some(3));
    assert_eq!(free.len(), 1);
    assert_eq!(free[0], 8..16);

    insert_free_quad_range(&mut free, 0..3);
    insert_free_quad_range(&mut free, 3..8);
    assert_eq!(free.len(), 1);
    assert_eq!(free[0], 0..16);
    assert_eq!(take_free_quad_range(&mut free, 16), Some(0));
    assert!(free.is_empty());
}

#[test]
fn bind_group_cache_rebuilds_only_when_a_buffer_identity_changes() {
    let cached = [11_u64, 12, 13];

    assert!(!bind_group_needs_rebuild(true, Some(&cached), &cached));
    assert!(bind_group_needs_rebuild(false, Some(&cached), &cached));
    assert!(bind_group_needs_rebuild(true, None, &cached));
    assert!(bind_group_needs_rebuild(true, Some(&cached), &[11, 99, 13],));
    assert!(bind_group_needs_rebuild(true, Some(&cached), &[11, 12, 99],));
}

#[test]
fn gpu_growth_plan_copies_the_old_allocation_without_a_host_shadow_upload() {
    let growth = plan_arena_growth(8, 9, PACKED_QUAD_BYTES, 16)
        .unwrap()
        .unwrap();
    assert_eq!(growth.new_capacity, 16);
    assert_eq!(growth.gpu_copy_bytes, 64);

    let stats = account_chunk_gpu_uploads(
        ChunkUploadBudget::new(2, u64::MAX),
        2,
        72,
        growth.gpu_copy_bytes,
    );

    assert_eq!(stats.chunk_updates, 2);
    assert_eq!(stats.chunk_budget, 2);
    assert_eq!(stats.incremental_bytes, 72);
    assert_eq!(stats.gpu_copy_bytes, 64);
    assert_eq!(stats.full_shadow_bytes, 0);
    assert_eq!(stats.total_bytes, 136);
}

#[test]
fn migration_copy_bytes_count_against_the_literal_frame_byte_cap() {
    let budget = ChunkUploadBudget::new(1, 103);
    let mut reservation = GpuUploadReservation {
        growth_copy_bytes: 64,
        ..GpuUploadReservation::default()
    };

    assert!(!reservation.try_reserve(budget, 40));
    assert!(reservation.try_reserve(budget, 39));
}

#[test]
fn migration_allowance_never_exceeds_the_frame_ceiling_or_slice() {
    let frame = PublicationServiceConfig::PHASE2_GATE.maximum_frame_bytes;
    assert_eq!(
        GpuUploadReservation::default().migration_allowance(),
        ARENA_MIGRATION_FRAME_BYTES
    );
    let nearly_full = GpuUploadReservation {
        items: 1,
        incremental_bytes: frame - 12,
        growth_copy_bytes: 0,
    };
    assert_eq!(nearly_full.migration_allowance(), 12);
    let spent = GpuUploadReservation {
        growth_copy_bytes: ARENA_MIGRATION_FRAME_BYTES,
        ..GpuUploadReservation::default()
    };
    assert_eq!(spent.migration_allowance(), 0);
}

#[test]
fn first_arena_growth_names_the_first_short_stream_and_rejects_adapter_overflow() {
    let capacities = ArenaRequiredLengths {
        quads: 8,
        geometry_stream_words: 16,
        origins: 4,
        biome_words: 8,
    };
    let limits = ArenaLimits {
        max_quad_items: 32,
        max_geometry_stream_words: 32,
        max_origin_items: 32,
        max_biome_words: 32,
    };
    let fits = capacities;
    assert_eq!(first_arena_growth(capacities, fits, limits), Ok(None));
    let geometry_short = ArenaRequiredLengths {
        geometry_stream_words: 17,
        origins: 5,
        ..capacities
    };
    assert_eq!(
        first_arena_growth(capacities, geometry_short, limits),
        Ok(Some((
            ArenaStream::GeometryStreams,
            ArenaGrowthPlan {
                new_capacity: 32,
                gpu_copy_bytes: 16 * GEOMETRY_STREAM_WORD_BYTES,
            }
        )))
    );
    let overflow = ArenaRequiredLengths {
        biome_words: 33,
        ..geometry_short
    };
    assert_eq!(
        first_arena_growth(capacities, overflow, limits),
        Err(ArenaGrowthError)
    );
}

#[test]
fn render_world_update_plan_is_capped_before_arena_mutation() {
    let mut world = World::new();
    let candidates = (0..5)
        .map(|index| GpuUpdateCandidate {
            entity: world.spawn_empty().id(),
            key: SubChunkKey::new(0, index, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: ChunkUploadPriority::new(0.0),
        })
        .collect::<Vec<_>>();
    let allocations = HashMap::new();

    let selected = plan_gpu_chunk_updates(
        candidates,
        &allocations,
        Vec3::ZERO,
        ChunkBiomeTintIdentity::default(),
        &GpuUpdateFairness::default(),
    );

    assert_eq!(selected.into_iter().take(2).count(), 2);
    assert!(allocations.is_empty());
}

#[test]
fn upload_budget_has_a_hard_byte_cap_as_well_as_an_item_cap() {
    let budget = ChunkUploadBudget::new(4, 1_024);

    assert!(budget.can_fit(0, 0, 1, 1_024));
    assert!(!budget.can_fit(0, 0, 1, 1_025));
    assert!(!budget.can_fit(4, 0, 1, 1));
}

#[test]
fn public_upload_estimate_matches_the_bounded_queue_accounting() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let mesh = solid_test_mesh();
    let biome = PackedBiomeRecord::fallback();
    let expected = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let mut queue = ChunkRenderQueue::default();

    queue
        .try_insert_with_biome(key, mesh, biome, ChunkUploadPriority::new(0.0))
        .unwrap();

    assert_eq!(queue.pending_bytes(), expected);
}

#[test]
fn failing_candidates_do_not_starve_a_later_fitting_candidate() {
    let mut world = World::new();
    let failing = world.spawn_empty().id();
    let fitting = world.spawn_empty().id();
    let candidates = vec![
        GpuUpdateCandidate {
            entity: failing,
            key: SubChunkKey::new(0, -10, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: ChunkUploadPriority::new(0.0),
        },
        GpuUpdateCandidate {
            entity: fitting,
            key: SubChunkKey::new(0, 10, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: ChunkUploadPriority::new(0.0),
        },
    ];
    let selected = plan_gpu_chunk_updates(
        candidates,
        &HashMap::new(),
        Vec3::ZERO,
        ChunkBiomeTintIdentity::default(),
        &GpuUpdateFairness::default(),
    );
    let mut len = 2;
    let mut free = std::iter::once(0..2).collect::<Vec<_>>();
    let successful = selected
        .into_iter()
        .filter(|entity| {
            let required = if *entity == failing { 3 } else { 2 };
            allocate_quad_range(&mut len, &mut free, required, 2).is_some()
        })
        .collect::<Vec<_>>();

    assert_eq!(successful, [fitting]);
}

#[test]
fn recovery_planner_prefers_near_high_key_over_far_low_key() {
    let mut world = World::new();
    let far = world.spawn_empty().id();
    let near = world.spawn_empty().id();
    let far_key = SubChunkKey::new(0, -100, 0, 0);
    let near_key = SubChunkKey::new(0, 100, 0, 0);
    let candidates = vec![
        GpuUpdateCandidate {
            entity: far,
            key: far_key,
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: ChunkUploadPriority::new(0.0),
        },
        GpuUpdateCandidate {
            entity: near,
            key: near_key,
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: ChunkUploadPriority::new(0.0),
        },
    ];

    let selected = plan_gpu_chunk_updates(
        candidates,
        &HashMap::new(),
        Vec3::new(1_608.0, 8.0, 8.0),
        ChunkBiomeTintIdentity::default(),
        &GpuUpdateFairness::default(),
    );

    assert_eq!(selected[0], near);
    assert!(
        ChunkUploadPriority::from_camera(near_key, Vec3::new(1_608.0, 8.0, 8.0))
            < ChunkUploadPriority::from_camera(far_key, Vec3::new(1_608.0, 8.0, 8.0))
    );
}

#[test]
fn recurring_near_replacements_do_not_starve_an_older_far_gpu_update() {
    let mut world = World::new();
    let near = world.spawn_empty().id();
    let far = world.spawn_empty().id();
    let tint = ChunkBiomeTintIdentity::default();
    let near_key = SubChunkKey::new(0, 0, 4, 0);
    let far_key = SubChunkKey::new(0, 0, 4, 5);
    let mut allocations = HashMap::new();
    let mut fairness = GpuUpdateFairness::default();
    for (entity, key) in [(near, near_key), (far, far_key)] {
        let mut allocation = retirement_test_allocation();
        allocation.generation = 0;
        allocation.tint_identity = tint;
        allocation.gpu.key = key;
        allocation.gpu.generation = 0;
        allocation.gpu.tint_identity = tint;
        allocations.insert(entity, allocation);
    }

    let mut far_selected = false;
    for near_generation in 1..=8 {
        let candidates = vec![
            GpuUpdateCandidate {
                entity: near,
                key: near_key,
                generation: near_generation,
                tint_identity: tint,
                priority: ChunkUploadPriority::new(0.0),
            },
            GpuUpdateCandidate {
                entity: far,
                key: far_key,
                generation: 1,
                tint_identity: tint,
                priority: ChunkUploadPriority::new(0.0),
            },
        ];
        let selected =
            plan_gpu_chunk_updates(candidates, &allocations, Vec3::ZERO, tint, &fairness);
        let chosen = selected[0];
        fairness.finish_frame(&selected, &[chosen], &[]);
        if chosen == far {
            far_selected = true;
            break;
        }
        allocations.get_mut(&near).unwrap().generation = near_generation;
    }

    assert!(
        far_selected,
        "recurring nearer remeshes consumed every one-item frame budget"
    );
}

#[test]
fn gpu_update_fairness_is_bounded_prunes_inactive_and_clears_success_or_reset() {
    let mut world = World::new();
    let a = world.spawn_empty().id();
    let b = world.spawn_empty().id();
    let c = world.spawn_empty().id();
    let mut fairness = GpuUpdateFairness::with_limit(2);

    fairness.finish_frame(&[a, b, c], &[], &[]);
    assert_eq!(fairness.len(), 2);
    assert_eq!(fairness.wait_age(a), 1);
    assert_eq!(fairness.wait_age(b), 1);
    assert_eq!(fairness.wait_age(c), 0);

    fairness.finish_frame(&[b, c], &[], &[]);
    assert_eq!(fairness.len(), 2);
    assert_eq!(fairness.wait_age(a), 0);
    assert_eq!(fairness.wait_age(b), 2);
    assert_eq!(fairness.wait_age(c), 1);

    fairness.finish_frame(&[b, c], &[b], &[]);
    assert_eq!(fairness.wait_age(b), 0);
    assert_eq!(fairness.wait_age(c), 2);
    fairness.reset();
    assert!(fairness.is_empty());

    for _ in 0..70_000 {
        fairness.finish_frame(&[c], &[], &[]);
    }
    assert_eq!(fairness.wait_age(c), 70_000);
}

#[test]
fn tracked_empty_mesh_acknowledges_only_after_bounded_application() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let token = ChunkUploadToken {
        generation: 7,
        dirty_since: Instant::now(),
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(1));
    let acknowledgements = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .clone();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked(
            key,
            ChunkMesh::default(),
            ChunkUploadPriority::new(0.0),
            token,
        )
        .unwrap();

    assert!(acknowledgements.drain().is_empty());
    app.update();
    let applied = acknowledgements.drain();

    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].key, key);
    assert_eq!(applied[0].token, token);
}

#[test]
fn one_upload_budget_still_applies_later_zero_byte_changes() {
    let existing_key = SubChunkKey::new(0, 10, 0, 0);
    let first_upload_key = SubChunkKey::new(0, 11, 0, 0);
    let deferred_upload_key = SubChunkKey::new(0, 12, 0, 0);
    let empty_key = SubChunkKey::new(0, 13, 0, 0);
    let now = Instant::now();
    let first_upload_token = ChunkUploadToken {
        generation: 1,
        dirty_since: now,
    };
    let deferred_upload_token = ChunkUploadToken {
        generation: 2,
        dirty_since: now,
    };
    let removal_token = ChunkUploadToken {
        generation: 3,
        dirty_since: now,
    };
    let empty_token = ChunkUploadToken {
        generation: 4,
        dirty_since: now,
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(1));
    let acknowledgements = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .clone();

    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(
            existing_key,
            solid_test_mesh(),
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    app.update();
    assert!(acknowledgements.drain().is_empty());

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_update_tracked(
                first_upload_key,
                solid_test_mesh(),
                ChunkUploadPriority::new(0.0),
                first_upload_token,
            )
            .unwrap();
        queue
            .try_update_tracked(
                deferred_upload_key,
                solid_test_mesh(),
                ChunkUploadPriority::new(1.0),
                deferred_upload_token,
            )
            .unwrap();
        queue
            .try_remove_tracked(existing_key, ChunkUploadPriority::new(2.0), removal_token)
            .unwrap();
        queue
            .try_update_tracked(
                empty_key,
                ChunkMesh::default(),
                ChunkUploadPriority::new(3.0),
                empty_token,
            )
            .unwrap();
    }

    app.update();

    let applied = acknowledgements
        .drain()
        .into_iter()
        .map(|acknowledgement| {
            assert_eq!(acknowledgement.uploaded_bytes, 0);
            (acknowledgement.key, acknowledgement.token)
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        applied,
        BTreeMap::from([(existing_key, removal_token), (empty_key, empty_token)])
    );
    assert_eq!(
        app.world().resource::<ChunkRenderQueue>().retained_len(),
        1,
        "the second non-empty upload must retain its place for a later frame"
    );
    let world = app.world_mut();
    let mut instances = world.query::<&ChunkRenderInstance>();
    let rendered = instances
        .iter(world)
        .map(ChunkRenderInstance::key)
        .collect::<Vec<_>>();
    assert_eq!(rendered, [first_upload_key]);
}

#[test]
fn zero_byte_applications_never_exceed_the_retained_queue_hard_cap() {
    let total = DEFAULT_RENDER_QUEUE_ITEMS + 44;
    let maximum_zero_byte_operations =
        PublicationServiceConfig::PHASE2_GATE.maximum_zero_byte_operations_per_frame;
    let now = Instant::now();
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: total,
            max_bytes: DEFAULT_RENDER_QUEUE_BYTES,
        }))
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::new(1));

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        for index in 0..total {
            queue
                .try_remove_tracked(
                    SubChunkKey::new(0, index as i32, 0, 0),
                    ChunkUploadPriority::new(index as f32),
                    ChunkUploadToken {
                        generation: index as u64 + 1,
                        dirty_since: now,
                    },
                )
                .unwrap();
        }
    }

    app.update();

    let applied = acknowledgements
        .drain()
        .into_iter()
        .map(|acknowledgement| (acknowledgement.key, acknowledgement.token))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(applied.len(), maximum_zero_byte_operations);
    for index in 0..maximum_zero_byte_operations {
        assert_eq!(
            applied.get(&SubChunkKey::new(0, index as i32, 0, 0)),
            Some(&ChunkUploadToken {
                generation: index as u64 + 1,
                dirty_since: now,
            })
        );
    }
    assert_eq!(
        app.world().resource::<ChunkRenderQueue>().retained_len(),
        total - maximum_zero_byte_operations
    );
}

#[test]
fn acknowledgement_surface_is_bounded_and_coalesces_same_key() {
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let now = Instant::now();
    let repeated = SubChunkKey::new(0, 0, 0, 0);
    for generation in 1..=2 {
        acknowledgements.record(ChunkUploadAcknowledgement {
            key: repeated,
            token: ChunkUploadToken {
                generation,
                dirty_since: now,
            },
            applied_at: now,
            uploaded_bytes: 0,
        });
    }
    for index in 1..=DEFAULT_RENDER_QUEUE_ITEMS {
        acknowledgements.record(ChunkUploadAcknowledgement {
            key: SubChunkKey::new(0, index as i32, 0, 0),
            token: ChunkUploadToken {
                generation: 1,
                dirty_since: now,
            },
            applied_at: now,
            uploaded_bytes: 0,
        });
    }

    let pending = acknowledgements.drain();

    assert!(pending.len() <= DEFAULT_RENDER_QUEUE_ITEMS);
    assert_eq!(
        pending
            .iter()
            .filter(|acknowledgement| acknowledgement.key == repeated)
            .count(),
        1
    );
    assert_eq!(
        pending
            .iter()
            .find(|acknowledgement| acknowledgement.key == repeated)
            .unwrap()
            .token
            .generation,
        2
    );
}

#[test]
fn acknowledgement_reservation_defers_when_full_and_retries_after_drain() {
    let acknowledgements = ChunkUploadAcknowledgements::with_capacity(1);
    let first = SubChunkKey::new(0, 1, 0, 0);
    let second = SubChunkKey::new(0, 2, 0, 0);
    let now = Instant::now();
    let first_token = ChunkUploadToken {
        generation: 1,
        dirty_since: now,
    };
    let second_token = ChunkUploadToken {
        generation: 2,
        dirty_since: now,
    };

    assert!(acknowledgements.is_empty());
    assert!(acknowledgements.try_reserve(first, first_token));
    assert!(!acknowledgements.is_empty());
    assert!(!acknowledgements.try_reserve(second, second_token));
    assert!(!acknowledgements.complete(first, second_token, now));
    assert!(acknowledgements.complete(first, first_token, now));
    assert_eq!(acknowledgements.drain().len(), 1);
    assert!(acknowledgements.is_empty());
    assert!(acknowledgements.try_reserve(second, second_token));
}

#[test]
fn adapter_failure_releases_capacity_for_later_fitting_extracted_instance() {
    fn encode_zig_zag_i32(value: i32) -> Vec<u8> {
        let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
        let mut encoded = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            encoded.push(byte);
            if value == 0 {
                return encoded;
            }
        }
    }

    fn solid_sub_chunk(runtime_id: u32) -> world::SubChunk {
        let mut encoded = vec![9, 1, 0, 1];
        encoded.extend(encode_zig_zag_i32(runtime_id as i32));
        world::SubChunk::decode(&encoded, &world::RawBlockIds { air: 0 })
    }

    let impossible_key = SubChunkKey::new(0, 0, 0, 0);
    let fitting_key = SubChunkKey::new(0, 10, 0, 0);
    let now = Instant::now();
    let impossible_token = ChunkUploadToken {
        generation: 1,
        dirty_since: now,
    };
    let fitting_token = ChunkUploadToken {
        generation: 2,
        dirty_since: now,
    };
    let solid = solid_sub_chunk(1);
    let classifier = meshing::BlockClassifier::new(0);
    let impossible_mesh = meshing::mesh_sub_chunk(
        &classifier,
        opaque_runtime_assets(),
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &solid,
    );
    let fitting_mesh = meshing::mesh_sub_chunk(
        &classifier,
        opaque_runtime_assets(),
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty()
            .with_negative_x(&solid)
            .with_positive_x(&solid)
            .with_negative_y(&solid)
            .with_positive_y(&solid)
            .with_negative_z(&solid),
        &solid,
    );
    assert_eq!(impossible_mesh.quad_count(), 6);
    assert_eq!(fitting_mesh.quad_count(), 1);

    let acknowledgements = ChunkUploadAcknowledgements::with_capacity(1);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::new(2));
    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_update_tracked(
                impossible_key,
                impossible_mesh,
                ChunkUploadPriority::new(0.0),
                impossible_token,
            )
            .unwrap();
        queue
            .try_update_tracked(
                fitting_key,
                fitting_mesh,
                ChunkUploadPriority::new(1.0),
                fitting_token,
            )
            .unwrap();
    }
    app.update();

    let extracted = app
        .world_mut()
        .query::<(Entity, &ChunkRenderInstance)>()
        .iter(app.world())
        .map(|(entity, instance)| (entity, instance.clone()))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        extracted.len(),
        2,
        "acknowledgement capacity must not block main-to-render extraction"
    );

    let candidates = extracted
        .iter()
        .map(|(&entity, instance)| GpuUpdateCandidate {
            entity,
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
            priority: instance.priority,
        })
        .collect::<Vec<_>>();
    let selected = plan_gpu_chunk_updates(
        candidates,
        &HashMap::new(),
        Vec3::ZERO,
        ChunkBiomeTintIdentity::default(),
        &GpuUpdateFairness::default(),
    );
    let mut quad_len = 0;
    let mut free_quads = Vec::new();
    let mut failed = Vec::new();
    let mut successful = Vec::new();
    for entity in selected {
        let instance = &extracted[&entity];
        let required = u32::try_from(instance.quads().len()).unwrap();
        let token = instance.token.expect("tracked upload token");
        assert!(acknowledgements.try_reserve(instance.key, token));
        if allocate_quad_range(&mut quad_len, &mut free_quads, required, 5).is_none() {
            assert!(acknowledgements.cancel(instance.key, token));
            failed.push(instance.key);
            continue;
        }
        let uploaded_bytes = buffer_byte_len(instance.quads().len(), PACKED_QUAD_BYTES)
            .saturating_add(CHUNK_ORIGIN_BYTES);
        assert!(acknowledgements.complete_with_bytes(instance.key, token, now, uploaded_bytes,));
        successful.push(instance.key);
    }

    assert_eq!(failed, [impossible_key]);
    assert_eq!(successful, [fitting_key]);
    let applied = acknowledgements.drain();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].key, fitting_key);
    assert_eq!(applied[0].token, fitting_token);
    assert_eq!(
        applied[0].uploaded_bytes,
        PACKED_QUAD_BYTES + CHUNK_ORIGIN_BYTES
    );
    assert!(
        extracted
            .values()
            .any(|instance| instance.key == impossible_key)
    );
}

#[test]
fn same_key_ready_supersession_preserves_bytes_and_latest_token() {
    let acknowledgements = ChunkUploadAcknowledgements::with_capacity(1);
    let key = SubChunkKey::new(0, 1, 2, 3);
    let now = Instant::now();
    let first = ChunkUploadToken {
        generation: 1,
        dirty_since: now,
    };
    let latest = ChunkUploadToken {
        generation: 2,
        dirty_since: now,
    };

    assert!(acknowledgements.try_reserve(key, first));
    assert!(acknowledgements.complete_with_bytes(key, first, now, 40));
    assert!(acknowledgements.try_reserve(key, latest));
    assert!(acknowledgements.complete_with_bytes(key, latest, now, 24));

    let drained = acknowledgements.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].key, key);
    assert_eq!(drained[0].token, latest);
    assert_eq!(drained[0].uploaded_bytes, 64);
    assert!(acknowledgements.drain().is_empty());
}

#[test]
fn arena_growth_clamps_to_adapter_limits_and_rejects_one_past() {
    let limits = arena_limits_from_device_limits(64, 32);
    assert_eq!(limits.max_quad_items, 4);
    assert_eq!(limits.max_geometry_stream_words, 8);
    assert_eq!(limits.max_origin_items, 1);
    assert_eq!(limits.max_biome_words, 8);

    assert_eq!(
        plan_arena_growth(1, 4, PACKED_QUAD_BYTES, 4).unwrap(),
        Some(ArenaGrowthPlan {
            new_capacity: 4,
            gpu_copy_bytes: 8,
        })
    );
    assert_eq!(
        plan_arena_growth(1, 3, PACKED_QUAD_BYTES, 3).unwrap(),
        Some(ArenaGrowthPlan {
            new_capacity: 3,
            gpu_copy_bytes: 8,
        })
    );
    assert!(plan_arena_growth(1, 5, PACKED_QUAD_BYTES, 4).is_err());
}

#[test]
fn quad_allocator_reuses_and_trims_high_water_without_a_cpu_shadow() {
    let mut len = 0;
    let mut free = Vec::new();
    let first = allocate_quad_range(&mut len, &mut free, 4, 16).unwrap();
    let second = allocate_quad_range(&mut len, &mut free, 6, 16).unwrap();
    assert_eq!((first, second, len), (0, 4, 10));

    release_quad_range(&mut len, &mut free, 0..4);
    assert_eq!(len, 10);
    assert_eq!(free.len(), 1);
    assert_eq!(free[0], 0..4);
    release_quad_range(&mut len, &mut free, 4..10);
    assert_eq!(len, 0);
    assert!(free.is_empty());
    assert_eq!(allocate_quad_range(&mut len, &mut free, 16, 16), Some(0));
    assert_eq!(allocate_quad_range(&mut len, &mut free, 1, 16), None);
}

#[test]
fn biome_range_planning_reserves_zero_and_rolls_back_as_one_transaction() {
    let limits = ArenaLimits {
        max_quad_items: 8,
        max_geometry_stream_words: 8,
        max_origin_items: 8,
        max_biome_words: FALLBACK_BIOME_WORDS + 8,
    };
    let plan = |quad_len, biome_len, quad_required, biome_required, limits| {
        plan_chunk_range_update(
            quad_len,
            &[],
            0,
            &[],
            biome_len,
            &[],
            GeometryStreamCounts {
                cube: quad_required,
                ..Default::default()
            },
            biome_required,
            None,
            false,
            limits,
        )
    };
    let fallback = plan(0, FALLBACK_BIOME_WORDS, 1, 0, limits).unwrap();
    assert_eq!(fallback.biome_start, 0);
    assert_eq!(fallback.biome_capacity, 0);
    assert_eq!(fallback.biome_len, FALLBACK_BIOME_WORDS);

    let real = plan(0, FALLBACK_BIOME_WORDS, 1, 2, limits).unwrap();
    assert_eq!(real.biome_start, FALLBACK_BIOME_WORDS as u32);
    assert_eq!(real.biome_len, FALLBACK_BIOME_WORDS + 2);

    assert!(
        plan(
            4,
            FALLBACK_BIOME_WORDS,
            1,
            1,
            ArenaLimits {
                max_quad_items: 8,
                max_geometry_stream_words: 8,
                max_origin_items: 8,
                max_biome_words: FALLBACK_BIOME_WORDS,
            },
        )
        .is_none(),
        "a successful temporary quad allocation must not escape when biome allocation fails"
    );

    let mut len = real.biome_len;
    let mut free = real.free_biomes;
    release_quad_range(
        &mut len,
        &mut free,
        real.biome_start..real.biome_start + real.biome_capacity,
    );
    assert_eq!(len, FALLBACK_BIOME_WORDS);
    assert!(free.is_empty());
}

#[derive(Component)]
struct RemovalProbe;

#[derive(Resource, Default)]
struct RemovalDeltas(Vec<Entity>);

fn record_removal_deltas(
    mut removed: RemovedComponents<RemovalProbe>,
    mut deltas: ResMut<RemovalDeltas>,
) {
    deltas.0.extend(removed.read());
}

#[test]
fn removed_components_are_reported_once_without_a_presence_scan() {
    let mut app = App::new();
    app.init_resource::<RemovalDeltas>()
        .add_systems(Update, record_removal_deltas);
    let retained = app.world_mut().spawn(RemovalProbe).id();
    let removed = app.world_mut().spawn(RemovalProbe).id();
    let despawned = app.world_mut().spawn(RemovalProbe).id();

    app.update();
    assert!(app.world().resource::<RemovalDeltas>().0.is_empty());

    app.world_mut().entity_mut(removed).remove::<RemovalProbe>();
    app.world_mut().entity_mut(despawned).despawn();
    app.update();
    let mut actual = app.world().resource::<RemovalDeltas>().0.clone();
    actual.sort_unstable();
    let mut expected = vec![removed, despawned];
    expected.sort_unstable();
    assert_eq!(actual, expected);
    assert!(app.world().get::<RemovalProbe>(retained).is_some());

    app.update();
    let mut actual = app.world().resource::<RemovalDeltas>().0.clone();
    actual.sort_unstable();
    assert_eq!(actual, expected);
}

/// A tracked change older than the key's newest one was reordered upstream and must not win.
#[test]
fn an_older_tracked_change_never_replaces_a_newer_one() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let now = Instant::now();
    let token = |generation| ChunkUploadToken {
        generation,
        dirty_since: now,
    };
    let urgent = ChunkUploadPriority::urgent();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ChunkRenderPlugin::new(1));

    let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
    queue
        .try_update_tracked(key, solid_test_mesh(), urgent, token(5))
        .unwrap();
    queue.try_remove_tracked(key, urgent, token(3)).unwrap();
    assert_eq!(queue.pending[&key].generation, 5);
    assert!(queue.removals.is_empty());
    app.update();

    let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
    assert_eq!(queue.render_manifest.get(&key), Some(&5));
    queue.try_remove_tracked(key, urgent, token(4)).unwrap();
    queue
        .try_update_tracked(key, ChunkMesh::default(), urgent, token(4))
        .unwrap();
    assert_eq!(queue.retained_len(), 0, "stale changes are dropped");
    assert_eq!(queue.render_manifest.get(&key), Some(&5));

    queue.try_remove_tracked(key, urgent, token(6)).unwrap();
    assert!(queue.removals.contains_key(&key));
    queue.reset_session();
    queue
        .try_update_tracked(key, solid_test_mesh(), urgent, token(1))
        .unwrap();
    assert_eq!(
        queue.pending[&key].generation, 1,
        "a new session restarts generations"
    );
}
