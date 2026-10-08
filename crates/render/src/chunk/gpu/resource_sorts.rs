//! Transparent address sets are uploaded into the candidate arena before publication.
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
    // Liquid quads bound the sort's refs, so the fresh arena grows once before borrowing.
    let liquid_refs = instances
        .iter()
        .map(|(_, instance)| instance.liquid_quads.len())
        .sum::<usize>();
    let (device, queue) = (
        world.resource::<RenderDevice>().clone(),
        world.resource::<RenderQueue>().clone(),
    );
    ensure_transparent_ref_capacity(
        &mut world.resource_mut::<ChunkGpuArena>(),
        &device,
        &queue,
        liquid_refs,
        &liquids.state,
    );
    let arena = world.resource::<ChunkGpuArena>();
    let assets = world.resource::<ChunkTextureAssets>();
    let tints = world.resource::<ChunkBiomeTints>();
    let (mut manifest, mut model_candidates, mut model_manifest) =
        (Vec::new(), Vec::new(), Vec::new());
    let mut liquid_instances = HashMap::new();
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
            liquid_instances.insert(instance.key, instance);
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
    validate_transparent_sort_ref_count(model_candidates.len()).ok()?;
    let translation = view.transform.translation();
    let key = ViewSortKey::try_new(
        translation.to_array(),
        manifest,
        assets.identity(),
        tints.table_identity(),
    )
    .ok()?;
    let groups = key
        .visible_allocations
        .iter()
        .map(|identity| {
            build_transparent_group(liquid_instances[&identity.key], identity.clone(), tints)
                .map(Arc::new)
        })
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    validate_transparent_sort_ref_count(groups.iter().map(|group| group.centroids.len()).sum())
        .ok()?;
    liquids.view_entity = Some(view.entity);
    let generation = liquids.state.request(&key);
    let sorted = sort_transparent_groups(translation, &groups, &[]);
    let refs = sorted.refs;
    liquids.group_orders = sorted
        .fresh
        .into_iter()
        .map(|order| (order.identity.key, order))
        .collect();
    liquids
        .state
        .complete(TransparentSortResult::new(generation, key, refs).ok()?)
        .ok()?;
    while let Some(batch) = liquids.state.next_upload_batch() {
        let offset = transparent_ref_offset(
            batch.buffer_slot(),
            arena.transparent_slot_refs,
            batch.ref_range().start,
        );
        queue.write_buffer(
            &arena.transparent_ref_buffer,
            offset,
            bytemuck::cast_slice(batch.refs()),
        );
        liquids.state.acknowledge_upload();
    }
    let snapshot = liquids.state.committed()?;
    queue.write_buffer(
        &arena.transparent_indirect_buffer,
        0,
        bytemuck::bytes_of(&transparent_indirect_args(
            snapshot,
            arena.transparent_slot_refs,
        )?),
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
            &queue,
            u64::from(batch.draw_range.start) * GEOMETRY_STREAM_WORD_BYTES,
            bytemuck::cast_slice(&batch.words),
        );
        models.draw_orders.publish(&model_address, batch);
    }
    models.committed = Some(TransparentModelSortKey {
        view_entity: view.entity,
        order_camera: TransparentFaceMetric::new(translation).order_camera(
            model_address
                .allocations
                .iter()
                .map(|identity| identity.key),
        ),
        address: model_address,
    });
    Some((liquids, models))
}
