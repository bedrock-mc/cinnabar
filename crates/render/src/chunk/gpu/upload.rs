use crate::chunk::*;
use meshing::liquid::LIQUID_FACE_INSET;
mod arena_writes;
mod lighting;
#[cfg(test)]
mod liquid_tests;
mod model_draw_bases;
mod publication_removals;
use arena_writes::ArenaWrites;
pub(in crate::chunk) use lighting::packed_lighting_records;
#[cfg(test)]
pub(in crate::chunk) use model_draw_bases::absolutize_model_draw_refs;
pub(in crate::chunk) use model_draw_bases::absolutize_partitioned_model_draw_refs;
use publication_removals::prepare_publication_removals;
type AllChunkInstances<'w, 's> = Query<'w, 's, (Entity, &'static ChunkRenderInstance)>;
type ChangedChunkInstances<'w, 's> =
    Query<'w, 's, (Entity, &'static ChunkRenderInstance), Changed<ChunkRenderInstance>>;
#[derive(SystemParam)]
pub(in crate::chunk) struct ChunkInstanceQueries<'w, 's> {
    queries: ParamSet<'w, 's, (AllChunkInstances<'w, 's>, ChangedChunkInstances<'w, 's>)>,
}
#[derive(SystemParam)]
pub(in crate::chunk) struct ChunkUploadPublication<'w> {
    acknowledgements: Res<'w, ChunkUploadAcknowledgements>,
    gpu_removals: Res<'w, ChunkGpuRemovalQueue>,
    terrain_generations:
        Option<ResMut<'w, crate::dropped_item_render::terrain_items::TerrainItemMeshGenerations>>,
}
#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_gpu_chunks(
    mut commands: Commands,
    mut instances: ChunkInstanceQueries<'_, '_>,
    views: Query<&ExtractedView, With<ExtractedCamera>>,
    mut removed_instances: RemovedComponents<ChunkRenderInstance>,
    mut arena: ResMut<ChunkGpuArena>,
    budget: Res<ChunkUploadBudget>,
    mut upload_stats: ResMut<ChunkGpuUploadStats>,
    biome_tints: Res<ChunkBiomeTints>,
    texture_assets: Res<ChunkTextureAssets>,
    publication: ChunkUploadPublication,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    retirement_fence: Res<TransparentRetirementFence>,
    mut fairness: ResMut<GpuUpdateFairness>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let ChunkUploadPublication {
        acknowledgements,
        gpu_removals,
        mut terrain_generations,
    } = publication;
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::GpuPreparation));
    release_completed_transparent_retirements(&mut arena, retirement_fence.completed_epoch());
    let active_tint_identity = biome_tints.table_identity();
    let tint_identity_changed = fairness.last_tint_identity != Some(active_tint_identity);
    let candidates = if tint_identity_changed || fairness.recover_untracked {
        instances
            .queries
            .p0()
            .iter()
            .map(|(entity, instance)| GpuUpdateCandidate::from_instance(entity, instance))
            .collect()
    } else {
        let mut candidates = Vec::with_capacity(fairness.wait_ages.len());
        {
            let all_instances = instances.queries.p0();
            candidates.extend(fairness.wait_ages.keys().filter_map(|&entity| {
                all_instances
                    .get(entity)
                    .ok()
                    .map(|(_, instance)| GpuUpdateCandidate::from_instance(entity, instance))
            }));
        }
        candidates.extend(
            instances
                .queries
                .p1()
                .iter()
                .filter(|(entity, _)| !fairness.wait_ages.contains_key(entity))
                .map(|(entity, instance)| GpuUpdateCandidate::from_instance(entity, instance)),
        );
        candidates
    };
    let urgent_updates = candidates
        .iter()
        .filter(|candidate| candidate.priority.is_urgent())
        .map(|candidate| candidate.entity)
        .collect::<Vec<_>>();
    fairness.last_tint_identity = Some(active_tint_identity);
    let camera_position = views
        .iter()
        .next()
        .map(|view| view.world_from_view.translation())
        .unwrap_or(Vec3::ZERO);
    let selected = plan_gpu_chunk_updates(
        candidates,
        &arena.allocations,
        camera_position,
        active_tint_identity,
        &fairness,
    );

    arena.pending_removals.extend(removed_instances.read());
    let retirement_pressure = prepare_publication_removals(
        &mut arena,
        *budget,
        &gpu_removals,
        &acknowledgements,
        terrain_generations.as_deref_mut(),
    );

    let mut writes = ArenaWrites::default();
    let mut applied_tokens = Vec::new();
    let mut applied_publication_permits = Vec::new();
    let mut successful_updates = Vec::new();
    let mut upload_reservation = GpuUploadReservation::default();
    upload_reservation.growth_copy_bytes = advance_arena_migration(
        &mut arena,
        &render_device,
        &render_queue,
        upload_reservation.migration_allowance(),
    );
    let all_instances = instances.queries.p0();
    for &entity in &selected {
        if arena
            .migration
            .as_ref()
            .is_some_and(ArenaMigration::blocks_arena_writes)
        {
            break;
        }
        let Ok((_, instance)) = all_instances.get(entity) else {
            continue;
        };
        let old = arena.allocations.get(&entity).cloned();
        // Deferred removals already occupy resident GPU ranges. Do not admit
        // more fresh chunks while completion cannot make retirement room.
        // Replacements still use their ordinary bounded COW admission below.
        if retirement_pressure && old.is_none() {
            continue;
        }
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
        if liquid_required != liquid_lighting_required {
            bevy::log::error!(
                "sub-chunk liquid-lighting count must exactly match the liquid-quad count"
            );
            continue;
        }
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
        let Some(origin_plan) = plan_origin_allocation(
            arena.origin_len,
            arena.free_origins.len(),
            arena.limits.max_origin_items,
            old.is_some(),
        ) else {
            bevy::log::warn!(
                "chunk origin arena cannot admit this allocation while preserving COW headroom"
            );
            continue;
        };
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
        // Every generation update rewrites GPU-visible data. Reusing a range
        // that a previously submitted frame can still read lets a queued
        // write mutate that frame in place, which presents as intermittent
        // blank water, whole-chunk flashes, or stale geometry. Allocate a
        // fresh complete allocation and keep the old one resident until the
        // queue-completion fence proves that no submitted frame can refer to
        // it anymore.
        let retirement = old
            .as_ref()
            .map(|old| RetiredArenaAllocation::full(entity, old.clone()));
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
        let Some(projected_ranges) = plan_fresh_chunk_ranges(&arena, stream_counts, biome_required)
        else {
            continue;
        };
        let required_lengths = ArenaRequiredLengths {
            quads: projected_ranges.quad_len,
            geometry_stream_words: projected_ranges.geometry_stream_len,
            origins: origin_plan.projected_origin_len,
            biome_words: projected_ranges.biome_len,
        };
        let fits = loop {
            match first_arena_growth(arena_capacities(&arena), required_lengths, arena.limits) {
                Err(_) => break false,
                Ok(None) => break true,
                Ok(Some((stream, growth))) => {
                    if arena.migration.is_none() {
                        begin_arena_migration(&mut arena, &render_device, stream, growth);
                    }
                    // A slice copy is submitted at once; stage this frame's earlier
                    // writes first so the copy carries them into the new buffer.
                    if !writes.is_empty() {
                        writes.issue(&arena, &render_queue);
                    }
                    upload_reservation.growth_copy_bytes += advance_arena_migration(
                        &mut arena,
                        &render_device,
                        &render_queue,
                        upload_reservation.migration_allowance(),
                    );
                    if arena.migration.is_some() {
                        break false;
                    }
                }
            }
        };
        if !fits {
            continue;
        }
        if let Some(slot) = &instance.publication_permit
            && (slot.stage() != Some(PublicationPermitStage::RenderEntity)
                || slot.is_zero_byte()
                || slot.bytes() != Some(instance_bytes))
        {
            drop(slot.take());
            continue;
        }
        if instance
            .token
            .is_some_and(|token| !acknowledgements.try_reserve(instance.key, token))
        {
            continue;
        }
        let mut next_upload_reservation = upload_reservation;
        let mut prepared_publication_permit = None;
        let reserved = if let Some(slot) = &instance.publication_permit {
            if !next_upload_reservation.try_reserve_permitted(instance_bytes) {
                false
            } else {
                let Some(permit) = slot.take() else {
                    if let Some(token) = instance.token {
                        acknowledgements.cancel(instance.key, token);
                    }
                    continue;
                };
                match permit.into_gpu_prepared() {
                    Ok(permit) => {
                        prepared_publication_permit = Some(permit);
                        true
                    }
                    Err(permit) => {
                        if let Err(permit) = slot.restore(permit) {
                            drop(permit);
                            unreachable!("the linear publication permit slot was taken once")
                        }
                        false
                    }
                }
            }
        } else {
            next_upload_reservation.try_reserve(*budget, instance_bytes)
        };
        if !reserved {
            if let Some(token) = instance.token {
                acknowledgements.cancel(instance.key, token);
            }
            continue;
        }
        commit_fresh_chunk_ranges(&mut arena, &projected_ranges);
        let plan = projected_ranges;
        if let Some(retirement) = retirement {
            let bytes = retirement.owned_bytes();
            assert!(arena.retirement_budget.try_reserve(1, bytes));
            arena.retired_allocations.push(retirement);
        }
        let metadata_index = allocate_origin(&mut arena)
            .expect("origin capacity was checked before quad allocation");
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
        writes.quads.push((plan.quad_start, words));
        writes.model.push((plan.model_start, model_words));
        writes
            .model_lighting
            .push((plan.model_lighting_start, model_lighting_words));
        writes
            .model_draw
            .push((plan.model_draw_start, model_draw_words));
        writes.transparent_model_draw.push((
            plan.transparent_model_draw_start,
            transparent_model_draw_words,
        ));
        writes.liquid.push((plan.liquid_start, liquid_words));
        writes
            .liquid_lighting
            .push((plan.liquid_lighting_start, liquid_lighting_words));
        writes
            .cube_lighting
            .push((plan.cube_lighting_start, cube_lighting_words));
        if !biome_words.is_empty() {
            writes.biome.push((plan.biome_start, biome_words));
        }
        writes.origins.push((metadata_index, origin));
        let gpu = GpuChunkAllocation {
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
            quad_range,
            cube_layout: instance
                .cube_layout
                .checked(&instance.cube_quads, texture_assets.assets().materials()),
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
            applied_tokens.push((instance.key, token, instance_bytes));
        }
        upload_reservation = next_upload_reservation;
        if let Some(permit) = prepared_publication_permit {
            applied_publication_permits.push(permit);
        }
        successful_updates.push(entity);
    }
    fairness.finish_frame(&selected, &successful_updates, &urgent_updates);

    writes.issue(&arena, &render_queue);
    let applied_at = Instant::now();
    for (key, token, uploaded_bytes) in applied_tokens {
        acknowledgements.complete_with_bytes(key, token, applied_at, uploaded_bytes);
        if let Some(terrain) = terrain_generations.as_deref_mut() {
            terrain.record(key, token.generation);
        }
    }
    for permit in applied_publication_permits {
        let retired = permit.retire();
        debug_assert!(retired);
    }

    *upload_stats = account_chunk_gpu_uploads(
        *budget,
        upload_reservation.items,
        upload_reservation.incremental_bytes,
        upload_reservation.growth_copy_bytes,
    );
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
    let top_inset = if quad.has_top_height_inset() {
        LIQUID_FACE_INSET
    } else {
        0.0
    };
    let mut centroid = [
        chunk_origin[0] as f32 + f32::from(origin[0]) + 0.5,
        chunk_origin[1] as f32 + f32::from(origin[1]) + average_height,
        chunk_origin[2] as f32 + f32::from(origin[2]) + 0.5,
    ];
    match quad.face() {
        Face::NegativeX => centroid[0] -= 0.5 - LIQUID_FACE_INSET,
        Face::PositiveX => centroid[0] += 0.5 - LIQUID_FACE_INSET,
        Face::NegativeY => centroid[1] = chunk_origin[1] as f32 + f32::from(origin[1]),
        Face::PositiveY => centroid[1] -= top_inset,
        Face::NegativeZ => centroid[2] -= 0.5 - LIQUID_FACE_INSET,
        Face::PositiveZ => centroid[2] += 0.5 - LIQUID_FACE_INSET,
    }
    if !matches!(quad.face(), Face::NegativeY | Face::PositiveY) {
        centroid[1] -= top_inset * 0.5;
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
