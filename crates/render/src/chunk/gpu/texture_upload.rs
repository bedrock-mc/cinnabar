//! Immutable carrier atlas creation and upload for bootstrap and server replacements.

use super::bind_groups::{
    AnimationGpu, ChunkTextureUploadStats, MaterialGpu, PreparedChunkTextureAssets,
    chunk_sampler_descriptor, encode_model_template_words, gpu_texture_reference,
    plan_queue_texture_mips, storage_table_fits, write_texture_mips,
};
use crate::chunk::*;
use crate::queue_writes::ProfiledQueue as _;
use render_scene::{
    TextureArrayLimits, TextureMipUploadPlan, diagnostic_texture_page, plan_texture_page_bindings,
};

#[cfg(test)]
mod tests;

/// Builds replacement GPU tables off the render thread, retaining the previous generation until ready.
pub(in crate::chunk) fn build_chunk_texture_assets(
    assets: &ChunkTextureAssets,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> Option<(PreparedChunkTextureAssets, ChunkTextureUploadStats)> {
    let _zone = bevy::log::info_span!("pack_apply.gpu_atlas").entered();
    let identity = assets.identity();
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
        usage: BufferUsages::STORAGE,
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
    let prepared = PreparedChunkTextureAssets {
        identity,
        material_buffer,
        animation_buffer,
        animation_frame_buffer,
        model_template_buffer,
        _textures: textures.try_into().expect("every physical page uploads"),
        views: views
            .try_into()
            .expect("every physical page uploads a view"),
        native_leaf_views,
        native_leaf_sampler,
        sampler,
    };
    Some((prepared, stats))
}

/// Uploads validated mip bytes directly, without application-side row padding.
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
