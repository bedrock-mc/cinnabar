use super::groups::spawn_transparent_sort;
use super::manifest::{build_resident_group, sorted_addresses_are_resident, view_displaces_water};
use super::state::{
    TransparentOrderedSnapshot, TransparentSortError, TransparentSortResult,
    TransparentSortRuntime, TransparentSortState, TransparentSortWork, ViewSortKey,
};
use super::{
    MAX_TRANSPARENT_VIEWS, PackedTransparentDrawRef, ensure_transparent_ref_capacity,
    transparent_indirect_args, transparent_ref_offset,
};
use crate::chunk::transparent::face_metric::TransparentFaceMetric;
use crate::chunk::*;
use std::cell::RefCell;

/// Whether every allocation `key` sorts is still readable from `resident_allocations`
/// (containing it) or `retired_allocations` (matching it exactly).
pub(in crate::chunk) fn transparent_snapshot_addresses_are_resident<'a, 'b>(
    key: &ViewSortKey,
    resident_allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
    retired_allocations: impl IntoIterator<Item = &'b GpuChunkAllocation>,
    active_asset_identity: ChunkTextureAssetIdentity,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> bool {
    if key.asset_identity != active_asset_identity || key.tint_identity != active_tint_identity {
        return false;
    }
    if key.sorted_allocations.is_empty() {
        return true;
    }
    // `ViewSortKey` keeps identities sorted by key with each key at most once, so every
    // allocation can satisfy only the identity it binary-searches to.
    let visible = &key.sorted_allocations;
    thread_local! {
        static SATISFIED: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    }
    SATISFIED.with_borrow_mut(|satisfied| {
        satisfied.clear();
        satisfied.resize(visible.len(), false);
        let mut remaining = visible.len();
        let mut mark =
            |allocation: &GpuChunkAllocation,
             matches: fn(&TransparentAllocationIdentity, &GpuChunkAllocation) -> bool| {
                if allocation.tint_identity != active_tint_identity {
                    return;
                }
                let Ok(index) =
                    visible.binary_search_by(|identity| identity.key.cmp(&allocation.key))
                else {
                    return;
                };
                if !satisfied[index] && matches(&visible[index], allocation) {
                    satisfied[index] = true;
                    remaining -= 1;
                }
            };
        for allocation in resident_allocations {
            mark(allocation, transparent_resident_allocation_contains);
        }
        for allocation in retired_allocations {
            mark(allocation, transparent_allocation_is_exact);
        }
        remaining == 0
    })
}

/// Writes the committed slot's `spans` from its CPU refs and returns the bytes written.
fn write_committed_spans(
    render_queue: &RenderQueue,
    arena: &ChunkGpuArena,
    state: &TransparentSortState,
    spans: Vec<Range<usize>>,
    upload_budget: &mut TransparentUploadBudget,
) -> u64 {
    let Some(snapshot) = state.committed() else {
        return 0;
    };
    let mut bytes = 0;
    for span in spans {
        // Urgent spans are written whatever the budget; the worker bounds them by it.
        if !upload_budget.consume(span.len()) {
            upload_budget.consume(upload_budget.remaining());
        }
        bytes += write_transparent_refs(
            render_queue,
            arena,
            snapshot.buffer_slot(),
            span.start,
            &snapshot.refs()[span],
        );
    }
    bytes
}

fn write_transparent_refs(
    render_queue: &RenderQueue,
    arena: &ChunkGpuArena,
    buffer_slot: u8,
    first_ref: usize,
    refs: &[PackedTransparentDrawRef],
) -> u64 {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "terrain.transparent_refs_write",
        buffer_slot,
        first_ref,
        refs = refs.len(),
        bytes = std::mem::size_of_val(refs),
    )
    .entered();
    render_queue.write_buffer(
        &arena.transparent_ref_buffer,
        transparent_ref_offset(buffer_slot, arena.transparent_slot_refs, first_ref),
        bytemuck::cast_slice(refs),
    );
    std::mem::size_of_val(refs) as u64
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_transparent_sorts(
    views: Query<
        (
            Entity,
            &ExtractedView,
            &RenderVisibleEntities,
            Has<crate::EnhancedRendering>,
        ),
        With<ExtractedCamera>,
    >,
    instances: Query<&ChunkRenderInstance>,
    diagnostic_instances: Query<(Entity, &ChunkRenderInstance)>,
    allocations: Query<&GpuChunkAllocation>,
    texture_assets: Res<ChunkTextureAssets>,
    biome_tints: Res<ChunkBiomeTints>,
    (render_device, render_queue): (Res<RenderDevice>, Res<RenderQueue>),
    mut arena: ResMut<ChunkGpuArena>,
    mut runtime: ResMut<TransparentSortRuntime>,
    metrics: Res<TransparentSortMetrics>,
    witness_request: Res<TransparentWitnessRequest>,
    witness_evidence: Res<TransparentWitnessEvidence>,
    mut upload_budget: ResMut<TransparentUploadBudget>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let worker_profiler = profiler.as_deref().cloned();
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::TransparentPreparation));
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
        match result.output {
            Ok(output) => {
                bevy::log::debug!(
                    generation = result.generation.get(),
                    sorted_refs = output.sorted_refs,
                    slot_refs = output.refs.len(),
                    in_place = output.patch.is_some(),
                    "transparent water sort result"
                );
                let ref_bytes = output.refs.len() as u64
                    * std::mem::size_of::<PackedTransparentDrawRef>() as u64;
                // A patch may extend the committed slot, so the slot grows before it commits.
                if ensure_transparent_ref_capacity(
                    &mut arena,
                    &render_device,
                    &render_queue,
                    output.refs.len(),
                    &runtime.state,
                ) {
                    runtime.last_indirect_identity = None;
                }
                let sort_result =
                    TransparentSortResult::planned(result.generation, result.key, output)
                        .expect("worker prevalidates the hard transparent reference ceiling");
                match runtime.state.complete(sort_result) {
                    Ok(true) => {
                        runtime.committed_distinct_tint_count = result.distinct_tint_count;
                        let ref_count = runtime
                            .state
                            .committed()
                            .map_or(0, TransparentOrderedSnapshot::live_ref_count);
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
        if let Some((_generation, mut work)) = next {
            // The queued job plans against whatever this result just committed.
            work.base = runtime.state.base_for(&work.key);
            spawn_transparent_sort(runtime.result_sender.clone(), work, worker_profiler.clone());
        }
        runtime.prune_request_metadata();
    }
    // Ranges a draw would otherwise misread land before anything else this frame.
    let urgent = runtime.state.take_urgent_patch();
    let patched_bytes = write_committed_spans(
        &render_queue,
        &arena,
        &runtime.state,
        urgent,
        &mut upload_budget,
    );
    if patched_bytes != 0 {
        metrics.update(|snapshot| {
            snapshot.upload_bytes = snapshot.upload_bytes.saturating_add(patched_bytes);
        });
    }

    let mut visible_views = views.iter().collect::<Vec<_>>();
    visible_views.sort_by_key(|(entity, ..)| *entity);
    if visible_views.len() > MAX_TRANSPARENT_VIEWS {
        bevy::log::warn!(
            "transparent chunk renderer supports one retained 3D view; extra views are rejected"
        );
        visible_views.truncate(MAX_TRANSPARENT_VIEWS);
    }
    let Some((view_entity, view, visible_entities, enhanced)) = visible_views.into_iter().next()
    else {
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

    let camera = view.world_from_view.translation();
    if !camera.is_finite() {
        fail_closed_transparent_sort_key_error(
            &mut runtime,
            &metrics,
            TransparentSortError::InvalidCameraTransform,
        );
        return;
    }
    let texture_identity = texture_assets.identity();
    let tint_identity = biome_tints.table_identity();
    // Displaced water can overlap itself even when flat, so such views sort all of it.
    let sort_order_independent = view_displaces_water(enhanced);
    runtime.direct_order_independent = !sort_order_independent;
    let metric = TransparentFaceMetric::new(camera);
    let arena_view: &ChunkGpuArena = &arena;
    let manifest = runtime.resident_manifest(
        arena_view,
        sort_order_independent,
        tint_identity,
        metric.camera_chunk(),
        |resident| build_resident_group(resident, &instances, arena_view, &biome_tints),
        &metrics,
    );
    let near = runtime.manifest_has_near(metric);
    let key = match ViewSortKey::from_canonical(
        camera,
        manifest,
        near,
        texture_identity,
        tint_identity,
    ) {
        Ok(key) => key,
        Err(error) => {
            fail_closed_transparent_sort_key_error(&mut runtime, &metrics, error);
            return;
        }
    };
    if witness_request.enabled() {
        let visible = visible_entities
            .get::<ChunkRenderInstance>()
            .iter()
            .map(|&(entity, _)| entity)
            .collect::<BTreeSet<_>>();
        let committed = runtime.state.committed();
        let drawn_directly = |key: SubChunkKey| {
            runtime.direct_order_independent
                && arena
                    .transparent_liquids
                    .get(key)
                    .is_some_and(|resident| resident.order_independent)
        };
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
                    // Water that blends the same in any order is drawn without the sort.
                    committed_member: committed
                        .is_some_and(|snapshot| snapshot.key().allocation(required).is_some())
                        || drawn_directly(required),
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
        let readable = |sorted: &ViewSortKey| {
            sorted.address_identity_eq(&key)
                || sorted_addresses_are_resident(sorted, &arena, texture_identity, tint_identity)
        };
        let committed_addresses_are_resident = runtime
            .state
            .committed()
            .is_some_and(|snapshot| readable(&snapshot.key));
        let staged_addresses_are_resident = runtime.state.staged_key().is_some_and(readable);
        let canceled_staged = runtime.state.staged_generation();
        let generation = runtime.state.request_retaining_resident_snapshot(
            &key,
            committed_addresses_are_resident,
            staged_addresses_are_resident,
        );
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
            let work = TransparentSortWork {
                generation,
                requested_at,
                base: runtime.state.base_for(&key),
                key,
                camera,
                groups: runtime.manifest_groups(),
                upload_cap: runtime.state.upload_cap,
            };
            runtime.requested_at.insert(generation, requested_at);
            let (start, replaced) = runtime.gate.submit_with_replacement(generation, work);
            if let Some(replaced) = replaced {
                runtime.requested_at.remove(&replaced);
                runtime.staged_distinct_tint_counts.remove(&replaced);
            }
            if let Some((_generation, work)) = start {
                spawn_transparent_sort(
                    runtime.result_sender.clone(),
                    work,
                    worker_profiler.clone(),
                );
            }
            runtime.prune_request_metadata();
        }
    }

    let staged_refs = runtime.state.staged_ref_count();
    if ensure_transparent_ref_capacity(
        &mut arena,
        &render_device,
        &render_queue,
        staged_refs,
        &runtime.state,
    ) {
        runtime.last_indirect_identity = None;
    }
    let mut staged_bytes = 0;
    if let Some(batch) = runtime.state.next_upload_batch() {
        if upload_budget.consume(batch.refs().len()) {
            staged_bytes = write_transparent_refs(
                &render_queue,
                &arena,
                batch.buffer_slot(),
                batch.ref_range().start,
                batch.refs(),
            );
        } else {
            bevy::log::error!(
                "transparent water sort batch exceeds the shared per-frame reference upload budget"
            );
        }
    }
    // Lagging orders of committed groups take whatever budget the staged slot left.
    let lagging = runtime.state.take_patch_within(upload_budget.remaining());
    let uploaded_bytes = staged_bytes
        + write_committed_spans(
            &render_queue,
            &arena,
            &runtime.state,
            lagging,
            &mut upload_budget,
        );
    if uploaded_bytes != 0 {
        metrics.update(|snapshot| {
            snapshot.upload_bytes = snapshot.upload_bytes.saturating_add(uploaded_bytes);
        });
    }
    if staged_bytes != 0 {
        let committed = runtime.state.acknowledge_upload();
        if committed
            && let Some((generation, ref_count)) = runtime
                .state
                .committed()
                .map(|snapshot| (snapshot.generation(), snapshot.live_ref_count()))
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
            transparent_indirect_args(snapshot, arena.transparent_slot_refs)?,
        ))
    }) && runtime.last_indirect_identity != Some(identity)
    {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "terrain.transparent_indirect_write",
            buffer_slot = identity.0,
            refs = identity.1
        )
        .entered();
        render_queue.write_buffer(
            &arena.transparent_indirect_buffer,
            0,
            bytemuck::bytes_of(&command),
        );
        runtime.last_indirect_identity = Some(identity);
    }
}
