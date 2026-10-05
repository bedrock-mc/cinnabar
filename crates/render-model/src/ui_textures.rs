//! Immutable logical UI pages and checked dimension-bucket admission.

use std::sync::Arc;

use assets::RuntimeFontCatalog;
use sha2::{Digest, Sha256};

use crate::ui::{
    MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, MAX_UI_TEXTURE_SIDE, UiRenderRejectReason,
};

pub const MAX_UI_TEXTURE_BUCKETS: usize = 8;
/// Replaceable pages after static UI: models, session glyphs, server UI, then a local font.
pub const MAX_UI_DYNAMIC_PAGES: usize = UI_LOCAL_FONT_PAGE_OFFSET + 1;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiTexturePage {
    dimensions: [u32; 2],
    identity: [u8; 32],
    pixels: Pixels,
}

impl UiTexturePage {
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
        if source.rgba8.len() != page_bytes(dimensions)? {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        // RuntimeFontCatalog can only be obtained through its authenticated decoder.
        Ok(Self {
            dimensions,
            identity: source.pixels_sha256,
            pixels: Pixels::Font { catalog, page },
        })
    }

    pub fn pixels(&self) -> &[u8] {
        match &self.pixels {
            Pixels::Owned(pixels) => pixels,
            Pixels::Font { catalog, page } => &catalog.pages()[*page].rgba8,
        }
    }

    pub const fn dimensions(&self) -> [u32; 2] {
        self.dimensions
    }
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }
}

fn page_bytes([width, height]: [u32; 2]) -> Result<usize, UiRenderRejectReason> {
    if width == 0 || height == 0 || width > MAX_UI_TEXTURE_SIDE || height > MAX_UI_TEXTURE_SIDE {
        return Err(UiRenderRejectReason::InvalidTextureExtent);
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
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
        if dimensions.is_empty() || dimensions.len() > MAX_UI_TEXTURE_LAYERS as usize {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let mut buckets = Vec::<UiTextureBucket>::new();
        let mut locations = Vec::with_capacity(dimensions.len());
        let mut bytes = 0usize;
        for &dimensions in dimensions {
            bytes = bytes
                .checked_add(page_bytes(dimensions)?)
                .ok_or(UiRenderRejectReason::InvalidTextureExtent)?;
            if bytes > MAX_UI_TEXTURE_BYTES {
                return Err(UiRenderRejectReason::TextureByteLimitExceeded {
                    actual: bytes,
                    limit: MAX_UI_TEXTURE_BYTES,
                });
            }
            let bucket =
                if let Some(index) = buckets.iter().position(|b| b.dimensions == dimensions) {
                    index
                } else {
                    if buckets.len() == MAX_UI_TEXTURE_BUCKETS {
                        return Err(UiRenderRejectReason::InvalidTextureExtent);
                    }
                    buckets.push(UiTextureBucket {
                        dimensions,
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
                *offset != UI_SESSION_ICON_PAGE_OFFSET && page.dimensions == [UI_ART_PAGE_SIDE; 2]
            })
            .count();
        let small = dynamic.len() - art;
        if small > MAX_UI_DYNAMIC_PAGES
            || art > MAX_UI_ART_PAGES
            || dynamic.iter().enumerate().any(|(offset, page)| {
                (matches!(&page.pixels, Pixels::Font { .. }) && offset != UI_LOCAL_FONT_PAGE_OFFSET)
                    || !valid_dynamic_dimensions(offset, page.dimensions)
            })
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let dimensions = pages
            .iter()
            .map(UiTexturePage::dimensions)
            .collect::<Vec<_>>();
        let plan = UiTexturePlan::new(&dimensions)?;
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
