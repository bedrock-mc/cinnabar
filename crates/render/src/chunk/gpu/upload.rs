use crate::chunk::*;

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_gpu_chunks(
    mut commands: Commands,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    views: Query<&ExtractedView, With<ExtractedCamera>>,
    mut removed_instances: RemovedComponents<ChunkRenderInstance>,
    mut arena: ResMut<ChunkGpuArena>,
    budget: Res<ChunkUploadBudget>,
    mut upload_stats: ResMut<ChunkGpuUploadStats>,
    biome_tints: Res<ChunkBiomeTints>,
    texture_assets: Res<ChunkTextureAssets>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    retirement_fence: Res<TransparentRetirementFence>,
    mut fairness: ResMut<GpuUpdateFairness>,
) {
    release_completed_transparent_retirements(&mut arena, retirement_fence.completed_epoch());
    let candidates = instances
        .iter()
        .map(|(entity, instance)| GpuUpdateCandidate {
            entity,
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
        })
        .collect();
    let camera_position = views
        .iter()
        .next()
        .map(|view| view.world_from_view.translation())
        .unwrap_or(Vec3::ZERO);
    let selected = plan_gpu_chunk_updates(
        candidates,
        &arena.allocations,
        camera_position,
        biome_tints.table_identity(),
        &fairness,
    );

    arena.pending_removals.extend(removed_instances.read());
    for entity in arena.pending_removals.iter().copied().collect::<Vec<_>>() {
        let Some(allocation) = arena.allocations.get(&entity).cloned() else {
            arena.pending_removals.remove(&entity);
            continue;
        };
        if allocation.liquid_range.is_none() {
            free_allocation(&mut arena, entity);
            arena.pending_removals.remove(&entity);
            continue;
        }
        let retirement = RetiredArenaAllocation::full(entity, allocation);
        let bytes = retirement.owned_bytes();
        if !arena.retirement_budget.can_reserve(1, bytes) {
            continue;
        }
        arena
            .allocations
            .remove(&entity)
            .expect("pending removal retains its arena allocation");
        assert!(arena.retirement_budget.try_reserve(1, bytes));
        arena.retired_allocations.push(retirement);
        arena.pending_removals.remove(&entity);
    }

    let mut quad_writes = Vec::new();
    let mut model_writes = Vec::new();
    let mut model_lighting_writes = Vec::new();
    let mut model_draw_writes = Vec::new();
    let mut transparent_model_draw_writes = Vec::new();
    let mut liquid_writes = Vec::new();
    let mut liquid_lighting_writes = Vec::new();
    let mut cube_lighting_writes = Vec::new();
    let mut biome_writes = Vec::new();
    let mut origin_writes = Vec::new();
    let mut applied_tokens = Vec::new();
    let mut successful_updates = Vec::new();
    let mut upload_reservation = GpuUploadReservation::default();
    for &entity in &selected {
        let Ok((_, instance)) = instances.get(entity) else {
            continue;
        };
        let instance_bytes = chunk_instance_upload_byte_len(instance);
        if !validate_partitioned_model_streams(
            &instance.model_refs,
            &instance.model_lighting,
            &instance.model_draw_refs,
            &instance.transparent_model_draw_refs,
            texture_assets.assets().model_templates(),
            texture_assets.assets().model_quads(),
            texture_assets.assets().materials(),
        ) {
            bevy::log::error!("sub-chunk model streams are not an exact material partition");
            continue;
        }
        let old = arena.allocations.get(&entity).cloned();
        let required = match u32::try_from(instance.cube_quads.len()) {
            Ok(required) => required,
            Err(_) => {
                bevy::log::error!("sub-chunk mesh exceeds the u32 instance range");
                continue;
            }
        };
        let Ok(cube_lighting_required) = u32::try_from(instance.cube_lighting.len()) else {
            bevy::log::error!("sub-chunk cube-lighting stream exceeds the u32 instance range");
            continue;
        };
        if required != cube_lighting_required {
            bevy::log::error!(
                "sub-chunk cube-lighting count must exactly match the cube-quad count"
            );
            continue;
        }
        let Ok(model_required) = u32::try_from(instance.model_refs.len()) else {
            bevy::log::error!("sub-chunk model stream exceeds the u32 instance range");
            continue;
        };
        let Ok(model_lighting_required) = u32::try_from(instance.model_lighting.len()) else {
            bevy::log::error!("sub-chunk model-lighting stream exceeds the u32 instance range");
            continue;
        };
        let Ok(model_draw_required) = u32::try_from(instance.model_draw_refs.len()) else {
            bevy::log::error!("sub-chunk model-draw stream exceeds the u32 instance range");
            continue;
        };
        let Ok(transparent_model_draw_required) =
            u32::try_from(instance.transparent_model_draw_refs.len())
        else {
            bevy::log::error!(
                "sub-chunk transparent-model-draw stream exceeds the u32 instance range"
            );
            continue;
        };
        let Ok(liquid_required) = u32::try_from(instance.liquid_quads.len()) else {
            bevy::log::error!("sub-chunk liquid stream exceeds the u32 instance range");
            continue;
        };
        let Ok(liquid_lighting_required) = u32::try_from(instance.liquid_lighting.len()) else {
            bevy::log::error!("sub-chunk liquid-lighting stream exceeds the u32 instance range");
            continue;
        };
        let biome_words = if biome_record_is_fallback(&instance.biome) {
            Vec::new()
        } else {
            instance.biome.words().to_vec()
        };
        let biome_required = match u32::try_from(biome_words.len()) {
            Ok(required) => required,
            Err(_) => {
                bevy::log::error!("sub-chunk biome record exceeds the u32 word range");
                continue;
            }
        };
        if old.is_none()
            && arena.free_origins.is_empty()
            && arena.origin_len >= arena.limits.max_origin_items
        {
            bevy::log::warn!("chunk origin arena is at the adapter storage-buffer limit");
            continue;
        }
        let stream_counts = GeometryStreamCounts {
            cube: required,
            cube_lighting: cube_lighting_required,
            model: model_required,
            model_lighting: model_lighting_required,
            model_draw: model_draw_required,
            transparent_model_draw: transparent_model_draw_required,
            liquid: liquid_required,
            liquid_lighting: liquid_lighting_required,
        };
        let preserve_old_geometry = old
            .as_ref()
            .is_some_and(|old| transparent_geometry_update_requires_cow(old, stream_counts));
        let retirement = preserve_old_geometry
            .then(|| {
                old.as_ref()
                    .and_then(|old| RetiredArenaAllocation::geometry_only(entity, old))
            })
            .flatten();
        if let Some(retirement) = retirement.as_ref()
            && !arena
                .retirement_budget
                .can_reserve(1, retirement.owned_bytes())
        {
            if let Some(token) = instance.token {
                acknowledgements.cancel(instance.key, token);
            }
            continue;
        }
        let Some(projected_ranges) = plan_chunk_range_update(
            arena.quad_len,
            &arena.free_quads,
            arena.geometry_stream_len,
            &arena.free_geometry_stream_words,
            arena.biome_len,
            &arena.free_biomes,
            stream_counts,
            biome_required,
            old.as_ref(),
            preserve_old_geometry,
            arena.limits,
        ) else {
            continue;
        };
        let projected_origin_len = arena
            .origin_len
            .saturating_add(usize::from(old.is_none() && arena.free_origins.is_empty()));
        let Some(projected_growth_copy_bytes) = planned_arena_growth_copy_bytes(
            ArenaRequiredLengths {
                quads: arena.quad_capacity,
                geometry_stream_words: arena.geometry_stream_capacity,
                origins: arena.origin_capacity,
                biome_words: arena.biome_capacity,
            },
            ArenaRequiredLengths {
                quads: projected_ranges.quad_len,
                geometry_stream_words: projected_ranges.geometry_stream_len,
                origins: projected_origin_len,
                biome_words: projected_ranges.biome_len,
            },
            arena.limits,
        ) else {
            continue;
        };
        let mut next_upload_reservation = upload_reservation;
        if !next_upload_reservation.try_reserve(
            *budget,
            instance_bytes,
            projected_growth_copy_bytes,
        ) {
            continue;
        }
        if instance
            .token
            .is_some_and(|token| !acknowledgements.try_reserve(instance.key, token))
        {
            continue;
        }
        let plan = commit_chunk_range_plan(&mut arena, projected_ranges);
        if let Some(retirement) = retirement {
            let bytes = retirement.owned_bytes();
            assert!(arena.retirement_budget.try_reserve(1, bytes));
            arena.retired_allocations.push(retirement);
        }
        let metadata_index = match old {
            Some(old) => old.gpu.metadata_index,
            None => allocate_origin(&mut arena)
                .expect("origin capacity was checked before quad allocation"),
        };
        let cube_range = checked_geometry_range(plan.quad_start, required);
        let cube_lighting_range = checked_geometry_range(
            plan.cube_lighting_start,
            cube_lighting_required
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)
                .expect("validated cube-lighting layout fits u32 words"),
        );
        let model_range = checked_geometry_range(
            plan.model_start,
            model_required * (PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let model_lighting_range = checked_geometry_range(
            plan.model_lighting_start,
            model_lighting_required
                * (PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let model_draw_range = checked_geometry_range(
            plan.model_draw_start,
            model_draw_required * (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let transparent_model_draw_range = checked_geometry_range(
            plan.transparent_model_draw_start,
            transparent_model_draw_required
                * (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let liquid_range = checked_geometry_range(
            plan.liquid_start,
            liquid_required * (PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let liquid_lighting_range = checked_geometry_range(
            plan.liquid_lighting_start,
            liquid_lighting_required
                * (PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
        );
        let depth_liquid_range = instance.depth_liquid_start.and_then(|local_start| {
            let liquid = liquid_range.as_ref()?;
            let record_start = liquid.start.checked_div(4)?;
            let record_end = liquid.end.checked_div(4)?;
            Some(record_start.checked_add(local_start)?..record_end)
        });
        let quad_range = cube_range
            .clone()
            .unwrap_or(plan.quad_start..plan.quad_start);
        let words = instance
            .cube_quads
            .iter()
            .map(PackedQuad::words)
            .collect::<Vec<_>>();
        let mut model_words = instance
            .model_refs
            .iter()
            .copied()
            .map(PackedModelRef::words)
            .collect::<Vec<_>>();
        absolutize_model_lighting_bases(&mut model_words, plan.model_lighting_start);
        let model_lighting_words = packed_lighting_records(&instance.model_lighting);
        let mut model_draw_words = instance
            .model_draw_refs
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>();
        let mut transparent_model_draw_words = instance
            .transparent_model_draw_refs
            .iter()
            .copied()
            .map(PackedModelDrawRef::words)
            .collect::<Vec<_>>();
        absolutize_partitioned_model_draw_refs(
            &mut model_draw_words,
            &mut transparent_model_draw_words,
            plan.model_start,
        )
        .expect("validated model draw refs and atomic arena plan fit absolute addressing");
        let mut liquid_words = instance
            .liquid_quads
            .iter()
            .copied()
            .map(PackedLiquidQuad::words)
            .collect::<Vec<_>>();
        absolutize_liquid_lighting_indices(&mut liquid_words, plan.liquid_lighting_start);
        let liquid_lighting_words = packed_lighting_records(&instance.liquid_lighting);
        let cube_lighting_words = packed_lighting_records(&instance.cube_lighting);
        let origin = gpu_chunk_origin(
            instance.origin,
            plan.biome_start,
            plan.quad_start,
            plan.cube_lighting_start,
        )
        .expect("aligned arena layout and bounded biome offset produce a valid origin record");
        quad_writes.push((plan.quad_start, words));
        model_writes.push((plan.model_start, model_words));
        model_lighting_writes.push((plan.model_lighting_start, model_lighting_words));
        model_draw_writes.push((plan.model_draw_start, model_draw_words));
        transparent_model_draw_writes.push((
            plan.transparent_model_draw_start,
            transparent_model_draw_words,
        ));
        liquid_writes.push((plan.liquid_start, liquid_words));
        liquid_lighting_writes.push((plan.liquid_lighting_start, liquid_lighting_words));
        cube_lighting_writes.push((plan.cube_lighting_start, cube_lighting_words));
        if !biome_words.is_empty() {
            biome_writes.push((plan.biome_start, biome_words));
        }
        origin_writes.push((metadata_index, origin));
        let gpu = GpuChunkAllocation {
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
            quad_range,
            cube_lighting_range: cube_lighting_range.clone(),
            model_range,
            model_lighting_range,
            model_draw_range,
            transparent_model_draw_range,
            liquid_range,
            liquid_lighting_range,
            has_depth_liquid: instance.has_depth_liquid,
            has_transparent_liquid: instance.has_transparent_liquid,
            depth_liquid_range,
            metadata_index,
        };
        commands.entity(entity).insert(gpu.clone());
        arena.allocations.insert(
            entity,
            ArenaAllocation {
                generation: instance.generation,
                tint_identity: instance.tint_identity,
                cube_range,
                cube_lighting_range,
                model_range: gpu.model_range.clone(),
                model_lighting_range: gpu.model_lighting_range.clone(),
                model_draw_range: gpu.model_draw_range.clone(),
                transparent_model_draw_range: gpu.transparent_model_draw_range.clone(),
                liquid_range: gpu.liquid_range.clone(),
                liquid_lighting_range: gpu.liquid_lighting_range.clone(),
                quad_capacity: plan.quad_capacity,
                geometry_stream_range: checked_geometry_range(
                    plan.geometry_stream_start,
                    GeometryStreamCounts {
                        cube: required,
                        cube_lighting: cube_lighting_required,
                        model: model_required,
                        model_lighting: model_lighting_required,
                        model_draw: model_draw_required,
                        transparent_model_draw: transparent_model_draw_required,
                        liquid: liquid_required,
                        liquid_lighting: liquid_lighting_required,
                    }
                    .shared_word_count()
                    .expect("stream counts were checked before allocation"),
                ),
                geometry_stream_capacity: plan.geometry_stream_capacity,
                biome_range: plan.biome_start..plan.biome_start + biome_required,
                biome_capacity: plan.biome_capacity,
                gpu,
            },
        );
        if let Some(token) = instance.token {
            let uploaded_bytes = buffer_byte_len(instance.cube_quads.len(), PACKED_QUAD_BYTES)
                .saturating_add(buffer_byte_len(
                    instance.cube_lighting.len(),
                    PACKED_QUAD_LIGHTING_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.model_refs.len(),
                    PACKED_MODEL_REF_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.model_lighting.len(),
                    PACKED_QUAD_LIGHTING_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.model_draw_refs.len(),
                    PACKED_MODEL_DRAW_REF_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.transparent_model_draw_refs.len(),
                    PACKED_MODEL_DRAW_REF_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.liquid_quads.len(),
                    PACKED_LIQUID_QUAD_BYTES,
                ))
                .saturating_add(buffer_byte_len(
                    instance.liquid_lighting.len(),
                    PACKED_QUAD_LIGHTING_BYTES,
                ))
                .saturating_add(CHUNK_ORIGIN_BYTES)
                .saturating_add(biome_record_byte_len(&instance.biome));
            applied_tokens.push((instance.key, token, uploaded_bytes));
        }
        upload_reservation = next_upload_reservation;
        successful_updates.push(entity);
    }
    fairness.finish_frame(&selected, &successful_updates);

    let quad_incremental_bytes = quad_writes.iter().fold(0_u64, |total, (_, words)| {
        total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_BYTES))
    });
    let stream_incremental_bytes = model_writes
        .iter()
        .fold(0_u64, |total, (_, words)| {
            total.saturating_add(buffer_byte_len(words.len(), PACKED_MODEL_REF_BYTES))
        })
        .saturating_add(
            model_lighting_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_LIGHTING_BYTES))
                }),
        )
        .saturating_add(model_draw_writes.iter().fold(0_u64, |total, (_, words)| {
            total.saturating_add(buffer_byte_len(words.len(), PACKED_MODEL_DRAW_REF_BYTES))
        }))
        .saturating_add(
            transparent_model_draw_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_MODEL_DRAW_REF_BYTES))
                }),
        )
        .saturating_add(liquid_writes.iter().fold(0_u64, |total, (_, words)| {
            total.saturating_add(buffer_byte_len(words.len(), PACKED_LIQUID_QUAD_BYTES))
        }))
        .saturating_add(
            liquid_lighting_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_LIGHTING_BYTES))
                }),
        )
        .saturating_add(
            cube_lighting_writes
                .iter()
                .fold(0_u64, |total, (_, words)| {
                    total.saturating_add(buffer_byte_len(words.len(), PACKED_QUAD_LIGHTING_BYTES))
                }),
        );
    let origin_incremental_bytes = buffer_byte_len(origin_writes.len(), CHUNK_ORIGIN_BYTES);
    let biome_incremental_bytes = biome_writes.iter().fold(0_u64, |total, (_, words)| {
        total.saturating_add(buffer_byte_len(words.len(), BIOME_WORD_BYTES))
    });
    let quad_gpu_copy_bytes = ensure_quad_capacity(&mut arena, &render_device, &render_queue);
    let stream_gpu_copy_bytes =
        ensure_geometry_stream_capacities(&mut arena, &render_device, &render_queue);
    let origin_gpu_copy_bytes = ensure_origin_capacity(&mut arena, &render_device, &render_queue);
    let biome_gpu_copy_bytes = ensure_biome_capacity(&mut arena, &render_device, &render_queue);
    let gpu_copy_bytes = quad_gpu_copy_bytes
        .saturating_add(stream_gpu_copy_bytes)
        .saturating_add(origin_gpu_copy_bytes)
        .saturating_add(biome_gpu_copy_bytes);
    debug_assert_eq!(gpu_copy_bytes, upload_reservation.growth_copy_bytes);
    for (offset, words) in quad_writes {
        if !words.is_empty() {
            render_queue.write_buffer(
                &arena.quad_buffer,
                u64::from(offset) * PACKED_QUAD_BYTES,
                bytemuck::cast_slice(&words),
            );
        }
    }
    for (index, origin) in origin_writes {
        render_queue.write_buffer(
            &arena.origin_buffer,
            u64::from(index) * CHUNK_ORIGIN_BYTES,
            bytemuck::bytes_of(&origin),
        );
    }
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        model_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        model_lighting_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        model_draw_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        transparent_model_draw_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        liquid_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        liquid_lighting_writes,
    );
    write_stream_records(
        &render_queue,
        &arena.geometry_stream_buffer,
        GEOMETRY_STREAM_WORD_BYTES,
        cube_lighting_writes,
    );
    for (offset, words) in biome_writes {
        render_queue.write_buffer(
            &arena.biome_buffer,
            u64::from(offset) * BIOME_WORD_BYTES,
            bytemuck::cast_slice(&words),
        );
    }
    let applied_at = Instant::now();
    for (key, token, uploaded_bytes) in applied_tokens {
        acknowledgements.complete_with_bytes(key, token, applied_at, uploaded_bytes);
    }

    *upload_stats = account_chunk_gpu_uploads(
        *budget,
        upload_reservation.items,
        quad_incremental_bytes.saturating_add(stream_incremental_bytes),
        origin_incremental_bytes,
        biome_incremental_bytes,
        quad_gpu_copy_bytes.saturating_add(stream_gpu_copy_bytes),
        origin_gpu_copy_bytes,
        biome_gpu_copy_bytes,
    );
    debug_assert_eq!(upload_stats.total_bytes, upload_reservation.total_bytes());
    if upload_stats.chunk_updates > upload_stats.chunk_budget {
        bevy::log::warn!(
            "chunk GPU preparation observed {} updates despite a {}-chunk upload budget",
            upload_stats.chunk_updates,
            upload_stats.chunk_budget,
        );
    }
}

