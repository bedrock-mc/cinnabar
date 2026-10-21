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
    let identity = assets.binding_identity();
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
            #[cfg(feature = "enhanced")]
            {
                gpu_assets.authored_pending = AuthoredUpload::for_assets(
                    &assets,
                    gpu_assets
                        .prepared
                        .as_ref()
                        .expect("published carrier atlas"),
                    &render_device,
                );
            }
        }
    }
    #[cfg(feature = "enhanced")]
    if gpu_assets
        .authored_pending
        .as_ref()
        .is_some_and(|pending| !pending.matches(identity))
    {
        gpu_assets.authored_pending = None;
    }
    #[cfg(feature = "enhanced")]
    let authored_only = gpu_assets
        .prepared
        .as_ref()
        .is_some_and(|prepared| same_carrier(prepared.identity, identity));
    #[cfg(not(feature = "enhanced"))]
    let authored_only = false;
    // Bootstrap may publish without an optional reload transaction.
    if texture_asset_needs_rebuild(gpu_assets.attempted_identity, identity)
        && requested
            .as_ref()
            .is_none_or(|candidate| candidate.assets().binding_identity() != identity)
        && (gpu_assets.prepared.is_none()
            || authored_only
            || arena.allocations.is_empty() && arena.retired_allocations.is_empty())
    {
        gpu_assets.attempted_identity = Some(identity);
        gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
        rebuild_bootstrap_atlas(
            &assets,
            &mut gpu_assets,
            &mut stats,
            &render_device,
            &render_queue,
        );
    }
    #[cfg(feature = "enhanced")]
    if let Some(pending) = gpu_assets.authored_pending.as_mut() {
        pending.step(&render_queue, authored_upload::FRAME_UPLOAD_BYTES);
        if pending.complete() {
            let completed = gpu_assets
                .authored_pending
                .take()
                .expect("complete authored upload");
            completed.publish(
                gpu_assets
                    .prepared
                    .as_mut()
                    .expect("retained carrier atlas"),
                &mut stats,
            );
        }
    }
    let Some(candidate) = reload.preparation() else {
        return;
    };
    if gpu_assets.staged.is_some()
        || gpu_assets.pending.is_some()
        || reload
            .status(candidate.assets().binding_identity())
            .is_some()
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
    let previous = gpu_assets.prepared.clone();
    let work = move || {
        let result = build_carrier_texture_assets(&candidate, previous.as_ref(), &device, &queue)
            .and_then(|(atlas, stats)| {
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
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(work);
    #[cfg(target_arch = "wasm32")]
    crate::browser_jobs::submit(crate::browser_jobs::Lane::Textures, work);
}

/// Installs the current atlas without a reload transaction. Enhanced keeps an unchanged carrier
/// and replaces only its authored maps, which upload over later frames.
fn rebuild_bootstrap_atlas(
    assets: &ChunkTextureAssets,
    gpu_assets: &mut ChunkGpuTextureAssets,
    stats: &mut ChunkTextureUploadStats,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) {
    #[cfg(feature = "enhanced")]
    let identity = assets.binding_identity();
    #[cfg(feature = "enhanced")]
    if gpu_assets
        .prepared
        .as_ref()
        .is_some_and(|prepared| same_carrier(prepared.identity, identity))
    {
        if let Some(authored) = assets.enhanced() {
            gpu_assets.authored_pending =
                AuthoredUpload::new(identity, authored.clone(), render_device);
            if gpu_assets.authored_pending.is_none() {
                bevy::log::warn!("invalid authored terrain upload; retained previous bindings");
            }
        } else {
            authored_upload::detach(
                gpu_assets
                    .prepared
                    .as_mut()
                    .expect("existing carrier atlas"),
                identity,
                render_device,
                stats,
            );
        }
        return;
    }
    #[cfg(feature = "enhanced")]
    {
        gpu_assets.authored_pending = None;
    }
    if let Some((prepared, uploaded)) = build_carrier_texture_assets(
        assets,
        gpu_assets.prepared.as_ref(),
        render_device,
        render_queue,
    ) {
        gpu_assets.prepared = Some(prepared);
        *stats = uploaded;
        #[cfg(feature = "enhanced")]
        {
            gpu_assets.authored_pending = AuthoredUpload::for_assets(
                assets,
                gpu_assets
                    .prepared
                    .as_ref()
                    .expect("published carrier atlas"),
                render_device,
            );
        }
    }
}

#[cfg(test)]
mod tests;
