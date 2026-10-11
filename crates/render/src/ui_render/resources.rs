//! Shared UI GPU resource creation, publication admission and arena accounting.

use super::*;

/// Creates the shared viewport uniform and samplers for this render device.
pub(super) fn init_ui_gpu(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    tick: SystemChangeTick,
) {
    let viewport_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("shared UI viewport uniform"),
        contents: bytemuck::bytes_of(&UiViewportUniform {
            viewport_size: [1.0, 1.0],
            time_seconds: 0.0,
            glint_strength: UiGlintSettings::default().strength,
        }),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let sampler_with = |label, filter| {
        render_device.create_sampler(&SamplerDescriptor {
            label: Some(label),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..default()
        })
    };
    let sampler = sampler_with("shared nearest UI texture sampler", FilterMode::Nearest);
    let linear_sampler = sampler_with("shared bilinear UI texture sampler", FilterMode::Linear);
    commands.insert_resource(UiGpu {
        device: render_device.wgpu_device().clone(),
        device_observation: DeviceObservation::new(tick.this_run()),
        vertex_buffer: None,
        index_buffer: None,
        vertex_capacity: 0,
        index_capacity: 0,
        vertex_arena_id: 0,
        index_arena_id: 0,
        viewport_buffer,
        viewport_uploads: viewport::ViewportUploads::default(),
        #[cfg(test)]
        geometry_writes: [0; 2],
        viewport_size: [1, 1],
        started: std::time::Instant::now(),
        textures: UiGpuTextures::default(),
        sampler,
        linear_sampler,
        batches: Arc::from([]),
        accepted_revision: None,
        animated: false,
        last_admitted_revision: None,
        last_admitted_publication: Weak::new(),
        index_count: 0,
        uploads: uploads::BufferUploads::default(),
        view_pipelines: std::collections::BTreeMap::new(),
        composite_pipelines: std::collections::BTreeMap::new(),
        world_view_pipelines: std::collections::BTreeMap::new(),
        model_view_pipelines: std::collections::BTreeMap::new(),
    });
}

/// Optional preparation inputs share one system parameter without requiring diagnostics.
type UiPreparationOptions<'w> = (
    Option<Res<'w, UiHandCoverage>>,
    Option<Res<'w, UiGlintSettings>>,
    Option<Res<'w, profile::UiProfile>>,
    Option<Res<'w, crate::upload_staging::BufferUploadStaging>>,
);