pub(in crate::chunk) fn chunk_instance_upload_byte_len(instance: &ChunkRenderInstance) -> u64 {
    buffer_byte_len(instance.cube_quads.len(), PACKED_QUAD_BYTES)
        .saturating_add(buffer_byte_len(
            instance.cube_lighting.len(),
            PACKED_QUAD_LIGHTING_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            instance.model_refs.len(),
            PACKED_MODEL_REF_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            instance.model_lighting.len(),
            PACKED_QUAD_LIGHTING_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            instance.model_draw_refs.len(),
            PACKED_MODEL_DRAW_REF_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            instance.transparent_model_draw_refs.len(),
            PACKED_MODEL_DRAW_REF_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            instance.liquid_quads.len(),
            PACKED_LIQUID_QUAD_BYTES,
        ))
        .saturating_add(buffer_byte_len(
            instance.liquid_lighting.len(),
            PACKED_QUAD_LIGHTING_BYTES,
        ))
        .saturating_add(CHUNK_ORIGIN_BYTES)
        .saturating_add(biome_record_byte_len(&instance.biome))
}

pub(in crate::chunk) fn liquid_quad_centroid(
    chunk_origin: [i32; 3],
    quad: PackedLiquidQuad,
) -> [f32; 3] {
    let origin = quad.origin();
    let heights = quad.heights();
    let average_height = heights.into_iter().map(f32::from).sum::<f32>() / (4.0 * 255.0);
    let mut centroid = [
        chunk_origin[0] as f32 + f32::from(origin[0]) + 0.5,
        chunk_origin[1] as f32 + f32::from(origin[1]) + average_height,
        chunk_origin[2] as f32 + f32::from(origin[2]) + 0.5,
    ];
    match quad.face() {
        Face::NegativeX => centroid[0] -= 0.5,
        Face::PositiveX => centroid[0] += 0.5,
        Face::NegativeY => centroid[1] = chunk_origin[1] as f32 + f32::from(origin[1]),
        Face::PositiveY => {}
        Face::NegativeZ => centroid[2] -= 0.5,
        Face::PositiveZ => centroid[2] += 0.5,
    }
    centroid
}

