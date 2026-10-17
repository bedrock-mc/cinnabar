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
    remaining_bytes: usize,
    inherit_base: bool,
) -> Result<Vec<(u32, DecodedTexture)>, String> {
    resolve_key_with_carrier(view, paths, key, remaining, remaining_bytes, |path| {
        inherit_base
            .then(|| super::vanilla::texture(path))
            .flatten()
    })
}

/// Uses the same priority and budgets for pack rasters and optional base sprite donors.
fn resolve_key_with_carrier(
    view: &LayeredPackView,
    paths: &std::collections::HashMap<String, Vec<String>>,
    key: &str,
    remaining: usize,
    mut remaining_bytes: usize,
    carrier: impl Fn(&str) -> Option<DecodedTexture>,
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
                let mut texture = decode(view, path, &carrier)?;
                texture.retain_first_strip_frame();
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
        if let Some(mut texture) = decode(view, &path, &carrier) {
            texture.retain_first_strip_frame();
            if texture.rgba8.len() <= remaining_bytes {
                return Ok(vec![(0, texture)]);
            }
        }
    }
    tried.push(format!("no textures/items/{bare}"));
    Err(tried.join("; "))
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

/// Pack pixels have priority; only an absent file may inherit a base sprite.
fn decode(
    view: &LayeredPackView,
    path: &str,
    carrier: impl Fn(&str) -> Option<DecodedTexture>,
) -> Option<DecodedTexture> {
    decode_pack_texture_with_policy(view, path, device_policy(), true).or_else(|| {
        if pack_runtime::pack_textures::indexed_image_candidate(view, path).is_some() {
            return None;
        }
        carrier(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_item_catalog_aliases_use_base_pixels_and_keep_pack_priority() {
        let view = super::super::tests::view();
        let paths = std::collections::HashMap::from([(
            "fixture:alias".into(),
            vec![
                "textures/items/base_tool".into(),
                "textures/items/gem".into(),
            ],
        )]);
        let carrier = |_: &str| {
            Some(DecodedTexture {
                width: 16,
                height: 16,
                rgba8: [12, 34, 56, 255].repeat(16 * 16).into(),
            })
        };
        let icons =
            resolve_key_with_carrier(&view, &paths, "fixture:alias", 2, 16 * 16 * 4 * 2, carrier)
                .unwrap();
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].0, 0);
        assert_eq!(&icons[0].1.rgba8[..4], &[12, 34, 56, 255]);
        assert_eq!(icons[1].0, 1);
        assert_eq!(&icons[1].1.rgba8[..4], &[0, 0, 0, 255]);
        let bounded =
            resolve_key_with_carrier(&view, &paths, "fixture:alias", 2, 16 * 16 * 4, carrier)
                .unwrap();
        assert_eq!(bounded.len(), 1);
    }
}
