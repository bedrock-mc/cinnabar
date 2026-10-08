//! One retained media texture shared by host UI, quad and entity-material adapters.

use bevy::render::{
    render_resource::{
        Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
        TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
        TextureViewDescriptor,
    },
    renderer::{RenderDevice, RenderQueue},
};

pub struct MediaTexture {
    texture: Texture,
    view: TextureView,
    size: [u32; 2],
    generation: u64,
}

impl MediaTexture {
    /// Reserves one allocation; the caller charges these bytes to its server GPU budget.
    pub fn new(
        device: &RenderDevice,
        size: [u32; 2],
        generation: u64,
        remaining_bytes: u64,
    ) -> Option<Self> {
        let [width, height] = size;
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(4)?;
        let max_side = device.limits().max_texture_dimension_2d;
        if width == 0
            || height == 0
            || width > max_side
            || height > max_side
            || bytes > remaining_bytes
        {
            return None;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Cinnabar server media"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        Some(Self {
            texture,
            view,
            size,
            generation,
        })
    }

    /// Updates existing storage; stale frames and midstream size changes are rejected.
    pub fn upload(
        &self,
        queue: &RenderQueue,
        size: [u32; 2],
        generation: u64,
        rgba: &[u8],
    ) -> bool {
        if size != self.size
            || generation != self.generation
            || rgba.len() as u64 != self.allocated_bytes()
        {
            return false;
        }
        let [width, height] = size;
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: Default::default(),
            },
            rgba,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        true
    }

    /// Every authorized surface samples this view; it does not create another decoder.
    pub fn view(&self, generation: u64) -> Option<&TextureView> {
        (generation == self.generation).then_some(&self.view)
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// Exact texture charge; decoder surfaces and staging are accounted separately.
    pub fn allocated_bytes(&self) -> u64 {
        u64::from(self.size[0]) * u64::from(self.size[1]) * 4
    }
}
