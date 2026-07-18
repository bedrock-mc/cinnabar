use super::*;

fn noop_gpu_publication_app(
    acknowledgements: ChunkUploadAcknowledgements,
    gpu_removals: ChunkGpuRemovalQueue,
) -> App {
    use bevy::{
        ecs::system::RunSystemOnce,
        render::renderer::{RenderDevice, RenderQueue, WgpuWrapper},
    };

    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut app = App::new();
    app.insert_resource(RenderDevice::from(device))
        .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
        .insert_resource(ChunkUploadBudget::new(0, 0))
        .insert_resource(ChunkGpuUploadStats::default())
        .insert_resource(ChunkBiomeTints::default())
        .insert_resource(ChunkTextureAssets::default())
        .insert_resource(acknowledgements)
        .insert_resource(gpu_removals)
        .insert_resource(PresentedFrameGate::default())
        .insert_resource(ActiveFrameProbe::default())
        .insert_resource(ActiveVisibilityFrameProbe::default())
        .insert_resource(VisibilityDiagnostics::default())
        .insert_resource(TransparentSortMetrics::default())
        .insert_resource(TransparentPresentationFence::default())
        .insert_resource(TransparentRetirementFence::default())
        .insert_resource(TransparentSortRuntime::default())
        .insert_resource(TransparentWitnessRequest::default())
        .insert_resource(TransparentWitnessEvidence::default())
        .insert_resource(GpuUpdateFairness::default());
    app.world_mut()
        .run_system_once(init_chunk_gpu_arena)
        .unwrap();
    app
}

#[test]
fn shared_publication_allowance_limits_payload_and_zero_byte_work_independently() {
    let now = Instant::now();
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let budget = ChunkUploadBudget::new(2, u64::MAX);
    let zero_byte_operations = DEFAULT_ZERO_BYTE_OPERATIONS_PER_FRAME;
    let total = zero_byte_operations + 2;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::with_limits(ChunkRenderQueueLimits {
            max_items: total,
            max_bytes: DEFAULT_RENDER_QUEUE_BYTES,
        }))
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(budget));

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        for index in 0..zero_byte_operations {
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
        for index in 0..2 {
            queue
                .try_update_tracked(
                    SubChunkKey::new(0, index + 300, 0, 0),
                    solid_test_mesh(),
                    ChunkUploadPriority::new((index + 10) as f32),
                    ChunkUploadToken {
                        generation: index as u64 + 10,
                        dirty_since: now,
                    },
                )
                .unwrap();
        }
    }

    app.update();

    assert_eq!(acknowledgements.drain().len(), zero_byte_operations);
    let world = app.world_mut();
    assert_eq!(
        world.query::<&ChunkRenderInstance>().iter(world).count(),
        2,
        "zero-byte removals consumed the non-empty item allowance"
    );
    assert_eq!(world.resource::<ChunkRenderQueue>().retained_len(), 0);
}

