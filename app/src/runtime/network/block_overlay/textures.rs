//! Terrain texture keys and flipbooks resolved across the stack.

use std::collections::{HashMap, HashSet};

use resource_pack::LayeredPackView;
use serde_json::Value;

pub(super) use super::super::resource_packs::DecodedTexture;
mod diagnostics;
mod terrain;
mod tint;
use super::super::resource_packs::{
    MAX_CATALOG_ENTRIES, decode_pack_texture, parse_pack_json, texture_key_paths,
};
use diagnostics::TextureDiagnostics;
pub(super) use terrain::{admit_static_rectangle, source_mip_chain};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Flipbook {
    pub(super) frames: Option<Vec<u32>>,
    pub(super) ticks_per_frame: u32,
    pub(super) blend: bool,
}

/// Texture keys resolved across the stack; a higher pack replaces a key.
pub(super) struct TextureCatalog<'a> {
    view: &'a LayeredPackView,
    terrain: HashMap<String, String>,
    tints: HashMap<String, [u8; 3]>,
    server_keys: HashSet<String>,
    flipbooks: HashMap<String, Flipbook>,
    diagnostics: TextureDiagnostics,
}

impl<'a> TextureCatalog<'a> {
    pub(super) fn new(view: &'a LayeredPackView, base: Option<&assets::MaterialKeys>) -> Self {
        let mut terrain = super::super::resource_packs::base_terrain_catalog();
        if let Some(base) = base {
            terrain.extend(
                base.aliases()
                    .map(|(key, path)| (key.to_owned(), path.to_owned())),
            );
        }
        let server_terrain = texture_key_paths(view, "textures/terrain_texture.json");
        let server_keys = server_terrain.keys().cloned().collect();
        terrain.extend(server_terrain);
        let diagnostics = TextureDiagnostics::default();
        diagnostics.catalogs(view);
        let mut flipbooks = HashMap::new();
        for layer in view.read_layers("textures/flipbook_textures.json") {
            let Some(Value::Array(entries)) = parse_pack_json(&layer) else {
                continue;
            };
            for entry in entries.iter().take(MAX_CATALOG_ENTRIES) {
                let Some(tile) = entry["atlas_tile"].as_str() else {
                    continue;
                };
                let frames = entry["frames"].as_array().map(|frames| {
                    frames
                        .iter()
                        .filter_map(|frame| {
                            frame.as_u64().and_then(|frame| u32::try_from(frame).ok())
                        })
                        .collect()
                });
                let ticks = entry["ticks_per_frame"]
                    .as_u64()
                    .unwrap_or(1)
                    .clamp(1, 1 << 16);
                flipbooks.insert(
                    tile.to_owned(),
                    Flipbook {
                        frames,
                        ticks_per_frame: ticks as u32,
                        blend: entry["blend_frames"]
                            .as_bool()
                            .unwrap_or(pack_compiler::DEFAULT_BLEND_FRAMES),
                    },
                );
            }
        }
        Self {
            view,
            terrain,
            tints: tint::catalog_tints(view, base),
            server_keys,
            flipbooks,
            diagnostics,
        }
    }

    pub(super) fn terrain_keys(&self) -> impl Iterator<Item = &str> {
        self.terrain.keys().map(String::as_str)
    }

    pub(super) fn flipbook(&self, key: &str) -> Option<&Flipbook> {
        self.flipbooks.get(key)
    }

    /// Decodes the image a terrain key names.
    pub(super) fn decode(&self, key: &str) -> Option<DecodedTexture> {
        let Some(path) = self.terrain.get(key) else {
            self.diagnostics.failure(key, None, "terrain_key_missing");
            return None;
        };
        let mut texture = decode_pack_texture(self.view, path);
        if let Some(tint) = self.tints.get(key)
            && let Some(texture) = texture.as_mut()
        {
            pack_compiler::apply_atlas_tint(&mut texture.rgba8, *tint);
        }
        if texture.is_none() {
            let reason = diagnostics::failure_reason(self.view, path);
            // Base aliases have no server raster until a pack overrides them.
            if self.server_keys.contains(key) || reason != "texture_file_missing" {
                self.diagnostics.failure(key, Some(path), reason);
            }
        }
        texture
    }
}

/// Splits a vertical strip into up to `max_frames` square frames, each shrunk to
/// `max_side`; a non-strip image is its single shrunk frame. Frames are shrunk
/// as they are cut so the caller never holds full-size copies.
pub(super) fn flipbook_frames(
    texture: &DecodedTexture,
    flipbook: &Flipbook,
    max_frames: usize,
    max_side: u32,
) -> Vec<DecodedTexture> {
    let side = texture.width;
    if max_frames == 0 {
        return Vec::new();
    }
    if texture.height <= side || !texture.height.is_multiple_of(side) {
        return vec![shrink_to_max(texture, max_side)];
    }
    let count = texture.height / side;
    let order = flipbook
        .frames
        .clone()
        .filter(|frames| !frames.is_empty())
        .unwrap_or_else(|| (0..count).collect());
    let frame_bytes = (side * side * 4) as usize;
    order
        .into_iter()
        .take(max_frames)
        .map(|frame| {
            let start = (frame.min(count - 1) as usize) * frame_bytes;
            let frame = DecodedTexture {
                width: side,
                height: side,
                rgba8: texture.rgba8[start..start + frame_bytes].into(),
            };
            shrink_to_max(&frame, max_side)
        })
        .collect()
}

/// Downscales aspect-preserving so the longest side is at most `max_side`;
/// smaller images are returned unchanged. Nearest-neighbour keeps pixel art crisp.
pub(super) fn shrink_to_max(texture: &DecodedTexture, max_side: u32) -> DecodedTexture {
    let longest = texture.width.max(texture.height);
    if longest <= max_side {
        return texture.clone();
    }
    let scale = |side: u32| (side * max_side / longest).max(1);
    let (width, height) = (scale(texture.width), scale(texture.height));
    let mut rgba8 = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let source_y = (u64::from(y) * u64::from(texture.height) / u64::from(height)) as usize;
        for x in 0..width {
            let source_x = (u64::from(x) * u64::from(texture.width) / u64::from(width)) as usize;
            let offset = (source_y * texture.width as usize + source_x) * 4;
            rgba8.extend_from_slice(&texture.rgba8[offset..offset + 4]);
        }
    }
    DecodedTexture {
        width,
        height,
        rgba8: rgba8.into_boxed_slice(),
    }
}

/// Resamples to a square power-of-two tile: exact halvings in linear light when
/// possible, nearest-neighbour otherwise so pixel art stays crisp.
pub(super) fn resample_square(texture: &DecodedTexture, tile: u32) -> Box<[u8]> {
    if texture.width == texture.height && texture.width.is_power_of_two() && texture.width >= tile {
        let mut pixels = texture.rgba8.clone();
        let mut size = texture.width;
        while size > tile {
            pixels = assets::downsample_linear_premultiplied(&pixels, size);
            size /= 2;
        }
        return pixels;
    }
    let mut pixels = Vec::with_capacity((tile * tile * 4) as usize);
    for y in 0..tile {
        let source_y = (u64::from(y) * u64::from(texture.height) / u64::from(tile)) as usize;
        for x in 0..tile {
            let source_x = (u64::from(x) * u64::from(texture.width) / u64::from(tile)) as usize;
            let offset = (source_y * texture.width as usize + source_x) * 4;
            pixels.extend_from_slice(&texture.rgba8[offset..offset + 4]);
        }
    }
    pixels.into_boxed_slice()
}
