//! Immutable logical UI pages and checked dimension-bucket admission.

use std::sync::Arc;

use assets::RuntimeFontCatalog;
use sha2::{Digest, Sha256};

use crate::ui::{
    MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, MAX_UI_TEXTURE_SIDE, UiRenderRejectReason,
};

/// Buckets bind independently; page count and byte limits bound their residency.
pub const MAX_UI_TEXTURE_BUCKETS: usize = MAX_UI_TEXTURE_LAYERS as usize;
/// Replaceable pages after static UI: models, session glyphs, server UI, then a local font.
pub const MAX_UI_DYNAMIC_PAGES: usize = UI_FALLBACK_FONT_PAGE_OFFSET + MAX_UI_FALLBACK_FONT_PAGES;
/// Side length shared by general dynamic UI pages outside original-resolution model slots.
pub const UI_DYNAMIC_PAGE_SIDE: u32 = 256;
/// Fixed dynamic slot carrying the original player skin, not a projected thumbnail.
pub const UI_PLAYER_SKIN_PAGE_OFFSET: usize = 1;
pub const UI_MODEL_ATLAS_PAGE_OFFSET: usize = UI_PLAYER_SKIN_PAGE_OFFSET + 1;
pub const MAX_UI_MODEL_ATLAS_PAGES: usize = 7;
pub const UI_SESSION_ICON_PAGE_OFFSET: usize =
    UI_MODEL_ATLAS_PAGE_OFFSET + MAX_UI_MODEL_ATLAS_PAGES;
pub const UI_MODEL_ATLAS_SIDE: u32 = 512;
/// Dedicated bounded local-font slot, outside the server glyph and UI allocations.
pub const UI_LOCAL_FONT_PAGE_OFFSET: usize = 34;
pub const UI_LOCAL_FONT_PAGE_SIDE: u32 = 512;
pub const UI_FALLBACK_FONT_PAGE_OFFSET: usize = UI_LOCAL_FONT_PAGE_OFFSET + 1;
pub const MAX_UI_FALLBACK_FONT_PAGES: usize = assets::MAX_FONT_FALLBACK_PAGES;
pub const UI_FALLBACK_FONT_PAGE_SIDE: u32 = assets::FONT_FALLBACK_ATLAS_SIDE;
/// Replaceable full-resolution pages after the small ones, for menu artwork.
pub const MAX_UI_ART_PAGES: usize = 2;
pub const UI_ART_PAGE_SIDE: u32 = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Pixels {
    Owned(Arc<[u8]>),
    Font {
        catalog: Arc<RuntimeFontCatalog>,
        page: usize,
    },
}

/// How a page's texels are stored, and so the format of the bucket it uploads to.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum UiTextureFormat {
    #[default]
    Rgba8,
    /// One alpha byte per texel of a white page; the shader samples it as white with that alpha.
    Coverage,
}

