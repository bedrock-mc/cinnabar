use super::resource_geometry::PreparedResourceGeometry;
use crate::chunk::*;

#[cfg(test)]
mod water_tint_tests;

#[derive(Clone, PartialEq, Eq)]
pub(in crate::chunk) struct ChunkBindGroupBuffers {
    pub(in crate::chunk) view: BufferId,
    pub(in crate::chunk) quads: BufferId,
    pub(in crate::chunk) origins: BufferId,
    pub(in crate::chunk) biomes: BufferId,
    pub(in crate::chunk) materials: BufferId,
    pub(in crate::chunk) animations: BufferId,
    pub(in crate::chunk) animation_frames: BufferId,
    pub(in crate::chunk) animation_clock: BufferId,
    pub(in crate::chunk) model_templates: BufferId,
    pub(in crate::chunk) geometry_streams: BufferId,
    pub(in crate::chunk) transparent_refs: BufferId,
    pub(in crate::chunk) biome_tints: BufferId,
    pub(in crate::chunk) atmosphere: BufferId,
    pub(in crate::chunk) biome_tint_table: ChunkBiomeTintResourceIdentity,
    pub(in crate::chunk) textures: ChunkTextureAssetIdentity,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(in crate::chunk) struct MaterialGpu {
    pub(in crate::chunk) texture: u32,
    pub(in crate::chunk) flags: u32,
    pub(in crate::chunk) animation: u32,
    pub(in crate::chunk) variation_start: u32,
    pub(in crate::chunk) variation_count: u32,
    pub(in crate::chunk) variation_weight: u32,
}

pub(in crate::chunk) const _: () =
    assert!(std::mem::size_of::<MaterialGpu>() == assets::MATERIAL_BYTES);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(in crate::chunk) struct AnimationGpu {
    pub(in crate::chunk) frame_start: u32,
    pub(in crate::chunk) frame_count: u32,
    pub(in crate::chunk) ticks_per_frame: u32,
    pub(in crate::chunk) flags: u32,
    pub(in crate::chunk) uv_scale: f32,
}

pub(in crate::chunk) const _: () = assert!(std::mem::size_of::<AnimationGpu>() == 5 * 4);

#[repr(C, align(16))]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(in crate::chunk) struct BiomeTintGpu {
    pub(in crate::chunk) grass: u32,
    pub(in crate::chunk) foliage: u32,
    pub(in crate::chunk) birch: u32,
    pub(in crate::chunk) evergreen: u32,
    pub(in crate::chunk) dry_foliage: u32,
    pub(in crate::chunk) water: u32,
    pub(in crate::chunk) flags: u32,
    pub(in crate::chunk) water_opacity: f32,
    pub(in crate::chunk) seasonal_foliage: [[f32; 4]; assets::SEASONAL_FOLIAGE_COUNT],
}

pub(in crate::chunk) const _: () =
    assert!(std::mem::size_of::<BiomeTintGpu>() == 8 * 4 + assets::SEASONAL_FOLIAGE_COUNT * 16);

pub(in crate::chunk) fn pack_linear_rgb10(rgb: [f32; 3]) -> u32 {
    let component = |value: f32| {
        if value.is_finite() {
            (value.clamp(0.0, 1.0) * 1023.0).round() as u32
        } else {
            0
        }
    };
    component(rgb[0]) | (component(rgb[1]) << 10) | (component(rgb[2]) << 20)
}

