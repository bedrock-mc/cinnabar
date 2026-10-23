use crate::chunk::*;
use meshing::liquid::LIQUID_FACE_INSET;
use render_scene::{
    ChunkBiomeTints, ChunkRenderInstance, ChunkTextureAssets, ChunkUploadAcknowledgements,
    ChunkUploadBudget, RuntimeStageProfiler,
};
mod arena_writes;
#[allow(
    dead_code,
    reason = "the word-vector encoder serves chunk regression tests"
)]
mod lighting;
#[cfg(test)]
mod liquid_tests;
mod model_draw_bases;
pub(in crate::chunk) mod model_validation;
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
    queries: ParamSet<
        'w,
        's,
        (
            AllChunkInstances<'w, 's>,
            ChangedChunkInstances<'w, 's>,
            Query<'w, 's, &'static mut ChunkRenderInstance>,
        ),
    >,
}
#[derive(SystemParam)]
pub(in crate::chunk) struct ChunkUploadPublication<'w> {
    acknowledgements: Res<'w, ChunkUploadAcknowledgements>,
    textures: Option<Res<'w, ChunkGpuTextureAssets>>,
    gpu_removals: Res<'w, ChunkGpuRemovalQueue>,
    terrain_generations:
        Option<ResMut<'w, crate::dropped_item_render::terrain_items::TerrainItemMeshGenerations>>,
    coverage: Option<ResMut<'w, crate::chunk::TerrainCoverage>>,
    recovery: Option<Res<'w, super::super::recovery::TerrainGpuRecovery>>,
}
/// Sub-chunk uploads admitted per frame while a shader pack draws the terrain. The pack
/// expands at most `FRAME_EXPANSION_FACES` a frame from these sections, so admitting
/// a whole streaming burst (up to the publication's 512 items, ~50 µs of CPU each) only puts
/// the burst's cost into one frame; 24 a frame stays above the server's arrival rate.
const SHADER_PACK_FRAME_UPLOADS: usize = 24;
/// The upload budget, and whether a shader pack draws the terrain these uploads feed.
#[derive(SystemParam)]
pub(in crate::chunk) struct ChunkUploadLimits<'w> {
    budget: Res<'w, ChunkUploadBudget>,
    pack: Option<Res<'w, render_scene::shaderpack::ShaderPackDrawnCategories>>,
}
#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_gpu_chunks(
    mut commands: Commands,
    mut instances: ChunkInstanceQueries<'_, '_>,
    views: Query<&ExtractedView, With<ExtractedCamera>>,
    mut removed_instances: RemovedComponents<ChunkRenderInstance>,
    mut arena: ResMut<ChunkGpuArena>,
    limits: ChunkUploadLimits,
    capacity_hint: Option<Res<render_scene::chunk::capacity::ChunkArenaCapacityHint>>,
    mut upload_stats: ResMut<ChunkGpuUploadStats>,
    biome_tints: Res<ChunkBiomeTints>,
    texture_assets: Res<ChunkTextureAssets>,
    publication: ChunkUploadPublication,
    upload_queue: TerrainUploadQueue,
    retirement_fence: Res<TransparentRetirementFence>,
    mut fairness: ResMut<GpuUpdateFairness>,
    mut writes: Local<ArenaWrites>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let budget = &*limits.budget;
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("prepare_gpu_chunks").entered();
    let ChunkUploadPublication {
        acknowledgements,
        textures,
        gpu_removals,
        mut terrain_generations,
        mut coverage,
        recovery,
    } = publication;
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::GpuPreparation));
    release_completed_transparent_retirements(&mut arena, retirement_fence.completed_epoch());
    if recovery.as_ref().is_some_and(|recovery| recovery.waiting()) {
        *upload_stats = default();
        return;
    }
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
    let recovering = recovery
        .as_ref()
        .is_some_and(|recovery| recovery.recovering);

    for entity in removed_instances.read() {
        if let Some(coverage) = coverage.as_deref_mut() {
            coverage.invalidate();
        }
        arena.pending_removals.insert(entity);
        // Stop drawing immediately; GPU ranges retire behind their completion fence.
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<GpuChunkAllocation>();
        }
    }
    let retirement_pressure = prepare_publication_removals(
        &mut arena,
        *budget,
        &gpu_removals,
        &acknowledgements,
        terrain_generations.as_deref_mut(),
    );

    // A selected CPU atlas can outlive a session reset. Keep old draw tables coherent,
    // while the physical-removal and completion-fence work above continues to drain.
    if textures.as_ref().is_some_and(|textures| {
        textures.prepared.as_ref().is_none_or(|prepared| {
            !super::bind_groups::same_carrier(prepared.identity, texture_assets.binding_identity())
        })
    }) {
        fairness.recover_untracked = true;
        *upload_stats = ChunkGpuUploadStats::default();
        return;
    }

    let mut applied_tokens = Vec::new();
    let mut applied_publication_permits = Vec::new();
    let mut successful_updates = Vec::new();
    let mut upload_reservation = GpuUploadReservation::default();
    let mut result = Ok(());
    let uploads = upload_queue.uploads();
    upload_reservation.growth_copy_bytes = advance_arena_migration(
        &mut arena,
        uploads,
        upload_reservation.migration_allowance(),
    );
    let all_instances = instances.queries.p0();
    let capacity_target = super::capacity::backlog_capacity_target(
        &arena,
        selected.iter().filter_map(|entity| {
            all_instances
                .get(*entity)
                .ok()
                .map(|(_, instance)| instance)
        }),
        capacity_hint.as_deref().map_or(0, |hint| hint.sections),
    );
    let mut migrations_started = 0;
    let mut uploads_waiting_for_growth = 0;
    let mut candidates_examined = 0;
    let mut model_streams_validated = 0;
    let mut range_plans = 0;
    // Set when the device ran out of memory for a growth buffer; uploads needing growth then
    // wait for the next frame's attempt instead of retrying it per upload.
    let mut growth_failed = false;
    let frame_items = if limits.pack.as_deref().is_some_and(|drawn| drawn.terrain) {
        SHADER_PACK_FRAME_UPLOADS
    } else {
        budget
            .max_per_frame
            .max(PublicationServiceConfig::DEFAULT.maximum_frame_items)
    };
    // Spread large buffer allocations across frames while leaving room for uploads now.
    if !recovering
        && arena.origin_len == 0
        && arena.quad_len == 0
        && arena.geometry_stream_len == 0
        && arena.migration.is_none()
        && !arena.growth_blocked
    {
        let initial_target = super::capacity::initial_capacity_target(
            capacity_target,
            super::capacity::backlog_capacity_target(
                &arena,
                selected.iter().take(frame_items).filter_map(|entity| {
                    all_instances
                        .get(*entity)
                        .ok()
                        .map(|(_, instance)| instance)
                }),
                0,
            ),
        );
        while let Ok(Some((stream, minimum))) =
            first_arena_growth(arena_capacities(&arena), initial_target, arena.limits)
        {
            let growth = super::capacity::backlog_growth(&arena, stream, minimum, initial_target);
            if !begin_arena_migration(
                &mut arena,
                uploads.device,
                stream,
                growth,
                crate::gpu_allocation::Requirement::Optional,
            ) {
                break;
            }
            migrations_started += 1;
            upload_reservation.growth_copy_bytes += advance_arena_migration(
                &mut arena,
                uploads,
                upload_reservation.migration_allowance(),
            );
            if arena.migration.is_some() {
                break;
            }
        }
    }
    for entity in fairness.examination_order(&selected) {
        if upload_reservation.items >= frame_items {
            break;
        }
        let Ok((_, instance)) = all_instances.get(entity) else {
            continue;
        };
        candidates_examined += 1;
        let publication_permit = instance.publication_permit.as_ref();
        let token = instance.token;
        // Deferred removals already occupy resident GPU ranges. Do not admit
        // more fresh chunks while completion cannot make retirement room.
        // Replacements still use their ordinary bounded COW admission below.
        if retirement_pressure && !arena.allocations.contains_key(&entity) {
            continue;
        }
        let instance_bytes = chunk_instance_upload_byte_len(instance);
        if let Some(slot) = publication_permit
            && (slot.stage() != Some(PublicationPermitStage::RenderEntity)
                || slot.is_zero_byte()
                || slot.bytes() != Some(instance_bytes))
        {
            drop(slot.take());
            continue;
        }
        // This lower bound needs no stream walk, biome copy, or allocation plan.
        // Exact admission below also charges writes mirrored by arena migration.
        let mut preliminary = upload_reservation;
        let can_prepare = if recovering {
            preliminary.try_reserve_rebuild(*budget, instance_bytes)
        } else if publication_permit.is_some() {
            preliminary.try_reserve_permitted(instance_bytes)
        } else {
            preliminary.try_reserve(*budget, instance_bytes)
        };
        if !can_prepare {
            continue;
        }
        let validation_limit = fairness.limit;
        let (valid, validated) =
            fairness
                .model_validation
                .validate(entity, instance, &texture_assets, validation_limit);
        model_streams_validated += usize::from(validated);
        if !valid {
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
        if liquid_required != liquid_lighting_required {
            bevy::log::error!(
                "sub-chunk liquid-lighting count must exactly match the liquid-quad count"
            );
            continue;
        }
        let fallback_biome = biome_record_is_fallback(&instance.biome);
        // A remesh rarely changes biomes: an identical record keeps its resident range.
        let kept_biome = old
            .as_ref()
            .filter(|_| !fallback_biome)
            .and_then(|old| old.reusable_biome(&instance.biome));
        let biome_words = if fallback_biome || kept_biome.is_some() {
            &[][..]
        } else {
            instance.biome.words()
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
            if let Some(token) = token {
                acknowledgements.cancel(instance.key, token);
            }
            continue;
        }
        range_plans += 1;
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
                Ok(Some((stream, minimum))) => {
                    if arena.migration.is_none() {
                        if growth_failed {
                            break false;
                        }
                        // Earlier uploads reserved one write. Issue them before a migration
                        // can make the write path mirror them into an uncharged destination.
                        if !writes.is_empty() {
                            result = result.and(writes.issue(&arena, uploads));
                        }
                        let growth = if recovering {
                            minimum
                        } else {
                            super::capacity::backlog_growth(
                                &arena,
                                stream,
                                minimum,
                                capacity_target,
                            )
                        };
                        if !begin_arena_migration_or_minimum(
                            &mut arena,
                            uploads.device,
                            stream,
                            growth,
                            minimum,
                        ) {
                            growth_failed = true;
                            break false;
                        }
                        migrations_started += 1;
                    }
                    // A slice copy is submitted at once; stage this frame's earlier
                    // writes first so the copy carries them into the new buffer.
                    if !writes.is_empty() {
                        result = result.and(writes.issue(&arena, uploads));
                    }
                    upload_reservation.growth_copy_bytes += advance_arena_migration(
                        &mut arena,
                        uploads,
                        upload_reservation.migration_allowance(),
                    );
                    if arena.migration.is_some() {
                        break false;
                    }
                }
            }
        };
        if !fits {
            uploads_waiting_for_growth += 1;
            continue;
        }
        if token.is_some_and(|token| !acknowledgements.try_reserve(instance.key, token)) {
            continue;
        }
        let charged_bytes = instance_bytes.saturating_add(super::capacity::mirrored_upload_bytes(
            &arena,
            stream_counts,
            biome_required,
        ));
        let mut next_upload_reservation = upload_reservation;
        let mut prepared_publication_permit = None;
        let reserved = if recovering && publication_permit.is_none() {
            next_upload_reservation.try_reserve_rebuild(*budget, charged_bytes)
        } else if let Some(slot) = publication_permit {
            if !next_upload_reservation.try_reserve_permitted(charged_bytes) {
                false
            } else {
                let Some(permit) = slot.take() else {
                    if let Some(token) = token {
                        acknowledgements.cancel(instance.key, token);
                    }
                    continue;
                };
                prepared_publication_permit = Some((slot.clone(), permit));
                true
            }
        } else {
            next_upload_reservation.try_reserve(*budget, charged_bytes)
        };
        if !reserved {
            if let Some(token) = token {
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
        if let Some((range, _)) = &kept_biome {
            arena.shared_biome_ranges.share(range.start);
        }
        let (biome_range, biome_capacity) = kept_biome.unwrap_or((
            plan.biome_start..plan.biome_start + biome_required,
            plan.biome_capacity,
        ));
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
        let origin = gpu_chunk_origin(
            instance.origin,
            biome_range.start,
            plan.quad_start,
            plan.cube_lighting_start,
            plan.liquid_start,
            plan.liquid_lighting_start,
        )
        .expect("aligned arena layout and bounded biome offset produce a valid origin record");
        writes.collect_chunk(instance, &plan, metadata_index, biome_words, origin);
        // Ice and other transparent models interleave with the water's own face order.
        let order_independent_liquid = instance.has_transparent_liquid
            && transparent_model_draw_range.is_none()
            && liquid_quads_are_order_independent(
                instance
                    .liquid_quads
                    .get(
                        ..instance
                            .depth_liquid_start
                            .map_or(instance.liquid_quads.len(), |start| start as usize),
                    )
                    .unwrap_or_default(),
            );
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
            order_independent_liquid,
            metadata_index,
        };
        commands.entity(entity).insert(gpu.clone());
        if let Some(coverage) = coverage.as_deref_mut() {
            coverage.publish_full(instance.key, instance.generation);
        }
        arena.transparent_liquids.record(entity, &gpu);
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
                biome_range,
                biome_capacity,
                biome_record: (!fallback_biome).then(|| instance.biome.clone()),
                gpu,
            },
        );
        if let Some(token) = token {
            applied_tokens.push((instance.key, token, instance_bytes));
        }
        upload_reservation = next_upload_reservation;
        if let Some(permit) = prepared_publication_permit {
            applied_publication_permits.push(permit);
        }
        successful_updates.push(entity);
    }
    fairness.finish_frame(&selected, &successful_updates, &urgent_updates);

    result = result.and(writes.issue(&arena, uploads));
    if result.is_ok() {
        let applied_at = Instant::now();
        for (key, token, uploaded_bytes) in applied_tokens {
            acknowledgements.complete_with_bytes(key, token, applied_at, uploaded_bytes);
            if let Some(terrain) = terrain_generations.as_deref_mut() {
                terrain.record(key, token.generation);
            }
        }
        for (_, permit) in applied_publication_permits {
            let retired = permit
                .into_gpu_prepared()
                .unwrap_or_else(|_| unreachable!("validated publication permit"))
                .retire();
            debug_assert!(retired);
        }
        // Completed payloads retain no acknowledgement or permit to replay.
        let mut completed = instances.queries.p2();
        for &entity in &successful_updates {
            if let Ok(mut instance) = completed.get_mut(entity) {
                instance.token = None;
                instance.publication_permit = None;
            }
        }
    } else {
        for (key, token, _) in applied_tokens {
            acknowledgements.cancel(key, token);
        }
        for (slot, permit) in applied_publication_permits {
            slot.restore(permit)
                .unwrap_or_else(|_| unreachable!("the publication slot was taken once"));
        }
    }

    *upload_stats = account_chunk_gpu_uploads(
        *budget,
        upload_reservation.items,
        upload_reservation.incremental_bytes,
        upload_reservation.growth_copy_bytes,
    );
    upload_stats.migrations_started = migrations_started;
    upload_stats.uploads_waiting_for_growth = uploads_waiting_for_growth;
    upload_stats.candidates_examined = candidates_examined;
    upload_stats.model_streams_validated = model_streams_validated;
    upload_stats.range_plans = range_plans;
    upload_stats.candidates_deferred = selected.len().saturating_sub(successful_updates.len());
    #[cfg(feature = "tracy")]
    if let Some(client) = tracy_client::Client::running() {
        use tracy_client::plot_name;
        client.plot(
            plot_name!("terrain upload examined"),
            candidates_examined as f64,
        );
        client.plot(
            plot_name!("terrain upload validated"),
            model_streams_validated as f64,
        );
        client.plot(
            plot_name!("terrain upload admitted"),
            successful_updates.len() as f64,
        );
        client.plot(
            plot_name!("terrain upload deferred"),
            upload_stats.candidates_deferred as f64,
        );
    }
    if !recovering && upload_stats.chunk_updates > upload_stats.chunk_budget {
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
