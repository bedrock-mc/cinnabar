//! Item glint instance parameters, sampling and retained texture uploads.

use std::sync::Arc;

use bevy::{
    prelude::default,
    render::{
        render_resource::{
            AddressMode, Extent3d, FilterMode, Sampler, SamplerDescriptor, TextureDataOrder,
            TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
            TextureViewDescriptor,
        },
        renderer::{RenderDevice, RenderQueue},
    },
};
use render_scene::DroppedItemScene;

use super::{GpuItemInstance, ItemGpu};

/// Packs one drawn copy, marking whether its per-vertex item glint scale applies.
pub(super) fn pack_instance(instance: &render_model::DroppedItemInstance) -> GpuItemInstance {
    GpuItemInstance {
        rows: instance.world_from_item,
        meta: [
            instance.model,
            instance.block_level,
            instance.sky_level,
            instance.overlay_rgba8,
        ],
        glint: [if instance.glint { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
    }
}

/// Creates the repeat sampler used by the item's two animated glint layers.
pub(super) fn sampler(device: &RenderDevice) -> Sampler {
    device.create_sampler(&SamplerDescriptor {
        label: Some("repeat dropped item glint sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    })
}

/// Uploads the scene's item glint, or a transparent texel without one, keeping the texture
/// while its pixels are unchanged.
pub(super) fn upload(
    scene: &DroppedItemScene,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu: &mut ItemGpu,
) {
    let raster = scene.glint.as_ref().filter(|raster| {
        u32::from(raster.width.max(raster.height)) <= device.limits().max_texture_dimension_2d
    });
    let pixels = raster.map(|raster| Arc::clone(&raster.rgba8));
    if let Some((_, _, current)) = &gpu.glint
        && match (current, &pixels) {
            (Some(current), Some(pixels)) => Arc::ptr_eq(current, pixels),
            (None, None) => true,
            _ => false,
        }
    {
        return;
    }
    let (width, height, data): (u32, u32, &[u8]) = match raster {
        Some(raster) => (
            u32::from(raster.width),
            u32::from(raster.height),
            &raster.rgba8,
        ),
        None => (1, 1, &[0; 4]),
    };
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("dropped item glint"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        data,
    );
    let view = texture.create_view(&TextureViewDescriptor::default());
    gpu.glint = Some((texture, view, pixels));
    gpu.bind_group = None;
}
