//! Bounded source image and texture catalog preparation for pack subscribers.

use assets::texture::policy::{MAX_DECODE_BYTES, TexturePolicy};
use image::{ImageFormat, ImageReader, Limits};
use pack_json::normalize_jsonc;
use resource_pack::LayeredPackView;
use serde_json::Value;
use std::{collections::HashMap, io::Cursor};

const MAX_TEXTURE_SOURCE_BYTES: usize = resource_pack::MAX_PACK_TEXTURE_BYTES as usize;
const LEGACY_TEXTURE_SIDE: u32 = 1024;
pub const MAX_CATALOG_ENTRIES: usize = 16_384;

/// Straight-alpha RGBA8 pixels decoded from a pack image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedTexture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// Maps each `texture_data` key of a texture catalog (terrain or item) to its
/// image path; a higher pack replaces a key.
pub fn texture_key_paths(view: &LayeredPackView, catalog: &str) -> HashMap<String, String> {
    let mut paths = HashMap::new();
    for layer in view.read_layers(catalog) {
        merge_texture_catalog(&mut paths, &layer);
    }
    paths
}

/// Reads terrain UV grids from valid winning entries, resetting omitted grids to zero.
pub fn terrain_texture_grids(view: &LayeredPackView) -> HashMap<String, u8> {
    let mut grids = HashMap::new();
    for layer in view.read_layers("textures/terrain_texture.json") {
        let Some(Value::Object(data)) =
            parse_pack_json(&layer).map(|mut root| root["texture_data"].take())
        else {
            continue;
        };
        for (key, entry) in data {
            if grids.len() >= MAX_CATALOG_ENTRIES && !grids.contains_key(&key) {
                break;
            }
            if first_texture_path(&entry["textures"]).is_some() {
                grids.insert(
                    key,
                    (entry["quad"].as_u64().unwrap_or(0) as u32 & assets::TERRAIN_QUAD_SHIFT_MASK)
                        as u8,
                );
            }
        }
    }
    grids
}

static BASE_TERRAIN_CATALOG: std::sync::OnceLock<HashMap<String, String>> =
    std::sync::OnceLock::new();

/// Supplies the base texture aliases so a pack can replace rasters without repeating the catalog.
pub fn set_base_terrain_catalog<'a>(aliases: impl IntoIterator<Item = (&'a str, &'a str)>) {
    let paths = aliases
        .into_iter()
        .map(|(key, path)| (key.to_owned(), path.to_owned()))
        .collect();
    let _ = BASE_TERRAIN_CATALOG.set(paths);
}

/// Immutable aliases from the world carrier's sidecar, below all optional catalog layers.
pub fn base_terrain_catalog() -> HashMap<String, String> {
    BASE_TERRAIN_CATALOG.get().cloned().unwrap_or_default()
}

/// Whether the carrier's aliases are installed; once installed they never change.
pub fn base_terrain_catalog_installed() -> bool {
    BASE_TERRAIN_CATALOG.get().is_some()
}

/// Reads valid entries independently, preserving lower aliases for malformed entries.
fn merge_texture_catalog(paths: &mut HashMap<String, String>, bytes: &[u8]) {
    let Some(Value::Object(data)) =
        parse_pack_json(bytes).map(|mut root| root["texture_data"].take())
    else {
        return;
    };
    for (key, entry) in data {
        if paths.len() >= MAX_CATALOG_ENTRIES && !paths.contains_key(&key) {
            break;
        }
        if let Some(path) = first_texture_path(&entry["textures"]) {
            paths.insert(key, path);
        }
    }
}

const IMAGE_EXTENSIONS: [(&str, ImageFormat); 4] = [
    ("png", ImageFormat::Png),
    ("tga", ImageFormat::Tga),
    ("jpg", ImageFormat::Jpeg),
    ("jpeg", ImageFormat::Jpeg),
];
const MAX_TEXTURE_SET_BYTES: u64 = 64 * 1024;

/// Decodes the winning image at `path`, trying `.png`, `.tga`, and `.jpg` as
/// vanilla does, then the color layer of a `.texture_set.json`.
pub fn decode_pack_texture(view: &LayeredPackView, path: &str) -> Option<DecodedTexture> {
    decode_pack_texture_with_policy(
        view,
        path,
        TexturePolicy::for_device(LEGACY_TEXTURE_SIDE),
        false,
    )
}

/// Resolves an image with caller-supplied device bounds; strips are cut before GPU upload.
pub fn decode_pack_texture_with_policy(
    view: &LayeredPackView,
    path: &str,
    policy: TexturePolicy,
    vertical_strip: bool,
) -> Option<DecodedTexture> {
    decode_image_file(view, path, policy, vertical_strip)
        .or_else(|| decode_texture_set(view, path, policy, vertical_strip))
}