pub(in crate::chunk) fn transparent_allocation_matches(
    instance: &ChunkRenderInstance,
    allocation: &GpuChunkAllocation,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> bool {
    if instance.key != allocation.key
        || instance.generation != allocation.generation
        || instance.tint_identity != allocation.tint_identity
        || allocation.tint_identity != active_tint_identity
        || instance.liquid_quads.len() != instance.liquid_lighting.len()
    {
        return false;
    }
    let (Some(liquid), Some(lighting)) = (
        allocation.liquid_range.as_ref(),
        allocation.liquid_lighting_range.as_ref(),
    ) else {
        return instance.liquid_quads.is_empty();
    };
    liquid.start % 4 == 0
        && liquid.end % 4 == 0
        && lighting.start.is_multiple_of(2)
        && lighting.end.is_multiple_of(2)
        && usize::try_from(liquid.end.saturating_sub(liquid.start)).ok()
            == instance.liquid_quads.len().checked_mul(4)
        && usize::try_from(lighting.end.saturating_sub(lighting.start)).ok()
            == instance.liquid_lighting.len().checked_mul(2)
}

pub(in crate::chunk) fn packed_stream_range_matches(
    range: Option<&Range<u32>>,
    record_count: usize,
    words_per_record: usize,
) -> bool {
    match range {
        Some(range) => {
            record_count != 0
                && usize::try_from(range.start)
                    .ok()
                    .is_some_and(|start| start.is_multiple_of(words_per_record))
                && usize::try_from(range.end.saturating_sub(range.start)).ok()
                    == record_count.checked_mul(words_per_record)
        }
        None => record_count == 0,
    }
}

pub(in crate::chunk) fn transparent_model_allocation_matches(
    instance: &ChunkRenderInstance,
    allocation: &GpuChunkAllocation,
) -> bool {
    instance.key == allocation.key
        && instance.generation == allocation.generation
        && packed_stream_range_matches(
            allocation.model_range.as_ref(),
            instance.model_refs.len(),
            4,
        )
        && packed_stream_range_matches(
            allocation.model_lighting_range.as_ref(),
            instance.model_lighting.len(),
            2,
        )
        && packed_stream_range_matches(
            allocation.model_draw_range.as_ref(),
            instance.model_draw_refs.len(),
            2,
        )
        && packed_stream_range_matches(
            allocation.transparent_model_draw_range.as_ref(),
            instance.transparent_model_draw_refs.len(),
            2,
        )
}

pub(in crate::chunk) fn absolutize_model_lighting_bases(
    model_refs: &mut [[u32; 4]],
    lighting_word_start: u32,
) {
    let lighting_record_base = lighting_word_start / 2;
    for words in model_refs {
        words[2] = words[2]
            .checked_add(lighting_record_base)
            .expect("atomic model-lighting arena plan fits u32 record addressing");
    }
}

#[cfg(test)]
pub(in crate::chunk) fn validate_local_model_streams(
    model_refs: &[PackedModelRef],
    model_lighting: &[PackedQuadLighting],
    model_draw_refs: &[PackedModelDrawRef],
    model_templates: &[ModelTemplate],
) -> bool {
    let present = [
        !model_refs.is_empty(),
        !model_lighting.is_empty(),
        !model_draw_refs.is_empty(),
    ];
    if present.iter().any(|&value| value) && !present.iter().all(|&value| value) {
        return false;
    }
    if !present[0] {
        return true;
    }

    let mut draw_index = 0;
    let mut expected_lighting_base = 0_usize;
    for (model_ref_index, model_ref) in model_refs.iter().copied().enumerate() {
        let Ok(model_ref_index) = u32::try_from(model_ref_index) else {
            return false;
        };
        let words = model_ref.words();
        let lighting_base = words[2] as usize;
        let visible_mask = words[3];
        let Some(template) = model_templates.get(words[1] as usize) else {
            return false;
        };
        let Ok(template_quad_count) = usize::try_from(template.quad_count) else {
            return false;
        };
        if !(1..=32).contains(&template_quad_count)
            || visible_mask == 0
            || lighting_base != expected_lighting_base
        {
            return false;
        }
        let valid_mask = if template_quad_count == 32 {
            u32::MAX
        } else {
            (1_u32 << template_quad_count) - 1
        };
        if visible_mask & !valid_mask != 0 {
            return false;
        }
        let Some(lighting_end) = lighting_base.checked_add(template_quad_count) else {
            return false;
        };
        if lighting_end > model_lighting.len() {
            return false;
        }
        expected_lighting_base = lighting_end;
        let mut visible = words[3];
        while visible != 0 {
            let quad_index = visible.trailing_zeros();
            let Some(draw_ref) = model_draw_refs.get(draw_index).copied() else {
                return false;
            };
            let draw_words = draw_ref.words();
            if draw_words != [model_ref_index, quad_index]
                || quad_index >= 32
                || lighting_base
                    .checked_add(quad_index as usize)
                    .is_none_or(|index| index >= model_lighting.len())
            {
                return false;
            }
            draw_index += 1;
            visible &= visible - 1;
        }
    }
    draw_index == model_draw_refs.len() && expected_lighting_base == model_lighting.len()
}

pub(in crate::chunk) fn validate_partitioned_model_streams(
    model_refs: &[PackedModelRef],
    model_lighting: &[PackedQuadLighting],
    opaque_draw_refs: &[PackedModelDrawRef],
    blend_draw_refs: &[PackedModelDrawRef],
    model_templates: &[ModelTemplate],
    model_quads: &[assets::ModelQuad],
    materials: &[Material],
) -> bool {
    let any_draw = !opaque_draw_refs.is_empty() || !blend_draw_refs.is_empty();
    if model_refs.is_empty() != model_lighting.is_empty() || model_refs.is_empty() == any_draw {
        return false;
    }
    if model_refs.is_empty() {
        return true;
    }

    let mut opaque_index = 0;
    let mut blend_index = 0;
    let mut expected_lighting_base = 0_usize;
    for (model_ref_index, model_ref) in model_refs.iter().copied().enumerate() {
        let Ok(model_ref_index) = u32::try_from(model_ref_index) else {
            return false;
        };
        let words = model_ref.words();
        let lighting_base = words[2] as usize;
        let visible_mask = words[3];
        let Some(template) = model_templates.get(words[1] as usize) else {
            return false;
        };
        let Ok(template_quad_count) = usize::try_from(template.quad_count) else {
            return false;
        };
        let template_start = template.quad_start as usize;
        let Some(template_end) = template_start.checked_add(template_quad_count) else {
            return false;
        };
        let Some(template_quads) = model_quads.get(template_start..template_end) else {
            return false;
        };
        if !(1..=32).contains(&template_quad_count)
            || visible_mask == 0
            || lighting_base != expected_lighting_base
        {
            return false;
        }
        let valid_mask = if template_quad_count == 32 {
            u32::MAX
        } else {
            (1_u32 << template_quad_count) - 1
        };
        if visible_mask & !valid_mask != 0 {
            return false;
        }
        let Some(lighting_end) = lighting_base.checked_add(template_quad_count) else {
            return false;
        };
        if lighting_end > model_lighting.len() {
            return false;
        }
        expected_lighting_base = lighting_end;

        let mut visible = visible_mask;
        while visible != 0 {
            let quad_index = visible.trailing_zeros();
            let Some(quad) = template_quads.get(quad_index as usize) else {
                return false;
            };
            let is_blend = if quad.material == assets::DIAGNOSTIC_MATERIAL {
                false
            } else {
                let Some(material) = materials.get(quad.material as usize) else {
                    return false;
                };
                material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND != 0
            };
            let (draw_refs, draw_index) = if is_blend {
                (blend_draw_refs, &mut blend_index)
            } else {
                (opaque_draw_refs, &mut opaque_index)
            };
            let Some(draw_ref) = draw_refs.get(*draw_index).copied() else {
                return false;
            };
            if draw_ref.words() != [model_ref_index, quad_index]
                || lighting_base
                    .checked_add(quad_index as usize)
                    .is_none_or(|index| index >= model_lighting.len())
            {
                return false;
            }
            *draw_index += 1;
            visible &= visible - 1;
        }
    }
    opaque_index == opaque_draw_refs.len()
        && blend_index == blend_draw_refs.len()
        && expected_lighting_base == model_lighting.len()
}

#[cfg(test)]
pub(in crate::chunk) fn absolutize_model_draw_refs(
    draw_refs: &mut [[u32; 2]],
    model_word_start: u32,
) -> Option<()> {
    if !model_word_start.is_multiple_of(4) {
        return None;
    }
    let model_record_base = model_word_start / 4;
    if draw_refs
        .iter()
        .any(|words| words[0].checked_add(model_record_base).is_none())
    {
        return None;
    }
    for words in draw_refs {
        words[0] += model_record_base;
    }
    Some(())
}

pub(in crate::chunk) fn absolutize_partitioned_model_draw_refs(
    opaque_draw_refs: &mut [[u32; 2]],
    blend_draw_refs: &mut [[u32; 2]],
    model_word_start: u32,
) -> Option<()> {
    if !model_word_start.is_multiple_of(4) {
        return None;
    }
    let model_record_base = model_word_start / 4;
    if opaque_draw_refs
        .iter()
        .chain(blend_draw_refs.iter())
        .any(|words| words[0].checked_add(model_record_base).is_none())
    {
        return None;
    }
    for words in opaque_draw_refs
        .iter_mut()
        .chain(blend_draw_refs.iter_mut())
    {
        words[0] += model_record_base;
    }
    Some(())
}

pub(in crate::chunk) fn absolutize_liquid_lighting_indices(
    liquid_quads: &mut [[u32; 4]],
    lighting_word_start: u32,
) {
    let lighting_record_base = lighting_word_start / 2;
    for words in liquid_quads {
        words[3] = words[3]
            .checked_add(lighting_record_base)
            .expect("atomic liquid-lighting arena plan fits u32 record addressing");
    }
}

#[cfg(test)]
pub(in crate::chunk) const PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR: f32 = 0.2;
#[cfg(test)]
pub(in crate::chunk) const PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR: f32 = 0.04;

#[cfg(test)]
pub(in crate::chunk) fn packed_light_factor(sample: u16, daylight: f32) -> f32 {
    const CURVE: [f32; 16] = [
        0.0,
        0.017_543_86,
        0.037_037_037,
        0.058_823_53,
        0.083_333_336,
        0.111_111_11,
        0.142_857_15,
        0.179_487_18,
        0.222_222_22,
        0.272_727_28,
        0.333_333_34,
        0.407_407_4,
        0.5,
        0.619_047_64,
        0.777_777_8,
        1.0,
    ];
    let block_light = CURVE[usize::from(sample & 0x0f)];
    let effective_daylight = daylight
        .clamp(0.0, 1.0)
        .max(PROVISIONAL_NIGHT_SKY_TRANSFER_FLOOR);
    let sky_light = CURVE[usize::from((sample >> 4) & 0x0f)] * effective_daylight;
    let ao = f32::from((sample >> 8) & 0x03);
    let channel_light = block_light.max(sky_light);
    (PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR
        + (1.0 - PROVISIONAL_ZERO_LIGHT_AMBIENT_FLOOR) * channel_light)
        * (1.0 - ao * 0.12)
}

pub(in crate::chunk) fn packed_lighting_records(lighting: &[PackedQuadLighting]) -> Vec<[u16; 4]> {
    lighting
        .iter()
        .copied()
        .map(PackedQuadLighting::samples)
        .collect()
}
