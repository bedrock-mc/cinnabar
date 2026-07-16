pub(in crate::chunk) fn transparent_snapshot_addresses_are_resident<'a, 'b>(
    snapshot: &TransparentOrderedSnapshot,
    resident_allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
    retired_allocations: impl IntoIterator<Item = &'b GpuChunkAllocation>,
    active_asset_identity: ChunkTextureAssetIdentity,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> bool {
    if snapshot.key.asset_identity != active_asset_identity
        || snapshot.key.tint_identity != active_tint_identity
    {
        return false;
    }
    let resident_allocations = resident_allocations
        .into_iter()
        .filter(|allocation| allocation.tint_identity == active_tint_identity)
        .filter(|allocation| {
            let (Some(liquid), Some(lighting)) = (
                allocation.liquid_range.as_ref(),
                allocation.liquid_lighting_range.as_ref(),
            ) else {
                return false;
            };
            !liquid.is_empty()
                && !lighting.is_empty()
                && liquid.start % 4 == 0
                && liquid.end % 4 == 0
                && lighting.start.is_multiple_of(2)
                && lighting.end.is_multiple_of(2)
                && liquid.end.saturating_sub(liquid.start) / 4
                    == lighting.end.saturating_sub(lighting.start) / 2
        })
        .collect::<Vec<_>>();
    let retired_allocations = retired_allocations
        .into_iter()
        .filter(|allocation| allocation.tint_identity == active_tint_identity)
        .collect::<Vec<_>>();
    snapshot.key.visible_allocations.iter().all(|identity| {
        let active = resident_allocations.iter().any(|allocation| {
            let liquid = allocation
                .liquid_range
                .as_ref()
                .expect("resident allocations retain a liquid range");
            allocation.key == identity.key
                && allocation.metadata_index == identity.metadata_index
                && liquid.start == identity.liquid_range.start
                && liquid.end >= identity.liquid_range.end
        });
        active
            || retired_allocations.iter().any(|allocation| {
                allocation.key == identity.key
                    && allocation.generation == identity.mesh_generation
                    && allocation.metadata_index == identity.metadata_index
                    && allocation.liquid_range.as_ref() == Some(&identity.liquid_range)
                    && allocation.liquid_lighting_range.as_ref() == Some(&identity.lighting_range)
            })
    })
}

pub(in crate::chunk) fn build_transparent_candidates(
    visible_entities: &RenderVisibleEntities,
    instances: &Query<&ChunkRenderInstance>,
    allocations: &Query<&GpuChunkAllocation>,
    biome_tints: &ChunkBiomeTints,
) -> Result<(Vec<TransparentSortCandidate>, usize), TransparentSortError> {
    let mut candidates = Vec::new();
    let mut distinct_tint_colors = BTreeSet::new();
    for &(entity, _) in visible_entities.get::<ChunkRenderInstance>() {
        let (Ok(instance), Ok(allocation)) = (instances.get(entity), allocations.get(entity))
        else {
            continue;
        };
        if !transparent_allocation_matches(instance, allocation, biome_tints.table_identity()) {
            continue;
        }
        let (Some(liquid_range), Some(_lighting_range)) = (
            allocation.liquid_range.as_ref(),
            allocation.liquid_lighting_range.as_ref(),
        ) else {
            continue;
        };
        let Some(record_start) = liquid_range.start.checked_div(4) else {
            continue;
        };
        let subchunk_center = [
            instance.origin[0] as f32 + 8.0,
            instance.origin[1] as f32 + 8.0,
            instance.origin[2] as f32 + 8.0,
        ];
        let transparent_end = instance
            .depth_liquid_start
            .map_or(instance.liquid_quads.len(), |start| start as usize);
        for (local_index, &quad) in instance.liquid_quads[..transparent_end].iter().enumerate() {
            let local_quad_index =
                u32::try_from(local_index).map_err(|_| TransparentSortError::ReferenceCeiling {
                    requested: candidates.len().saturating_add(1),
                    ceiling: MAX_TRANSPARENT_DRAW_REFS,
                })?;
            if candidates.len() == MAX_TRANSPARENT_DRAW_REFS {
                return Err(TransparentSortError::ReferenceCeiling {
                    requested: candidates.len().saturating_add(1),
                    ceiling: MAX_TRANSPARENT_DRAW_REFS,
                });
            }
            let liquid_record_index = record_start.checked_add(local_quad_index).ok_or(
                TransparentSortError::ReferenceCeiling {
                    requested: candidates.len().saturating_add(1),
                    ceiling: MAX_TRANSPARENT_DRAW_REFS,
                },
            )?;
            let local = quad.origin();
            if let Some(tint_index) = instance.biome.tint_index(local[0], local[1], local[2])
                && let Some(tint) = biome_tints.entries().get(tint_index as usize)
            {
                distinct_tint_colors.insert(tint.water.map(f32::to_bits));
            }
            candidates.push(TransparentSortCandidate::new(
                instance.key,
                local_quad_index,
                liquid_record_index,
                allocation.metadata_index,
                subchunk_center,
                liquid_quad_centroid(instance.origin, quad),
            ));
        }
    }
    validate_transparent_sort_ref_count(candidates.len())?;
    Ok((candidates, distinct_tint_colors.len()))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_transparent_sorts(
    views: Query<(Entity, &ExtractedView, &RenderVisibleEntities), With<ExtractedCamera>>,
    instances: Query<&ChunkRenderInstance>,
    diagnostic_instances: Query<(Entity, &ChunkRenderInstance)>,
    allocations: Query<&GpuChunkAllocation>,
    texture_assets: Res<ChunkTextureAssets>,
    biome_tints: Res<ChunkBiomeTints>,
    render_queue: Res<RenderQueue>,
    arena: Res<ChunkGpuArena>,
    mut runtime: ResMut<TransparentSortRuntime>,
    metrics: Res<TransparentSortMetrics>,
    witness_request: Res<TransparentWitnessRequest>,
    witness_evidence: Res<TransparentWitnessEvidence>,
    mut upload_budget: ResMut<TransparentUploadBudget>,
) {
    upload_budget.reset();
    let completed = {
        let receiver = runtime
            .result_receiver
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        receiver.try_recv().ok()
    };
    if let Some(result) = completed {
        let next = runtime.gate.complete(result.generation);
        metrics.update(|snapshot| {
            snapshot.result_generation = result.generation.get();
            snapshot.cpu_duration = result.cpu_duration;
        });
        match result.refs {
            Ok(refs) => {
                let ref_bytes =
                    refs.len() as u64 * std::mem::size_of::<PackedTransparentDrawRef>() as u64;
                let sort_result = TransparentSortResult::new(result.generation, result.key, refs)
                    .expect("worker prevalidates the hard transparent reference ceiling");
                match runtime.state.complete(sort_result) {
                    Ok(true) => {
                        runtime.committed_distinct_tint_count = result.distinct_tint_count;
                        let ref_count = runtime
                            .state
                            .committed()
                            .map_or(0, |snapshot| snapshot.refs().len());
                        runtime.requested_at.remove(&result.generation);
                        let latency = transparent_request_to_commit_latency(
                            result.requested_at,
                            Instant::now(),
                        );
                        runtime
                            .staged_distinct_tint_counts
                            .remove(&result.generation);
                        metrics.update(|snapshot| {
                            snapshot.committed_generation = result.generation.get();
                            snapshot.ref_count = ref_count;
                            snapshot.request_to_commit_latency = latency;
                            snapshot.active_slot_age_frames = 0;
                            snapshot.transparent_water_distinct_tint_count =
                                result.distinct_tint_count;
                        });
                    }
                    Ok(false) => {
                        if runtime.state.staged_ref_count() != 0 {
                            runtime
                                .requested_at
                                .insert(result.generation, result.requested_at);
                            runtime
                                .staged_distinct_tint_counts
                                .insert(result.generation, result.distinct_tint_count);
                            metrics.update(|snapshot| {
                                snapshot.staged_bytes =
                                    snapshot.staged_bytes.saturating_add(ref_bytes);
                            });
                        } else {
                            runtime.requested_at.remove(&result.generation);
                            runtime
                                .staged_distinct_tint_counts
                                .remove(&result.generation);
                            metrics.update(|snapshot| {
                                snapshot.stale_reject_count =
                                    snapshot.stale_reject_count.saturating_add(1);
                            });
                        }
                    }
                    Err(TransparentSortError::ReferenceCeiling { .. }) => {
                        runtime.requested_at.remove(&result.generation);
                        metrics.update(|snapshot| {
                            snapshot.ceiling_reject_count =
                                snapshot.ceiling_reject_count.saturating_add(1);
                        });
                    }
                    Err(TransparentSortError::ConflictingAllocation { .. }) => unreachable!(),
                    Err(TransparentSortError::InvalidCameraTransform) => unreachable!(),
                }
            }
            Err(TransparentSortError::ReferenceCeiling { .. }) => {
                runtime.requested_at.remove(&result.generation);
                metrics.update(|snapshot| {
                    snapshot.ceiling_reject_count = snapshot.ceiling_reject_count.saturating_add(1);
                });
            }
            Err(TransparentSortError::ConflictingAllocation { .. }) => {}
            Err(TransparentSortError::InvalidCameraTransform) => {}
        }
        if let Some((_generation, work)) = next {
            spawn_transparent_sort(runtime.result_sender.clone(), work);
        }
        runtime.prune_request_metadata();
    }

    let mut visible_views = views.iter().collect::<Vec<_>>();
    visible_views.sort_by_key(|(entity, _, _)| *entity);
    if visible_views.len() > MAX_TRANSPARENT_VIEWS {
        bevy::log::warn!(
            "transparent chunk renderer supports one retained 3D view; extra views are rejected"
        );
        visible_views.truncate(MAX_TRANSPARENT_VIEWS);
    }
    let Some((view_entity, view, visible_entities)) = visible_views.into_iter().next() else {
        if runtime.view_entity.is_some() {
            runtime.reset_for_view(None);
            clear_active_transparent_metrics(&metrics);
        }
        return;
    };
    if runtime.view_entity != Some(view_entity) {
        runtime.reset_for_view(Some(view_entity));
        clear_active_transparent_metrics(&metrics);
    }

    let mut manifest = Vec::new();
    for &(entity, _) in visible_entities.get::<ChunkRenderInstance>() {
        let (Ok(instance), Ok(allocation)) = (instances.get(entity), allocations.get(entity))
        else {
            continue;
        };
        if !transparent_allocation_matches(instance, allocation, biome_tints.table_identity()) {
            continue;
        }
        if allocation.has_transparent_liquid
            && let (Some(liquid), Some(lighting)) = (
                allocation.liquid_range.clone(),
                allocation.liquid_lighting_range.clone(),
            )
        {
            manifest.push(TransparentAllocationIdentity::new(
                allocation.key,
                allocation.generation,
                liquid,
                lighting,
                allocation.metadata_index,
            ));
        }
    }
    let world_from_view = view.world_from_view;
    let (_, rotation, translation) = world_from_view.to_scale_rotation_translation();
    let texture_identity = texture_assets.identity();
    let tint_identity = biome_tints.table_identity();
    let key = match ViewSortKey::try_new(
        translation.to_array(),
        rotation.to_array(),
        manifest,
        texture_identity,
        tint_identity,
    ) {
        Ok(key) => key,
        Err(error @ TransparentSortError::ConflictingAllocation { .. })
        | Err(error @ TransparentSortError::InvalidCameraTransform) => {
            fail_closed_transparent_sort_key_error(&mut runtime, &metrics, error);
            return;
        }
        Err(TransparentSortError::ReferenceCeiling { .. }) => unreachable!(),
    };
    if witness_request.enabled() {
        let visible = visible_entities
            .get::<ChunkRenderInstance>()
            .iter()
            .map(|&(entity, _)| entity)
            .collect::<BTreeSet<_>>();
        let committed = runtime.state.committed();
        let records = witness_request
            .keys()
            .iter()
            .copied()
            .map(|required| {
                let found = diagnostic_instances
                    .iter()
                    .find(|(_, instance)| instance.key == required);
                let (entity, instance) = found.unzip();
                let allocation = entity.and_then(|entity| allocations.get(entity).ok());
                TransparentWitnessStageRecord {
                    key: required,
                    extracted_visible: entity.is_some_and(|entity| visible.contains(&entity)),
                    instance_present: instance.is_some(),
                    liquid_quad_count: instance.map_or(0, |instance| instance.liquid_quads.len()),
                    instance_generation: instance.map_or(0, |instance| instance.generation),
                    allocation_present: allocation.is_some(),
                    liquid_range_len: allocation
                        .and_then(|allocation| allocation.liquid_range.as_ref())
                        .map_or(0, |range| range.end.saturating_sub(range.start)),
                    lighting_range_len: allocation
                        .and_then(|allocation| allocation.liquid_lighting_range.as_ref())
                        .map_or(0, |range| range.end.saturating_sub(range.start)),
                    allocation_matches: instance.zip(allocation).is_some_and(
                        |(instance, allocation)| {
                            transparent_allocation_matches(
                                instance,
                                allocation,
                                biome_tints.table_identity(),
                            )
                        },
                    ),
                    committed_member: committed.is_some_and(|snapshot| {
                        snapshot
                            .key()
                            .visible_allocations
                            .iter()
                            .any(|allocation| allocation.key == required)
                    }),
                }
            })
            .collect();
        witness_evidence.record_stage_snapshot(
            witness_request.revision(),
            committed.map_or(0, |snapshot| snapshot.generation().get()),
            records,
        );
    }
    let committed_matches = runtime
        .state
        .committed()
        .is_some_and(|snapshot| snapshot.key() == &key)
        && runtime.state.staged_ref_count() == 0;
    if !committed_matches {
        let had_committed = runtime.state.committed().is_some();
        let committed_addresses_are_resident = runtime.state.committed().is_some_and(|snapshot| {
            snapshot.key.address_identity_eq(&key)
                || transparent_snapshot_addresses_are_resident(
                    snapshot,
                    arena.allocations.values().map(|allocation| &allocation.gpu),
                    arena
                        .retired_allocations
                        .iter()
                        .map(|allocation| &allocation.identity),
                    texture_identity,
                    tint_identity,
                )
        });
        let canceled_staged = runtime.state.staged_generation();
        let generation = runtime
            .state
            .request_retaining_resident_snapshot(&key, committed_addresses_are_resident);
        if had_committed && runtime.state.committed().is_none() {
            runtime.committed_distinct_tint_count = 0;
            metrics.update(|snapshot| {
                snapshot.committed_generation = 0;
                snapshot.encoded_generation = 0;
                snapshot.presented_generation = 0;
                snapshot.ref_count = 0;
                snapshot.active_slot_age_frames = 0;
                snapshot.transparent_water_distinct_tint_count = 0;
            });
        }
        if let Some(canceled) = canceled_staged
            && runtime.state.staged_generation() != Some(canceled)
        {
            runtime.requested_at.remove(&canceled);
            runtime.staged_distinct_tint_counts.remove(&canceled);
        }
        metrics.update(|snapshot| snapshot.request_generation = generation.get());
        if runtime.generation_needs_sort_job(generation) {
            let requested_at = Instant::now();
            match runtime.resolve_candidate_cache(&key, || {
                build_transparent_candidates(
                    visible_entities,
                    &instances,
                    &allocations,
                    &biome_tints,
                )
            }) {
                Ok((candidates, distinct_tint_count)) => {
                    let request = TransparentSortRequest {
                        generation,
                        requested_at,
                        key,
                        view_from_world: Mat4::from(world_from_view.affine().inverse()),
                    };
                    let work = TransparentSortWork {
                        generation: request.generation,
                        requested_at: request.requested_at,
                        key: request.key,
                        view_from_world: request.view_from_world,
                        candidates,
                        distinct_tint_count,
                    };
                    runtime.requested_at.insert(generation, requested_at);
                    let (start, replaced) = runtime.gate.submit_with_replacement(generation, work);
                    if let Some(replaced) = replaced {
                        runtime.requested_at.remove(&replaced);
                        runtime.staged_distinct_tint_counts.remove(&replaced);
                    }
                    if let Some((_generation, work)) = start {
                        spawn_transparent_sort(runtime.result_sender.clone(), work);
                    }
                    runtime.prune_request_metadata();
                }
                Err(TransparentSortError::ReferenceCeiling { .. }) => {
                    metrics.update(|snapshot| {
                        snapshot.ceiling_reject_count =
                            snapshot.ceiling_reject_count.saturating_add(1);
                    });
                }
                Err(TransparentSortError::ConflictingAllocation { .. }) => {}
                Err(TransparentSortError::InvalidCameraTransform) => {}
            }
        }
    }

    let mut uploaded_bytes = 0_u64;
    if let Some(batch) = runtime.state.next_upload_batch() {
        if !upload_budget.consume(batch.refs().len()) {
            bevy::log::error!(
                "transparent water sort batch exceeds the shared per-frame reference upload budget"
            );
            return;
        }
        let offset = u64::try_from(batch.buffer_slot() as usize * TRANSPARENT_REF_SLOT_BYTES)
            .unwrap()
            .saturating_add(
                u64::try_from(
                    batch.ref_range().start * std::mem::size_of::<PackedTransparentDrawRef>(),
                )
                .unwrap(),
            );
        render_queue.write_buffer(
            &arena.transparent_ref_buffer,
            offset,
            bytemuck::cast_slice(batch.refs()),
        );
        uploaded_bytes =
            batch.refs().len() as u64 * std::mem::size_of::<PackedTransparentDrawRef>() as u64;
    }
    if uploaded_bytes != 0 {
        let committed = runtime.state.acknowledge_upload();
        metrics.update(|snapshot| {
            snapshot.upload_bytes = snapshot.upload_bytes.saturating_add(uploaded_bytes);
        });
        if committed
            && let Some((generation, ref_count)) = runtime
                .state
                .committed()
                .map(|snapshot| (snapshot.generation(), snapshot.refs().len()))
        {
            runtime.committed_distinct_tint_count = runtime
                .staged_distinct_tint_counts
                .remove(&generation)
                .unwrap_or_default();
            let requested_at = runtime
                .requested_at
                .remove(&generation)
                .expect("accepted staged generation retains its request timestamp");
            let latency = transparent_request_to_commit_latency(requested_at, Instant::now());
            let tint_count = runtime.committed_distinct_tint_count;
            metrics.update(|current| {
                current.committed_generation = generation.get();
                current.ref_count = ref_count;
                current.request_to_commit_latency = latency;
                current.active_slot_age_frames = 0;
                current.transparent_water_distinct_tint_count = tint_count;
            });
        }
    }
    metrics.update(|snapshot| {
        if runtime.state.committed().is_some() {
            snapshot.active_slot_age_frames = snapshot.active_slot_age_frames.saturating_add(1);
        }
    });
    if let Some((identity, command)) = runtime.state.committed().and_then(|snapshot| {
        Some((
            (snapshot.buffer_slot(), snapshot.refs().len()),
            transparent_indirect_args(snapshot)?,
        ))
    }) && runtime.last_indirect_identity != Some(identity)
    {
        render_queue.write_buffer(
            &arena.transparent_indirect_buffer,
            0,
            bytemuck::bytes_of(&command),
        );
        runtime.last_indirect_identity = Some(identity);
    }
}