pub(in crate::chunk) fn prepare_biome_tint_entries(entries: &[BiomeTint]) -> Vec<BiomeTintGpu> {
    entries
        .iter()
        .map(|entry| BiomeTintGpu {
            grass: pack_linear_rgb10(entry.grass),
            foliage: pack_linear_rgb10(entry.foliage),
            birch: pack_linear_rgb10(entry.birch),
            evergreen: pack_linear_rgb10(entry.evergreen),
            dry_foliage: pack_linear_rgb10(entry.dry_foliage),
            // Native water tint is RGB8. Linear RGB10 loses several dark-blue
            // palette bytes before the ordinary liquid colour is quantized.
            water: u32::from_le_bytes(
                Color::linear_rgb(entry.water[0], entry.water[1], entry.water[2])
                    .to_srgba()
                    .to_u8_array(),
            ),
            flags: entry.flags,
            water_opacity: entry.water_opacity,
            seasonal_foliage: entry.seasonal_foliage.map(|rgb| {
                // Seasonal world colours can exceed one; only the CPU
                // particle path clamps doubled palette samples in native.
                let [r, g, b] = rgb.map(|channel| if channel.is_finite() { channel } else { 0.0 });
                [r, g, b, 1.0]
            }),
        })
        .collect()
}

pub(in crate::chunk) struct PreparedChunkBiomeTints {
    pub(in crate::chunk) identity: ChunkBiomeTintResourceIdentity,
    pub(in crate::chunk) buffer: Buffer,
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkGpuBiomeTints {
    pub(in crate::chunk) prepared: Option<PreparedChunkBiomeTints>,
    pub(in crate::chunk) _retained_entries: Option<Arc<[BiomeTint]>>,
}

pub(in crate::chunk) fn biome_tint_gpu_buffer_needs_rebuild(
    current: Option<ChunkBiomeTintResourceIdentity>,
    next: ChunkBiomeTintResourceIdentity,
) -> bool {
    current != Some(next)
}

pub(in crate::chunk) fn biome_tint_bind_group_needs_rebuild(
    current: Option<ChunkBiomeTintResourceIdentity>,
    next: ChunkBiomeTintResourceIdentity,
) -> bool {
    current != Some(next)
}

pub(in crate::chunk) fn prepare_chunk_biome_tints(
    render_device: Res<RenderDevice>,
    source: Res<ChunkBiomeTints>,
    mut gpu: ResMut<ChunkGpuBiomeTints>,
) {
    let identity = source.resource_identity();
    if !biome_tint_gpu_buffer_needs_rebuild(
        gpu.prepared.as_ref().map(|prepared| prepared.identity),
        identity,
    ) {
        return;
    }
    let entries = prepare_biome_tint_entries(source.entries());
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("packed chunk biome tints"),
        contents: bytemuck::cast_slice(&entries),
        usage: BufferUsages::STORAGE,
    });
    gpu._retained_entries = Some(Arc::clone(&source.entries));
    gpu.prepared = Some(PreparedChunkBiomeTints { identity, buffer });
}

pub(in crate::chunk) struct PreparedChunkTextureAssets {
    pub(in crate::chunk) identity: ChunkTextureAssetIdentity,
    pub(in crate::chunk) material_buffer: Buffer,
    pub(in crate::chunk) animation_buffer: Buffer,
    pub(in crate::chunk) animation_frame_buffer: Buffer,
    pub(in crate::chunk) model_template_buffer: Buffer,
    pub(in crate::chunk) _textures: [Texture; 2],
    pub(in crate::chunk) views: [TextureView; 2],
    pub(in crate::chunk) native_leaf_views: [TextureView; assets::MAX_TEXTURE_PAGES],
    pub(in crate::chunk) sampler: Sampler,
    pub(in crate::chunk) native_leaf_sampler: Sampler,
}

#[derive(Resource)]
pub(in crate::chunk) struct ChunkGpuAnimationClock {
    pub(in crate::chunk) buffer: Buffer,
}

