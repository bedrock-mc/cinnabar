//! The local player's own skin, loaded once at startup from `<assets>/skin/player.png`.
//!
//! Cosmetic and non-fatal: any load failure logs once and falls back to the vanilla default
//! skin (see `render::default_actor_skin_rgba8`). The same bytes back both the ClientData login
//! upload and the local body / HUD paperdoll render.

use std::path::Path;
use std::sync::Arc;

use bevy::prelude::Resource;
use sha2::{Digest, Sha256};

use crate::install_layout::InstallLayout;

const DEFAULT_ARM_SIZE: &str = "wide";

/// The local player's client-authored skin and stable local uuid. Cheap to clone (Arc-backed):
/// `local_uuid` keys the synthetic self profile the client-world stream inserts when the server
/// never echoes the local player onto the player list.
#[derive(Debug, Clone, Resource)]
pub(crate) struct LocalPlayerSkin {
    pub rgba8: protocol::SkinRgba8,
    pub width: u32,
    pub height: u32,
    pub arm_size: Arc<str>,
    pub local_uuid: [u8; 16],
}

impl LocalPlayerSkin {
    /// Loads a classic skin within vanilla's upload size limit; a failure uses the default skin.
    #[must_use]
    pub fn load(layout: &InstallLayout, display_name: &str) -> Self {
        let path = layout.player_skin_asset();
        let (rgba8, side) = match load_normalized_skin(&path) {
            Ok(pixels) => pixels,
            Err(reason) => {
                bevy::log::warn!(
                    path = %path.display(),
                    reason = %reason,
                    "local player skin unavailable; using the default skin"
                );
                (
                    render::default_actor_skin_rgba8(),
                    protocol::CLASSIC_SKIN_SIDE,
                )
            }
        };
        Self::from_rgba8(rgba8, side, display_name)
    }

    /// A default-skinned identity, for construction sites without a loaded PNG (e.g. tests).
    #[cfg(test)]
    #[must_use]
    pub fn generated_default(display_name: &str) -> Self {
        Self::from_rgba8(
            render::default_actor_skin_rgba8(),
            protocol::CLASSIC_SKIN_SIDE,
            display_name,
        )
    }

    /// Keeps classic upload dimensions independent of the renderer's larger shared array.
    fn from_rgba8(packed: protocol::SkinRgba8, side: usize, display_name: &str) -> Self {
        let mut rgba8 = Vec::with_capacity(side * side * 4);
        for y in 0..side {
            for x in 0..side {
                let source = (y * render::STANDARD_SKIN_SIDE / side * render::STANDARD_SKIN_SIDE
                    + x * render::STANDARD_SKIN_SIDE / side)
                    * 4;
                rgba8.extend_from_slice(&packed[source..source + 4]);
            }
        }
        Self {
            rgba8: rgba8.into(),
            width: side as u32,
            height: side as u32,
            arm_size: Arc::from(DEFAULT_ARM_SIZE),
            local_uuid: stable_local_uuid(display_name),
        }
    }

    /// The per-frame render skin for the local body and HUD paperdoll.
    #[must_use]
    pub fn player_skin(&self) -> protocol::PlayerSkin {
        protocol::PlayerSkin::Standard(protocol::StandardSkin {
            geometry: None,
            cape: None,
            width: self.width,
            height: self.height,
            rgba8: self.rgba8.clone(),
        })
    }

    /// The login upload payload; allocates the byte copy the JWT encoder needs.
    #[must_use]
    pub fn to_client_skin(&self) -> protocol::ClientSkin {
        protocol::ClientSkin {
            rgba8: self.rgba8.to_vec(),
            width: self.width,
            height: self.height,
            arm_size: self.arm_size.to_string(),
        }
    }
}

/// Packs supported source pixels, retaining a legal classic size for the login upload.
fn load_normalized_skin(path: &Path) -> Result<(protocol::SkinRgba8, usize), String> {
    let image = image::open(path).map_err(|error| error.to_string())?;
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let pixels = render::ActorSkinPixels {
        width,
        height,
        rgba8: rgba.into_raw().into(),
    };
    render::normalize_actor_skin(&pixels)
        .map(|pixels| {
            (
                pixels,
                (width as usize).min(protocol::MAX_CLASSIC_SKIN_SIDE),
            )
        })
        .ok_or_else(|| format!("unsupported skin dimensions {width}x{height} or byte length"))
}

/// A deterministic non-zero uuid derived from the display name, stable across a session so the
/// client-world synthetic self profile keeps one key. Not a network identity: the login uuid is
/// generated independently and the server never echoes the local player onto the player list.
fn stable_local_uuid(display_name: &str) -> [u8; 16] {
    let digest = Sha256::digest(display_name.as_bytes());
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&digest[..16]);
    // A SHA-256 prefix is never all-zero in practice; guarantee the non-zero invariant anyway.
    uuid[0] |= 1;
    uuid
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An HD classic upload keeps every texel without acquiring the GPU array dimensions.
    #[test]
    fn packed_resolution_does_not_change_classic_login_dimensions() {
        let side = protocol::MAX_CLASSIC_SKIN_SIDE;
        let original: Arc<[u8]> = (0..side * side * 4).map(|value| value as u8).collect();
        let packed = render::normalize_actor_skin(&render::ActorSkinPixels {
            width: side as u32,
            height: side as u32,
            rgba8: Arc::clone(&original).into(),
        })
        .unwrap();
        let skin = LocalPlayerSkin::from_rgba8(packed, side, "fixture").to_client_skin();
        assert_eq!((skin.width, skin.height), (side as u32, side as u32));
        assert_eq!(skin.rgba8.as_slice(), original.as_ref());
    }
}
