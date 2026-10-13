//! Shared bounds for decoded pack images and their retained texture pages.

use std::sync::OnceLock;

/// Maximum physical extent supported by the pack texture consumers.
pub const MAX_TEXTURE_SIDE: u32 = 4096;
/// One decoded source may retain at most four million RGBA8 texels.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Bounds decoder working storage, including higher-bit-depth source pixels.
const DECODE_BYTES_PER_RGBA8_BYTE: u64 = 4;
pub const MAX_DECODE_BYTES: u64 = (MAX_IMAGE_BYTES as u64) * DECODE_BYTES_PER_RGBA8_BYTE;
/// Each subscriber bounds its retained sources and upload pages independently.
pub const MAX_SUBSCRIBER_BYTES: usize = MAX_IMAGE_BYTES * 4;
/// Terrain has a 64 MiB floor and grows to at most 1 GiB with the memory tier.
pub const MIN_TERRAIN_BYTES: usize = MAX_SUBSCRIBER_BYTES;
const MAX_TERRAIN_MEMORY_TIER: u32 = 4;
pub const MAX_TERRAIN_BYTES: usize = MIN_TERRAIN_BYTES << MAX_TERRAIN_MEMORY_TIER;
/// A 512px animation can decode thirty-two RGBA8 frames before resizing.
pub const MAX_TERRAIN_IMAGE_BYTES: usize = MAX_IMAGE_BYTES * 2;

/// Device dimensions combined with the shared source and storage budgets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TexturePolicy {
    max_side: u32,
    storage_bytes: usize,
    image_bytes: usize,
}

impl TexturePolicy {
    /// Restricts physical textures to the device and the consumers' supported extent.
    pub const fn for_device(max_dimension_2d: u32) -> Self {
        Self {
            max_side: if max_dimension_2d < MAX_TEXTURE_SIDE {
                max_dimension_2d
            } else {
                MAX_TEXTURE_SIDE
            },
            storage_bytes: MAX_SUBSCRIBER_BYTES,
            image_bytes: MAX_IMAGE_BYTES,
        }
    }

    /// Sizes terrain storage from the pack memory tier and the device's texture extent.
    pub const fn for_terrain(max_dimension_2d: u32, memory_tier: u32) -> Self {
        let tier = if memory_tier > MAX_TERRAIN_MEMORY_TIER {
            MAX_TERRAIN_MEMORY_TIER
        } else {
            memory_tier
        };
        Self {
            storage_bytes: MIN_TERRAIN_BYTES << tier,
            image_bytes: MAX_TERRAIN_IMAGE_BYTES,
            ..Self::for_device(max_dimension_2d)
        }
    }

    /// Maximum retained source or upload bytes for this subscriber.
    pub const fn storage_bytes(self) -> usize {
        self.storage_bytes
    }

    /// Bounds decoder working storage, including higher-bit-depth source pixels.
    pub const fn decode_bytes(self) -> u64 {
        (self.image_bytes as u64) * DECODE_BYTES_PER_RGBA8_BYTE
    }

    /// Largest physical texture dimension admitted on this device.
    pub const fn max_side(self) -> u32 {
        self.max_side
    }

    /// Checks source area before decoding; row and column strips upload as square frames.
    pub fn source_bytes(self, width: u32, height: u32, allow_strip: bool) -> Option<usize> {
        let side = width.min(height);
        let extent = width.max(height);
        let strip = allow_strip
            && side > 0
            && side < extent
            && side <= self.max_side
            && extent <= u32::from(u16::MAX)
            && extent.is_multiple_of(side);
        if (width > self.max_side || height > self.max_side) && !strip {
            return None;
        }
        rgba8_bytes(width, height).filter(|&bytes| bytes <= self.image_bytes)
    }

    /// Checks physical page dimensions and its retained RGBA8 allocation.
    pub fn page_bytes(self, width: u32, height: u32) -> Option<usize> {
        if width > self.max_side || height > self.max_side {
            return None;
        }
        rgba8_bytes(width, height).filter(|&bytes| bytes <= self.storage_bytes)
    }

    /// Checks every mip and layer before admitting a terrain array at full resolution.
    pub fn terrain_bytes(self, side: u32, layers: usize) -> Option<usize> {
        if !side.is_power_of_two() {
            return None;
        }
        let mut bytes = 0usize;
        let mut mip = side;
        loop {
            bytes = bytes.checked_add(self.page_bytes(mip, mip)?.checked_mul(layers)?)?;
            if bytes > self.storage_bytes {
                return None;
            }
            if mip == 1 {
                return Some(bytes);
            }
            mip /= 2;
        }
    }
}

/// Returns a checked nonempty RGBA8 length without allocating pixels.
pub fn rgba8_bytes(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)
}

static DEVICE_POLICY: OnceLock<TexturePolicy> = OnceLock::new();
static TERRAIN_DEVICE_POLICY: OnceLock<TexturePolicy> = OnceLock::new();

/// Publishes terrain capacity before the renderer releases pack preparation workers.
pub fn set_terrain_device_policy(max_dimension_2d: u32, memory_tier: u32) {
    let _ = TERRAIN_DEVICE_POLICY.set(TexturePolicy::for_terrain(max_dimension_2d, memory_tier));
}

/// Uses the installed terrain budget, or the storage floor for offline preparation.
pub fn terrain_texture_policy() -> TexturePolicy {
    TERRAIN_DEVICE_POLICY
        .get()
        .copied()
        .unwrap_or(TexturePolicy::for_terrain(MAX_TEXTURE_SIDE, 0))
}

/// Publishes the immutable device bound before optional pack preparation starts.
pub fn set_device_policy(max_dimension_2d: u32) {
    let _ = DEVICE_POLICY.set(TexturePolicy::for_device(max_dimension_2d));
}

/// Uses the installed device policy, or conservative limits for offline preparation.
pub fn device_policy() -> TexturePolicy {
    DEVICE_POLICY
        .get()
        .copied()
        .unwrap_or(TexturePolicy::for_device(MAX_TEXTURE_SIDE))
}

#[cfg(test)]
mod tests;
