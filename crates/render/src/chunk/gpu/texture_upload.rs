//! Immutable carrier atlas creation and upload, shared by bootstrap and server replacements.
use super::bind_groups::{
    AnimationGpu, ChunkTextureUploadStats, MaterialGpu, PreparedChunkTextureAssets,
    chunk_sampler_descriptor, encode_model_template_words, storage_table_fits,
};
use crate::chunk::*;
/// Unused material bindings remain valid without deriving materials from carrier albedo.
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

/// Builds replacement GPU tables off the render thread, retaining the previous generation until ready.
pub(in crate::chunk) fn build_chunk_texture_assets(
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
    let pbr_pages = [neutral_pbr_layers(), neutral_pbr_layers()];
    let mut upload_plans = Vec::with_capacity(2);
    for texture in bound_pages {
        let tile_size = texture.mips.first().map_or(0, |mip| mip.size);
        if let Err(error) = limits.validate(texture.layers, tile_size) {
            bevy::log::error!(?error, "chunk texture page exceeds adapter limits");
            return None;
        }
        let plans =
            match plan_texture_mip_uploads(texture, RenderDevice::align_copy_bytes_per_row(1)) {
                Ok(plans) => plans,
                Err(error) => {
                    bevy::log::error!(?error, "invalid chunk texture-page upload layout");
                    return None;
                }
            };
        upload_plans.push(plans);
    }
    let pbr_upload_plans = pbr_pages.iter().map(|(normal, mer)| {
        let normal_plans =
            plan_texture_mip_uploads(&normal, RenderDevice::align_copy_bytes_per_row(1)).ok()?;
        let mer_plans =
            plan_texture_mip_uploads(&mer, RenderDevice::align_copy_bytes_per_row(1)).ok()?;
        Some((normal_plans, mer_plans))
    });
    let pbr_upload_plans = pbr_upload_plans.into_iter().collect::<Option<Vec<_>>>()?;

    // Missing authored maps use neutral shader defaults; placeholder views keep bindings valid.
    let fallback_color_pages = [
        diagnostic_texture_page(bound_pages[0]).ok()?,
        diagnostic_texture_page(bound_pages[1]).ok()?,
    ];
    let fallback_normal_pages = [
        diagnostic_texture_page(&pbr_pages[0].0).ok()?,
        diagnostic_texture_page(&pbr_pages[1].0).ok()?,
    ];
    let fallback_mer_pages = [
        diagnostic_texture_page(&pbr_pages[0].1).ok()?,
        diagnostic_texture_page(&pbr_pages[1].1).ok()?,
    ];
    let fallback_texture_refs =
        vec![u32::MAX; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS];
    let enhanced = assets.enhanced();
    let enhanced_color_pages = enhanced.map_or(&fallback_color_pages, |assets| &assets.color_pages);
    let enhanced_normal_pages =
        enhanced.map_or(&fallback_normal_pages, |assets| &assets.normal_pages);
    let enhanced_mer_pages = enhanced.map_or(&fallback_mer_pages, |assets| &assets.mer_pages);
    let enhanced_texture_refs: &[u32] =
        enhanced.map_or(&fallback_texture_refs, |assets| &assets.texture_refs);
    let enhanced_pages = [
        &enhanced_color_pages[0],
        &enhanced_color_pages[1],
        &enhanced_normal_pages[0],
        &enhanced_normal_pages[1],
        &enhanced_mer_pages[0],
        &enhanced_mer_pages[1],
    ];
    for texture in enhanced_pages {
        let tile_size = texture.mips.first().map_or(0, |mip| mip.size);
        if let Err(error) = limits.validate(texture.layers, tile_size) {
            bevy::log::error!(
                ?error,
                "authored Enhanced texture page exceeds adapter limits"
            );
            return None;
        }
    }
    let enhanced_upload_plans = enhanced_pages
        .iter()
        .map(|texture| {
            plan_texture_mip_uploads(texture, RenderDevice::align_copy_bytes_per_row(1)).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    let enhanced_ref_bytes = enhanced_texture_refs.len() * std::mem::size_of::<u32>();
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
    let (normal_texture_0, normal_view_0, normal_padded_0) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[0].0,
        &pbr_upload_plans[0].0,
        "enhanced normal page 0",
    );
    let (normal_texture_1, normal_view_1, normal_padded_1) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[1].0,
        &pbr_upload_plans[1].0,
        "enhanced normal page 1",
    );
    let (mer_texture_0, mer_view_0, mer_padded_0) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[0].1,
        &pbr_upload_plans[0].1,
        "enhanced MER page 0",
    );
    let (mer_texture_1, mer_view_1, mer_padded_1) = upload_linear_texture_page(
        render_device,
        render_queue,
        &pbr_pages[1].1,
        &pbr_upload_plans[1].1,
        "enhanced MER page 1",
    );
    let (enhanced_color_texture_0, enhanced_color_view_0, enhanced_color_padded_0) =
        upload_texture_page(
            render_device,
            render_queue,
            enhanced_pages[0],
            &enhanced_upload_plans[0],
            "authored Enhanced color page 0",
        );
    let (enhanced_color_texture_1, enhanced_color_view_1, enhanced_color_padded_1) =
        upload_texture_page(
            render_device,
            render_queue,
            enhanced_pages[1],
            &enhanced_upload_plans[1],
            "authored Enhanced color page 1",
        );
    let (enhanced_normal_texture_0, enhanced_normal_view_0, enhanced_normal_padded_0) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[2],
            &enhanced_upload_plans[2],
            "authored Enhanced normal page 0",
        );
    let (enhanced_normal_texture_1, enhanced_normal_view_1, enhanced_normal_padded_1) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[3],
            &enhanced_upload_plans[3],
            "authored Enhanced normal page 1",
        );
    let (enhanced_mer_texture_0, enhanced_mer_view_0, enhanced_mer_padded_0) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[4],
            &enhanced_upload_plans[4],
            "authored Enhanced MER page 0",
        );
    let (enhanced_mer_texture_1, enhanced_mer_view_1, enhanced_mer_padded_1) =
        upload_linear_texture_page(
            render_device,
            render_queue,
            enhanced_pages[5],
            &enhanced_upload_plans[5],
            "authored Enhanced MER page 1",
        );
    let enhanced_texture_refs = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("authored Enhanced texture references"),
        contents: bytemuck::cast_slice(enhanced_texture_refs),
        usage: BufferUsages::STORAGE,
    });
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
    stats.padded_upload_bytes = padded_0
        .saturating_add(padded_1)
        .saturating_add(normal_padded_0)
        .saturating_add(normal_padded_1)
        .saturating_add(mer_padded_0)
        .saturating_add(mer_padded_1)
        .saturating_add(enhanced_color_padded_0)
        .saturating_add(enhanced_color_padded_1)
        .saturating_add(enhanced_normal_padded_0)
        .saturating_add(enhanced_normal_padded_1)
        .saturating_add(enhanced_mer_padded_0)
        .saturating_add(enhanced_mer_padded_1);
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
    let prepared = PreparedChunkTextureAssets {
        identity,
        material_buffer,
        animation_buffer,
        animation_frame_buffer,
        model_template_buffer,
        _textures: [texture_0, texture_1],
        views: [view_0, view_1],
        _pbr_textures: [
            normal_texture_0,
            normal_texture_1,
            mer_texture_0,
            mer_texture_1,
        ],
        pbr_views: [normal_view_0, normal_view_1, mer_view_0, mer_view_1],
        _enhanced_textures: [
            enhanced_color_texture_0,
            enhanced_color_texture_1,
            enhanced_normal_texture_0,
            enhanced_normal_texture_1,
            enhanced_mer_texture_0,
            enhanced_mer_texture_1,
        ],
        enhanced_views: [
            enhanced_color_view_0,
            enhanced_color_view_1,
            enhanced_normal_view_0,
            enhanced_normal_view_1,
            enhanced_mer_view_0,
            enhanced_mer_view_1,
        ],
        enhanced_texture_refs,
        authored_bytes: enhanced_pages
            .iter()
            .flat_map(|texture| texture.mips.iter())
            .map(|mip| mip.rgba8.len() as u64)
            .sum(),
        authored_upload_bytes: enhanced_color_padded_0
            .saturating_add(enhanced_color_padded_1)
            .saturating_add(enhanced_normal_padded_0)
            .saturating_add(enhanced_normal_padded_1)
            .saturating_add(enhanced_mer_padded_0)
            .saturating_add(enhanced_mer_padded_1),
        native_leaf_views,
        native_leaf_sampler,
        sampler,
        pbr_sampler: render_device
            .create_sampler(&crate::material_shader::pbr_sampler_descriptor()),
        enhanced_sampler: render_device
            .create_sampler(&crate::material_shader::pbr_sampler_descriptor()),
    };
    Some((prepared, stats))
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