impl UiTextureFormat {
    pub const fn bytes_per_texel(self) -> usize {
        match self {
            Self::Rgba8 => 4,
            Self::Coverage => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiTexturePage {
    dimensions: [u32; 2],
    identity: [u8; 32],
    format: UiTextureFormat,
    pixels: Pixels,
}

impl UiTexturePage {
    /// One alpha byte per texel for demand-filled white font pages.
    pub fn coverage(dimensions: [u32; 2], pixels: Arc<[u8]>) -> Result<Self, UiRenderRejectReason> {
        let expected = page_bytes_in(dimensions, UiTextureFormat::Coverage)?;
        if pixels.len() != expected {
            return Err(UiRenderRejectReason::TextureByteLengthInvalid {
                actual: pixels.len(),
                expected,
            });
        }
        Ok(Self {
            dimensions,
            identity: Sha256::digest(&pixels).into(),
            format: UiTextureFormat::Coverage,
            pixels: Pixels::Owned(pixels),
        })
    }
    pub fn owned(dimensions: [u32; 2], pixels: Arc<[u8]>) -> Result<Self, UiRenderRejectReason> {
        let expected = page_bytes(dimensions)?;
        if pixels.len() != expected {
            return Err(UiRenderRejectReason::TextureByteLengthInvalid {
                actual: pixels.len(),
                expected,
            });
        }
        Ok(Self {
            dimensions,
            identity: Sha256::digest(&pixels).into(),
            format: UiTextureFormat::Rgba8,
            pixels: Pixels::Owned(pixels),
        })
    }

    pub fn font(
        catalog: Arc<RuntimeFontCatalog>,
        page: usize,
    ) -> Result<Self, UiRenderRejectReason> {
        let source = catalog
            .pages()
            .get(page)
            .ok_or(UiRenderRejectReason::InvalidTextureExtent)?;
        let dimensions = [source.width, source.height];
        let format = match source.pixels {
            assets::FontPixels::Rgba8(_) => UiTextureFormat::Rgba8,
            assets::FontPixels::Coverage(_) => UiTextureFormat::Coverage,
        };
        if source.pixels.bytes().len() != page_bytes_in(dimensions, format)? {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        // RuntimeFontCatalog can only be obtained through its authenticated decoder.
        Ok(Self {
            dimensions,
            identity: source.pixels_sha256,
            format,
            pixels: Pixels::Font { catalog, page },
        })
    }

    pub fn pixels(&self) -> &[u8] {
        match &self.pixels {
            Pixels::Owned(pixels) => pixels,
            Pixels::Font { catalog, page } => catalog.pages()[*page].pixels.bytes(),
        }
    }

    pub const fn dimensions(&self) -> [u32; 2] {
        self.dimensions
    }
    pub const fn format(&self) -> UiTextureFormat {
        self.format
    }
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }
}

fn page_bytes(dimensions: [u32; 2]) -> Result<usize, UiRenderRejectReason> {
    page_bytes_in(dimensions, UiTextureFormat::Rgba8)
}

fn page_bytes_in(
    [width, height]: [u32; 2],
    format: UiTextureFormat,
) -> Result<usize, UiRenderRejectReason> {
    if width == 0 || height == 0 || width > MAX_UI_TEXTURE_SIDE || height > MAX_UI_TEXTURE_SIDE {
        return Err(UiRenderRejectReason::InvalidTextureExtent);
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(format.bytes_per_texel()))
        .ok_or(UiRenderRejectReason::InvalidTextureExtent)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiTextureLocation {
    pub bucket: usize,
    pub layer: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiTextureBucket {
    pub dimensions: [u32; 2],
    pub format: UiTextureFormat,
    pub layers: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiTexturePlan {
    buckets: Box<[UiTextureBucket]>,
    locations: Box<[UiTextureLocation]>,
    bytes: usize,
}

impl UiTexturePlan {
    /// Dry plan all pages, including blank reservations, before allocating pixels.
    pub fn new(dimensions: &[[u32; 2]]) -> Result<Self, UiRenderRejectReason> {
        let pages = dimensions
            .iter()
            .map(|&dimensions| (dimensions, UiTextureFormat::Rgba8))
            .collect::<Vec<_>>();
        Self::with_formats(&pages)
    }

    /// Plans pages of mixed storage; each format gets its own buckets.
    pub fn with_formats(
        pages: &[([u32; 2], UiTextureFormat)],
    ) -> Result<Self, UiRenderRejectReason> {
        if pages.is_empty() || pages.len() > MAX_UI_TEXTURE_LAYERS as usize {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let mut buckets = Vec::<UiTextureBucket>::new();
        let mut locations = Vec::with_capacity(pages.len());
        let mut bytes = 0usize;
        for &(dimensions, format) in pages {
            bytes = bytes
                .checked_add(page_bytes_in(dimensions, format)?)
                .ok_or(UiRenderRejectReason::InvalidTextureExtent)?;
            if bytes > MAX_UI_TEXTURE_BYTES {
                return Err(UiRenderRejectReason::TextureByteLimitExceeded {
                    actual: bytes,
                    limit: MAX_UI_TEXTURE_BYTES,
                });
            }
            let bucket = if let Some(index) = buckets
                .iter()
                .position(|b| b.dimensions == dimensions && b.format == format)
            {
                index
            } else {
                if buckets.len() == MAX_UI_TEXTURE_BUCKETS {
                    return Err(UiRenderRejectReason::InvalidTextureExtent);
                }
                buckets.push(UiTextureBucket {
                    dimensions,
                    format,
                    layers: 0,
                });
                buckets.len() - 1
            };
            locations.push(UiTextureLocation {
                bucket,
                layer: buckets[bucket].layers,
            });
            buckets[bucket].layers += 1;
        }
        Ok(Self {
            buckets: buckets.into(),
            locations: locations.into(),
            bytes,
        })
    }

    pub fn validate_device(
        &self,
        max_side: u32,
        max_layers: u32,
    ) -> Result<(), UiRenderRejectReason> {
        if self
            .buckets
            .iter()
            .any(|b| b.dimensions.iter().any(|&side| side > max_side) || b.layers > max_layers)
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        Ok(())
    }
    pub fn buckets(&self) -> &[UiTextureBucket] {
        &self.buckets
    }
    pub fn locations(&self) -> &[UiTextureLocation] {
        &self.locations
    }
    pub const fn bytes(&self) -> usize {
        self.bytes
    }
}

#[derive(Clone, Debug)]
pub struct UiTextureCatalog {
    pages: Arc<[UiTexturePage]>,
    plan: UiTexturePlan,
    dynamic_start: usize,
    identity: [u8; 32],
    static_identity: [u8; 32],
    source_identity: [u8; 32],
}

impl PartialEq for UiTextureCatalog {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.static_identity == other.static_identity
            && self.plan == other.plan
            && self.dynamic_start == other.dynamic_start
    }
}
impl Eq for UiTextureCatalog {}

impl UiTextureCatalog {
    pub fn new(
        pages: Vec<UiTexturePage>,
        dynamic_start: usize,
    ) -> Result<Self, UiRenderRejectReason> {
        Self::with_source_identity(pages, dynamic_start, [0; 32])
    }

    /// Source identity is an invalidation namespace, never proof of pixels or
    /// budget admission. Production derives it from its loaded asset catalogs.
    pub fn with_source_identity(
        pages: Vec<UiTexturePage>,
        dynamic_start: usize,
        source_identity: [u8; 32],
    ) -> Result<Self, UiRenderRejectReason> {
        if dynamic_start > pages.len()
            || pages.is_empty()
            || pages.len() > MAX_UI_TEXTURE_LAYERS as usize
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let dynamic = &pages[dynamic_start..];
        let art = dynamic
            .iter()
            .enumerate()
            .filter(|(offset, page)| {
                *offset >= MAX_UI_DYNAMIC_PAGES && page.dimensions == [UI_ART_PAGE_SIDE; 2]
            })
            .count();
        let small = dynamic.len() - art;
        if small > MAX_UI_DYNAMIC_PAGES
            || art > MAX_UI_ART_PAGES
            || dynamic.iter().enumerate().any(|(offset, page)| {
                (matches!(&page.pixels, Pixels::Font { .. })
                    && offset != UI_LOCAL_FONT_PAGE_OFFSET
                    && !(UI_FALLBACK_FONT_PAGE_OFFSET..MAX_UI_DYNAMIC_PAGES).contains(&offset))
                    || !valid_dynamic_dimensions(offset, page.dimensions)
            })
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let planned = pages
            .iter()
            .map(|page| (page.dimensions, page.format))
            .collect::<Vec<_>>();
        let plan = UiTexturePlan::with_formats(&planned)?;
        let mut all = Sha256::new();
        let mut static_pages = Sha256::new();
        all.update(source_identity);
        static_pages.update(source_identity);
        all.update((dynamic_start as u64).to_le_bytes());
        static_pages.update((dynamic_start as u64).to_le_bytes());
        // Reserved logical slots keep their identity when model or session icon pages resize.
        all.update((pages.len() as u64).to_le_bytes());
        static_pages.update((pages.len() as u64).to_le_bytes());
        for (index, page) in pages.iter().enumerate() {
            for side in page.dimensions {
                all.update(side.to_le_bytes());
                if index < dynamic_start || !is_resizable_slot(index - dynamic_start) {
                    static_pages.update(side.to_le_bytes());
                }
            }
            all.update(page.identity);
            if index < dynamic_start {
                static_pages.update(page.identity);
            }
        }
        Ok(Self {
            pages: pages.into(),
            plan,
            dynamic_start,
            identity: all.finalize().into(),
            static_identity: static_pages.finalize().into(),
            source_identity,
        })
    }

    pub fn replace_dynamic(
        &self,
        replacement: Vec<UiTexturePage>,
    ) -> Result<Self, UiRenderRejectReason> {
        if replacement.len() != self.pages.len() - self.dynamic_start
            || replacement
                .iter()
                .zip(&self.pages[self.dynamic_start..])
                .enumerate()
                .any(|(offset, (a, b))| !is_resizable_slot(offset) && a.dimensions != b.dimensions)
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let mut pages = self.pages[..self.dynamic_start].to_vec();
        pages.extend(replacement);
        Self::with_source_identity(pages, self.dynamic_start, self.source_identity)
    }
    pub fn pages(&self) -> &[UiTexturePage] {
        &self.pages
    }
    pub fn plan(&self) -> &UiTexturePlan {
        &self.plan
    }
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }
    pub const fn static_identity(&self) -> [u8; 32] {
        self.static_identity
    }
    pub const fn dynamic_start(&self) -> usize {
        self.dynamic_start
    }
}

/// Model sources and session icons can resize without changing their logical slots.
fn is_resizable_slot(offset: usize) -> bool {
    (UI_PLAYER_SKIN_PAGE_OFFSET..=UI_SESSION_ICON_PAGE_OFFSET).contains(&offset)
}

/// Accepts only the dimensions supported by each reserved dynamic page's producer.
fn valid_dynamic_dimensions(offset: usize, [width, height]: [u32; 2]) -> bool {
    if (UI_FALLBACK_FONT_PAGE_OFFSET..MAX_UI_DYNAMIC_PAGES).contains(&offset) {
        return [width, height] == [UI_FALLBACK_FONT_PAGE_SIDE; 2];
    }
    if offset == UI_LOCAL_FONT_PAGE_OFFSET {
        return [width, height] == [UI_LOCAL_FONT_PAGE_SIDE; 2];
    }
    if [width, height] == [UI_DYNAMIC_PAGE_SIDE; 2] {
        return true;
    }
    if offset == UI_PLAYER_SKIN_PAGE_OFFSET {
        let classic = render_api::CLASSIC_SKIN_SIDE as u32;
        return (width == classic && height == classic / 2)
            || (width == height
                && width.is_power_of_two()
                && (classic..=render_api::MAX_STANDARD_SKIN_SIDE).contains(&width));
    }
    if (UI_MODEL_ATLAS_PAGE_OFFSET..UI_SESSION_ICON_PAGE_OFFSET).contains(&offset) {
        return [width, height] == [UI_MODEL_ATLAS_SIDE; 2];
    }
    if offset == UI_SESSION_ICON_PAGE_OFFSET {
        return width == height
            && width.is_power_of_two()
            && (UI_DYNAMIC_PAGE_SIDE..=MAX_UI_TEXTURE_SIDE).contains(&width);
    }
    [width, height] == [UI_ART_PAGE_SIDE; 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_fonts_art_and_skin_dimensions_share_the_resident_budget() {
        let mut pages = [256, 512, 1024]
            .map(|side| ([side; 2], UiTextureFormat::Coverage))
            .to_vec();
        pages.extend(
            [16, 256, 512, 1024, 2048, 3072].map(|side| ([side; 2], UiTextureFormat::Rgba8)),
        );
        let plan = UiTexturePlan::with_formats(&pages).unwrap();
        assert_eq!(plan.buckets().len(), pages.len());
        assert!(plan.bytes() < MAX_UI_TEXTURE_BYTES);
        for (page, location) in pages.iter().zip(plan.locations()) {
            let bucket = plan.buckets()[location.bucket];
            assert_eq!((bucket.dimensions, bucket.format), *page);
            assert_eq!(location.layer, 0);
        }
        assert!(plan.validate_device(4096, 1).is_ok());
    }

    /// Coverage pages get their own one-byte-per-texel buckets beside same-sized RGBA pages.
    #[test]
    fn coverage_pages_plan_separate_quarter_size_buckets() {
        let side = [64, 64];
        let plan = UiTexturePlan::with_formats(&[
            (side, UiTextureFormat::Coverage),
            (side, UiTextureFormat::Rgba8),
            (side, UiTextureFormat::Coverage),
        ])
        .unwrap();
        assert_eq!(plan.buckets().len(), 2);
        assert_eq!(plan.buckets()[0].format, UiTextureFormat::Coverage);
        assert_eq!(plan.buckets()[0].layers, 2);
        assert_eq!(
            plan.locations()[2],
            UiTextureLocation {
                bucket: 0,
                layer: 1
            }
        );
        assert_eq!(plan.bytes(), 64 * 64 * (1 + 4 + 1));
    }
}
