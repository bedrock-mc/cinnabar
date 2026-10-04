//! Transparent address sets are uploaded into the candidate arena before publication.
use crate::chunk::transparent::sort::{quantized_camera_orientation, quantized_camera_position};
use crate::chunk::*;

pub(super) struct ResourceView {
    pub(super) entity: Entity,
    pub(super) transform: GlobalTransform,
}

/// Sorts all residents so camera movement cannot expose an unstaged transparent chunk.
pub(super) fn prepare(
    app: &mut App,
    view: Option<ResourceView>,
) -> Option<(TransparentSortRuntime, TransparentModelSortRuntime)> {
    let mut liquids = TransparentSortRuntime::default();
    let mut models = TransparentModelSortRuntime::default();
    let Some(view) = view else {
        return Some((liquids, models));
    };
    let world = app.world_mut();
    let instances: Vec<_> = world
        .query::<(Entity, &ChunkRenderInstance)>()
        .iter(world)
        .map(|(entity, instance)| (entity, instance.clone()))
        .collect();
    let arena = world.resource::<ChunkGpuArena>();
    let assets = world.resource::<ChunkTextureAssets>();
    let queue = world.resource::<RenderQueue>();
    let (mut candidates, mut manifest, mut model_candidates, mut model_manifest) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (entity, instance) in &instances {
        let Some(allocation) = arena.allocations.get(entity).map(|entry| &entry.gpu) else {
            continue;
        };
        if allocation.has_transparent_liquid {
            let range = allocation.liquid_range.clone()?;
            manifest.push(TransparentAllocationIdentity::new(
                instance.key,
                allocation.generation,
                range.clone(),
                allocation.liquid_lighting_range.clone()?,
                allocation.metadata_index,
            ));
            let end = instance
                .depth_liquid_start
                .map_or(instance.liquid_quads.len(), |start| start as usize);
            for (index, quad) in instance.liquid_quads[..end].iter().enumerate() {
                candidates.push(TransparentSortCandidate::new(
                    instance.key,
                    index as u32,
                    range.start / 4 + index as u32,
                    allocation.metadata_index,
                    instance.origin.map(|value| value as f32 + 8.0),
                    liquid_quad_centroid(instance.origin, *quad),
                ));
            }
        }
        if let (Some(model_range), Some(draw_range)) = (
            &allocation.model_range,
            &allocation.transparent_model_draw_range,
        ) {
            model_manifest.push(TransparentModelAllocationIdentity {
                entity: *entity,
                key: instance.key,
                generation: allocation.generation,
                model_range: model_range.clone(),
                draw_range: draw_range.clone(),
            });
            for (index, draw) in instance.transparent_model_draw_refs.iter().enumerate() {
                let (centroid, words) = transparent_model_draw_candidate(
                    instance.key,
                    &instance.model_refs,
                    *draw,
                    assets.assets().model_templates(),
                    assets.assets().model_quads(),
                    model_range.start / 4,
                )?;
                model_candidates.push(TransparentModelSortCandidate {
                    entity: *entity,
                    key: instance.key,
                    draw_range: draw_range.clone(),
                    stable_index: index as u32,
                    centroid,
                    words,
                });
            }
        }
    }
    validate_transparent_sort_ref_count(candidates.len()).ok()?;
    validate_transparent_sort_ref_count(model_candidates.len()).ok()?;
    let (_, rotation, translation) = view.transform.to_scale_rotation_translation();
    let matrix = Mat4::from(view.transform.affine().inverse());
    let key = ViewSortKey::try_new(
        quantized_camera_position(translation.to_array()),
        quantized_camera_orientation(rotation.to_array()),
        manifest,
        assets.identity(),
        world.resource::<ChunkBiomeTints>().table_identity(),
    )
    .ok()?;
    liquids.view_entity = Some(view.entity);
    let generation = liquids.state.request(&key);
    let refs = sort_transparent_candidates(matrix, candidates.into());
    liquids
        .state
        .complete(TransparentSortResult::new(generation, key, refs).ok()?)
        .ok()?;
    while let Some(batch) = liquids.state.next_upload_batch() {
        let offset = batch.buffer_slot() as usize * TRANSPARENT_REF_SLOT_BYTES
            + batch.ref_range().start * std::mem::size_of::<PackedTransparentDrawRef>();
        queue.write_buffer(
            &arena.transparent_ref_buffer,
            offset as u64,
            bytemuck::cast_slice(batch.refs()),
        );
        liquids.state.acknowledge_upload();
    }
    let snapshot = liquids.state.committed()?;
    queue.write_buffer(
        &arena.transparent_indirect_buffer,
        0,
        bytemuck::bytes_of(&transparent_indirect_args(snapshot)?),
    );
    liquids.last_indirect_identity = Some((snapshot.buffer_slot(), snapshot.refs().len()));
    model_manifest.sort_by_key(|entry| (entry.key, entry.draw_range.start));
    let model_address = TransparentModelAddressIdentity {
        asset_identity: assets.identity(),
        allocations: model_manifest.into(),
    };
    for batch in sort_transparent_model_candidates(translation, model_candidates.into()) {
        write_geometry_stream_words(
            arena,
            queue,
            u64::from(batch.draw_range.start) * GEOMETRY_STREAM_WORD_BYTES,
            bytemuck::cast_slice(&batch.words),
        );
        models.draw_orders.publish(&model_address, batch);
    }
    models.committed = Some(TransparentModelSortKey {
        view_entity: view.entity,
        camera_position_bits: crate::chunk::transparent::model::camera_position_bits(translation)?,
        address: model_address,
    });
    Some((liquids, models))
}