pub(in crate::chunk) fn init_chunk_gpu_animation_clock(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
) {
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animation clock"),
        contents: bytemuck::bytes_of(&ChunkAnimationClock::default()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    commands.insert_resource(ChunkGpuAnimationClock { buffer });
}

pub(in crate::chunk) fn prepare_chunk_animation_clock(
    clock: Res<ChunkAnimationClock>,
    gpu_clock: Res<ChunkGpuAnimationClock>,
    render_queue: Res<RenderQueue>,
) {
    render_queue.write_buffer(&gpu_clock.buffer, 0, bytemuck::bytes_of(&*clock));
}

type PreparedReplacement = (
    PreparedChunkTextureAssets,
    ChunkTextureUploadStats,
    Option<PreparedResourceGeometry>,
);

type PendingTextures = std::sync::Mutex<std::sync::mpsc::Receiver<Option<PreparedReplacement>>>;

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkGpuTextureAssets {
    pub(in crate::chunk) attempted_identity: Option<ChunkTextureAssetIdentity>,
    pub(in crate::chunk) _attempted_assets: Option<Arc<RuntimeAssets>>,
    pub(in crate::chunk) prepared: Option<PreparedChunkTextureAssets>,
    pending: Option<PendingTextures>,
    pending_identity: Option<ChunkTextureAssetIdentity>,
    staged: Option<PreparedReplacement>,
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChunkTextureUploadStats {
    pub upload_count: u64,
    pub material_bytes: u64,
    pub animation_bytes: u64,
    pub animation_frame_bytes: u64,
    pub texture_bytes_including_mips: u64,
    pub padded_upload_bytes: u64,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_chunk_texture_assets(
    mut commands: Commands,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    views: Query<(Entity, &ExtractedView), With<ExtractedCamera>>,
    mut arena: ResMut<ChunkGpuArena>,
    assets: Res<ChunkTextureAssets>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu_assets: ResMut<ChunkGpuTextureAssets>,
    mut stats: ResMut<ChunkTextureUploadStats>,
    reload: Res<ChunkTextureReload>,
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
        if let Some(pending) = gpu_assets.pending_identity.take() {
            reload.finish(pending, completed.is_some());
        }
        gpu_assets.staged = completed;
    }
    let requested = reload.requested();
    if gpu_assets.staged.as_ref().is_some_and(|(prepared, _, _)| {
        prepared.identity != identity
            && requested
                .as_ref()
                .is_none_or(|candidate| candidate.identity() != prepared.identity)
    }) {
        gpu_assets.staged = None;
    }
    if gpu_assets
        .staged
        .as_ref()
        .is_some_and(|(prepared, _, _)| prepared.identity == identity)
    {
        let (prepared, uploaded, geometry) =
            gpu_assets.staged.take().expect("matching staged atlas");
        if let Some(geometry) = geometry
            && !geometry.publish(&mut commands, &instances, &mut arena)
        {
            reload.finish(identity, false);
            gpu_assets.attempted_identity = Some(identity);
            bevy::log::warn!("discarding resource geometry with retired chunk keys");
            return;
        }
        reload.published();
        gpu_assets.prepared = Some(prepared);
        gpu_assets.attempted_identity = Some(identity);
        gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
        *stats = uploaded;
    }
    // Bootstrap may publish without an optional reload transaction.
    if texture_asset_needs_rebuild(gpu_assets.attempted_identity, identity) {
        gpu_assets.attempted_identity = Some(identity);
        gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
        if let Some((prepared, uploaded)) =
            build_chunk_texture_assets(&assets, &render_device, &render_queue)
        {
            gpu_assets.prepared = Some(prepared);
            *stats = uploaded;
        }
    }
    let Some(candidate) = requested else {
        return;
    };
    if candidate.identity() == identity
        || gpu_assets.pending.is_some()
        || reload.status(candidate.identity()).is_some()
    {
        return;
    }
    let device = render_device.clone();
    let queue = render_queue.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    gpu_assets.pending_identity = Some(candidate.identity());
    gpu_assets.pending = Some(std::sync::Mutex::new(receiver));
    let geometry = reload.geometry();
    let view = views
        .iter()
        .min_by_key(|(entity, _)| *entity)
        .map(|(entity, view)| super::resource_sorts::ResourceView {
            entity,
            transform: view.world_from_view,
        });
    std::thread::spawn(move || {
        let result =
            build_chunk_texture_assets(&candidate, &device, &queue).and_then(|(atlas, stats)| {
                let geometry = match geometry {
                    Some(instances) => Some(PreparedResourceGeometry::build(
                        &instances, candidate, device, queue, view,
                    )?),
                    None => None,
                };
                Some((atlas, stats, geometry))
            });
        let _ = sender.send(result);
    });
}

/// Builds replacement GPU tables off the render thread, retaining the previous generation until ready.
fn build_chunk_texture_assets(
    assets: &ChunkTextureAssets,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> Option<(PreparedChunkTextureAssets, ChunkTextureUploadStats)> {
    let identity = assets.identity();
    let pages = assets.assets().texture_pages();
    let Some(page_bindings) = plan_texture_page_bindings(pages.len()) else {
        bevy::log::error!(
            page_count = pages.len(),
            "chunk assets require one or two texture pages"
        );
        return None;
    };
    let diagnostic_fallback = if page_bindings.contains(&TexturePageBinding::DiagnosticFallback) {
        match diagnostic_texture_page(&pages[0].texture) {
            Ok(texture) => Some(texture),
            Err(error) => {
                bevy::log::error!(?error, "invalid diagnostic texture-page fallback");
                return None;
            }
        }
    } else {
        None
    };
    let bound_pages = page_bindings.map(|binding| match binding {
        TexturePageBinding::Asset(index) => &pages[index].texture,
        TexturePageBinding::DiagnosticFallback => diagnostic_fallback
            .as_ref()
            .expect("binding plan includes a diagnostic fallback"),
    });
    let device_limits = render_device.limits();
    if !crate::material_shader::chunk_atlas_views_fit(&device_limits) {
        bevy::log::error!(
            supported = device_limits.max_sampled_textures_per_shader_stage,
            supported_group_entries = device_limits.max_bindings_per_bind_group,
            required = crate::material_shader::CHUNK_SAMPLED_TEXTURE_BINDINGS,
            "chunk renderer requires sRGB and native leaf views of each texture page"
        );
        return None;
    }
    let limits = TextureArrayLimits {
        max_layers: device_limits.max_texture_array_layers,
        max_dimension_2d: device_limits.max_texture_dimension_2d,
    };
    let mut upload_plans = Vec::with_capacity(2);
    let mut terrain_pages = Vec::with_capacity(2);
    for texture in bound_pages {
        let tile_size = texture.mips.first().map_or(0, |mip| mip.size);
        if let Err(error) = limits.validate(texture.layers, tile_size) {
            bevy::log::error!(?error, "chunk texture page exceeds adapter limits");
            return None;
        }
        let texture = match assets::rebuild_legacy_terrain_mips(texture) {
            Ok(texture) => texture,
            Err(error) => {
                bevy::log::error!(?error, "invalid terrain mip source");
                return None;
            }
        };
        let plans =
            match plan_texture_mip_uploads(&texture, RenderDevice::align_copy_bytes_per_row(1)) {
                Ok(plans) => plans,
                Err(error) => {
                    bevy::log::error!(?error, "invalid chunk texture-page upload layout");
                    return None;
                }
            };
        upload_plans.push(plans);
        terrain_pages.push(texture);
    }
    let bound_pages = [&terrain_pages[0], &terrain_pages[1]];

    let material_words = assets
        .assets()
        .materials()
        .iter()
        .map(|material| MaterialGpu {
            texture: material.texture.raw(),
            flags: material.flags,
            animation: material.animation,
            variation_start: material.variation_start,
            variation_count: material.variation_count,
            variation_weight: material.variation_weight,
        })
        .collect::<Vec<_>>();
    let animation_words = assets
        .assets()
        .animations()
        .iter()
        .map(|animation| AnimationGpu {
            frame_start: animation.frame_start,
            frame_count: animation.frame_count,
            ticks_per_frame: animation.ticks_per_frame,
            flags: animation.flags,
            uv_scale: 1.0 / animation.replicate as f32,
        })
        .collect::<Vec<_>>();
    let animation_frame_words = assets
        .assets()
        .animation_frames()
        .iter()
        .map(|frame| frame.raw())
        .collect::<Vec<_>>();
    let model_template_words = encode_model_template_words(assets.assets());
    let material_bytes = material_words
        .len()
        .saturating_mul(std::mem::size_of::<MaterialGpu>());
    let animation_bytes = animation_words
        .len()
        .saturating_mul(std::mem::size_of::<AnimationGpu>());
    let animation_frame_bytes = animation_frame_words
        .len()
        .saturating_mul(std::mem::size_of::<u32>());
    let model_template_bytes = model_template_words
        .len()
        .saturating_mul(std::mem::size_of::<u32>());
    for (label, bytes) in [
        ("material", material_bytes),
        ("animation", animation_bytes),
        ("animation frame", animation_frame_bytes),
        ("model template", model_template_bytes),
    ] {
        if !storage_table_fits(
            bytes,
            device_limits.max_buffer_size,
            device_limits.max_storage_buffer_binding_size,
        ) {
            bevy::log::error!(label, bytes, "chunk asset table exceeds adapter limits");
            return None;
        }
    }
    let material_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk materials"),
        contents: bytemuck::cast_slice(&material_words),
        usage: BufferUsages::STORAGE,
    });
    let animation_sentinel = [AnimationGpu {
        frame_start: 0,
        frame_count: 1,
        ticks_per_frame: 1,
        flags: 0,
        uv_scale: 1.0,
    }];
    let animation_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animations"),
        contents: if animation_words.is_empty() {
            bytemuck::cast_slice(&animation_sentinel)
        } else {
            bytemuck::cast_slice(&animation_words)
        },
        usage: BufferUsages::STORAGE,
    });
    let animation_frame_sentinel = [TextureRef::DIAGNOSTIC.raw()];
    let animation_frame_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animation frames"),
        contents: bytemuck::cast_slice(if animation_frame_words.is_empty() {
            &animation_frame_sentinel
        } else {
            &animation_frame_words
        }),
        usage: BufferUsages::STORAGE,
    });
    let model_template_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk model templates"),
        contents: bytemuck::cast_slice(&model_template_words),
        usage: BufferUsages::STORAGE,
    });
    let (texture_0, view_0, padded_0) = upload_texture_page(
        render_device,
        render_queue,
        bound_pages[0],
        &upload_plans[0],
        "global chunk texture page 0",
    );
    let (texture_1, view_1, padded_1) = upload_texture_page(
        render_device,
        render_queue,
        bound_pages[1],
        &upload_plans[1],
        "global chunk texture page 1",
    );
    // Current atlas upload retains RGBA8_UNORM.
    // A view of each existing allocation preserves gamma-space filtering for
    // world leaves without duplicating texture memory or changing other art.
    let native_leaf_views = [&texture_0, &texture_1].map(|texture| {
        texture.create_view(&TextureViewDescriptor {
            label: Some("native world leaf atlas view"),
            format: Some(TextureFormat::Rgba8Unorm),
            dimension: Some(TextureViewDimension::D2Array),
            ..Default::default()
        })
    });
    let sampler = render_device.create_sampler(&chunk_sampler_descriptor());
    let native_leaf_sampler =
        render_device.create_sampler(&crate::material_shader::native_leaf_sampler_descriptor());

    let mut stats = ChunkTextureUploadStats {
        upload_count: 1,
        ..Default::default()
    };
    stats.material_bytes = material_bytes as u64;
    stats.animation_bytes = animation_bytes as u64;
    stats.animation_frame_bytes = animation_frame_bytes as u64;
    stats.texture_bytes_including_mips = bound_pages
        .iter()
        .flat_map(|texture| texture.mips.iter())
        .map(|mip| mip.rgba8.len() as u64)
        .sum();
    stats.padded_upload_bytes = padded_0.saturating_add(padded_1);
    let prepared = PreparedChunkTextureAssets {
        identity,
        material_buffer,
        animation_buffer,
        animation_frame_buffer,
        model_template_buffer,
        _textures: [texture_0, texture_1],
        views: [view_0, view_1],
        native_leaf_views,
        native_leaf_sampler,
        sampler,
    };
    Some((prepared, stats))
}

