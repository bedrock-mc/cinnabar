use super::resource_geometry::PreparedResourceGeometry;
use super::{
    authored_upload, authored_upload::AuthoredUpload, texture_upload::build_carrier_texture_assets,
};
use crate::chunk::*;
#[cfg(feature = "enhanced")]
use bevy::render::render_resource::TextureViewId;

#[cfg(test)]
use super::texture_upload::build_chunk_texture_assets;
#[cfg(test)]
mod carrier_tests;
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
    pub(in crate::chunk) enhanced_texture_refs: BufferId,
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

#[derive(Clone)]
pub(in crate::chunk) struct PreparedChunkTextureAssets {
    /// Retains the authored source represented by the current GPU bindings.
    pub(super) authored_source: Option<Arc<EnhancedTextureAssets>>,
    pub(in crate::chunk) identity: ChunkTextureAssetIdentity,
    pub(in crate::chunk) material_buffer: Buffer,
    pub(in crate::chunk) animation_buffer: Buffer,
    pub(in crate::chunk) animation_frame_buffer: Buffer,
    pub(in crate::chunk) model_template_buffer: Buffer,
    pub(in crate::chunk) _textures: [Texture; 2],
    pub(in crate::chunk) views: [TextureView; 2],
    pub(in crate::chunk) _pbr_textures: [Texture; 4],
    pub(in crate::chunk) pbr_views: [TextureView; 4],
    pub(in crate::chunk) _enhanced_textures: [Texture; 6],
    pub(in crate::chunk) enhanced_views: [TextureView; 6],
    pub(in crate::chunk) enhanced_texture_refs: Buffer,
    pub(in crate::chunk) authored_bytes: u64,
    pub(in crate::chunk) authored_upload_bytes: u64,
    pub(in crate::chunk) native_leaf_views: [TextureView; assets::MAX_TEXTURE_PAGES],
    pub(in crate::chunk) sampler: Sampler,
    pub(in crate::chunk) pbr_sampler: Sampler,
    pub(in crate::chunk) enhanced_sampler: Sampler,
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
    let words = crate::material_shader::world_uniform_words(bytemuck::cast_slice(
        std::slice::from_ref(&ChunkAnimationClock::default()),
    ));
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk clock and biome tables"),
        contents: bytemuck::cast_slice(&words),
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
pub(crate) struct ChunkGpuTextureAssets {
    pub(in crate::chunk) attempted_identity: Option<ChunkTextureAssetIdentity>,
    pub(in crate::chunk) _attempted_assets: Option<Arc<RuntimeAssets>>,
    pub(in crate::chunk) prepared: Option<PreparedChunkTextureAssets>,
    pending: Option<PendingTextures>,
    pending_identity: Option<ChunkTextureAssetIdentity>,
    staged: Option<PreparedReplacement>,
    authored_pending: Option<AuthoredUpload>,
}

#[cfg(feature = "enhanced")]
impl ChunkGpuTextureAssets {
    /// Identifies the published color pages and references that determine shadow cutouts.
    pub(crate) fn shadow_texture_key(&self) -> Option<([TextureViewId; 2], BufferId)> {
        self.prepared.as_ref().map(|prepared| {
            (
                [prepared.enhanced_views[0].id(), prepared.enhanced_views[1].id()],
                prepared.enhanced_texture_refs.id(),
            )
        })
    }
}

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChunkTextureUploadStats {
    pub upload_count: u64,
    pub material_bytes: u64,
    pub animation_bytes: u64,
    pub animation_frame_bytes: u64,
    pub texture_bytes_including_mips: u64,
    pub queue_upload_bytes: u64,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_chunk_texture_assets(
    mut commands: Commands,
    instances: Query<(Entity, &ChunkRenderInstance)>,
    views: Query<(Entity, &ExtractedView, Has<crate::EnhancedRendering>), With<ExtractedCamera>>,
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
        // Geometry built for chunks a session reset retired cannot publish. The main world
        // already meshes against this atlas, so it still installs and the reload completes.
        if let Some(geometry) = geometry
            && !geometry.publish(&mut commands, &instances, &mut arena)
        {
            bevy::log::warn!("discarding resource geometry with retired chunk keys");
        }
        reload.published();
        gpu_assets.prepared = Some(prepared);
        gpu_assets.attempted_identity = Some(identity);
        gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
        *stats = uploaded;
        gpu_assets.authored_pending = AuthoredUpload::for_assets(
            &assets,
            gpu_assets
                .prepared
                .as_ref()
                .expect("published carrier atlas"),
            &render_device,
        );
    }
    // Bootstrap may publish without an optional reload transaction.
    if gpu_assets
        .authored_pending
        .as_ref()
        .is_some_and(|pending| !pending.matches(identity))
    {
        gpu_assets.authored_pending = None;
    }
    if texture_asset_needs_rebuild(gpu_assets.attempted_identity, identity) {
        gpu_assets.attempted_identity = Some(identity);
        gpu_assets._attempted_assets = Some(Arc::clone(assets.assets()));
        if gpu_assets
            .prepared
            .as_ref()
            .is_some_and(|prepared| authored_upload::same_carrier(prepared.identity, identity))
        {
            if let Some(authored) = assets.enhanced() {
                gpu_assets.authored_pending =
                    AuthoredUpload::new(identity, authored.clone(), &render_device);
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
                    &render_device,
                    &mut stats,
                );
            }
        } else {
            gpu_assets.authored_pending = None;
            if let Some((prepared, uploaded)) = build_carrier_texture_assets(
                &assets,
                gpu_assets.prepared.as_ref(),
                &render_device,
                &render_queue,
            ) {
                gpu_assets.prepared = Some(prepared);
                *stats = uploaded;
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
    if let Some(pending) = gpu_assets.authored_pending.as_mut() {
        pending.step(
            &render_device,
            &render_queue,
            authored_upload::FRAME_UPLOAD_BYTES,
        );
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
        .min_by_key(|(entity, ..)| *entity)
        .map(
            |(entity, view, enhanced)| super::resource_sorts::ResourceView {
                entity,
                transform: view.world_from_view,
                sort_order_independent: view_displaces_water(enhanced),
            },
        );
    let previous = gpu_assets.prepared.clone();
    std::thread::spawn(move || {
        let result = build_carrier_texture_assets(&candidate, previous.as_ref(), &device, &queue)
            .and_then(|(atlas, stats)| {
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
        arena.transparent_bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let Some(view_buffer) = view_uniforms.uniforms.buffer() else {
        arena.bind_group = None;
        arena.transparent_bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let Some(biome_tints) = biome_tints.prepared.as_ref() else {
        arena.bind_group = None;
        arena.transparent_bind_group = None;
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
        enhanced_texture_refs: texture_assets.enhanced_texture_refs.id(),
    };
    if !bind_group_needs_rebuild(
        arena.bind_group.is_some() && arena.transparent_bind_group.is_some(),
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
        arena.transparent_bind_group = None;
        arena.bind_group_buffers = None;
        return;
    };
    let entries = [
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
            binding: crate::material_shader::BIOME_QUERY_TABLES_BINDING,
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
            binding: TRANSPARENT_REFS_BINDING,
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
        BindGroupEntry {
            binding: crate::material_shader::PBR_NORMAL_TEXTURE_BINDINGS[0],
            resource: BindingResource::TextureView(&texture_assets.pbr_views[0]),
        },
        BindGroupEntry {
            binding: crate::material_shader::PBR_NORMAL_TEXTURE_BINDINGS[1],
            resource: BindingResource::TextureView(&texture_assets.pbr_views[1]),
        },
        BindGroupEntry {
            binding: crate::material_shader::PBR_MER_TEXTURE_BINDINGS[0],
            resource: BindingResource::TextureView(&texture_assets.pbr_views[2]),
        },
        BindGroupEntry {
            binding: crate::material_shader::PBR_MER_TEXTURE_BINDINGS[1],
            resource: BindingResource::TextureView(&texture_assets.pbr_views[3]),
        },
        BindGroupEntry {
            binding: crate::material_shader::PBR_SAMPLER_BINDING,
            resource: BindingResource::Sampler(&texture_assets.pbr_sampler),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_COLOR_TEXTURE_BINDINGS[0],
            resource: BindingResource::TextureView(&texture_assets.enhanced_views[0]),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_COLOR_TEXTURE_BINDINGS[1],
            resource: BindingResource::TextureView(&texture_assets.enhanced_views[1]),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS[0],
            resource: BindingResource::TextureView(&texture_assets.enhanced_views[2]),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS[1],
            resource: BindingResource::TextureView(&texture_assets.enhanced_views[3]),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_MER_TEXTURE_BINDINGS[0],
            resource: BindingResource::TextureView(&texture_assets.enhanced_views[4]),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_MER_TEXTURE_BINDINGS[1],
            resource: BindingResource::TextureView(&texture_assets.enhanced_views[5]),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_SAMPLER_BINDING,
            resource: BindingResource::Sampler(&texture_assets.enhanced_sampler),
        },
        BindGroupEntry {
            binding: crate::material_shader::ENHANCED_TEXTURE_REF_BINDING,
            resource: texture_assets.enhanced_texture_refs.as_entire_binding(),
        },
    ];
    let transparent_bind_group = render_device.create_bind_group(
        "transparent packed chunk bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.transparent_bind_group_layout),
        &entries,
    );
    let opaque_entries: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.binding != TRANSPARENT_REFS_BINDING)
        .collect();
    let bind_group = render_device.create_bind_group(
        "opaque packed chunk bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &opaque_entries,
    );
    arena.bind_group = Some(bind_group);
    arena.transparent_bind_group = Some(transparent_bind_group);
    arena.bind_group_buffers = Some(buffers);
}

/// Adds runtime source dimensions when a layer's physical page uses another pixel scale.
pub(super) fn gpu_texture_reference(
    assets: &assets::RuntimeAssets,
    reference: assets::TextureRef,
) -> u32 {
    let page_size = assets.texture_pages()[reference.page() as usize]
        .texture
        .mips[0]
        .size;
    let grid = assets.texture_source_grid(reference);
    crate::material_shader::gpu_grid_texture_ref(
        reference,
        assets
            .texture_source_size(reference)
            .map(|axis| axis << grid),
        page_size,
        grid,
    )
}

/// Validates tightly packed queue writes; encoder buffer copies still use aligned plans.
pub(super) fn plan_queue_texture_mips(
    texture: &TextureArray,
) -> Result<Vec<TextureMipUploadPlan>, TextureUploadPlanError> {
    plan_texture_mip_uploads(texture, 1)
}

/// Sends each validated mip to the queue writer and counts the submitted bytes.
pub(super) fn write_texture_mips(
    texture: &TextureArray,
    plans: &[TextureMipUploadPlan],
    mut write: impl FnMut(&TextureMipUploadPlan, &[u8]),
) -> u64 {
    let mut uploaded_bytes = 0_u64;
    for (mip, plan) in texture.mips.iter().zip(plans) {
        let bytes = mip.rgba8.as_ref();
        uploaded_bytes = uploaded_bytes.saturating_add(bytes.len() as u64);
        write(plan, bytes);
    }
    uploaded_bytes
}

#[cfg(test)]
#[path = "texture_upload_tests.rs"]
mod texture_upload_tests;
