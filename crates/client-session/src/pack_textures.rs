//! Bounded source image and texture catalog preparation for pack subscribers.

use image::{ImageFormat, ImageReader, Limits};
use resource_pack::{LayeredPackView, normalize_jsonc};
use serde_json::Value;
use std::{collections::HashMap, io::Cursor};

const MAX_TEXTURE_SOURCE_BYTES: usize = resource_pack::MAX_PACK_TEXTURE_BYTES as usize;
const MAX_TEXTURE_SIDE: u32 = 1024;
const MAX_DECODE_ALLOC: u64 = 16 * 1024 * 1024;
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
    decode_image_file(view, path).or_else(|| decode_texture_set(view, path))
}

/// Tries the literal image path, then the supported file extensions in precedence order.
fn decode_image_file(view: &LayeredPackView, path: &str) -> Option<DecodedTexture> {
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
            .and_then(|bytes| decode_image(&bytes, format))
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
            decode_image(&bytes, format)
        })
}

/// A texture set's `color` is a sibling image name or a solid `[r, g, b(, a)]`.
fn decode_texture_set(view: &LayeredPackView, path: &str) -> Option<DecodedTexture> {
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
                .find_map(|target| decode_image_file(view, &target))
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
fn decode_image(bytes: &[u8], format: ImageFormat) -> Option<DecodedTexture> {
    if bytes.is_empty() || bytes.len() > MAX_TEXTURE_SOURCE_BYTES {
        return None;
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;
    if width == 0 || height == 0 || width > MAX_TEXTURE_SIDE || height > MAX_TEXTURE_SIDE {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_SIDE);
    limits.max_image_height = Some(MAX_TEXTURE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .ok()?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    Some(DecodedTexture {
        width,
        height,
        rgba8,
    })
}