/// Array layers repeat atlas tiles without changing vanilla's texel or mip filtering.
pub(in crate::chunk) fn chunk_sampler_descriptor() -> SamplerDescriptor<'static> {
    SamplerDescriptor {
        label: Some("global chunk repeat sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        ..crate::material_shader::native_leaf_sampler_descriptor()
    }
}

pub(in crate::chunk) fn encode_model_template_words(assets: &RuntimeAssets) -> Vec<u32> {
    let template_count = u32::try_from(assets.model_templates().len()).unwrap_or(u32::MAX);
    let mut words = Vec::with_capacity(
        1 + assets.model_templates().len() * 3 + assets.model_quads().len() * 12,
    );
    words.push(template_count);
    for template in assets.model_templates() {
        words.extend([template.quad_start, template.quad_count, template.flags]);
    }
    for quad in assets.model_quads() {
        let mut i16_values = quad.positions.iter().flatten().copied();
        for _ in 0..6 {
            let low = i16_values.next().expect("twelve model position components") as u16;
            let high = i16_values.next().expect("twelve model position components") as u16;
            words.push(u32::from(low) | (u32::from(high) << 16));
        }
        let mut u16_values = quad.uvs.iter().flatten().copied();
        for _ in 0..4 {
            let low = u16_values.next().expect("eight model UV components");
            let high = u16_values.next().expect("eight model UV components");
            words.push(u32::from(low) | (u32::from(high) << 16));
        }
        words.extend([quad.material, quad.flags]);
    }
    words
}

