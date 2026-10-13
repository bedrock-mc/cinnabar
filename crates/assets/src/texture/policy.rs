//! Shared bounds for decoded pack images and their retained texture pages.

use std::sync::OnceLock;

/// Maximum physical extent supported by the pack texture consumers.
pub const MAX_TEXTURE_SIDE: u32 = 4096;
/// One decoded source may retain at most four million RGBA8 texels.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Bounds decoder working storage, including higher-bit-depth source pixels.
pub const MAX_DECODE_BYTES: u64 = (MAX_IMAGE_BYTES as u64) * 4;
/// Each subscriber bounds its retained sources and upload pages independently.
pub const MAX_SUBSCRIBER_BYTES: usize = MAX_IMAGE_BYTES * 4;

/// Device dimensions combined with the shared source and storage budgets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TexturePolicy {
    max_side: u32,
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
        }
    }

    /// Largest physical texture dimension admitted on this device.
    pub const fn max_side(self) -> u32 {
        self.max_side
    }

    /// Checks source area before decoding; vertical strips upload as square frames.
    pub fn source_bytes(self, width: u32, height: u32, vertical_strip: bool) -> Option<usize> {
        let strip = vertical_strip
            && width > 0
            && height > width
            && height <= u32::from(u16::MAX)
            && height.is_multiple_of(width);
        if width > self.max_side || (height > self.max_side && !strip) {
            return None;
        }
        rgba8_bytes(width, height).filter(|&bytes| bytes <= MAX_IMAGE_BYTES)
    }

    /// Checks physical page dimensions and its retained RGBA8 allocation.
    pub fn page_bytes(self, width: u32, height: u32) -> Option<usize> {
        if width > self.max_side || height > self.max_side {
            return None;
        }
        rgba8_bytes(width, height).filter(|&bytes| bytes <= MAX_SUBSCRIBER_BYTES)
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
            if bytes > MAX_SUBSCRIBER_BYTES {
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
