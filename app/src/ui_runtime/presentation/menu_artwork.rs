//! Bounded decoding and atlas packing for service-provided launcher artwork.
//!
//! The authenticated Go catalog downloads remote images into the local cache.
//! This module treats those files as untrusted input: reads, decoded dimensions,
//! allocation, output layers, and total GPU bytes are all capped before a
//! thumbnail can enter the retained UI texture array.

use std::{
    collections::{BTreeSet, HashMap},
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use image::{ImageReader, Limits, imageops::FilterType};
use sha2::{Digest, Sha256};

use super::IconRef;

const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_SOURCE_SIDE: u32 = 4_096;
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;
const ARTWORK_WIDTH: u32 = 96;
const ARTWORK_HEIGHT: u32 = 96;
const GUTTER: u32 = 1;
const MAX_ARTWORKS: usize = 32;

#[derive(Default)]
pub(super) struct MenuArtworkAtlas {
    pub(super) signature: [u8; 32],
    pub(super) layers: u32,
    pub(super) rgba8: Vec<u8>,
    pub(super) refs: HashMap<String, IconRef>,
}

pub(super) fn load(
    paths: &[String],
    page_width: u32,
    page_height: u32,
    first_page: u16,
    max_layers: u32,
    max_bytes: usize,
) -> MenuArtworkAtlas {
    let cell_width = ARTWORK_WIDTH + GUTTER * 2;
    let cell_height = ARTWORK_HEIGHT + GUTTER * 2;
    let columns = page_width / cell_width;
    let rows = page_height / cell_height;
    let per_layer = columns.saturating_mul(rows);
    let layer_bytes = page_width as usize * page_height as usize * 4;
    if per_layer == 0 || max_layers == 0 || layer_bytes == 0 {
        return MenuArtworkAtlas::default();
    }
    let byte_layers = max_bytes / layer_bytes;
    let usable_layers = max_layers.min(u32::try_from(byte_layers).unwrap_or(u32::MAX));
    if usable_layers == 0 {
        return MenuArtworkAtlas::default();
    }

    let mut unique = BTreeSet::new();
    let mut decoded = Vec::new();
    let mut signature = Sha256::new();
    signature.update(b"cinnabar-menu-artwork-v1");
    for path in paths.iter().take(MAX_ARTWORKS) {
        if path.is_empty() || !unique.insert(path.clone()) {
            continue;
        }
        let Some((pixels, source_hash)) = decode(Path::new(path)) else {
            continue;
        };
        signature.update(path.as_bytes());
        signature.update(source_hash);
        decoded.push((path.clone(), pixels));
    }
    let capacity = usize::try_from(per_layer.saturating_mul(usable_layers)).unwrap_or(usize::MAX);
    decoded.truncate(capacity);
    if decoded.is_empty() {
        return MenuArtworkAtlas::default();
    }

    let layers = u32::try_from(decoded.len())
        .unwrap_or(u32::MAX)
        .div_ceil(per_layer);
    let mut rgba8 = vec![0; layer_bytes.saturating_mul(layers as usize)];
    let mut refs = HashMap::with_capacity(decoded.len());
    for (index, (path, pixels)) in decoded.into_iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let layer = index / per_layer;
        let slot = index % per_layer;
        let column = slot % columns;
        let row = slot / columns;
        let left = column * cell_width + GUTTER;
        let top = row * cell_height + GUTTER;
        let layer_start = layer as usize * layer_bytes;
        for source_y in 0..ARTWORK_HEIGHT as usize {
            let source_start = source_y * ARTWORK_WIDTH as usize * 4;
            let target_start =
                layer_start + ((top as usize + source_y) * page_width as usize + left as usize) * 4;
            rgba8[target_start..target_start + ARTWORK_WIDTH as usize * 4]
                .copy_from_slice(&pixels[source_start..source_start + ARTWORK_WIDTH as usize * 4]);
        }
        let Ok(page) = u16::try_from(u32::from(first_page) + layer) else {
            continue;
        };
        let Ok(left) = u16::try_from(left) else {
            continue;
        };
        let Ok(top) = u16::try_from(top) else {
            continue;
        };
        refs.insert(
            path,
            IconRef {
                page,
                uv: [
                    left,
                    top,
                    left.saturating_add(ARTWORK_WIDTH as u16),
                    top.saturating_add(ARTWORK_HEIGHT as u16),
                ],
            },
        );
    }
    MenuArtworkAtlas {
        signature: signature.finalize().into(),
        layers,
        rgba8,
        refs,
    }
}

fn decode(path: &Path) -> Option<(Vec<u8>, [u8; 32])> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let format = image::guess_format(&bytes).ok()?;
    let dimensions = ImageReader::with_format(Cursor::new(&bytes), format)
        .into_dimensions()
        .ok()?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > MAX_SOURCE_SIDE
        || dimensions.1 > MAX_SOURCE_SIDE
    {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let resized = reader
        .decode()
        .ok()?
        .resize(ARTWORK_WIDTH, ARTWORK_HEIGHT, FilterType::Lanczos3)
        .into_rgba8();
    let mut pixels = vec![0; (ARTWORK_WIDTH * ARTWORK_HEIGHT * 4) as usize];
    let left = (ARTWORK_WIDTH - resized.width()) / 2;
    let top = (ARTWORK_HEIGHT - resized.height()) / 2;
    for row in 0..resized.height() as usize {
        let source_start = row * resized.width() as usize * 4;
        let target_start = ((top as usize + row) * ARTWORK_WIDTH as usize + left as usize) * 4;
        pixels[target_start..target_start + resized.width() as usize * 4].copy_from_slice(
            &resized.as_raw()[source_start..source_start + resized.width() as usize * 4],
        );
    }
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        pixel[0] = ((u16::from(pixel[0]) * alpha + 127) / 255) as u8;
        pixel[1] = ((u16::from(pixel[1]) * alpha + 127) / 255) as u8;
        pixel[2] = ((u16::from(pixel[2]) * alpha + 127) / 255) as u8;
    }
    Some((pixels, Sha256::digest(&bytes).into()))
}