/// Admits a publication and updates its shared GPU storage without waiting for readback.
pub(crate) fn prepare_ui_resources(
    scene: Res<UiRenderSceneResource>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<UiGpu>,
    stats: Res<UiRenderStatsResource>,
    tick: SystemChangeTick,
    (coverage, glint, profile, staging): UiPreparationOptions<'_>,
) {
    let same_device = &gpu.device == render_device.wgpu_device();
    let device_valid =
        gpu.device_observation
            .observe(render_device.last_changed(), tick.this_run(), same_device);
    if let Some(coverage) = coverage {
        coverage.clear();
    }
    let Some(input) = scene.input.as_ref() else {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        stats.update(|s| {
            s.accepted_revision = None;
            s.draw_calls = 0;
        });
        return;
    };
    if !device_valid {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        record_render_rejection(
            &stats,
            input.revision,
            UiRenderRejectReason::InvalidTextureExtent,
        );
        return;
    }
    let animated = if gpu.accepted_revision == Some(input.revision) {
        gpu.animated
    } else {
        input
            .vertices
            .iter()
            .any(|vertex| vertex.style_flags & render_model::UI_STYLE_GLINT != 0)
    };
    let started = gpu.started;
    let UiGpu {
        viewport_uploads,
        viewport_buffer,
        ..
    } = &mut *gpu;
    viewport_uploads.upload(
        input.viewport_size,
        glint.as_deref().copied().unwrap_or_default(),
        animated,
        || started.elapsed().as_secs_f32(),
        |uniform| {
            #[cfg(feature = "tracy")]
            let _span =
                bevy::log::info_span!("ui.viewport_write", bytes = size_of::<UiViewportUniform>())
                    .entered();
            crate::upload_staging::write_batch(
                staging.as_deref(),
                &render_device,
                &render_queue,
                &[(&*viewport_buffer, 0, bytemuck::bytes_of(uniform))],
            );
            if let Some(profile) = profile.as_deref() {
                profile.record_upload(
                    profile::UploadKind::Viewport,
                    size_of::<UiViewportUniform>() as u64,
                );
            }
        },
    );
    if let Some(previous) = gpu.last_admitted_revision {
        let reason = if input.revision < previous {
            Some(UiRenderRejectReason::StaleRevision {
                current: previous,
                rejected: input.revision,
            })
        } else if input.revision == previous
            && !gpu.last_admitted_publication.ptr_eq(&Arc::downgrade(input))
        {
            Some(UiRenderRejectReason::RevisionConflict {
                revision: input.revision,
            })
        } else {
            None
        };
        if let Some(reason) = reason {
            gpu.accepted_revision = None;
            gpu.batches = Arc::from([]);
            record_render_rejection(&stats, input.revision, reason);
            return;
        }
    }
    if gpu.accepted_revision == Some(input.revision) {
        if !gpu.textures.resident(&input.textures)
            || (!input.vertices.is_empty() && gpu.vertex_buffer.is_none())
            || (!input.indices.is_empty() && gpu.index_buffer.is_none())
        {
            gpu.accepted_revision = None;
            gpu.batches = Arc::from([]);
            record_render_rejection(
                &stats,
                input.revision,
                UiRenderRejectReason::InvalidTextureExtent,
            );
        }
        return;
    }
    if let Err(reason) = input.validate() {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        record_render_rejection(&stats, input.revision, reason);
        return;
    }
    if let Err(reason) = gpu.textures.prepare(
        &input.textures,
        &render_device,
        &render_queue,
        profile.as_deref(),
    ) {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        record_render_rejection(&stats, input.revision, reason);
        return;
    }

    if let Err(reason) = gpu
        .textures
        .prepare_fonts(input, &render_queue, profile.as_deref())
    {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        record_render_rejection(&stats, input.revision, reason);
        return;
    }

    let fresh_vertices = gpu.vertex_capacity < input.vertices.len();
    let fresh_indices = gpu.index_capacity < input.indices.len();
    if fresh_vertices {
        let capacity = arena_capacity(input.vertices.len(), MAX_UI_VERTICES);
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.vertex_allocate",
            bytes = arena_bytes(capacity, size_of::<FontAtlasVertex>())
        )
        .entered();
        gpu.vertex_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("shared bounded UI vertex arena"),
            size: arena_bytes(capacity, size_of::<FontAtlasVertex>()),
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.vertex_capacity = capacity;
        gpu.vertex_arena_id = gpu.vertex_arena_id.saturating_add(1);
    }
    if fresh_indices {
        let capacity = arena_capacity(input.indices.len(), MAX_UI_INDICES);
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.index_allocate",
            bytes = arena_bytes(capacity, size_of::<u32>())
        )
        .entered();
        gpu.index_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("shared bounded UI index arena"),
            size: arena_bytes(capacity, size_of::<u32>()),
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.index_capacity = capacity;
        gpu.index_arena_id = gpu.index_arena_id.saturating_add(1);
    }
    let mut upload = gpu.uploads.plan(input, fresh_vertices, fresh_indices);
    upload.vertices = uploads::changed_range(
        &gpu.textures.fonts.previous_vertices,
        &gpu.textures.fonts.vertices,
        fresh_vertices,
    );
    if let Some(buffer) = gpu.vertex_buffer.as_ref()
        && !upload.vertices.is_empty()
    {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.vertex_write",
            revision = input.revision,
            vertices = upload.vertices.len(),
            bytes = upload.vertices.len() * size_of::<FontAtlasVertex>(),
        )
        .entered();
        render_queue.write_buffer(
            buffer,
            (upload.vertices.start * size_of::<FontAtlasVertex>()) as u64,
            bytemuck::cast_slice(&gpu.textures.fonts.vertices[upload.vertices.clone()]),
        );
        if let Some(profile) = profile.as_deref() {
            profile.record_upload(
                profile::UploadKind::Geometry,
                (upload.vertices.len() * size_of::<FontAtlasVertex>()) as u64,
            );
        }
        #[cfg(test)]
        {
            gpu.geometry_writes[0] += 1;
        }
    }
    if let Some(buffer) = gpu.index_buffer.as_ref()
        && !upload.indices.is_empty()
    {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.index_write",
            revision = input.revision,
            indices = upload.indices.len(),
            bytes = upload.indices.len() * size_of::<u32>(),
        )
        .entered();
        render_queue.write_buffer(
            buffer,
            (upload.indices.start * size_of::<u32>()) as u64,
            bytemuck::cast_slice(&input.indices[upload.indices.clone()]),
        );
        if let Some(profile) = profile.as_deref() {
            profile.record_upload(
                profile::UploadKind::Geometry,
                (upload.indices.len() * size_of::<u32>()) as u64,
            );
        }
        #[cfg(test)]
        {
            gpu.geometry_writes[1] += 1;
        }
    }
    gpu.textures.fonts.commit_vertices();
    gpu.viewport_size = input.viewport_size;

    gpu.batches = Arc::clone(&input.batches);
    gpu.animated = animated;
    gpu.index_count = input.indices.len();
    gpu.accepted_revision = Some(input.revision);
    gpu.last_admitted_revision = Some(input.revision);
    gpu.last_admitted_publication = Arc::downgrade(input);
    stats.update(|stats| {
        stats.accepted_revision = Some(input.revision);
        stats.uploaded_vertices = upload.vertices.len() as u32;
        stats.uploaded_indices = upload.indices.len() as u32;
        stats.draw_calls = input.batches.len() as u32;
        stats.vertex_arena_capacity = gpu.vertex_capacity as u32;
        stats.index_arena_capacity = gpu.index_capacity as u32;
        stats.per_node_gpu_allocations = 0;
        stats.retained_gpu_bytes =
            retained_gpu_bytes(gpu.vertex_capacity, gpu.index_capacity, gpu.textures.bytes);
        stats.rejected_revision = None;
        stats.rejected_reason = None;
    });
}

