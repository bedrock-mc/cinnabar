//! Reuses a small zero buffer to clear storage images of every supported dimension.

use wgpu::{Buffer, CommandEncoder, Device, Texture};

/// Scratch bytes reused across images and frames; larger images are copied in tiles.
const CLEAR_BYTES: u64 = 64 * 1024;

pub(super) struct ImageClears {
    zero: Buffer,
}

impl ImageClears {
    /// Allocates zero-initialized copy bytes without requiring optional device features.
    pub(super) fn new(device: &Device) -> Self {
        Self {
            zero: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shader pack image clear zeros"),
                size: CLEAR_BYTES,
                usage: wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
        }
    }

    /// Clears all texels, splitting wide rows and volume slices into bounded copies.
    pub(super) fn record(&self, encoder: &mut CommandEncoder, image: &Texture) {
        let bytes = image
            .format()
            .block_copy_size(None)
            .expect("storage image texel size");
        let tile_width = image.width().min(CLEAR_BYTES as u32 / bytes);
        for x in (0..image.width()).step_by(tile_width as usize) {
            let width = tile_width.min(image.width() - x);
            let pitch = (width * bytes).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let tile_height = (CLEAR_BYTES as u32 / pitch).min(image.height());
            for z in 0..image.depth_or_array_layers() {
                for y in (0..image.height()).step_by(tile_height as usize) {
                    encoder.copy_buffer_to_texture(
                        wgpu::TexelCopyBufferInfo {
                            buffer: &self.zero,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(pitch),
                                rows_per_image: None,
                            },
                        },
                        wgpu::TexelCopyTextureInfo {
                            origin: wgpu::Origin3d { x, y, z },
                            ..image.as_image_copy()
                        },
                        wgpu::Extent3d {
                            width,
                            height: tile_height.min(image.height() - y),
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_cover_wide_rows_and_every_volume_slice() {
        let Some(gpu) = crate::gpu_snapshot::Gpu::new() else {
            return;
        };
        let clears = ImageClears::new(&gpu.device);
        for (dimension, format, size, load) in [
            (
                wgpu::TextureDimension::D1,
                wgpu::TextureFormat::Rgba32Uint,
                [gpu.device.limits().max_texture_dimension_1d.min(8192), 1, 1],
                "textureLoad(image, i32(id.x), 0)",
            ),
            (
                wgpu::TextureDimension::D3,
                wgpu::TextureFormat::Rgba8Uint,
                [17, 257, 3],
                "textureLoad(image, vec3<i32>(i32(id.x % 17u), i32((id.x / 17u) % 257u), i32(id.x / (17u * 257u))), 0)",
            ),
        ] {
            let image = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: size[2],
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension,
                format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let texel_bytes = format.block_copy_size(None).unwrap();
            let count = size.into_iter().product::<u32>();
            gpu.queue.write_texture(
                image.as_image_copy(),
                &vec![255; (count * texel_bytes) as usize],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * texel_bytes),
                    rows_per_image: Some(size[1]),
                },
                image.size(),
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clears.record(&mut encoder, &image);
            gpu.queue.submit([encoder.finish()]);
            let view = image.create_view(&Default::default());
            let shape = if dimension == wgpu::TextureDimension::D1 {
                "1d"
            } else {
                "3d"
            };
            let source = format!("@group(0) @binding(0) var image: texture_{shape}<u32>;
                @group(0) @binding(1) var<storage, read_write> output: array<vec4<u32>>;
                @compute @workgroup_size(1) fn witness(@builtin(global_invocation_id) id: vec3<u32>) {{ output[id.x] = {load}; }}");
            let pixels = gpu.compute_readback(
                &source,
                &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                }],
                1,
                count as usize,
                count,
            );
            assert!(pixels.iter().all(|pixel| *pixel == [0; 4]), "{dimension:?}");
        }
    }
}
