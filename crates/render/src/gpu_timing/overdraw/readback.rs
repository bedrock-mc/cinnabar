use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use bevy::{camera::Viewport, render::renderer::RenderDevice};

pub(super) const IDLE: u8 = 0;
pub(super) const IN_FLIGHT: u8 = 1;
pub(super) const MAPPED: u8 = 2;
pub(super) const FAILED: u8 = 3;
const PIXEL_BYTES: u32 = 8;

pub(super) struct Target {
    pub(super) size: [u32; 2],
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub(super) buffer: wgpu::Buffer,
    pub(super) row_bytes: u32,
    pub(super) state: Arc<AtomicU8>,
    pub(super) frame: u64,
    pub(super) items: usize,
    pub(super) viewport: Viewport,
}

impl Target {
    /// Allocates one reusable single-sample accumulation target and padded readback buffer.
    pub(super) fn new(device: &RenderDevice, width: u32, height: u32) -> Self {
        let device = device.wgpu_device();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("opaque layer diagnostic"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: super::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let row_bytes = (width * PIXEL_BYTES).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("opaque layer diagnostic readback"),
            size: u64::from(row_bytes) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            size: [width, height],
            texture,
            view,
            buffer,
            row_bytes,
            state: Arc::new(AtomicU8::new(IDLE)),
            frame: 0,
            items: 0,
            viewport: Viewport {
                physical_position: bevy::math::UVec2::ZERO,
                physical_size: bevy::math::UVec2::new(width, height),
                depth: 0.0..1.0,
            },
        }
    }

    /// Reports only completed readbacks; no frame waits for the GPU or saves image data.
    pub(super) fn report(&self) {
        match self.state.load(Ordering::Acquire) {
            MAPPED => {
                let bytes = self.buffer.slice(..).get_mapped_range();
                let stats = summarize(&bytes, self.row_bytes as usize, &self.viewport);
                eprintln!(
                    "RUST_MCBE_OPAQUE_LAYERS {}",
                    serde_json::json!({
                        "frame": self.frame, "viewport": self.viewport.physical_size.to_array(),
                        "phase_items": self.items, "pixels": stats.pixels,
                        "covered_pixels": stats.covered, "layers_total": stats.layers,
                        "mean_layers": stats.layers as f64 / stats.pixels.max(1) as f64,
                        "max_layers": stats.maximum, "saturated_pixels": stats.saturated,
                        "histogram_0_to_15_then_16_plus": stats.histogram,
                        "definition": "alpha-surviving submitted solid/cutout/model samples with depth disabled"
                    })
                );
                drop(bytes);
                self.buffer.unmap();
                self.state.store(IDLE, Ordering::Release);
            }
            FAILED => {
                eprintln!("RUST_MCBE_OPAQUE_LAYERS readback_failed");
                self.state.store(IDLE, Ordering::Release);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct LayerStats {
    pub(super) pixels: u64,
    pub(super) covered: u64,
    pub(super) layers: u64,
    pub(super) maximum: u32,
    pub(super) saturated: u64,
    pub(super) histogram: [u64; 17],
}

/// Counts the viewport's red-channel half floats without including padded rows or other channels.
pub(super) fn summarize(bytes: &[u8], row_bytes: usize, viewport: &Viewport) -> LayerStats {
    let mut stats = LayerStats::default();
    let start = viewport.physical_position;
    for y in start.y..start.y + viewport.physical_size.y {
        for x in start.x..start.x + viewport.physical_size.x {
            let offset = y as usize * row_bytes + x as usize * PIXEL_BYTES as usize;
            let bits = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            let layers = positive_half(bits) as u32;
            stats.pixels += 1;
            stats.covered += u64::from(layers > 0);
            stats.layers += u64::from(layers);
            stats.maximum = stats.maximum.max(layers);
            stats.saturated += u64::from(layers >= 2048);
            stats.histogram[layers.min(16) as usize] += 1;
        }
    }
    stats
}

/// Layer accumulation is nonnegative; preserve the IEEE half-float exponent and mantissa exactly.
fn positive_half(bits: u16) -> f32 {
    let exponent = u32::from((bits >> 10) & 31);
    let fraction = u32::from(bits & 1023);
    if exponent == 0 {
        return fraction as f32 * (1.0 / 16_777_216.0);
    }
    f32::from_bits(((exponent + 112) << 23) | (fraction << 13))
}
