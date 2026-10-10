//! Manifest-generated descriptors for Cinnabar's original artwork.

/// An embedded raster, including the storage cells and playback steps of a sprite sheet.
#[derive(Clone, Copy)]
pub(crate) struct EmbeddedImage {
    pub key: &'static str,
    pub bytes: &'static [u8],
    pub size: [u32; 2],
    pub frames: u16,
    pub steps: u16,
    pub duration_ms: u32,
}

include!(concat!(env!("OUT_DIR"), "/oreui_art.rs"));
