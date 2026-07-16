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
fn biome_tint_table_is_revisioned_and_keeps_a_fallback_entry() {
    let fallback = ChunkBiomeTints::default();
    assert_eq!(fallback.entries().len(), 1);
    assert_eq!(fallback.revision(), 0);
    assert_eq!(prepare_biome_tint_entries(fallback.entries()).len(), 1);

    let empty = ChunkBiomeTints::with_revision(Arc::from([]), 7);
    assert_eq!(empty.entries().len(), 1);
    assert_eq!(empty.revision(), 7);

    let shared_entries = Arc::from([BiomeTint::default()]);
    let first = ChunkBiomeTints::with_revision(Arc::clone(&shared_entries), 7);
    let replacement = ChunkBiomeTints::with_revision(shared_entries, 8);
    assert_ne!(first.resource_identity(), replacement.resource_identity());

    assert_eq!(pack_linear_rgb10([0.0, 0.0, 0.0]), 0);
    assert_eq!(pack_linear_rgb10([1.0, 1.0, 1.0]), 0x3fff_ffff);
}

#[test]
fn biome_gpu_entries_pack_all_six_tint_classes_and_flags() {
    let entry = BiomeTint {
        grass: [0.1, 0.2, 0.3],
        foliage: [0.2, 0.3, 0.4],
        birch: [0.3, 0.4, 0.5],
        evergreen: [0.4, 0.5, 0.6],
        dry_foliage: [0.5, 0.6, 0.7],
        water: [0.6, 0.7, 0.8],
        flags: 0x5a,
    };
    let gpu = prepare_biome_tint_entries(&[entry])[0];

    assert_eq!(gpu.grass, pack_linear_rgb10(entry.grass));
    assert_eq!(gpu.foliage, pack_linear_rgb10(entry.foliage));
    assert_eq!(gpu.birch, pack_linear_rgb10(entry.birch));
    assert_eq!(gpu.evergreen, pack_linear_rgb10(entry.evergreen));
    assert_eq!(gpu.dry_foliage, pack_linear_rgb10(entry.dry_foliage));
    assert_eq!(gpu.water, pack_linear_rgb10(entry.water));
    assert_eq!(gpu.flags, entry.flags);
}

#[test]
fn tint_table_identity_rebuilds_the_gpu_buffer_and_shared_bind_group() {
    let entries = Arc::from([BiomeTint::default()]);
    let first =
        ChunkBiomeTints::with_identity(Arc::clone(&entries), ChunkBiomeTintIdentity::new(4, 7));
    let replacement = ChunkBiomeTints::with_identity(entries, ChunkBiomeTintIdentity::new(5, 7));
    let first_identity = first.resource_identity();
    let replacement_identity = replacement.resource_identity();

    assert!(!biome_tint_gpu_buffer_needs_rebuild(
        Some(first_identity),
        first_identity,
    ));
    assert!(biome_tint_gpu_buffer_needs_rebuild(
        Some(first_identity),
        replacement_identity,
    ));
    assert!(biome_tint_bind_group_needs_rebuild(
        Some(first_identity),
        replacement_identity,
    ));
}

