//! Shipped original OreUI artwork and optional panorama crops from the fetched pack.

pub(crate) mod dimensions;
pub(crate) mod embedded;
mod keys;
mod packing;
mod shipped;

use image::{AnimationDecoder, ImageDecoder, ImageReader, Limits};
use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

pub use keys::INBOX_ICONS;
#[cfg(test)]
use keys::referenced_keys;
pub(crate) use keys::{
    BASE_PACK_IMAGE, CHEVRON_DOWN_IMAGE, CHEVRON_LEFT_IMAGE, CHEVRON_UP_IMAGE, EXTERNAL_LINK_ICON,
    HARDCORE_ICON, INBOX_EMPTY_IMAGES, LOADING_ANIMATION, MISSING_PACK_IMAGE,
    OVERWORLD_BLOCK_IMAGE, PLAY_TAB_ICONS, PROFILE_BANNERS, PROFILE_ERRORS, PROFILE_GAMERSCORE,
    PROFILE_STAT_ICONS, PROFILE_SUMMARY_ICONS, RESET_ICON, SERVER_PING_IMAGES,
    SERVER_PLAYERS_IMAGE, SETTINGS_ICON_HIGHLIGHT_IMAGE, SETTINGS_ICONS, WORLD_CATEGORY_ICONS,
    WORLD_PREVIEW,
};
pub use shipped::{load_oreui_images, shipped_oreui_images};

/// Side of the packed OreUI page.
pub const OREUI_PAGE_SIDE: u32 = 3072;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = render_model::MAX_UI_TEXTURE_SIDE;
const MAX_ANIMATION_FRAMES: usize = 256;
const MAX_ANIMATION_PIXELS_BYTES: usize = 64 * 1024 * 1024;

/// A sprite keeps its original pixel rectangle on one packed page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OreUiSprite {
    pub page: u16,
    pub bounds: [u16; 4],
}

/// Immutable artwork pixels shared by presentation instances.
#[derive(Clone)]
pub struct OreUiImages {
    pub pages: Vec<OreUiPage>,
    pub sprites: Arc<HashMap<String, OreUiSprite>>,
    /// Sprite keys and frame durations in milliseconds, in playback order.
    pub loading_frames: Arc<Vec<(String, u32)>>,
    pub animations: Arc<HashMap<String, Vec<(String, u32)>>>,
}

#[derive(Clone)]
pub struct OreUiPage {
    pub dimensions: [u32; 2],
    pub pixels: Arc<[u8]>,
}

impl OreUiImages {
    /// Whether the named shipped artwork is resident.
    pub fn contains(&self, key: &str) -> bool {
        self.sprites.contains_key(key)
    }
}

fn image_reader(path: &Path) -> Result<ImageReader<Cursor<Vec<u8>>>, String> {
    let bytes = read_bounded(path, MAX_IMAGE_BYTES)?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

fn is_mask(key: &str) -> bool {
    key.contains(".icon-")
}

fn alpha_mask(pixels: &[u8]) -> Vec<u8> {
    pixels
        .chunks_exact(4)
        .flat_map(|pixel| [255, 255, 255, pixel[3]])
        .collect()
}

/// A decoded GIF frame: width, height, straight RGBA8 pixels and duration in milliseconds.
type AnimationFrame = (u32, u32, Vec<u8>, u32);

/// Decodes bounded GIF frames with their own timing and original pixels.
fn decode_animation(bytes: &[u8]) -> Result<Vec<AnimationFrame>, String> {
    let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_ANIMATION_PIXELS_BYTES as u64);
    decoder
        .set_limits(limits)
        .map_err(|error| error.to_string())?;
    let mut frames = Vec::new();
    let mut decoded_bytes = 0;
    for frame in decoder.into_frames() {
        if frames.len() == MAX_ANIMATION_FRAMES {
            return Err("OreUI animation has too many frames".into());
        }
        let frame = frame.map_err(|error| error.to_string())?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let millis = numerator.div_ceil(denominator).max(1);
        let buffer = frame.into_buffer();
        let (width, height) = buffer.dimensions();
        let pixels = buffer.into_raw();
        decoded_bytes += pixels.len();
        if decoded_bytes > MAX_ANIMATION_PIXELS_BYTES {
            return Err("OreUI animation pixels are too large".into());
        }
        frames.push((width, height, pixels, millis));
    }
    Ok(frames)
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!("{} is too large", path.display()));
    }
    Ok(bytes)
}

/// The shader applies alpha, so uploaded pixels retain straight source RGB.
fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let reader = image_reader(path)?;
    let image = reader
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

#[cfg(test)]
mod tests;
