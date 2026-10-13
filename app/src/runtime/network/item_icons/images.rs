//! Bounded full-resolution item frame decoding and icon construction.

use assets::texture::policy::device_policy;
use client_ui::ui_runtime::presentation::SessionIcon;
use pack_runtime::pack_textures::{DecodedTexture, decode_pack_texture_with_policy};
use resource_pack::LayeredPackView;
use std::sync::Arc;

/// The image for icon `key`: the catalog's path, else `textures/items/<key>`.
pub(super) fn resolve_key(
    view: &LayeredPackView,
    paths: &std::collections::HashMap<String, Vec<String>>,
    key: &str,
    remaining: usize,
    mut remaining_bytes: usize,
) -> Result<Vec<(u32, DecodedTexture)>, String> {
    if remaining == 0 {
        return Ok(Vec::new());
    }
    let mut tried = Vec::new();
    if let Some(variants) = paths.get(key) {
        let textures: Vec<_> = variants
            .iter()
            .enumerate()
            .filter_map(|(metadata, path)| {
                let texture = first_frame(decode_pack_texture_with_policy(
                    view,
                    path,
                    device_policy(),
                    true,
                )?);
                if texture.rgba8.len() > remaining_bytes {
                    return None;
                }
                remaining_bytes -= texture.rgba8.len();
                Some((metadata as u32, texture))
            })
            .take(remaining)
            .collect();
        if !textures.is_empty() {
            return Ok(textures);
        }
        tried.push(format!("catalog key {key} has no readable images"));
    } else {
        tried.push(format!("key '{key}' not in the merged item_texture.json"));
    }
    let bare = key.rsplit_once(':').map_or(key, |(_, name)| name);
    for path in [
        format!("textures/items/{key}"),
        format!("textures/items/{bare}"),
    ] {
        if let Some(texture) = decode_pack_texture_with_policy(view, &path, device_policy(), true) {
            let texture = first_frame(texture);
            if texture.rgba8.len() <= remaining_bytes {
                return Ok(vec![(0, texture)]);
            }
        }
    }
    tried.push(format!("no textures/items/{bare}"));
    Err(tried.join("; "))
}

/// Vertical strips animate in vanilla; the static icon is their first frame.
pub(super) fn first_frame(texture: DecodedTexture) -> DecodedTexture {
    let side = texture.width;
    if texture.height <= side || !texture.height.is_multiple_of(side) {
        return texture;
    }
    let bytes = (side * side * 4) as usize;
    DecodedTexture {
        width: side,
        height: side,
        rgba8: texture.rgba8[..bytes].into(),
    }
}

/// Retains the source pixels and dimensions of an admitted item frame.
pub(super) fn icon(identifier: Arc<str>, texture: DecodedTexture) -> SessionIcon {
    SessionIcon {
        identifier,
        metadata: 0,
        width: texture.width,
        height: texture.height,
        rgba8: texture.rgba8,
    }
}
