//! Immutable carrier atlas creation and upload, shared by bootstrap and server replacements.
use super::bind_groups::{
    AnimationGpu, ChunkTextureUploadStats, MaterialGpu, PreparedChunkTextureAssets,
    chunk_sampler_descriptor, encode_model_template_words, gpu_texture_reference,
    plan_queue_texture_mips, storage_table_fits, write_texture_mips,
};
use crate::chunk::*;
use crate::queue_writes::ProfiledQueue as _;
#[cfg(feature = "enhanced")]
use assets::texture::TextureMip;
use render_scene::{
    TextureArrayLimits, TextureMipUploadPlan, diagnostic_texture_page, plan_texture_page_bindings,
};
/// Unused material bindings remain valid without deriving materials from carrier albedo.
#[cfg(feature = "enhanced")]
fn neutral_pbr_layers() -> (TextureArray, TextureArray) {
    let [normal, material] = [[128, 128, 255, 128], [0, 0, 255, 0]].map(|rgba| TextureArray {
        layers: 1,
        mips: Box::new([TextureMip {
            size: 1,
            rgba8: Box::new(rgba),
        }]),
    });
    (normal, material)
}

/// Rebuilds the carrier while reusing unchanged authored GPU maps; changed maps start on fallback.
#[allow(unused_mut)]
pub(super) fn build_carrier_texture_assets(
    assets: &ChunkTextureAssets,
    _previous: Option<&PreparedChunkTextureAssets>,
    device: &RenderDevice,
    queue: &RenderQueue,
) -> Option<(PreparedChunkTextureAssets, ChunkTextureUploadStats)> {
    let carrier = assets.with_updated_enhanced(None);
    let (mut prepared, mut stats) = build_chunk_texture_assets(&carrier, device, queue)?;
    prepared.identity = assets.binding_identity();
    #[cfg(feature = "enhanced")]
    if let Some(previous) = _previous.filter(|previous| {
        previous
            .authored_source
            .as_ref()
            .zip(assets.enhanced())
            .is_some_and(|(bound, requested)| Arc::ptr_eq(bound, requested))
    }) {
        stats.texture_bytes_including_mips = stats
            .texture_bytes_including_mips
            .saturating_sub(prepared.authored_bytes)
            .saturating_add(previous.authored_bytes);
        prepared._enhanced_textures = previous._enhanced_textures.clone();
        prepared.enhanced_views = previous.enhanced_views.clone();
        prepared.enhanced_texture_refs = previous.enhanced_texture_refs.clone();
        prepared.authored_source = previous.authored_source.clone();
        prepared.authored_bytes = previous.authored_bytes;
        prepared.authored_upload_bytes = 0;
    }
    Some((prepared, stats))
}

#[cfg(all(test, not(feature = "enhanced")))]
#[path = "texture_upload/tests.rs"]
mod atlas_tests;