/// Tries the literal image path, then the supported file extensions in precedence order.
fn decode_image_file(
    view: &LayeredPackView,
    path: &str,
    policy: TexturePolicy,
    vertical_strip: bool,
) -> Option<DecodedTexture> {
    // Vanilla's loader also tries the literal path, so a pack path that already
    // names its image (`textures/items/gem.png`) resolves.
    let named = path.rsplit_once('.').and_then(|(_, extension)| {
        IMAGE_EXTENSIONS
            .into_iter()
            .find(|(known, _)| extension.eq_ignore_ascii_case(known))
    });
    if let Some((_, format)) = named
        && let Some(texture) = view
            .read_capped(path, MAX_TEXTURE_SOURCE_BYTES as u64)
            .and_then(|bytes| decode_image_with_policy(&bytes, format, policy, vertical_strip))
    {
        return Some(texture);
    }
    IMAGE_EXTENSIONS
        .into_iter()
        .find_map(|(extension, format)| {
            let bytes = view.read_capped(
                &format!("{path}.{extension}"),
                MAX_TEXTURE_SOURCE_BYTES as u64,
            )?;
            decode_image_with_policy(&bytes, format, policy, vertical_strip)
        })
}

/// A texture set's `color` is a sibling image name or a solid `[r, g, b(, a)]`.
fn decode_texture_set(
    view: &LayeredPackView,
    path: &str,
    policy: TexturePolicy,
    vertical_strip: bool,
) -> Option<DecodedTexture> {
    let bytes = view.read_capped(&format!("{path}.texture_set.json"), MAX_TEXTURE_SET_BYTES)?;
    let root = parse_pack_json(&bytes)?;
    match root.get("minecraft:texture_set")?.get("color")? {
        Value::String(name) => {
            let name = name.trim().trim_start_matches("./");
            let sibling = path
                .rsplit_once('/')
                .map_or_else(|| name.to_owned(), |(dir, _)| format!("{dir}/{name}"));
            [sibling, name.to_owned()]
                .into_iter()
                .filter(|target| target != path)
                .find_map(|target| decode_image_file(view, &target, policy, vertical_strip))
        }
        Value::Array(channels) if matches!(channels.len(), 3 | 4) => {
            let mut pixel = [0, 0, 0, 255];
            for (slot, channel) in pixel.iter_mut().zip(channels) {
                *slot = channel.as_f64()?.round().clamp(0.0, 255.0) as u8;
            }
            Some(DecodedTexture {
                width: 1,
                height: 1,
                rgba8: pixel.into(),
            })
        }
        _ => None,
    }
}

/// Parses the normalized JSON-with-comments representation used by pack files.
pub fn parse_pack_json(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(&normalize_jsonc(bytes)?).ok()
}

/// A texture entry is a path, an object with `path`, or a variation list whose
/// first element is used.
fn first_texture_path(value: &Value) -> Option<String> {
    let path = match value {
        Value::String(path) => path.as_str(),
        Value::Object(entry) => entry.get("path")?.as_str()?,
        Value::Array(entries) => return first_texture_path(entries.first()?),
        _ => return None,
    };
    let path = path.trim().trim_start_matches("./");
    (!path.is_empty()).then(|| path.to_owned())
}

/// Decodes only images whose source, dimensions, and allocation fit the established limits.
pub(crate) fn decode_image(bytes: &[u8], format: ImageFormat) -> Option<DecodedTexture> {
    decode_image_with_policy(
        bytes,
        format,
        TexturePolicy::for_device(LEGACY_TEXTURE_SIDE),
        false,
    )
}

/// Rejects source area and decoder allocations before retaining the decoded RGBA8 pixels.
fn decode_image_with_policy(
    bytes: &[u8],
    format: ImageFormat,
    policy: TexturePolicy,
    vertical_strip: bool,
) -> Option<DecodedTexture> {
    if bytes.is_empty() || bytes.len() > MAX_TEXTURE_SOURCE_BYTES {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(policy.max_side());
    limits.max_image_height = Some(if vertical_strip {
        u32::from(u16::MAX)
    } else {
        policy.max_side()
    });
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut header = ImageReader::with_format(Cursor::new(bytes), format);
    header.limits(limits.clone());
    let (width, height) = header.into_dimensions().ok()?;
    let expected = policy.source_bytes(width, height, vertical_strip)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .ok()?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    if rgba8.len() != expected {
        return None;
    }
    Some(DecodedTexture {
        width,
        height,
        rgba8,
    })
}

#[cfg(test)]
mod tests;
