//! Optional carrier for HUD sprites the pinned HUD carrier does not hold (hardcore hearts).
//!
//! Layout: magic, version, then one `width, height, rgba8` image per [`HudExtraRole`] in
//! declaration order, then a SHA-256 of everything before it.

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::HudTextureRole;

pub const HUD_EXTRAS_MAGIC: [u8; 8] = *b"MCBEHXT1";
pub const HUD_EXTRAS_VERSION: u32 = 1;
pub const HUD_EXTRA_SIDE: u32 = 9;
pub const MAX_HUD_EXTRAS_BYTES: usize = 64 * 1024;

const HASH_BYTES: usize = 32;
const HEADER_BYTES: usize = 12;
const IMAGE_BYTES: usize = (HUD_EXTRA_SIDE * HUD_EXTRA_SIDE * 4) as usize;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HudExtrasError {
    #[error("invalid HUD extras carrier: {0}")]
    Invalid(&'static str),
}

macro_rules! hardcore_hearts {
    ($($role:ident => $path:literal,)+) => {
        /// Hardcore-style heart sprites in their carrier order.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[repr(u8)]
        pub enum HudExtraRole { $($role,)+ }
        impl HudExtraRole {
            pub const ALL: [Self; [$(stringify!($role)),+].len()] = [$(Self::$role),+];
            /// Pack-relative source PNG.
            pub const fn source_path(self) -> &'static str { match self { $(Self::$role => $path,)+ } }
            /// The hardcore counterpart of a standard foreground role.
            pub const fn for_heart(role: HudTextureRole) -> Option<Self> {
                match role { $(HudTextureRole::$role => Some(Self::$role),)+ _ => None }
            }
        }
    };
}

hardcore_hearts! {
    HeartFull => "textures/ui/hardcore/heart.png",
    HeartHalf => "textures/ui/hardcore/heart_half.png",
    HeartFlashFull => "textures/ui/hardcore/heart_flash.png",
    HeartFlashHalf => "textures/ui/hardcore/heart_flash_half.png",
    PoisonHeartFull => "textures/ui/hardcore/poison_heart.png",
    PoisonHeartHalf => "textures/ui/hardcore/poison_heart_half.png",
    PoisonHeartFlashFull => "textures/ui/hardcore/poison_heart_flash.png",
    PoisonHeartFlashHalf => "textures/ui/hardcore/poison_heart_flash_half.png",
    WitherHeartFull => "textures/ui/hardcore/wither_heart.png",
    WitherHeartHalf => "textures/ui/hardcore/wither_heart_half.png",
    WitherHeartFlashFull => "textures/ui/hardcore/wither_heart_flash.png",
    WitherHeartFlashHalf => "textures/ui/hardcore/wither_heart_flash_half.png",
    FreezeHeartFull => "textures/ui/hardcore/freeze_heart.png",
    FreezeHeartHalf => "textures/ui/hardcore/freeze_heart_half.png",
    FreezeHeartFlashFull => "textures/ui/hardcore/freeze_heart_flash.png",
    FreezeHeartFlashHalf => "textures/ui/hardcore/freeze_heart_flash_half.png",
    AbsorptionHeartFull => "textures/ui/hardcore/absorption_heart.png",
    AbsorptionHeartHalf => "textures/ui/hardcore/absorption_heart_half.png",
}

/// Decoded 9x9 RGBA8 sprites indexed by `HudExtraRole as usize`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HudExtras {
    images: Vec<Box<[u8]>>,
}

impl HudExtras {
    /// `images` must hold one 9x9 RGBA8 buffer per role, in `HudExtraRole::ALL` order.
    pub fn new(images: Vec<Box<[u8]>>) -> Result<Self, HudExtrasError> {
        if images.len() != HudExtraRole::ALL.len() || images.iter().any(|i| i.len() != IMAGE_BYTES)
        {
            return Err(HudExtrasError::Invalid("unexpected image count or size"));
        }
        Ok(Self { images })
    }

