use super::groups::{build_transparent_group, spawn_transparent_sort};
use super::state::{
    TransparentAllocationIdentity, TransparentOrderedSnapshot, TransparentSortError,
    TransparentSortResult, TransparentSortRuntime, TransparentSortWork, ViewSortKey,
};
use super::{
    MAX_TRANSPARENT_VIEWS, PackedTransparentDrawRef, ensure_transparent_ref_capacity,
    transparent_indirect_args, transparent_ref_offset,
};
use crate::chunk::*;
use std::cell::RefCell;

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
    if snapshot.key.visible_allocations.is_empty() {
        return true;
    }
    // `ViewSortKey` keeps visible identities sorted by key with each key at most once, so every
    // allocation can satisfy only the identity it binary-searches to.
    let visible = &snapshot.key.visible_allocations;
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
    views: Query<(Entity, &ExtractedView, &RenderVisibleEntities), With<ExtractedCamera>>,
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
        // A fresh order is valid for its allocation and class whether or not it commits.
        for order in result.fresh {
            runtime.group_orders.insert(order.identity.key, order);
        }
        metrics.update(|snapshot| {
            snapshot.result_generation = result.generation.get();
            snapshot.cpu_duration = result.cpu_duration;
        });
        match result.refs {
            Ok(refs) => {
                let ref_bytes =
                    refs.len() as u64 * std::mem::size_of::<PackedTransparentDrawRef>() as u64;
                let sort_result = TransparentSortResult::with_patch(
                    result.generation,
                    result.key,
                    refs,
                    result.patch,
                )
                .expect("worker prevalidates the hard transparent reference ceiling");
                match runtime.state.complete(sort_result) {
                    Ok(true) => {
                        let patch = runtime.state.take_patch();
                        if let Some(snapshot) = runtime.state.committed()
                            && !patch.is_empty()
                        {
                            let mut patched_bytes = 0;
                            for span in patch {
                                upload_budget.consume(span.len());
                                patched_bytes += write_transparent_refs(
                                    &render_queue,
                                    &arena,
                                    snapshot.buffer_slot(),
                                    span.start,
                                    &snapshot.refs()[span],
                                );
                            }
                            metrics.update(|snapshot| {
                                snapshot.upload_bytes =
                                    snapshot.upload_bytes.saturating_add(patched_bytes);
                            });
                        }
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
            spawn_transparent_sort(runtime.result_sender.clone(), work, worker_profiler.clone());
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
    let camera = view.world_from_view.translation();
    let texture_identity = texture_assets.identity();
    let tint_identity = biome_tints.table_identity();
    let key =
        match ViewSortKey::try_new(camera.to_array(), manifest, texture_identity, tint_identity) {
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
            let mut entities = None;
            match runtime.resolve_candidate_cache(&key, |identity| {
                let entities = entities.get_or_insert_with(|| {
                    visible_entities
                        .get::<ChunkRenderInstance>()
                        .iter()
                        .filter_map(|&(entity, _)| Some((instances.get(entity).ok()?.key, entity)))
                        .collect::<HashMap<_, _>>()
                });
                let instance = entities
                    .get(&identity.key)
                    .and_then(|&entity| instances.get(entity).ok())
                    .ok_or(TransparentSortError::ConflictingAllocation { key: identity.key })?;
                build_transparent_group(instance, identity.clone(), &biome_tints)
            }) {
                Ok((groups, distinct_tint_count)) => {
                    let cached = runtime.cached_group_orders(&groups);
                    let base = runtime
                        .state
                        .committed()
                        .filter(|snapshot| snapshot.key.address_identity_eq(&key))
                        .map(|snapshot| Arc::clone(&snapshot.refs));
                    let work = TransparentSortWork {
                        generation,
                        requested_at,
                        key,
                        camera,
                        groups,
                        cached,
                        base,
                        distinct_tint_count,
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
    if let Some(batch) = runtime.state.next_upload_batch() {
        if !upload_budget.consume(batch.refs().len()) {
            bevy::log::error!(
                "transparent water sort batch exceeds the shared per-frame reference upload budget"
            );
            return;
        }
        uploaded_bytes = write_transparent_refs(
            &render_queue,
            &arena,
            batch.buffer_slot(),
            batch.ref_range().start,
            batch.refs(),
        );
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
