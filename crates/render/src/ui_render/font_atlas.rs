//! Issue the current frame's demand-filled font atlas before drawing.

use crate::queue_writes::ProfiledQueue as _;
use render_model::{UiRenderInput, UiRenderRejectReason};

impl super::textures::UiGpuTextures {
    /// Uploads all missing glyphs before the frame's remapped vertices are published.
    pub(super) fn prepare_fonts(
        &mut self,
        input: &UiRenderInput,
        device: &bevy::render::renderer::RenderDevice,
        queue: &bevy::render::renderer::RenderQueue,
        profile: Option<&super::profile::UiProfile>,
        now: std::time::Duration,
    ) -> Result<(), UiRenderRejectReason> {
        use bevy::render::render_resource::{
            Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo,
        };
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("ui.font_residency", revision = input.revision).entered();
        let buckets = &self.buckets;
        let locations = &self.locations;
        let uploaded = &self.state;
        let mut failed = Vec::new();
        let mut failed_bytes = 0;
        let result = self.fonts.prepare(input, |index, origin, size, pixels| {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("ui.font_write", page = index, bytes = pixels.len())
                .entered();
            let location = locations[index];
            let Some(bucket) = buckets[location.bucket].as_ref() else {
                return;
            };
            if uploaded.uploaded[index].is_none() {
                return;
            }
            if let Some(profile) = profile {
                profile.record_upload(super::profile::UploadKind::Texture, pixels.len() as u64);
            }
            let issued = crate::gpu_allocation::checked(
                device,
                "UI font atlas upload",
                pixels.len() as u64,
                || {
                    queue.profiled_write_texture(
                        TexelCopyTextureInfo {
                            texture: &bucket.texture,
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
                                size[0]
                                    * input.textures.pages()[index].format().bytes_per_texel()
                                        as u32,
                            ),
                            rows_per_image: Some(size[1]),
                        },
                        Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                    )
                },
            );
            if issued.is_none() {
                failed.push(index);
                failed_bytes += pixels.len() as u64;
            }
        });
        if !failed.is_empty() {
            self.failed_font_uploads(&failed, failed_bytes, now);
            return Err(UiRenderRejectReason::TextureAllocationFailed {
                bytes: failed_bytes,
            });
        }
        result
    }
}
