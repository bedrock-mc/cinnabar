//! The selected classic skin shared by login, menu previews and the local player.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;

use bevy::prelude::Resource;
use sha2::{Digest, Sha256};

#[cfg(not(target_arch = "wasm32"))]
use launcher::install_layout::InstallLayout;

#[cfg(not(target_arch = "wasm32"))]
pub mod catalog;

/// Session-only fallback entries retain the identity supplied by the browser host.
const SESSION_SKIN_ID_PREFIX: &str = "session:";

/// The local player's client-authored skin and stable local uuid. Cheap to clone (Arc-backed):
/// `local_uuid` keys the synthetic self profile the client-world stream inserts when the server
/// never echoes the local player onto the player list.
#[derive(Debug, Clone, Resource)]
pub struct LocalPlayerSkin {
    /// The selected pack and skin name sent at login and on appearance updates.
    pub skin_id: String,
    pub rgba8: render_api::SkinRgba8,
    pub width: u32,
    pub height: u32,
    pub skin_model: launcher::dressing_room::SkinModel,
    pub local_uuid: [u8; 16],
    pub geometry: Option<Arc<render_api::SkinGeometrySource>>,
    pub engine_version: Arc<str>,
    pub cape: Option<render_api::CapeImage>,
    pub cosmetics: Vec<cosmetics::Item>,
}

impl LocalPlayerSkin {
    /// Loads a classic skin within vanilla's upload size limit; a failure uses the default skin.
    #[must_use]
    #[cfg(not(target_arch = "wasm32"))]
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
                    render_api::CLASSIC_SKIN_SIDE,
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
            skin.skin_id = selected_skin_id(selected);
            skin.engine_version = selected.engine_version.clone();
        }
        skin.cosmetics = cosmetics::resolve(&catalog.selected_cosmetics, cosmetics::owns);
        skin
    }

    /// A default-skinned identity, for construction sites without a loaded PNG (e.g. tests).
    #[must_use]
    pub fn generated_default(display_name: &str) -> Self {
        Self::from_rgba8(
            render_model::default_actor_skin_rgba8(),
            render_api::CLASSIC_SKIN_SIDE,
            display_name,
        )
    }

    /// Keeps classic upload dimensions independent of the renderer's larger shared array.
    fn from_rgba8(packed: render_api::SkinRgba8, side: usize, display_name: &str) -> Self {
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
            skin_id: protocol::builtin_skin_id("Custom"),
            rgba8: rgba8.into(),
            width: side as u32,
            height: side as u32,
            skin_model: launcher::dressing_room::SkinModel::Classic,
            local_uuid: stable_local_uuid(display_name),
            geometry: None,
            engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
            cape: None,
            cosmetics: Vec::new(),
        }
    }

    /// The per-frame render skin for the local body and HUD paperdoll.
    #[must_use]
    pub fn player_skin(&self) -> protocol::PlayerSkin {
        protocol::PlayerSkin::Standard(self.standard_skin())
    }

    pub fn standard_skin(&self) -> render_api::StandardSkin {
        render_api::StandardSkin {
            geometry: self.geometry.clone(),
            cape: self.cape.clone(),
            width: self.width,
            height: self.height,
            rgba8: self.rgba8.clone(),
        }
    }

    pub fn model(&self) -> launcher::dressing_room::SkinModel {
        self.skin_model
    }

    pub fn set_selection(
        &mut self,
        skin: &render_api::StandardSkin,
        model: launcher::dressing_room::SkinModel,
    ) {
        self.rgba8 = skin.rgba8.clone();
        self.width = skin.width;
        self.height = skin.height;
        self.geometry = skin.geometry.clone();
        self.cape = skin.cape.clone();
        self.skin_model = model;
    }

    /// Overrides only the rendered cape for developer recordings; login identity stays intact.
    #[cfg(feature = "developer-control")]
    pub fn set_test_cape(&mut self, cape: Option<render_api::CapeImage>) {
        self.cape = cape;
    }

    /// Selects the developer appearance ahead of any echoed server profile while enabled.
    #[cfg(feature = "developer-control")]
    pub fn recording_cape_enabled(&self) -> bool {
        self.cape.is_some()
    }

    /// The login upload payload; allocates the byte copy the JWT encoder needs.
    #[must_use]
    pub fn to_client_skin(&self) -> protocol::ClientSkin {
        protocol::ClientSkin {
            id: self.skin_id.clone(),
            geometry: self
                .geometry
                .as_ref()
                .map(|geometry| protocol::ClientSkinGeometry {
                    resource_patch: geometry.resource_patch.to_string(),
                    geometry_data: geometry.geometry_data.to_string(),
                    engine_version: self.engine_version.to_string(),
                }),
            rgba8: self.rgba8.to_vec(),
            width: self.width,
            height: self.height,
            arm_size: self.skin_model.arm_size().to_string(),
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

/// Maps catalog selection to its pack identity using the persisted image name for imports.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn selected_skin_id(selected: &launcher::dressing_room::DressingRoomSkin) -> String {
    if !selected.imported
        && selected.path.is_empty()
        && let Some(id) = selected.id.strip_prefix(SESSION_SKIN_ID_PREFIX)
    {
        return id.to_owned();
    }
    if let Some(name) = selected.id.strip_prefix("vanilla:") {
        return protocol::builtin_skin_id(name);
    }
    let image_name = Path::new(&selected.path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&selected.id);
    protocol::custom_skin_id(image_name)
}

/// Packs supported source pixels, retaining a legal classic size for the login upload.
#[cfg(not(target_arch = "wasm32"))]
fn load_normalized_skin(path: &Path) -> Result<(render_api::SkinRgba8, usize), String> {
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
                (width as usize).min(render_api::MAX_CLASSIC_SKIN_SIDE),
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

    /// Catalog keys and local paths resolve to one stable upload ID across joins and updates.
    #[test]
    fn selected_skin_identity_uses_the_pack_and_persisted_image_name() {
        let mut local = LocalPlayerSkin::generated_default("fixture");
        for (id, path, expected) in [
            (
                "vanilla:Steve",
                "/fixture/starter.bin",
                protocol::builtin_skin_id("Steve"),
            ),
            (
                "imported:first",
                "/fixture/first.png",
                protocol::custom_skin_id("first"),
            ),
            (
                "imported:second",
                "/fixture/second.png",
                protocol::custom_skin_id("second"),
            ),
        ] {
            let selected = launcher::dressing_room::DressingRoomSkin {
                id: id.into(),
                name: "Fixture".into(),
                path: path.into(),
                imported: !id.starts_with("vanilla:"),
                model: launcher::dressing_room::SkinModel::Classic,
                engine_version: render_api::DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION.into(),
                skin: local.standard_skin(),
            };
            local.skin_id = selected_skin_id(&selected);
            assert_eq!(local.to_client_skin().id, expected);
            assert_eq!(selected_skin_id(&selected), expected);
        }
    }

    /// An HD classic upload keeps every texel without acquiring the GPU array dimensions.
    #[test]
    fn packed_resolution_does_not_change_classic_login_dimensions() {
        let side = render_api::MAX_CLASSIC_SKIN_SIDE;
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
