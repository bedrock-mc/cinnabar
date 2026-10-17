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
    if gpu_assets
        .pending
        .as_ref()
        .is_some_and(|pending| !reload.accepts(&pending.request, identity))
    {
        gpu_assets.pending = None;
    }
    if gpu_assets
        .staged
        .as_ref()
        .is_some_and(|staged| !reload.accepts(&staged.request, identity))
    {
        gpu_assets.staged = None;
    }
    let completed = gpu_assets.pending.as_ref().and_then(|pending| {
        match pending
            .result
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
        let pending = gpu_assets.pending.take().expect("completed worker");
        if reload.is_current(&pending.request) {
            if let Some(replacement) = completed {
                gpu_assets.staged = Some(StagedTextures {
                    request: pending.request,
                    replacement,
                });
            } else {
                reload.finish(&pending.request, false);
            }
        }
    }
    let requested = reload.preparation();
    let ready = gpu_assets.staged.as_ref().is_some_and(|staged| {
        let geometry = &staged.replacement.2;
        if let Some(geometry) = geometry {
            geometry.ready(&instances, &arena)
        } else {
            // Atlas-only publication waits for every physical draw.
            arena.allocations.is_empty() && arena.retired_allocations.is_empty()
        }
    });
    if let Some(staged) = gpu_assets.staged.as_ref() {
        if ready {
            reload.finish(&staged.request, true);
        }
        if staged.replacement.0.identity == identity {
            if !ready {
                return;
            }
            let StagedTextures {
                request,
                replacement: (prepared, uploaded, geometry),
            } = gpu_assets.staged.take().expect("matching staged atlas");
            if let Some(geometry) = geometry
                && let Err(geometry) = geometry.publish(
                    &mut commands,
                    &instances,
                    &mut arena,
                    coverage.as_deref_mut(),
                )
            {
                gpu_assets.staged = Some(StagedTextures {
                    request,
                    replacement: (prepared, uploaded, Some(geometry)),
                });
                return;
            }
            gpu_assets.prepared = Some(prepared);
            gpu_assets.attempted_identity = Some(identity);
            gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
            *stats = uploaded;
            reload.published(&request);
        }
    }
    // Bootstrap may publish without an optional reload transaction.
    if texture_asset_needs_rebuild(gpu_assets.attempted_identity, identity)
        && requested
            .as_ref()
            .is_none_or(|candidate| candidate.assets().identity() != identity)
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
        || reload.status(candidate.assets().identity()).is_some()
    {
        return;
    }
    let device = render_device.clone();
    let queue = render_queue.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let geometry = candidate.geometry().cloned();
    let assets = candidate.assets().clone();
    gpu_assets.pending = Some(PendingTextures {
        request: candidate,
        result: std::sync::Mutex::new(receiver),
    });
    let candidate = assets;
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
                Some((atlas, stats, geometry))
            });
        let _ = sender.send(result);
    });
}

#[cfg(test)]
mod tests;
