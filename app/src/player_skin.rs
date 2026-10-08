//! The selected classic skin shared by login, menu previews and the local player.

use std::path::Path;
use std::sync::Arc;

use bevy::prelude::Resource;
use sha2::{Digest, Sha256};

use crate::install_layout::InstallLayout;

pub(crate) mod catalog;

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
    pub geometry: Option<Arc<protocol::SkinGeometrySource>>,
    pub cape: Option<protocol::CapeImage>,
}

impl LocalPlayerSkin {
    /// Loads a classic skin within vanilla's upload size limit; a failure uses the default skin.
    #[must_use]
    pub fn load(layout: &InstallLayout, display_name: &str) -> Self {
        let path = layout.player_skin_asset();
        let (rgba8, side, has_asset) = match load_normalized_skin(&path) {
            Ok((rgba8, side)) => (rgba8, side, true),
            Err(reason) => {
                bevy::log::warn!(
                    path = %path.display(),
                    reason = %reason,
                    "local player skin unavailable; using the default skin"
                );
                (
                    render_model::default_actor_skin_rgba8(),
                    protocol::CLASSIC_SKIN_SIDE,
                    false,
                )
            }
        };
        let mut skin = Self::from_rgba8(rgba8, side, display_name);
        let catalog = catalog::load(layout, &skin);
        let selected = catalog.selected_skin().and_then(|selected| {
            if !has_asset && selected.id == "current" {
                catalog
                    .skins
                    .iter()
                    .find(|entry| entry.id.starts_with("vanilla:"))
                    .or(Some(selected))
            } else {
                Some(selected)
            }
        });
        if let Some(selected) = selected {
            let mut active = selected.skin.clone();
            active.cape = catalog.selected_cape().map(|entry| entry.cape.clone());
            skin.set_selection(&active, selected.model);
        }
        skin
    }

    /// A default-skinned identity, for construction sites without a loaded PNG (e.g. tests).
    #[cfg(test)]
    #[must_use]
    pub fn generated_default(display_name: &str) -> Self {
        Self::from_rgba8(
            render_model::default_actor_skin_rgba8(),
            protocol::CLASSIC_SKIN_SIDE,
            display_name,
        )
    }

    /// Keeps classic upload dimensions independent of the renderer's larger shared array.
    fn from_rgba8(packed: protocol::SkinRgba8, side: usize, display_name: &str) -> Self {
        let mut rgba8 = Vec::with_capacity(side * side * 4);
        for y in 0..side {
            for x in 0..side {
                let source = (y * render_model::STANDARD_SKIN_SIDE / side
                    * render_model::STANDARD_SKIN_SIDE
                    + x * render_model::STANDARD_SKIN_SIDE / side)
                    * 4;
                rgba8.extend_from_slice(&packed[source..source + 4]);
            }
        }
        Self {
            rgba8: rgba8.into(),
            width: side as u32,
            height: side as u32,
            arm_size: Arc::from(launcher::dressing_room::SkinModel::Classic.arm_size()),
            local_uuid: stable_local_uuid(display_name),
            geometry: None,
            cape: None,
        }
    }

    /// The per-frame render skin for the local body and HUD paperdoll.
    #[must_use]
    pub fn player_skin(&self) -> protocol::PlayerSkin {
        protocol::PlayerSkin::Standard(self.standard_skin())
    }

    pub(crate) fn standard_skin(&self) -> protocol::StandardSkin {
        protocol::StandardSkin {
            geometry: self.geometry.clone(),
            cape: self.cape.clone(),
            width: self.width,
            height: self.height,
            rgba8: self.rgba8.clone(),
        }
    }

    pub(crate) fn model(&self) -> launcher::dressing_room::SkinModel {
        if self.arm_size.as_ref() == launcher::dressing_room::SkinModel::Slim.arm_size() {
            launcher::dressing_room::SkinModel::Slim
        } else {
            launcher::dressing_room::SkinModel::Classic
        }
    }

    pub(crate) fn set_selection(
        &mut self,
        skin: &protocol::StandardSkin,
        model: launcher::dressing_room::SkinModel,
    ) {
        self.rgba8 = skin.rgba8.clone();
        self.width = skin.width;
        self.height = skin.height;
        self.geometry = skin.geometry.clone();
        self.cape = skin.cape.clone();
        self.arm_size = Arc::from(model.arm_size());
    }

    /// Overrides only the rendered cape for developer recordings; login identity stays intact.
    #[cfg(feature = "developer-control")]
    pub(crate) fn set_test_cape(&mut self, cape: Option<protocol::CapeImage>) {
        self.cape = cape;
    }

    /// Selects the developer appearance ahead of any echoed server profile while enabled.
    #[cfg(feature = "developer-control")]
    pub(crate) fn recording_cape_enabled(&self) -> bool {
        self.cape.is_some()
    }

    /// The login upload payload; allocates the byte copy the JWT encoder needs.
    #[must_use]
    pub fn to_client_skin(&self) -> protocol::ClientSkin {
        protocol::ClientSkin {
            rgba8: self.rgba8.to_vec(),
            width: self.width,
            height: self.height,
            arm_size: self.arm_size.to_string(),
            cape: self
                .cape
                .as_ref()
                .filter(|cape| cape.is_valid())
                .map(|cape| protocol::ClientCape {
                    rgba8: cape.rgba8.to_vec(),
                    width: cape.width,
                    height: cape.height,
                    id: protocol::cape_content_id(cape),
                }),
        }
    }
}

/// Packs supported source pixels, retaining a legal classic size for the login upload.
fn load_normalized_skin(path: &Path) -> Result<(protocol::SkinRgba8, usize), String> {
    let image = image::open(path).map_err(|error| error.to_string())?;
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let pixels = render_model::ActorSkinPixels {
        width,
        height,
        rgba8: rgba.into_raw().into(),
    };
    render_model::normalize_actor_skin(&pixels)
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
        let packed = render_model::normalize_actor_skin(&render_model::ActorSkinPixels {
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
