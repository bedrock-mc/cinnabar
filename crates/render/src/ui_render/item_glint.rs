//! Retained item glint texture, separate from UI atlas allocations.

use super::profile::{UiProfile, UploadKind};
use crate::queue_writes::ProfiledQueue as _;
use bevy::render::{
    render_resource::{
        AddressMode, Extent3d, FilterMode, Sampler, SamplerDescriptor, TexelCopyBufferLayout,
        Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    },
    renderer::{RenderDevice, RenderQueue},
};
use render_model::{UiTextureFormat, UiTexturePage};

pub(super) struct GpuItemGlint {
    texture: Texture,
    pub(super) view: TextureView,
    pub(super) sampler: Sampler,
    identity: Option<([u32; 2], [u8; 32])>,
    pub(super) bytes: usize,
}

impl GpuItemGlint {
    /// Creates a transparent fallback until the UI publishes its carrier or pack image.
    pub(super) fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        profile: Option<&UiProfile>,
    ) -> Self {
        let texture = upload(device, queue, None, profile);
        Self {
            view: texture.create_view(&Default::default()),
            texture,
            sampler: device.create_sampler(&SamplerDescriptor {
                label: Some("repeating item glint sampler"),
                address_mode_u: AddressMode::Repeat,
                address_mode_v: AddressMode::Repeat,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..Default::default()
            }),
            identity: None,
            bytes: 4,
        }
    }

    /// Uploads changed pixels once and reports whether cached UI bindings must be replaced.
    pub(super) fn prepare(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        page: Option<&UiTexturePage>,
        profile: Option<&UiProfile>,
    ) -> bool {
        let page = page.filter(|page| {
            page.format() == UiTextureFormat::Rgba8
                && page.font_atlas_side().is_none()
                && page
                    .dimensions()
                    .into_iter()
                    .all(|side| side <= device.limits().max_texture_dimension_2d)
        });
        let identity = page.map(|page| (page.dimensions(), page.identity()));
        if self.identity == identity {
            return false;
        }
        self.texture = upload(device, queue, page, profile);
        self.view = self.texture.create_view(&Default::default());
        self.identity = identity;
        self.bytes = page.map_or(4, |page| page.pixels().len());
        true
    }
}

/// Allocates and writes one bounded RGBA image, preserving its native dimensions.
fn upload(
    device: &RenderDevice,
    queue: &RenderQueue,
    page: Option<&UiTexturePage>,
    profile: Option<&UiProfile>,
) -> Texture {
    let [width, height] = page.map_or([1, 1], UiTexturePage::dimensions);
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("UI item glint texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let pixels = page.map_or(&[0; 4][..], UiTexturePage::pixels);
    queue.profiled_write_texture(
        texture.as_image_copy(),
        pixels,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    if let Some(profile) = profile {
        profile.record_upload(UploadKind::Texture, pixels.len() as u64);
    }
    texture
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn texture_identity_tracks_pixels_dimensions_and_removal() {
        let world = super::super::ordered_command_tests::binding_world();
        let device = world.resource::<RenderDevice>();
        let queue = world.resource::<RenderQueue>();
        let mut glint = GpuItemGlint::new(device, queue, None);
        let first =
            UiTexturePage::owned([1, 2], Arc::from([200, 0, 0, 255, 0, 200, 0, 255])).unwrap();
        let reshaped = UiTexturePage::owned([2, 1], Arc::from(first.pixels())).unwrap();
        let second =
            UiTexturePage::owned([1, 2], Arc::from([0, 0, 200, 255, 200, 200, 0, 255])).unwrap();
        assert!(glint.prepare(device, queue, Some(&first), None));
        assert_eq!(glint.identity, Some(([1, 2], first.identity())));
        assert_eq!(glint.bytes, 8);
        assert!(!glint.prepare(device, queue, Some(&first), None));
        assert!(glint.prepare(device, queue, Some(&reshaped), None));
        assert_eq!(glint.texture.width(), 2);
        assert!(glint.prepare(device, queue, Some(&second), None));
        assert_eq!(glint.identity, Some(([1, 2], second.identity())));
        assert!(glint.prepare(device, queue, None, None));
        assert_eq!(glint.texture.width(), 1);
        assert_eq!(glint.bytes, 4);
        assert!(!glint.prepare(device, queue, None, None));
    }
}