/// Counts rejected publications without discarding the admission watermark.
fn record_render_rejection(stats: &UiRenderStats, revision: u64, reason: UiRenderRejectReason) {
    static REJECTIONS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let count = REJECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if count.is_power_of_two() {
        bevy::log::warn!(count, revision, ?reason, "UI frame rejected");
    }
    stats.update(|stats| {
        stats.accepted_revision = None;
        stats.draw_calls = 0;
        stats.rejected_revision = Some(revision);
        stats.rejected_reason = Some(reason);
        stats.rejection_count = stats.rejection_count.saturating_add(1);
    });
}

/// Bounds arena growth to the shared vertex or index limit.
pub(super) fn arena_capacity(required: usize, limit: usize) -> usize {
    if required == 0 {
        return 0;
    }
    required
        .checked_next_power_of_two()
        .unwrap_or(limit)
        .min(limit)
}

/// Keeps empty buffer descriptors nonzero with a bounded byte conversion.
fn arena_bytes(capacity: usize, stride: usize) -> u64 {
    u64::try_from(capacity.saturating_mul(stride).max(4)).expect("bounded UI arena byte count")
}

/// Counts shared vertex, index, texture and viewport buffer storage.
pub(super) fn retained_gpu_bytes(vertices: usize, indices: usize, texture_bytes: usize) -> u64 {
    let bytes = vertices
        .saturating_mul(size_of::<FontAtlasVertex>())
        .saturating_add(indices.saturating_mul(size_of::<u32>()))
        .saturating_add(texture_bytes)
        .saturating_add(size_of::<UiViewportUniform>());
    bytes as u64
}