#[test]
fn matching_identity_uploads_acks_and_queues_direct_and_mdi_draws() {
    fn solid_sub_chunk() -> world::SubChunk {
        world::SubChunk::decode(&[9, 1, 0, 1, 2]).expect("uniform solid sub-chunk")
    }

    let active = ChunkBiomeTintIdentity::new(4, 7);
    let mismatched = ChunkBiomeTintIdentity::new(5, 7);
    let matching_key = SubChunkKey::new(0, 0, 0, 0);
    let mismatched_key = SubChunkKey::new(0, 1, 0, 0);
    let now = Instant::now();
    let matching_token = ChunkUploadToken {
        generation: 1,
        dirty_since: now,
    };
    let mismatched_token = ChunkUploadToken {
        generation: 2,
        dirty_since: now,
    };
    let solid = solid_sub_chunk();
    let mesh = || {
        meshing::mesh_sub_chunk(
            &meshing::BlockClassifier::new(0),
            opaque_runtime_assets(),
            assets::NetworkIdMode::Sequential,
            &meshing::Neighbourhood::empty(),
            &solid,
        )
    };
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(acknowledgements.clone())
        .insert_resource(ChunkBiomeTints::with_identity(
            Arc::from([BiomeTint::default()]),
            active,
        ))
        .add_plugins(ChunkRenderPlugin::new(2));
    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_update_tracked_with_biome_identity(
                matching_key,
                mesh(),
                PackedBiomeRecord::fallback(),
                active,
                ChunkUploadPriority::new(0.0),
                matching_token,
            )
            .unwrap();
        queue
            .try_update_tracked_with_biome_identity(
                mismatched_key,
                mesh(),
                PackedBiomeRecord::fallback(),
                mismatched,
                ChunkUploadPriority::new(1.0),
                mismatched_token,
            )
            .unwrap();
    }
    app.update();
    let instances = app
        .world_mut()
        .query::<(Entity, &ChunkRenderInstance)>()
        .iter(app.world())
        .map(|(entity, instance)| (entity, instance.clone()))
        .collect::<HashMap<_, _>>();
    let candidates = instances
        .iter()
        .map(|(&entity, instance)| GpuUpdateCandidate {
            entity,
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
        })
        .collect::<Vec<_>>();
    let selected = plan_gpu_chunk_updates(
        candidates,
        &HashMap::new(),
        Vec3::ZERO,
        active,
        &GpuUpdateFairness::default(),
    );
    assert_eq!(selected.len(), 1);
    let selected_entity = selected[0];
    let selected_instance = &instances[&selected_entity];
    assert_eq!(selected_instance.key, matching_key);
    assert!(acknowledgements.try_reserve(matching_key, matching_token));
    assert!(acknowledgements.complete_with_bytes(matching_key, matching_token, now, 64,));
    let acked = acknowledgements.drain();
    assert_eq!(acked.len(), 1);
    assert_eq!(acked[0].key, matching_key);

    let allocations = instances
        .iter()
        .enumerate()
        .map(|(index, (&entity, instance))| {
            (
                entity,
                GpuChunkAllocation {
                    key: instance.key,
                    generation: instance.generation,
                    tint_identity: instance.tint_identity,
                    quad_range: (index as u32 * 6)..(index as u32 * 6 + 6),
                    cube_lighting_range: Some((200 + index as u32 * 12)..(212 + index as u32 * 12)),
                    model_range: None,
                    model_lighting_range: None,
                    model_draw_range: None,
                    transparent_model_draw_range: None,
                    liquid_range: None,
                    liquid_lighting_range: None,
                    has_depth_liquid: false,
                    has_transparent_liquid: false,
                    depth_liquid_range: None,
                    metadata_index: index as u32,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let frame_probe = ActiveFrameProbe::default();
    let direct = allocations
        .iter()
        .filter_map(|(&entity, allocation)| {
            drawable_allocation_identity(&frame_probe, entity, allocation, active)
        })
        .collect::<Vec<_>>();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].key, matching_key);

    let (commands, drawn) = prepare_indirect_batch_draws(
        allocations
            .iter()
            .map(|(&entity, allocation)| (entity, allocation)),
        &frame_probe,
        active,
    );
    assert_eq!(commands.len(), 1);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1.key, matching_key);
    assert!(acknowledgements.drain().is_empty());
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
        40,
        32,
        0,
        growth.gpu_copy_bytes,
        0,
        0,
    );

    assert_eq!(stats.chunk_updates, 2);
    assert_eq!(stats.chunk_budget, 2);
    assert_eq!(stats.incremental_bytes, 72);
    assert_eq!(stats.gpu_copy_bytes, 64);
    assert_eq!(stats.full_shadow_bytes, 0);
    assert_eq!(stats.total_bytes, 72);
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
        },
        GpuUpdateCandidate {
            entity: fitting,
            key: SubChunkKey::new(0, 10, 0, 0),
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
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
        },
        GpuUpdateCandidate {
            entity: near,
            key: near_key,
            generation: 1,
            tint_identity: ChunkBiomeTintIdentity::default(),
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
            },
            GpuUpdateCandidate {
                entity: far,
                key: far_key,
                generation: 1,
                tint_identity: tint,
            },
        ];
        let selected =
            plan_gpu_chunk_updates(candidates, &allocations, Vec3::ZERO, tint, &fairness);
        let chosen = selected[0];
        fairness.finish_frame(&selected, &[chosen]);
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

    fairness.finish_frame(&[a, b, c], &[]);
    assert_eq!(fairness.len(), 2);
    assert_eq!(fairness.wait_age(a), 1);
    assert_eq!(fairness.wait_age(b), 1);
    assert_eq!(fairness.wait_age(c), 0);

    fairness.finish_frame(&[b, c], &[]);
    assert_eq!(fairness.len(), 2);
    assert_eq!(fairness.wait_age(a), 0);
    assert_eq!(fairness.wait_age(b), 2);
    assert_eq!(fairness.wait_age(c), 1);

    fairness.finish_frame(&[b, c], &[b]);
    assert_eq!(fairness.wait_age(b), 0);
    assert_eq!(fairness.wait_age(c), 2);
    fairness.reset();
    assert!(fairness.is_empty());

    for _ in 0..70_000 {
        fairness.finish_frame(&[c], &[]);
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
    assert_eq!(applied.len(), DEFAULT_RENDER_QUEUE_ITEMS);
    for index in 0..DEFAULT_RENDER_QUEUE_ITEMS {
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
        total - DEFAULT_RENDER_QUEUE_ITEMS
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
        world::SubChunk::decode(&encoded).expect("uniform solid sub-chunk")
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
