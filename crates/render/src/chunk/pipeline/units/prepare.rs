use render_model::terrain_units::{IndexedUnitPage, QUADS_PER_UNIT, TerrainUnit};

use super::TerrainUnitBatches;
use crate::chunk::gpu_cull::direct::DirectOcclusion;
use crate::chunk::*;
use crate::upload_staging::TerrainUploadStaging;

/// Updates geometry slots separately from the current pass's ordered visibility references.
pub(in crate::chunk) fn prepare_units(
    mut batches: ResMut<TerrainUnitBatches>,
    occlusion: Option<Res<DirectOcclusion>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    staging: Option<Res<TerrainUploadStaging>>,
) {
    let batches = &mut *batches;
    batches.views.retain(|_, view| view.queued);
    if batches.views.is_empty() {
        return;
    }
    if batches.index_buffer.is_none() {
        batches.index_buffer = Some(device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("terrain unit indices"),
            contents: bytemuck::cast_slice(&super::indices()),
            usage: BufferUsages::INDEX,
        }));
    }
    let limits = device.limits();
    let capacity = IndexedUnitPage::capacity_for(
        limits
            .max_buffer_size
            .min(limits.max_storage_buffer_binding_size),
    );
    let mut stats = batches.stats;
    for (&entity, view) in &mut batches.views {
        let facing = view.camera.and_then(meshing::facing_cell);
        for (stream, batch) in view.streams.iter_mut().enumerate() {
            batch.scratch.clear();
            let sections = &view.sources.streams[stream];
            let lookup = &view.sources.lookup[stream];
            if view.stale {
                #[cfg(feature = "tracy")]
                let _span =
                    bevy::log::info_span!("terrain.unit_descriptor_deltas", stream).entered();
                batch.indexed = batch.descriptors.synchronize(sections, lookup, capacity);
            }
            let early = occlusion
                .as_ref()
                .and_then(|state| state.solid_order(entity));
            if stream == 0
                && let Some(order) = early
            {
                stats.phase_items += u64::from(!order.is_empty());
                batch
                    .scratch
                    .extend(order.iter().map(|&(entity, _)| entity));
            } else {
                batch.scratch.extend(
                    sections
                        .iter()
                        .filter(|section| {
                            !occlusion
                                .as_ref()
                                .is_some_and(|state| state.skips(entity, section.entity))
                        })
                        .map(|section| section.entity),
                );
            }
            if view.stale
                || batch.selected != batch.scratch
                || (stream == 0 && view.facing != facing)
            {
                #[cfg(feature = "tracy")]
                let _span = bevy::log::info_span!(
                    "terrain.unit_compact",
                    stream,
                    sections = batch.scratch.len()
                )
                .entered();
                std::mem::swap(&mut batch.selected, &mut batch.scratch);
                batch.references.clear();
                batch.fallback.clear();
                if batch.indexed {
                    batch.padded_quads = batch.descriptors.select(
                        &batch.selected,
                        view.camera,
                        &mut batch.references,
                    );
                } else {
                    for entity in &batch.selected {
                        let Some(&index) = lookup.get(entity) else {
                            continue;
                        };
                        let section = &sections[index];
                        let mask = super::descriptors::facing(section.key, view.camera);
                        TerrainUnit::compact(
                            &section.units,
                            |unit| super::descriptors::faces(unit, mask),
                            &mut batch.fallback,
                        );
                    }
                    batch.padded_quads = batch
                        .fallback
                        .iter()
                        .map(|unit| u64::from(QUADS_PER_UNIT - unit.quad_count))
                        .sum();
                }
                let Ok(uploaded) = super::buffers::upload(
                    batch,
                    capacity,
                    &device,
                    &queue,
                    &cache,
                    staging.as_deref(),
                ) else {
                    return;
                };
                stats.descriptor_bytes_uploaded += uploaded.descriptors;
                stats.reference_bytes_uploaded += uploaded.references;
                stats.list_bytes_uploaded += uploaded.descriptors + uploaded.references;
                batch.descriptors.dirty.clear();
            }
            stats.draws += batch.pages.len() as u64;
            stats.units += (batch.references.len() + batch.fallback.len()) as u64;
            stats.padded_quads += batch.padded_quads;
            for page in &mut batch.pages {
                if let Some(occlusion) = &occlusion {
                    occlusion.prepare_unit_masks(
                        device.wgpu_device(),
                        &page.buffer,
                        &mut page.masks,
                    );
                }
                let group = u64::from(render_model::terrain_units::UNITS_PER_INSTANCE);
                let count = u64::from(page.count);
                stats.padded_quads +=
                    (count.div_ceil(group) * group - count) * u64::from(QUADS_PER_UNIT);
            }
        }
        if let Some(order) = occlusion.as_ref().and_then(|s| s.occluder_order(entity)) {
            let Ok(uploaded) = prepare_occluders(
                view,
                order,
                capacity,
                &device,
                &queue,
                &cache,
                staging.as_deref(),
            ) else {
                return;
            };
            stats.descriptor_bytes_uploaded += uploaded.descriptors;
            stats.reference_bytes_uploaded += uploaded.references;
            stats.list_bytes_uploaded += uploaded.descriptors + uploaded.references;
            stats.draws += view.occluders.pages.len() as u64;
            stats.occluder_units += view.occluders.fallback.len() as u64;
        }
        view.stale = false;
        view.facing = facing;
    }
    batches.stats = stats;
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "terrain.unit_batches",
        phase_items = stats.phase_items,
        draws = stats.draws,
        units = stats.units,
        occluder_units = stats.occluder_units,
        padded_quads = stats.padded_quads,
        list_bytes_uploaded = stats.list_bytes_uploaded,
        descriptor_bytes_uploaded = stats.descriptor_bytes_uploaded,
        reference_bytes_uploaded = stats.reference_bytes_uploaded,
    )
    .entered();
}

/// Keeps a bounded, near-first depth-only subset separate from the original color ordering.
fn prepare_occluders(
    view: &mut super::UnitView,
    order: &[(Entity, MainEntity)],
    capacity: usize,
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
    staging: Option<&TerrainUploadStaging>,
) -> Result<super::buffers::Uploaded, crate::gpu_allocation::TerrainWriteError> {
    const MAX_OCCLUDER_UNITS: usize = 1024;
    let stream = &mut view.occluders;
    stream.indexed = false;
    stream.fallback.clear();
    for &(entity, _) in order {
        let Some(&index) = view.sources.lookup[0].get(&entity) else {
            continue;
        };
        let section = &view.sources.streams[0][index];
        let facing = super::descriptors::facing(section.key, view.camera);
        stream.fallback.extend(
            section
                .units
                .iter()
                .filter(|u| super::descriptors::faces(u, facing))
                .map(TerrainUnit::descriptor)
                .take(MAX_OCCLUDER_UNITS - stream.fallback.len()),
        );
        if stream.fallback.len() == MAX_OCCLUDER_UNITS {
            break;
        }
    }
    super::buffers::upload(stream, capacity, device, queue, cache, staging)
}