#[test]
fn permitted_zero_byte_work_waits_for_gpu_apply_and_is_bounded_at_256_outstanding() {
    let now = Instant::now();
    let config = client_world::PublicationServiceConfig::PHASE2_GATE;
    let allowance = client_world::PublicationAllowance::new(config);
    allowance.begin_frame(1, 0, 0, 512);
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::default())
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(
            ChunkUploadBudget::new(0, 0).with_zero_byte_operations_per_frame(512),
        ));

    for index in 0..256 {
        let permit = allowance
            .try_admit_zero_byte()
            .expect("the literal zero-byte outstanding allowance is 256");
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_remove_tracked_permitted(
                SubChunkKey::new(0, index, 0, 0),
                ChunkUploadPriority::new(index as f32),
                ChunkUploadToken {
                    generation: index as u64 + 1,
                    dirty_since: now,
                },
                permit,
            )
            .unwrap();
    }
    assert!(allowance.try_admit_zero_byte().is_none());

    app.update();

    assert!(acknowledgements.drain().is_empty());
    assert_eq!(app.world().resource::<ChunkRenderQueue>().retained_len(), 0);
    assert_eq!(
        app.world().resource::<ChunkGpuRemovalQueue>().pending_len(),
        256
    );
    assert_eq!(allowance.live_permits(), 256);
    allowance.begin_frame(2, 0, 0, 512);
    assert!(allowance.try_admit_zero_byte().is_none());

    drop(app);
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn gpu_preparation_acknowledges_and_retires_a_permitted_known_air_removal_exactly_once() {
    use bevy::ecs::system::RunSystemOnce;

    let key = SubChunkKey::new(0, 7, 0, 9);
    let token = ChunkUploadToken {
        generation: 41,
        dirty_since: Instant::now(),
    };
    let allowance = client_world::PublicationAllowance::new(
        client_world::PublicationServiceConfig::PHASE2_GATE,
    );
    allowance.begin_frame(1, 0, 0, 1);
    let permit = allowance
        .try_admit_zero_byte()
        .unwrap()
        .into_handoff()
        .unwrap()
        .into_render_entity()
        .unwrap();
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let gpu_removals = ChunkGpuRemovalQueue::default();
    assert!(
        gpu_removals
            .push(PendingGpuRemoval {
                key,
                token: Some(token),
                permit,
            })
            .is_ok()
    );
    let mut app = noop_gpu_publication_app(acknowledgements.clone(), gpu_removals.clone());

    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    let completed = acknowledgements.drain();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].key, key);
    assert_eq!(completed[0].token, token);
    assert!(acknowledgements.is_empty());
    assert_eq!(gpu_removals.pending_len(), 0);
    assert_eq!(allowance.live_permits(), 0);

    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();
    assert!(acknowledgements.drain().is_empty());
}

#[test]
fn failed_gpu_removal_ack_reservation_requeues_without_retiring_or_leaking_an_ack() {
    use bevy::ecs::system::RunSystemOnce;

    let key = SubChunkKey::new(0, 8, 0, 9);
    let token = ChunkUploadToken {
        generation: 42,
        dirty_since: Instant::now(),
    };
    let allowance = client_world::PublicationAllowance::new(
        client_world::PublicationServiceConfig::PHASE2_GATE,
    );
    allowance.begin_frame(1, 0, 0, 1);
    let permit = allowance
        .try_admit_zero_byte()
        .unwrap()
        .into_handoff()
        .unwrap()
        .into_render_entity()
        .unwrap();
    let acknowledgements = ChunkUploadAcknowledgements::with_capacity(0);
    let gpu_removals = ChunkGpuRemovalQueue::default();
    assert!(
        gpu_removals
            .push(PendingGpuRemoval {
                key,
                token: Some(token),
                permit,
            })
            .is_ok()
    );
    let mut app = noop_gpu_publication_app(acknowledgements.clone(), gpu_removals.clone());

    app.world_mut().run_system_once(prepare_gpu_chunks).unwrap();

    assert!(acknowledgements.is_empty());
    assert!(acknowledgements.drain().is_empty());
    assert_eq!(gpu_removals.pending_len(), 1);
    assert_eq!(allowance.live_permits(), 1);
    drop(app);
    drop(gpu_removals);
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn a_newer_gpu_removal_supersedes_and_retires_the_same_key_carrier() {
    let key = SubChunkKey::new(0, 9, 0, 9);
    let allowance = client_world::PublicationAllowance::new(
        client_world::PublicationServiceConfig::PHASE2_GATE,
    );
    allowance.begin_frame(1, 0, 0, 2);
    let first = allowance
        .try_admit_zero_byte()
        .unwrap()
        .into_handoff()
        .unwrap()
        .into_render_entity()
        .unwrap();
    let second = allowance
        .try_admit_zero_byte()
        .unwrap()
        .into_handoff()
        .unwrap()
        .into_render_entity()
        .unwrap();
    let gpu_removals = ChunkGpuRemovalQueue::default();
    assert!(
        gpu_removals
            .push(PendingGpuRemoval {
                key,
                token: None,
                permit: first,
            })
            .is_ok()
    );
    assert_eq!(allowance.live_permits(), 2);

    assert!(
        gpu_removals
            .push(PendingGpuRemoval {
                key,
                token: None,
                permit: second,
            })
            .is_ok()
    );
    assert_eq!(gpu_removals.pending_len(), 1);
    assert_eq!(allowance.live_permits(), 1);

    gpu_removals.cancel(key);
    assert_eq!(gpu_removals.pending_len(), 0);
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn admitted_payload_carries_one_permit_from_queue_handoff_to_render_entity() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 12, 0, 12);
    let mesh = solid_test_mesh();
    let biome = PackedBiomeRecord::fallback();
    let bytes = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let allowance = client_world::PublicationAllowance::new(
        client_world::PublicationServiceConfig::PHASE2_GATE,
    );
    allowance.begin_frame(1, 1, bytes, 0);
    let permit = allowance.try_admit_payload(bytes).unwrap();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::default())
        .insert_resource(ChunkUploadAcknowledgements::default())
        .add_plugins(ChunkRenderPlugin::with_budget(ChunkUploadBudget::new(0, 0)));
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked_with_biome_identity_permitted(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: now,
            },
            permit,
        )
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .pending
            .get(&key)
            .and_then(|pending| pending.publication_permit.as_ref())
            .and_then(client_world::PublicationPermit::stage),
        Some(client_world::PublicationPermitStage::Handoff),
    );

    app.update();

    let slot = {
        let world = app.world_mut();
        let mut query = world.query::<&ChunkRenderInstance>();
        query
            .single(world)
            .unwrap()
            .publication_permit
            .clone()
            .expect("render entity carries the linear permit slot")
    };
    assert_eq!(
        slot.stage(),
        Some(client_world::PublicationPermitStage::RenderEntity)
    );
    assert_eq!(allowance.live_permits(), 1);
    assert!(slot.take().unwrap().retire());
}

