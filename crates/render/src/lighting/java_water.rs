//! Uploads optional water flipbooks without replacing server-authored terrain layers.

use crate::queue_writes::ProfiledQueue as _;
use bevy::{
    prelude::default,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
};
use render_scene::JavaWorldVisuals;

pub(super) struct JavaWaterGpu {
    pub view: TextureView,
    pub sampler: Sampler,
    pub parameters: Buffer,
    texture: Texture,
    identity: Option<[u8; 32]>,
}

impl JavaWaterGpu {
    /// Creates a one-pixel binding until the user enables the optional preset.
    pub fn new(device: &RenderDevice, queue: &RenderQueue) -> Self {
        let (texture, view) = upload(device, queue, 1, 1, 1, &[0; 4]);
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Java water nearest texels and linear mips"),
            min_filter: FilterMode::Nearest,
            mag_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..default()
        });
        let parameters = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("Java water flipbook parameters"),
            contents: bytemuck::cast_slice(
                &[[0_u32; 4]; assets::java_world::WATER_TEXTURE_PATHS.len()],
            ),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        Self {
            view,
            sampler,
            parameters,
            texture,
            identity: None,
        }
    }

    /// Uploads authenticated frames once when the preset is first enabled.
    pub fn prepare(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        java: Option<&JavaWorldVisuals>,
    ) -> bool {
        let Some(java) = java.filter(|java| java.enabled()) else {
            return false;
        };
        let identity = java.texture_identity([0; 32]);
        if self.identity == Some(identity) {
            return false;
        }
        let Some(assets) = java.assets() else {
            return false;
        };
        let side = assets
            .water
            .iter()
            .map(|water| water.image.width)
            .max()
            .expect("water images decoded");
        let levels = assets
            .water
            .iter()
            .map(|water| water.image.width.ilog2())
            .min()
            .unwrap()
            + 1;
        let layers = assets.water.iter().map(|water| water.frame_count()).sum();
        let mut bytes = Vec::new();
        let mut parameters = [[0_u32; 4]; assets::java_world::WATER_TEXTURE_PATHS.len()];
        let mut first_layer = 0;
        for (index, water) in assets.water.iter().enumerate() {
            let width = water.image.width;
            let frames = water.frame_count();
            parameters[index] = [first_layer, frames, water.frame_ticks, width];
            first_layer += frames;
            for frame in 0..frames {
                let mut pixels = Vec::with_capacity((side * side * 4) as usize);
                for y in 0..side {
                    for x in 0..side {
                        let offset = (((frame * width + y.min(width - 1)) * width
                            + x.min(width - 1))
                            * 4) as usize;
                        pixels.extend_from_slice(&water.image.rgba[offset..offset + 4]);
                    }
                }
                let mut pixels = pixels.into_boxed_slice();
                let mut size = side;
                for level in 0..levels {
                    bytes.extend_from_slice(&pixels);
                    if level + 1 < levels {
                        pixels = assets::java_world::mean_mip(&pixels, size)
                            .expect("validated square water frame");
                        size /= 2;
                    }
                }
            }
        }
        (self.texture, self.view) = upload(device, queue, side, layers, levels, &bytes);
        queue.profiled_write_buffer(&self.parameters, 0, bytemuck::cast_slice(&parameters));
        self.identity = Some(identity);
        true
    }
}

/// Uploads frame-major mip chains into one bounded texture array.
fn upload(
    device: &RenderDevice,
    queue: &RenderQueue,
    side: u32,
    layers: u32,
    levels: u32,
    pixels: &[u8],
) -> (Texture, TextureView) {
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("optional Java water flipbooks"),
            size: Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: layers,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        pixels,
    );
    let view = texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    (texture, view)
}