pub(in crate::chunk) fn storage_table_fits(
    bytes: usize,
    max_buffer_size: u64,
    max_binding_size: u32,
) -> bool {
    u64::try_from(bytes)
        .is_ok_and(|bytes| bytes <= max_buffer_size && bytes <= u64::from(max_binding_size))
}

pub(in crate::chunk) fn upload_texture_page(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    texture_array: &TextureArray,
    upload_plans: &[TextureMipUploadPlan],
    label: &'static str,
) -> (Texture, TextureView, u64) {
    let mip_level_count = u32::try_from(texture_array.mips.len())
        .expect("validated texture pages have a bounded mip count");
    let texture = render_device.create_texture(&TextureDescriptor {
        label: Some(label),
        // Pages may differ in layer size; server overlay pages keep source resolution.
        size: Extent3d {
            width: texture_array.mips.first().map_or(1, |mip| mip.size),
            height: texture_array.mips.first().map_or(1, |mip| mip.size),
            depth_or_array_layers: texture_array.layers,
        },
        mip_level_count,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[TextureFormat::Rgba8Unorm],
    });
    let mut padded_upload_bytes = 0_u64;
    for (mip, plan) in texture_array.mips.iter().zip(upload_plans) {
        let staging = padded_mip_bytes(mip.rgba8.as_ref(), texture_array.layers, plan);
        padded_upload_bytes = padded_upload_bytes.saturating_add(staging.len() as u64);
        render_queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: plan.mip_level,
                origin: Origin3d::default(),
                aspect: Default::default(),
            },
            &staging,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(plan.bytes_per_row),
                rows_per_image: Some(plan.rows_per_image),
            },
            Extent3d {
                width: plan.size,
                height: plan.size,
                depth_or_array_layers: texture_array.layers,
            },
        );
    }
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2Array),
        mip_level_count: Some(mip_level_count),
        array_layer_count: Some(texture_array.layers),
        ..Default::default()
    });
    (texture, view, padded_upload_bytes)
}

