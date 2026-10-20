//! Per-layer source ownership for original images split across texture pages.

use crate::{MAX_UI_TEXTURE_LAYERS, UiTexturePage};

const HEADER_WORDS: usize = 4;
const REGION_WORDS: usize = 8;
/// Byte size of the complete format header and per-layer ownership table.
pub const BYTE_SIZE: u64 =
    ((HEADER_WORDS + REGION_WORDS * MAX_UI_TEXTURE_LAYERS as usize) * size_of::<u32>()) as u64;

/// Initializes the shared format header and bounded per-layer region table.
pub fn words(coverage: bool) -> Vec<u32> {
    let mut words = vec![0; BYTE_SIZE as usize / size_of::<u32>()];
    words[0] = u32::from(coverage);
    words
}

/// Byte offset of a resident layer's source-ownership entry.
pub fn offset(layer: u32) -> u64 {
    ((HEADER_WORDS + REGION_WORDS * layer as usize) * size_of::<u32>()) as u64
}

/// The source texel bounds and actual page dimensions, excluding any bucket padding.
pub fn region(page: &UiTexturePage) -> [f32; REGION_WORDS] {
    let [w, h] = page.dimensions();
    let bounds = page.repeat_content().unwrap_or([0, 0, w, h]);
    [
        bounds[0] as f32,
        bounds[1] as f32,
        bounds[2] as f32,
        bounds[3] as f32,
        w as f32,
        h as f32,
        0.0,
        0.0,
    ]
}