    pub fn rgba8(&self, role: HudExtraRole) -> &[u8] {
        &self.images[role as usize]
    }
}

pub fn encode_hud_extras(extras: &HudExtras) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES + extras.images.len() * (8 + IMAGE_BYTES));
    bytes.extend_from_slice(&HUD_EXTRAS_MAGIC);
    bytes.extend_from_slice(&HUD_EXTRAS_VERSION.to_le_bytes());
    for image in &extras.images {
        bytes.extend_from_slice(&HUD_EXTRA_SIDE.to_le_bytes());
        bytes.extend_from_slice(&HUD_EXTRA_SIDE.to_le_bytes());
        bytes.extend_from_slice(image);
    }
    let hash: [u8; HASH_BYTES] = Sha256::digest(&bytes).into();
    bytes.extend_from_slice(&hash);
    bytes
}

/// Decodes and hash-checks a carrier; the second value is its SHA-256 identity.
pub fn decode_hud_extras(bytes: &[u8]) -> Result<(HudExtras, [u8; HASH_BYTES]), HudExtrasError> {
    let invalid = HudExtrasError::Invalid;
    if bytes.len() > MAX_HUD_EXTRAS_BYTES || bytes.len() < HEADER_BYTES + HASH_BYTES {
        return Err(invalid("bad length"));
    }
    let (body, hash) = bytes.split_at(bytes.len() - HASH_BYTES);
    let expected: [u8; HASH_BYTES] = Sha256::digest(body).into();
    if hash != expected {
        return Err(invalid("hash mismatch"));
    }
    if body[..8] != HUD_EXTRAS_MAGIC || body[8..12] != HUD_EXTRAS_VERSION.to_le_bytes() {
        return Err(invalid("unsupported header"));
    }
    let side = HUD_EXTRA_SIDE.to_le_bytes();
    let mut cursor = HEADER_BYTES;
    let mut images = Vec::with_capacity(HudExtraRole::ALL.len());
    for _ in HudExtraRole::ALL {
        let header = body.get(cursor..cursor + 8).ok_or(invalid("truncated"))?;
        if header[..4] != side || header[4..] != side {
            return Err(invalid("unexpected image dimensions"));
        }
        let start = cursor + 8;
        let image = body
            .get(start..start + IMAGE_BYTES)
            .ok_or(invalid("truncated"))?;
        images.push(Box::<[u8]>::from(image));
        cursor = start + IMAGE_BYTES;
    }
    if cursor != body.len() {
        return Err(invalid("trailing bytes"));
    }
    Ok((HudExtras { images }, expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> HudExtras {
        HudExtras::new(
            (0..HudExtraRole::ALL.len())
                .map(|seed| vec![seed as u8; IMAGE_BYTES].into_boxed_slice())
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn round_trips_with_a_stable_identity() {
        let bytes = encode_hud_extras(&sample());
        let (decoded, identity) = decode_hud_extras(&bytes).unwrap();
        assert_eq!(decoded, sample());
        assert_eq!(identity, decode_hud_extras(&bytes).unwrap().1);
    }

    #[test]
    fn rejects_corruption_truncation_and_wrong_counts() {
        let mut bytes = encode_hud_extras(&sample());
        assert!(decode_hud_extras(&bytes[..20]).is_err());
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(decode_hud_extras(&bytes).is_err());
        assert!(HudExtras::new(vec![]).is_err());
    }

    #[test]
    fn every_standard_heart_foreground_has_a_hardcore_counterpart() {
        for role in HudExtraRole::ALL {
            assert_eq!(HudExtraRole::ALL[role as usize], role);
        }
        assert_eq!(
            HudExtraRole::for_heart(HudTextureRole::HeartFull),
            Some(HudExtraRole::HeartFull)
        );
        assert_eq!(HudExtraRole::for_heart(HudTextureRole::Crosshair), None);
    }
}