pub(in crate::chunk) fn padded_mip_bytes(
    rgba8: &[u8],
    layers: u32,
    plan: &TextureMipUploadPlan,
) -> Vec<u8> {
    let mut staging = vec![0; plan.staging_bytes];
    let row_bytes = plan.size as usize * 4;
    let padded_row_bytes = plan.bytes_per_row as usize;
    for layer in 0..layers as usize {
        let source_layer = plan.layer_source_offsets[layer];
        let staging_layer = plan.layer_staging_offsets[layer];
        for row in 0..plan.size as usize {
            let source = source_layer + row * row_bytes;
            let destination = staging_layer + row * padded_row_bytes;
            staging[destination..destination + row_bytes]
                .copy_from_slice(&rgba8[source..source + row_bytes]);
        }
    }
    staging
}

pub(in crate::chunk) fn bind_group_needs_rebuild<K: PartialEq>(
    bind_group_exists: bool,
    cached: Option<&K>,
    next: &K,
) -> bool {
    !bind_group_exists || cached != Some(next)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_chunk_bind_group(
    pipeline: Res<ChunkPipeline>,
    pipeline_cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    render_device: Res<RenderDevice>,
    texture_assets: Res<ChunkGpuTextureAssets>,
    clock: Res<ChunkGpuAnimationClock>,
    biome_tints: Res<ChunkGpuBiomeTints>,
    atmosphere: Res<AtmosphereGpu>,
    mut arena: ResMut<ChunkGpuArena>,
) {
    let Some(texture_assets) = texture_assets.prepared.as_ref() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let Some(view_buffer) = view_uniforms.uniforms.buffer() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let Some(biome_tints) = biome_tints.prepared.as_ref() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let buffers = ChunkBindGroupBuffers {
        view: view_buffer.id(),
        quads: arena.quad_buffer.id(),
        origins: arena.origin_buffer.id(),
        biomes: arena.biome_buffer.id(),
        materials: texture_assets.material_buffer.id(),
        animations: texture_assets.animation_buffer.id(),
        animation_frames: texture_assets.animation_frame_buffer.id(),
        animation_clock: clock.buffer.id(),
        model_templates: texture_assets.model_template_buffer.id(),
        geometry_streams: arena.geometry_stream_buffer.id(),
        transparent_refs: arena.transparent_ref_buffer.id(),
        biome_tints: biome_tints.buffer.id(),
        atmosphere: atmosphere.buffer.id(),
        biome_tint_table: biome_tints.identity,
        textures: texture_assets.identity,
    };
    if !bind_group_needs_rebuild(
        arena.bind_group.is_some(),
        arena.bind_group_buffers.as_ref(),
        &buffers,
    ) && !biome_tint_bind_group_needs_rebuild(
        arena
            .bind_group_buffers
            .as_ref()
            .map(|buffers| buffers.biome_tint_table),
        biome_tints.identity,
    ) {
        return;
    }
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        arena.bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let bind_group = render_device.create_bind_group(
        "shared packed chunk bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: arena.quad_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: arena.origin_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: texture_assets.material_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::TextureView(&texture_assets.views[0]),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::TextureView(&texture_assets.views[1]),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::Sampler(&texture_assets.sampler),
            },
            BindGroupEntry {
                binding: 7,
                resource: arena.biome_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 8,
                resource: biome_tints.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 9,
                resource: texture_assets.animation_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 10,
                resource: texture_assets.animation_frame_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 11,
                resource: clock.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 12,
                resource: texture_assets.model_template_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 13,
                resource: arena.geometry_stream_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 14,
                resource: arena.transparent_ref_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 15,
                resource: atmosphere.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                resource: BindingResource::TextureView(&texture_assets.native_leaf_views[0]),
            },
            BindGroupEntry {
                binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                resource: BindingResource::TextureView(&texture_assets.native_leaf_views[1]),
            },
            BindGroupEntry {
                binding: crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                resource: BindingResource::Sampler(&texture_assets.native_leaf_sampler),
            },
        ],
    );
    arena.bind_group = Some(bind_group);
    arena.bind_group_buffers = Some(buffers);
}
