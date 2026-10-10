//! Issue the current frame's demand-filled font atlas before drawing.

use render_model::{UiRenderInput, UiRenderRejectReason};

impl super::textures::UiGpuTextures {
    /// Uploads all missing glyphs before the frame's remapped vertices are published.
    pub(super) fn prepare_fonts(
        &mut self,
        input: &UiRenderInput,
        queue: &bevy::render::renderer::RenderQueue,
        profile: Option<&super::profile::UiProfile>,
    ) -> Result<(), UiRenderRejectReason> {
        use bevy::render::render_resource::{
            Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo,
        };
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("ui.font_residency", revision = input.revision).entered();
        let buckets = &self.buckets;
        let locations = &self.locations;
        self.fonts.prepare(input, |index, origin, size, pixels| {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("ui.font_write", page = index, bytes = pixels.len())
                .entered();
            let location = locations[index];
            if let Some(profile) = profile {
                profile.record_upload(super::profile::UploadKind::Texture, pixels.len() as u64);
            }
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &buckets[location.bucket].texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: origin[0],
                        y: origin[1],
                        z: location.layer,
                    },
                    aspect: Default::default(),
                },
                pixels,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(
                        size[0] * input.textures.pages()[index].format().bytes_per_texel() as u32,
                    ),
                    rows_per_image: Some(size[1]),
                },
                Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
            );
        })
    }
}
