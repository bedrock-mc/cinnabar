//! Colour-buffer sides, mip views and sampled snapshots.

use wgpu::{Device, TextureFormat, TextureUsages, TextureView};

/// One colour buffer's textures.
pub(crate) struct ColorBuffer {
    pub size: [u32; 2],
    textures: Vec<wgpu::Texture>,
    /// Every mip, for sampling; per side.
    pub sampled: Vec<TextureView>,
    /// Mip 0 only, for attachments and storage; per side.
    pub attachment: Vec<TextureView>,
    /// Each mip level alone, per side (empty without mipmaps).
    pub levels: Vec<Vec<TextureView>>,
}

impl ColorBuffer {
    /// Allocates colour sides and their full-chain, attachment and per-level views.
    pub(super) fn new(
        device: &Device,
        label: &str,
        format: TextureFormat,
        size: [u32; 2],
        mips: u32,
        sides: usize,
        usage: TextureUsages,
    ) -> Self {
        let textures: Vec<_> = (0..sides)
            .map(|side| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(&format!(
                        "{label}{}",
                        if side == 1 { " alternate" } else { "" }
                    )),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: mips,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            })
            .collect();
        let level = |texture: &wgpu::Texture, mip| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        Self {
            size,
            sampled: textures
                .iter()
                .map(|texture| texture.create_view(&Default::default()))
                .collect(),
            attachment: textures.iter().map(|texture| level(texture, 0)).collect(),
            levels: if mips > 1 {
                textures
                    .iter()
                    .map(|texture| (0..mips).map(|mip| level(texture, mip)).collect())
                    .collect()
            } else {
                vec![]
            },
            textures,
        }
    }

    /// A sampled copy with the same allocated mip chain as this colour buffer.
    pub(super) fn snapshot(&self, device: &Device, label: &str) -> Self {
        Self::new(
            device,
            label,
            self.texture(0).format(),
            self.size,
            self.texture(0).mip_level_count(),
            1,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        )
    }

    /// Copies every mip of the selected side into its sampled snapshot.
    pub(super) fn copy_snapshot(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        side: usize,
        snapshot: &Self,
    ) {
        for (mip, size) in snapshot_levels(self.size, self.texture(side).mip_level_count()) {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    mip_level: mip,
                    ..self.texture(side).as_image_copy()
                },
                wgpu::TexelCopyTextureInfo {
                    mip_level: mip,
                    ..snapshot.texture(0).as_image_copy()
                },
                size,
            );
        }
    }

    /// The whole chain when mips are visible to this pass, otherwise only its base level.
    pub(super) fn sampled_view(&self, side: usize, mipmapped: bool) -> &TextureView {
        if mipmapped {
            &self.sampled[side]
        } else {
            &self.attachment[side]
        }
    }

    /// Index of the main (`false`) or alternate (`true`) texture; buffers without an
    /// alternate always use main.
    pub(crate) fn side(&self, alternate: bool) -> usize {
        usize::from(alternate && self.textures.len() > 1)
    }

    pub(crate) fn texture(&self, side: usize) -> &wgpu::Texture {
        &self.textures[side]
    }
}

/// The mip extents a colour snapshot must preserve, including rectangular tail levels.
fn snapshot_levels(size: [u32; 2], mips: u32) -> impl Iterator<Item = (u32, wgpu::Extent3d)> {
    (0..mips).map(move |mip| {
        (
            mip,
            wgpu::Extent3d {
                width: (size[0] >> mip).max(1),
                height: (size[1] >> mip).max(1),
                depth_or_array_layers: 1,
            },
        )
    })
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn snapshots_copy_every_allocated_mip() {
        let sizes: Vec<_> = snapshot_levels([16, 4], 5)
            .map(|(level, size)| (level, size.width, size.height))
            .collect();
        assert_eq!(
            sizes,
            [(0, 16, 4), (1, 8, 2), (2, 4, 1), (3, 2, 1), (4, 1, 1)]
        );
    }
    #[test]
    fn gpu_snapshots_keep_the_source_chain_and_per_pass_mip_visibility() {
        let Some(gpu) = crate::gpu_snapshot::Gpu::new() else {
            return;
        };
        let source = ColorBuffer::new(
            &gpu.device,
            "snapshot source",
            TextureFormat::Rgba8Unorm,
            [16, 4],
            5,
            1,
            TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC,
        );
        let snapshot = source.snapshot(&gpu.device, "snapshot copy");
        assert_eq!(snapshot.texture(0).mip_level_count(), 5);
        assert!(std::ptr::eq(
            snapshot.sampled_view(0, false),
            &snapshot.attachment[0]
        ));
        assert!(std::ptr::eq(
            snapshot.sampled_view(0, true),
            &snapshot.sampled[0]
        ));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        source.copy_snapshot(&mut encoder, 0, &snapshot);
        gpu.queue.submit([encoder.finish()]);
    }
}