/// Builds replacement GPU tables off the render thread, retaining the previous generation until ready.
pub(in crate::chunk) fn build_chunk_texture_assets(
    assets: &ChunkTextureAssets,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> Option<(PreparedChunkTextureAssets, ChunkTextureUploadStats)> {
    let _zone = bevy::log::info_span!("pack_apply.gpu_atlas").entered();
    let identity = assets.binding_identity();
    let pages = assets.assets().texture_pages();
    let Some(page_bindings) = plan_texture_page_bindings(pages.len()) else {
        bevy::log::error!(
            page_count = pages.len(),
            "chunk assets require one or {} texture pages",
            assets::MAX_TEXTURE_PAGES
        );
        return None;
    };
    // A session with fewer pages than the format allows binds the remaining
    // slots to the base carrier's layer zero, which the shaders never sample.
    let diagnostic_fallback = match diagnostic_texture_page(&pages[0].texture) {
        Ok(texture) => Some(texture),
        Err(error) => {
            bevy::log::error!(?error, "invalid diagnostic texture-page fallback");
            return None;
        }
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
    let mut upload_plans = Vec::with_capacity(assets::MAX_TEXTURE_PAGES);
    let mut terrain_pages = Vec::with_capacity(assets::MAX_TEXTURE_PAGES);
    for (binding, texture) in page_bindings.into_iter().zip(bound_pages) {
        let tile_size = texture.mips.first().map_or(0, |mip| mip.size);
        if let Err(error) = limits.validate(texture.layers, tile_size) {
            bevy::log::error!(?error, "chunk texture page exceeds adapter limits");
            return None;
        }
        let mips = bevy::log::info_span!("pack_apply.gpu_atlas_mips").entered();
        let texture = match match binding {
            TexturePageBinding::Asset(index) => assets.assets().terrain_texture_page(index),
            TexturePageBinding::DiagnosticFallback => {
                assets::rebuild_legacy_terrain_mips(texture).map(std::borrow::Cow::Owned)
            }
        } {
            Ok(texture) => texture,
            Err(error) => {
                bevy::log::error!(?error, "invalid terrain mip source");
                return None;
            }
        };
        drop(mips);
        let plans = match plan_queue_texture_mips(&texture) {
            Ok(plans) => plans,
            Err(error) => {
                bevy::log::error!(?error, "invalid chunk texture-page upload layout");
                return None;
            }
        };
        upload_plans.push(plans);
        terrain_pages.push(texture);
    }

    #[cfg(feature = "enhanced")]
    let pbr_pages = [neutral_pbr_layers(), neutral_pbr_layers()];
    #[cfg(feature = "enhanced")]
    let pbr_upload_plans = pbr_pages.iter().map(|(normal, mer)| {
        let normal_plans = plan_queue_texture_mips(normal).ok()?;
        let mer_plans = plan_queue_texture_mips(mer).ok()?;
        Some((normal_plans, mer_plans))
    });
    #[cfg(feature = "enhanced")]
    let pbr_upload_plans = pbr_upload_plans.into_iter().collect::<Option<Vec<_>>>()?;

    // Missing authored maps use neutral shader defaults; placeholder views keep bindings valid.
    #[cfg(feature = "enhanced")]
    let fallback_color_pages = [
        diagnostic_texture_page(&terrain_pages[0]).ok()?,
        diagnostic_texture_page(&terrain_pages[1]).ok()?,
    ];
    #[cfg(feature = "enhanced")]
    let fallback_normal_pages = [
        diagnostic_texture_page(&pbr_pages[0].0).ok()?,
        diagnostic_texture_page(&pbr_pages[1].0).ok()?,
    ];
    #[cfg(feature = "enhanced")]
    let fallback_mer_pages = [
        diagnostic_texture_page(&pbr_pages[0].1).ok()?,
        diagnostic_texture_page(&pbr_pages[1].1).ok()?,
    ];
    #[cfg(feature = "enhanced")]
    let fallback_texture_refs =
        vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS];
    #[cfg(feature = "enhanced")]
    let enhanced = assets.enhanced().filter(|authored| {
        let supported = authored.fits_device_limits(&device_limits);
        if !supported {
            bevy::log::warn!("unsupported authored Enhanced maps; using carrier materials");
        }
        supported
    });
    #[cfg(feature = "enhanced")]
    let enhanced_color_pages = enhanced.map_or(&fallback_color_pages, |assets| &assets.color_pages);
    #[cfg(feature = "enhanced")]
    let enhanced_normal_pages =
        enhanced.map_or(&fallback_normal_pages, |assets| &assets.normal_pages);
    #[cfg(feature = "enhanced")]
    let enhanced_mer_pages = enhanced.map_or(&fallback_mer_pages, |assets| &assets.mer_pages);
    #[cfg(feature = "enhanced")]
    let enhanced_texture_refs: &[u32] =
        enhanced.map_or(&fallback_texture_refs, |assets| &assets.texture_refs);
    #[cfg(feature = "enhanced")]
    let enhanced_pages = [
        &enhanced_color_pages[0],
        &enhanced_color_pages[1],
        &enhanced_normal_pages[0],
        &enhanced_normal_pages[1],
        &enhanced_mer_pages[0],
        &enhanced_mer_pages[1],
    ];
    #[cfg(feature = "enhanced")]
    let enhanced_upload_plans = enhanced_pages
        .iter()
        .map(|texture| plan_queue_texture_mips(texture).ok())
        .collect::<Option<Vec<_>>>()?;
    #[cfg(feature = "enhanced")]
    let enhanced_ref_bytes = std::mem::size_of_val(enhanced_texture_refs);
    #[cfg(feature = "enhanced")]
    if !storage_table_fits(
        enhanced_ref_bytes,
        device_limits.max_buffer_size,
        device_limits.max_storage_buffer_binding_size,
    ) {
        bevy::log::error!("authored Enhanced texture reference table exceeds adapter limits");
        return None;
    }
    let material_words = assets
        .assets()
        .materials()
        .iter()
        .map(|material| MaterialGpu {
            texture: gpu_texture_reference(assets.assets(), material.texture),
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
        })
        .collect::<Vec<_>>();
    let animation_frame_words = assets
        .assets()
        .animation_frames()
        .iter()
        .map(|&frame| gpu_texture_reference(assets.assets(), frame))
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
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
    });
    let animation_sentinel = [AnimationGpu {
        frame_start: 0,
        frame_count: 1,
        ticks_per_frame: 1,
        flags: 0,
    }];
    let animation_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animations"),
        contents: if animation_words.is_empty() {
            bytemuck::cast_slice(&animation_sentinel)
        } else {
            bytemuck::cast_slice(&animation_words)
        },
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
    });
    let animation_frame_sentinel = [TextureRef::DIAGNOSTIC.raw()];
    let animation_frame_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk animation frames"),
        contents: bytemuck::cast_slice(if animation_frame_words.is_empty() {
            &animation_frame_sentinel
        } else {
            &animation_frame_words
        }),
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
    });
    let model_template_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global chunk model templates"),
        contents: bytemuck::cast_slice(&model_template_words),
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
    });
    let mut uploaded_bytes = 0u64;
    let mut textures = Vec::with_capacity(assets::MAX_TEXTURE_PAGES);
    let mut views = Vec::with_capacity(assets::MAX_TEXTURE_PAGES);
    // Upload the mip-rebuilt pages the validator measured, not the raw sources.
    for (index, (texture, plans)) in terrain_pages.iter().zip(upload_plans).enumerate() {
        let (texture, view, uploaded) = upload_texture_page(
            render_device,
            render_queue,
            texture,
            &plans,
            match index {
                0 => "global chunk texture page 0",
                1 => "global chunk texture page 1",
                2 => "global chunk texture page 2",
                3 => "global chunk texture page 3",
                _ => "global chunk texture page 4",
            },
        );
        uploaded_bytes = uploaded_bytes.saturating_add(uploaded);
        textures.push(texture);
        views.push(view);
    }
    #[cfg(feature = "enhanced")]
    let (normal_texture_0, normal_view_0, normal_uploaded_0) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[0].0,
        &pbr_upload_plans[0].0,
        "enhanced normal page 0",
    );
    #[cfg(feature = "enhanced")]
    let (normal_texture_1, normal_view_1, normal_uploaded_1) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[1].0,
        &pbr_upload_plans[1].0,
        "enhanced normal page 1",
    );
    #[cfg(feature = "enhanced")]
    let (mer_texture_0, mer_view_0, mer_uploaded_0) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[0].1,
        &pbr_upload_plans[0].1,
        "enhanced MER page 0",
    );
    #[cfg(feature = "enhanced")]
    let (mer_texture_1, mer_view_1, mer_uploaded_1) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[1].1,
        &pbr_upload_plans[1].1,
        "enhanced MER page 1",
    );
    #[cfg(feature = "enhanced")]
    let (enhanced_color_texture_0, enhanced_color_view_0, enhanced_color_uploaded_0) =
        upload_texture_page(
            render_device,
            render_queue,
            enhanced_pages[0],
            &enhanced_upload_plans[0],
            "authored Enhanced color page 0",
        );
    #[cfg(feature = "enhanced")]
    let (enhanced_color_texture_1, enhanced_color_view_1, enhanced_color_uploaded_1) =
        upload_texture_page(
            render_device,
            render_queue,
            enhanced_pages[1],
            &enhanced_upload_plans[1],
            "authored Enhanced color page 1",
        );
    #[cfg(feature = "enhanced")]
    let (enhanced_normal_texture_0, enhanced_normal_view_0, enhanced_normal_uploaded_0) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[2],
            &enhanced_upload_plans[2],
            "authored Enhanced normal page 0",
        );
    #[cfg(feature = "enhanced")]
    let (enhanced_normal_texture_1, enhanced_normal_view_1, enhanced_normal_uploaded_1) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[3],
            &enhanced_upload_plans[3],
            "authored Enhanced normal page 1",
        );
    #[cfg(feature = "enhanced")]
    let (enhanced_mer_texture_0, enhanced_mer_view_0, enhanced_mer_uploaded_0) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[4],
            &enhanced_upload_plans[4],
            "authored Enhanced MER page 0",
        );
    #[cfg(feature = "enhanced")]
    let (enhanced_mer_texture_1, enhanced_mer_view_1, enhanced_mer_uploaded_1) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[5],
            &enhanced_upload_plans[5],
            "authored Enhanced MER page 1",
        );
    #[cfg(feature = "enhanced")]
    let enhanced_texture_refs = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("authored Enhanced texture references"),
        contents: bytemuck::cast_slice(enhanced_texture_refs),
        usage: BufferUsages::STORAGE,
    });
    // Current atlas upload retains RGBA8_UNORM.
    // A view of each existing allocation preserves gamma-space filtering for
    // world leaves without duplicating texture memory or changing other art.
    let native_leaf_views = textures
        .iter()
        .map(|texture| {
            texture.create_view(&TextureViewDescriptor {
                label: Some("native world leaf atlas view"),
                format: Some(TextureFormat::Rgba8Unorm),
                dimension: Some(TextureViewDimension::D2Array),
                ..Default::default()
            })
        })
        .collect::<Vec<_>>()
        .try_into()
        .expect("every physical page uploads a view");
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
    stats.queue_upload_bytes = uploaded_bytes;
    #[cfg(feature = "enhanced")]
    {
        stats.queue_upload_bytes = stats
            .queue_upload_bytes
            .saturating_add(normal_uploaded_0)
            .saturating_add(normal_uploaded_1)
            .saturating_add(mer_uploaded_0)
            .saturating_add(mer_uploaded_1)
            .saturating_add(enhanced_color_uploaded_0)
            .saturating_add(enhanced_color_uploaded_1)
            .saturating_add(enhanced_normal_uploaded_0)
            .saturating_add(enhanced_normal_uploaded_1)
            .saturating_add(enhanced_mer_uploaded_0)
            .saturating_add(enhanced_mer_uploaded_1);
        stats.texture_bytes_including_mips = stats.texture_bytes_including_mips.saturating_add(
            pbr_pages
                .iter()
                .flat_map(|(normal, mer)| normal.mips.iter().chain(mer.mips.iter()))
                .map(|mip| mip.rgba8.len() as u64)
                .sum::<u64>(),
        );
        stats.texture_bytes_including_mips = stats.texture_bytes_including_mips.saturating_add(
            enhanced_pages
                .iter()
                .flat_map(|texture| texture.mips.iter())
                .map(|mip| mip.rgba8.len() as u64)
                .sum::<u64>(),
        );
    }
    let prepared = PreparedChunkTextureAssets {
        #[cfg(feature = "enhanced")]
        authored_source: enhanced.cloned(),
        identity,
        material_buffer,
        animation_buffer,
        animation_frame_buffer,
        model_template_buffer,
        _textures: textures.try_into().expect("every physical page uploads"),
        views: views
            .try_into()
            .expect("every physical page uploads a view"),
        #[cfg(feature = "enhanced")]
        _pbr_textures: [
            normal_texture_0,
            normal_texture_1,
            mer_texture_0,
            mer_texture_1,
        ],
        #[cfg(feature = "enhanced")]
        pbr_views: [normal_view_0, normal_view_1, mer_view_0, mer_view_1],
        #[cfg(feature = "enhanced")]
        _enhanced_textures: [
            enhanced_color_texture_0,
            enhanced_color_texture_1,
            enhanced_normal_texture_0,
            enhanced_normal_texture_1,
            enhanced_mer_texture_0,
            enhanced_mer_texture_1,
        ],
        #[cfg(feature = "enhanced")]
        enhanced_views: [
            enhanced_color_view_0,
            enhanced_color_view_1,
            enhanced_normal_view_0,
            enhanced_normal_view_1,
            enhanced_mer_view_0,
            enhanced_mer_view_1,
        ],
        #[cfg(feature = "enhanced")]
        enhanced_texture_refs,
        #[cfg(feature = "enhanced")]
        authored_bytes: enhanced_pages
            .iter()
            .flat_map(|texture| texture.mips.iter())
            .map(|mip| mip.rgba8.len() as u64)
            .sum(),
        #[cfg(feature = "enhanced")]
        authored_upload_bytes: enhanced_color_uploaded_0
            .saturating_add(enhanced_color_uploaded_1)
            .saturating_add(enhanced_normal_uploaded_0)
            .saturating_add(enhanced_normal_uploaded_1)
            .saturating_add(enhanced_mer_uploaded_0)
            .saturating_add(enhanced_mer_uploaded_1),
        native_leaf_views,
        native_leaf_sampler,
        sampler,
    };
    Some((prepared, stats))
}

