//! Device inputs and the ordinary desktop render-distance recommendation.

pub const MIN_RENDER_DISTANCE_CHUNKS: u8 = 5;
const MAX_DEVICE_DISTANCE: u8 = 96;
const SHARED_MEMORY_DISTANCE: u8 = 16;
const SHARED_MEMORY_THRESHOLD_BYTES: u64 = 350 * 1024 * 1024;
const GPU_BYTES_PER_CHUNK_SQUARED: f64 = 514_718.554_687_5;

/// Installed RAM and dedicated graphics memory, in bytes rather than current free memory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderDistanceDevice {
    pub physical_memory_bytes: u64,
    pub dedicated_graphics_memory_bytes: u64,
    /// Platform type 2 can use the whole graphics-memory estimate; desktop uses half.
    pub use_full_graphics_memory: bool,
}

/// The ordinary device levels and their recommended first-launch selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderDistanceDefaults {
    pub recommended: u8,
    pub native_maximum: u8,
}

impl Default for RenderDistanceDefaults {
    /// Missing device information uses the smallest selectable distance.
    fn default() -> Self {
        Self {
            recommended: MIN_RENDER_DISTANCE_CHUNKS,
            native_maximum: MIN_RENDER_DISTANCE_CHUNKS,
        }
    }
}

impl RenderDistanceDevice {
    /// Computes the recommendation separately from the RAM-limited list of levels.
    pub fn defaults(self) -> RenderDistanceDefaults {
        if self.physical_memory_bytes == 0 {
            return RenderDistanceDefaults::default();
        }
        let ram_units = (self.physical_memory_bytes >> 10).wrapping_sub(0x180000) / 0xa00;
        let native_maximum = ((ram_units as f32).sqrt() as u32).clamp(
            MIN_RENDER_DISTANCE_CHUNKS as u32,
            MAX_DEVICE_DISTANCE as u32,
        ) as u8;
        let shared = self.dedicated_graphics_memory_bytes < SHARED_MEMORY_THRESHOLD_BYTES;
        let graphics_limit = if shared {
            SHARED_MEMORY_DISTANCE
        } else {
            let bytes =
                self.dedicated_graphics_memory_bytes >> u32::from(!self.use_full_graphics_memory);
            ((bytes as f64 / GPU_BYTES_PER_CHUNK_SQUARED).sqrt().round() as u32)
                .min(MAX_DEVICE_DISTANCE as u32) as u8
        };
        let limit = native_maximum.min(graphics_limit).max(7);
        let recommended = (MIN_RENDER_DISTANCE_CHUNKS + (limit - MIN_RENDER_DISTANCE_CHUNKS) / 2)
            .min(native_maximum);
        RenderDistanceDefaults {
            recommended,
            native_maximum,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_inputs_choose_the_recommendation_and_ram_levels_independently() {
        for (ram_gib, gpu_mib, full, expected) in [
            (2, 0, false, (9, 14)),
            (4, 0, false, (10, 32)),
            (8, 256, false, (10, 51)),
            (8, 4096, false, (28, 51)),
            (16, 4096, false, (35, 77)),
            (32, 8192, false, (48, 96)),
            (32, 8192, true, (50, 96)),
            (1, 0, false, (10, 96)),
        ] {
            let defaults = RenderDistanceDevice {
                physical_memory_bytes: ram_gib << 30,
                dedicated_graphics_memory_bytes: gpu_mib << 20,
                use_full_graphics_memory: full,
            }
            .defaults();
            assert_eq!((defaults.recommended, defaults.native_maximum), expected);
        }
    }

    #[test]
    fn shared_memory_threshold_and_small_level_lists_are_exact() {
        let device = RenderDistanceDevice {
            physical_memory_bytes: 16 << 30,
            dedicated_graphics_memory_bytes: SHARED_MEMORY_THRESHOLD_BYTES - 1,
            use_full_graphics_memory: false,
        };
        assert_eq!(device.defaults().recommended, 10);
        assert_eq!(
            RenderDistanceDevice {
                dedicated_graphics_memory_bytes: SHARED_MEMORY_THRESHOLD_BYTES,
                ..device
            }
            .defaults()
            .recommended,
            12
        );
        assert_eq!(
            RenderDistanceDevice {
                physical_memory_bytes: 1536 << 20,
                ..device
            }
            .defaults()
            .recommended,
            MIN_RENDER_DISTANCE_CHUNKS
        );
        assert_eq!(
            RenderDistanceDevice::default().defaults(),
            RenderDistanceDefaults::default()
        );
    }
}
