use super::*;

/// Publishes matching geometry and material tables only after their physical dependencies retire.
#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_chunk_texture_assets(
    mut commands: Commands,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    views: Query<
        (Entity, &ExtractedView, Has<render_scene::EnhancedRendering>),
        With<ExtractedCamera>,
    >,
    mut arena: ResMut<ChunkGpuArena>,
    assets: Res<ChunkTextureAssets>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu_assets: ResMut<ChunkGpuTextureAssets>,
    mut stats: ResMut<ChunkTextureUploadStats>,
    reload: Res<ChunkTextureReload>,
    mut coverage: Option<ResMut<TerrainCoverage>>,
) {
    let identity = assets.identity();
    let completed = gpu_assets.pending.as_ref().and_then(|pending| {
        match pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_recv()
        {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
        }
    });
    if let Some(completed) = completed {
        gpu_assets.pending = None;
        if let Some((pending, epoch)) = gpu_assets.pending_source.take() {
            if completed.is_none() {
                reload.finish(pending, epoch, false);
            }
        }
        gpu_assets.staged = completed;
    }
    let requested = reload.preparation();
    let source_epoch = reload.geometry_epoch();
    if gpu_assets
        .staged
        .as_ref()
        .is_some_and(|(prepared, _, _, _)| {
            (prepared.identity != identity
                || gpu_assets
                    .prepared
                    .as_ref()
                    .is_some_and(|active| active.identity == identity))
                && requested
                    .as_ref()
                    .is_none_or(|candidate| candidate.assets.identity() != prepared.identity)
        })
    {
        gpu_assets.staged = None;
    }
    // A reset cancels the unselected request in the mailbox. Retire its old worker
    // result without overwriting any new preparation started in the next session.
    if gpu_assets
        .staged
        .as_ref()
        .is_some_and(|(prepared, _, geometry, epoch)| {
            geometry.is_some() && *epoch != source_epoch && prepared.identity != identity
        })
    {
        gpu_assets.staged = None;
    }
    let ready = gpu_assets
        .staged
        .as_ref()
        .is_some_and(|(_, _, geometry, epoch)| {
            if let Some(geometry) = geometry.as_ref().filter(|_| *epoch == source_epoch) {
                geometry.ready(&instances, &arena)
            } else {
                // Atlas-only publication after a reset waits for every old physical draw.
                arena.allocations.is_empty() && arena.retired_allocations.is_empty()
            }
        });
    if let Some((prepared, _, _, _)) = gpu_assets.staged.as_ref() {
        if ready {
            reload.finish(prepared.identity, source_epoch, true);
        }
        if prepared.identity == identity {
            if !ready {
                return;
            }
            let (prepared, uploaded, geometry, epoch) =
                gpu_assets.staged.take().expect("matching staged atlas");
            if let Some(geometry) = geometry.filter(|_| epoch == source_epoch) {
                if let Err(geometry) = geometry.publish(
                    &mut commands,
                    &instances,
                    &mut arena,
                    coverage.as_deref_mut(),
                ) {
                    gpu_assets.staged = Some((prepared, uploaded, Some(geometry), epoch));
                    return;
                }
            }
            gpu_assets.prepared = Some(prepared);
            gpu_assets.attempted_identity = Some(identity);
            gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
            *stats = uploaded;
            reload.published(identity, source_epoch);
        }
    }
    // Bootstrap may publish without an optional reload transaction.
    if texture_asset_needs_rebuild(gpu_assets.attempted_identity, identity)
        && requested
            .as_ref()
            .is_none_or(|candidate| candidate.assets.identity() != identity)
        && (gpu_assets.prepared.is_none()
            || arena.allocations.is_empty() && arena.retired_allocations.is_empty())
    {
        gpu_assets.attempted_identity = Some(identity);
        gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
        if let Some((prepared, uploaded)) =
            build_chunk_texture_assets(&assets, &render_device, &render_queue)
        {
            gpu_assets.prepared = Some(prepared);
            *stats = uploaded;
        }
    }
    let Some(candidate) = reload.preparation() else {
        return;
    };
    if gpu_assets.staged.is_some()
        || gpu_assets.pending.is_some()
        || reload.status(candidate.assets.identity()).is_some()
    {
        return;
    }
    let device = render_device.clone();
    let queue = render_queue.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    gpu_assets.pending_source = Some((candidate.assets.identity(), candidate.epoch));
    gpu_assets.pending = Some(std::sync::Mutex::new(receiver));
    let geometry = candidate.geometry;
    let epoch = candidate.epoch;
    let candidate = candidate.assets;
    let view = views
        .iter()
        .min_by_key(|(entity, ..)| *entity)
        .map(
            |(entity, view, enhanced)| super::super::resource_sorts::ResourceView {
                entity,
                transform: view.world_from_view,
                sort_order_independent: view_displaces_water(enhanced),
            },
        );
    std::thread::spawn(move || {
        let result =
            build_chunk_texture_assets(&candidate, &device, &queue).and_then(|(atlas, stats)| {
                let geometry = match geometry {
                    Some(instances) => Some({
                        let _zone = bevy::log::info_span!(
                            "pack_apply.gpu_geometry",
                            chunks = instances.len()
                        )
                        .entered();
                        PreparedResourceGeometry::build(&instances, candidate, device, queue, view)?
                    }),
                    None => None,
                };
                Some((atlas, stats, geometry, epoch))
            });
        let _ = sender.send(result);
    });
}

#[cfg(test)]
mod tests;