/// Uploads validated sRGB carrier or authored color mip bytes.
pub(in crate::chunk) fn upload_texture_page(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    texture_array: &TextureArray,
    upload_plans: &[TextureMipUploadPlan],
    label: &'static str,
) -> (Texture, TextureView, u64) {
    upload_texture_page_format(
        render_device,
        render_queue,
        texture_array,
        upload_plans,
        label,
        TextureFormat::Rgba8UnormSrgb,
        &[TextureFormat::Rgba8Unorm],
    )
}

/// Uploads authored normal or material channels without sRGB conversion.
#[cfg(feature = "enhanced")]
fn upload_linear_texture_page(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    texture_array: &TextureArray,
    upload_plans: &[TextureMipUploadPlan],
    label: &'static str,
) -> (Texture, TextureView, u64) {
    upload_texture_page_format(
        render_device,
        render_queue,
        texture_array,
        upload_plans,
        label,
        TextureFormat::Rgba8Unorm,
        &[],
    )
}

/// Creates a texture and uploads its validated mip bytes in the selected format.
fn upload_texture_page_format(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    texture_array: &TextureArray,
    upload_plans: &[TextureMipUploadPlan],
    label: &'static str,
    format: TextureFormat,
    view_formats: &[TextureFormat],
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
        format,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats,
    });
    let queue_upload_bytes = write_texture_mips(texture_array, upload_plans, |plan, bytes| {
        render_queue.profiled_write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: plan.mip_level,
                origin: Origin3d::default(),
                aspect: Default::default(),
            },
            bytes,
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
    });
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2Array),
        mip_level_count: Some(mip_level_count),
        array_layer_count: Some(texture_array.layers),
        ..Default::default()
    });
    (texture, view, queue_upload_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "enhanced")]
    use render_scene::{EnhancedTextureAssets, plan_texture_mip_uploads};

    /// Ordinary carrier uploads fit the five-page sRGB and native-leaf views, two samplers,
    /// and the biome table at binding 26.
    #[cfg(not(feature = "enhanced"))]
    #[test]
    fn vanilla_carrier_keeps_its_binding_limits_and_upload_bytes() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_sampled_textures_per_shader_stage: 10,
                max_samplers_per_shader_stage: 2,
                max_bindings_per_bind_group: 27,
                ..Default::default()
            },
            ..Default::default()
        });
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let carrier = ChunkTextureAssets::default();
        let (prepared, stats) = build_chunk_texture_assets(&carrier, &device, &queue).unwrap();
        assert_eq!(prepared.identity, carrier.binding_identity());
        let pages = carrier.assets().texture_pages();
        let fallback = diagnostic_texture_page(&pages[0].texture).unwrap();
        let bindings = plan_texture_page_bindings(pages.len()).unwrap();
        let expected: u64 = bindings
            .into_iter()
            .map(|binding| {
                let page = match binding {
                    TexturePageBinding::Asset(index) => {
                        carrier.assets().terrain_texture_page(index).unwrap()
                    }
                    TexturePageBinding::DiagnosticFallback => std::borrow::Cow::Owned(
                        assets::rebuild_legacy_terrain_mips(&fallback).unwrap(),
                    ),
                };
                page.mips
                    .iter()
                    .map(|mip| mip.rgba8.len() as u64)
                    .sum::<u64>()
            })
            .sum();

        assert_eq!(stats.texture_bytes_including_mips, expected);
        assert_eq!(stats.queue_upload_bytes, expected);
    }

    #[cfg(feature = "enhanced")]
    #[test]
    fn oversized_optional_maps_do_not_reject_a_server_pack_atlas() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_texture_array_layers: 1,
                ..Default::default()
            },
            ..Default::default()
        });
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let base = Arc::new(RuntimeAssets::diagnostic());
        let carrier = ChunkTextureAssets::new(base.clone());
        assert!(build_chunk_texture_assets(&carrier, &device, &queue).is_some());
        let array = TextureArray {
            layers: 2,
            mips: Box::new([TextureMip {
                size: 1,
                rgba8: vec![128; 8].into_boxed_slice(),
            }]),
        };
        let authored = Arc::new(
            EnhancedTextureAssets::new(
                [array.clone(), array.clone()],
                [array.clone(), array.clone()],
                [array.clone(), array],
                vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS]
                    .into_boxed_slice(),
            )
            .unwrap(),
        );
        let current = carrier.with_updated_enhanced(Some(authored.clone()));
        assert!(
            super::super::authored_upload::AuthoredUpload::new(
                current.binding_identity(),
                authored,
                &device,
            )
            .is_none()
        );
        let replacement = ChunkTextureAssets::with_optional_enhanced(
            Arc::new(RuntimeAssets::diagnostic()),
            current.enhanced().cloned(),
            1,
        );
        let reload = ChunkTextureReload::default();
        reload.request(replacement.clone());
        let request = reload.preparation().expect("requested replacement");
        let prepared = build_chunk_texture_assets(&replacement, &device, &queue);
        reload.finish(&request, prepared.is_some());
        assert_eq!(
            reload.status(replacement.binding_identity()),
            Some(Ok(())),
            "rejected optional maps must not abort a valid server atlas replacement",
        );
        assert_eq!(prepared.unwrap().0.identity, replacement.binding_identity());
    }

    #[cfg(feature = "enhanced")]
    #[test]
    fn fallback_material_bindings_are_neutral_single_texels() {
        let (normal, material) = neutral_pbr_layers();
        for (texture, expected) in [(normal, [128, 128, 255, 128]), (material, [0, 0, 255, 0])] {
            assert_eq!(texture.layers, 1);
            assert_eq!(texture.mips.len(), 1);
            assert_eq!(texture.mips[0].size, 1);
            assert_eq!(texture.mips[0].rgba8.as_ref(), expected);
            let uploads = plan_texture_mip_uploads(&texture, 1).unwrap();
            assert_eq!(uploads.len(), 1);
            assert_eq!(uploads[0].staging_bytes, 4);
        }
    }
}