#[test]
fn admitted_payload_reaches_real_gpu_preparation_with_one_linear_permit_and_exact_ack() {
    use bevy::ecs::system::RunSystemOnce;

    let now = Instant::now();
    let key = SubChunkKey::new(0, 13, 0, 12);
    let mesh = solid_test_mesh();
    let biome = PackedBiomeRecord::fallback();
    let bytes = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let allowance = client_world::PublicationAllowance::new(
        client_world::PublicationServiceConfig::PHASE2_GATE,
    );
    allowance.begin_frame(
        1,
        1,
        client_world::PublicationServiceConfig::PHASE2_GATE.maximum_frame_bytes,
        0,
    );
    let permit = allowance.try_admit_payload(bytes).unwrap();
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut handoff_app = App::new();
    handoff_app
        .add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::default())
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(ChunkUploadBudget::new(0, 0)));
    handoff_app
        .world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked_with_biome_identity_permitted(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: now,
            },
            permit,
        )
        .unwrap();
    handoff_app.update();
    let instance = {
        let world = handoff_app.world_mut();
        let mut query = world.query::<&ChunkRenderInstance>();
        query.single(world).unwrap().clone()
    };
    drop(handoff_app);

    let gpu_removals = ChunkGpuRemovalQueue::default();
    let mut gpu_app = noop_gpu_publication_app(acknowledgements.clone(), gpu_removals.clone());
    gpu_app.world_mut().spawn(instance);
    gpu_app
        .world_mut()
        .run_system_once(prepare_gpu_chunks)
        .unwrap();

    let completed = acknowledgements.drain();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].key, key);
    assert_eq!(completed[0].token.generation, 1);
    assert!(completed[0].uploaded_bytes >= bytes);
    assert_eq!(
        gpu_app
            .world()
            .resource::<ChunkGpuArena>()
            .allocations
            .len(),
        1
    );
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn growth_deferred_for_frame_byte_authority_restores_the_linear_permit_then_acks_once() {
    use bevy::ecs::system::RunSystemOnce;

    let now = Instant::now();
    let key = SubChunkKey::new(0, 14, 0, 12);
    let mesh = solid_test_mesh();
    let biome = PackedBiomeRecord::fallback();
    let bytes = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let allowance = client_world::PublicationAllowance::new(
        client_world::PublicationServiceConfig::PHASE2_GATE,
    );
    allowance.begin_frame(1, 1, bytes, 0);
    let permit = allowance.try_admit_payload(bytes).unwrap();
    assert_eq!(allowance.frame_remaining_bytes(), 0);

    let acknowledgements = ChunkUploadAcknowledgements::default();
    let mut handoff_app = App::new();
    handoff_app
        .add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::default())
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(ChunkUploadBudget::new(0, 0)));
    handoff_app
        .world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked_with_biome_identity_permitted(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 2,
                dirty_since: now,
            },
            permit,
        )
        .unwrap();
    handoff_app.update();
    let instance = {
        let world = handoff_app.world_mut();
        let mut query = world.query::<&ChunkRenderInstance>();
        query.single(world).unwrap().clone()
    };
    let slot = instance
        .publication_permit
        .clone()
        .expect("render entity carries the linear permit slot");
    drop(handoff_app);

    let gpu_removals = ChunkGpuRemovalQueue::default();
    let mut gpu_app = noop_gpu_publication_app(acknowledgements.clone(), gpu_removals);
    gpu_app.world_mut().spawn(instance);
    gpu_app
        .world_mut()
        .run_system_once(prepare_gpu_chunks)
        .unwrap();

    assert!(acknowledgements.is_empty());
    assert_eq!(
        gpu_app
            .world()
            .resource::<ChunkGpuArena>()
            .allocations
            .len(),
        0
    );
    assert_eq!(slot.stage(), Some(PublicationPermitStage::RenderEntity));
    assert_eq!(slot.bytes(), Some(bytes));
    assert_eq!(allowance.live_permits(), 1);

    allowance.begin_frame(
        2,
        0,
        client_world::PublicationServiceConfig::PHASE2_GATE.maximum_frame_bytes,
        0,
    );
    gpu_app
        .world_mut()
        .run_system_once(prepare_gpu_chunks)
        .unwrap();

    let completed = acknowledgements.drain();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].key, key);
    assert_eq!(completed[0].token.generation, 2);
    assert!(completed[0].uploaded_bytes >= bytes);
    assert_eq!(
        gpu_app
            .world()
            .resource::<ChunkGpuArena>()
            .allocations
            .len(),
        1
    );
    assert_eq!(slot.stage(), None);
    assert_eq!(allowance.live_permits(), 0);

    gpu_app
        .world_mut()
        .run_system_once(prepare_gpu_chunks)
        .unwrap();
    assert!(acknowledgements.is_empty());
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn render_handoff_rejects_a_permit_with_the_wrong_class_or_exact_bytes() {
    let allowance = client_world::PublicationAllowance::new(
        client_world::PublicationServiceConfig::PHASE2_GATE,
    );
    allowance.begin_frame(1, 1, 64, 1);
    let payload = allowance.try_admit_payload(64).unwrap();
    let zero = allowance.try_admit_zero_byte().unwrap();
    let mut queue = ChunkRenderQueue::default();
    let key = SubChunkKey::new(0, 1, 0, 1);
    let token = ChunkUploadToken {
        generation: 1,
        dirty_since: Instant::now(),
    };

    assert!(
        queue
            .try_remove_tracked_permitted(key, ChunkUploadPriority::new(0.0), token, payload,)
            .is_err()
    );
    assert!(
        queue
            .try_update_tracked_with_biome_identity_permitted(
                key,
                solid_test_mesh(),
                PackedBiomeRecord::fallback(),
                ChunkBiomeTintIdentity::default(),
                ChunkUploadPriority::new(0.0),
                token,
                zero,
            )
            .is_err()
    );
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn manual_transfer_downstream_gpu_subgate_prepares_exact_6951_allocation_manifest() {
    use bevy::ecs::system::RunSystemOnce;

    const COHORT_ITEMS: usize = 6_951;
    const PAYLOADS_PER_FRAME: usize = 511;
    let config = client_world::PublicationServiceConfig::PHASE2_GATE;
    let allowance = client_world::PublicationAllowance::new(config);
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let gpu_removals = ChunkGpuRemovalQueue::default();
    let budget = ChunkUploadBudget::new(config.maximum_frame_items, config.maximum_frame_bytes)
        .with_zero_byte_operations_per_frame(config.maximum_zero_byte_operations_per_frame);
    let mut handoff_app = App::new();
    handoff_app
        .add_plugins(MinimalPlugins)
        .insert_resource(ChunkRenderQueue::default())
        .insert_resource(acknowledgements.clone())
        .insert_resource(gpu_removals.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(budget));
    let mut gpu_app = noop_gpu_publication_app(acknowledgements.clone(), gpu_removals.clone());
    gpu_app.world_mut().insert_resource(budget);

    let mesh = solid_test_mesh();
    let biome = PackedBiomeRecord::fallback();
    let publication_bytes = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let expected = (0..COHORT_ITEMS)
        .map(|index| {
            let x = 49 + (index % 33) as i32;
            let z = 49 + ((index / 33) % 33) as i32;
            let y = (index / (33 * 33)) as i32;
            (SubChunkKey::new(0, x, y, z), index as u64 + 1)
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(expected.len(), COHORT_ITEMS);
    let mut next_payload = 0_usize;
    let mut uploaded_keys = HashSet::new();
    let mut acknowledged = BTreeMap::new();
    let mut known_air_acknowledgements = 0_usize;
    let started = Instant::now();
    let mut completed_frames = 0_usize;

    while next_payload < COHORT_ITEMS {
        completed_frames += 1;
        assert!(completed_frames <= 16);
        allowance.begin_frame(
            completed_frames as u64,
            1_024,
            16 * 1024 * 1024,
            config.maximum_zero_byte_operations_per_frame,
        );
        let frame_end = next_payload
            .saturating_add(PAYLOADS_PER_FRAME)
            .min(COHORT_ITEMS);
        {
            let mut queue = handoff_app.world_mut().resource_mut::<ChunkRenderQueue>();
            for index in next_payload..frame_end {
                let (&key, &generation) = expected
                    .get_key_value(&SubChunkKey::new(
                        0,
                        49 + (index % 33) as i32,
                        (index / (33 * 33)) as i32,
                        49 + ((index / 33) % 33) as i32,
                    ))
                    .unwrap();
                let permit = allowance
                    .try_admit_payload(publication_bytes)
                    .expect("the 125 ms controller allowance covers 511 payloads");
                queue
                    .try_update_tracked_with_biome_identity_permitted(
                        key,
                        mesh.clone(),
                        biome.clone(),
                        ChunkBiomeTintIdentity::default(),
                        ChunkUploadPriority::new(index as f32),
                        ChunkUploadToken {
                            generation,
                            dirty_since: started,
                        },
                        permit,
                    )
                    .unwrap();
            }
            let air_index = completed_frames as i32;
            let air_key = SubChunkKey::new(0, -20_000 - air_index, 0, 0);
            let permit = allowance
                .try_admit_zero_byte()
                .expect("one known-air operation is available each frame");
            queue
                .try_remove_tracked_permitted(
                    air_key,
                    ChunkUploadPriority::new(f32::MAX),
                    ChunkUploadToken {
                        generation: u64::MAX - completed_frames as u64,
                        dirty_since: started,
                    },
                    permit,
                )
                .unwrap();
        }
        next_payload = frame_end;

        handoff_app.update();
        assert_eq!(
            handoff_app
                .world()
                .resource::<ChunkRenderQueue>()
                .retained_len(),
            0
        );
        let extracted = {
            let world = handoff_app.world_mut();
            let mut query = world.query::<&ChunkRenderInstance>();
            query
                .iter(world)
                .filter(|instance| !uploaded_keys.contains(&instance.key))
                .cloned()
                .collect::<Vec<_>>()
        };
        for instance in extracted {
            uploaded_keys.insert(instance.key);
            gpu_app.world_mut().spawn(instance);
        }
        gpu_app
            .world_mut()
            .run_system_once(prepare_gpu_chunks)
            .unwrap();

        for acknowledgement in acknowledgements.drain() {
            if let Some(&generation) = expected.get(&acknowledgement.key) {
                assert_eq!(acknowledgement.token.generation, generation);
                assert!(
                    acknowledged
                        .insert(acknowledgement.key, generation)
                        .is_none(),
                    "one logical payload produced a duplicate acknowledgement"
                );
            } else {
                known_air_acknowledgements += 1;
                assert_eq!(acknowledgement.uploaded_bytes, 0);
            }
        }
        assert!(allowance.remaining_items() <= config.maximum_burst_items);
        assert!(allowance.remaining_bytes() <= config.maximum_burst_bytes);
    }

    let arena_manifest = gpu_app
        .world()
        .resource::<ChunkGpuArena>()
        .allocations
        .values()
        .map(|allocation| (allocation.gpu.key, allocation.generation))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(completed_frames, 14);
    assert!(completed_frames * 125 <= 2_000);
    assert_eq!(acknowledged, expected);
    assert_eq!(arena_manifest, expected);
    assert_eq!(known_air_acknowledgements, completed_frames);
    assert_eq!(allowance.live_permits(), 0);
    assert!(acknowledgements.is_empty());
    assert_eq!(gpu_removals.pending_len(), 0);
    assert!(gpu_app.world().resource::<GpuUpdateFairness>().is_empty());
    assert_eq!(
        handoff_app
            .world()
            .resource::<ChunkRenderQueue>()
            .retained_len(),
        0
    );

    let expectation = TargetRenderExpectation {
        cohort: RenderViewCohort::new(0, [65, 65], 16),
        source_cohort: None,
        target_columns: None,
        target_keys: Some(Arc::from(expected.keys().copied().collect::<Vec<_>>())),
        manifest: Arc::from(
            expected
                .iter()
                .map(|(&key, &generation)| (key, generation))
                .collect::<Vec<_>>(),
        ),
        view_generation: 1,
        render_ready_at: Instant::now(),
    };
    let (instances, allocations) = {
        let world = gpu_app.world_mut();
        let mut query = world.query::<(Entity, &ChunkRenderInstance)>();
        let instances = query
            .iter(world)
            .map(|(entity, instance)| FrameInstanceIdentity {
                entity,
                key: instance.key,
                generation: instance.generation,
            })
            .collect::<Vec<_>>();
        let allocations = world
            .resource::<ChunkGpuArena>()
            .allocations
            .iter()
            .map(|(&entity, allocation)| FrameAllocationIdentity {
                entity,
                key: allocation.gpu.key,
                generation: allocation.generation,
            })
            .collect::<Vec<_>>();
        (instances, allocations)
    };
    let gate = gpu_app.world().resource::<PresentedFrameGate>().clone();
    gate.set_expectation(expectation.clone());
    for _ in 0..2 {
        let probe = FrameProbe::begin(
            expectation.clone(),
            instances.iter().copied(),
            allocations.iter().copied(),
        );
        let active = gpu_app.world().resource::<ActiveFrameProbe>();
        active.begin(probe);
        for &allocation in &allocations {
            assert!(active.record_visible(allocation.entity, allocation));
            assert!(active.record_direct_draw(allocation.entity, allocation));
        }
        gpu_app
            .world_mut()
            .run_system_once(submit_presented_frame_probe)
            .unwrap();
        gpu_app
            .world()
            .resource::<RenderDevice>()
            .poll(PollType::wait_indefinitely())
            .unwrap();
    }
    let presented = gate.drain();
    assert_eq!(presented.len(), 2);
    assert!(presented[0].is_exact());
    assert!(presented[0].forms_stable_exact_pair_with(&presented[1]));
}
