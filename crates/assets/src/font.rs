use std::{collections::BTreeMap, str, sync::Arc};

use sha2::{Digest, Sha256};
use thiserror::Error;

mod carrier;
pub use carrier::encode_font_catalog;
use carrier::{
    array_at, decode_glyphs, decode_pages, invalid_catalog, validate_envelope,
    validate_glyph_records, validate_page_offsets,
};
mod fallback;
#[cfg(test)]
mod merge_tests;
mod rendering;
mod runtime;
pub use fallback::FontGlyphRequests;
pub use rendering::{FONT_STYLE_COVERAGE_GAMMA, FONT_STYLE_SDF, FontRendering};

pub const FONT_CARRIER_MAGIC: [u8; 9] = *b"MCBEFONT1";
pub const FONT_CARRIER_SCHEMA: u32 = 2;
pub const MAX_FONT_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_FONT_PAGES: usize = 256;
pub const MAX_FONT_GLYPHS: usize = 65_536;
pub const MAX_FONT_KERNING_PAIRS: usize = 65_536;
pub const MAX_FONT_PAGE_SIDE: u32 = 4_096;
pub const MAX_FONT_PATH_BYTES: usize = 512;
/// Default raster em for the shipped Sans face. Semantic faces own their raster em.
pub const FONT_RASTER_EM_PIXELS: u32 = crate::carriers::FONT.font_face.unwrap().raster_em_pixels();
pub const FONT_FALLBACK_ATLAS_SIDE: u32 = 1024;
pub const MAX_FONT_FALLBACK_PAGES: usize = 16;

const MAX_FONT_DECODED_BYTES: usize = MAX_FONT_SOURCE_BYTES as usize;
pub const MAX_FONT_CARRIER_BYTES: usize = 128 * 1024 * 1024;
const HEADER_BYTES: usize = 96;
const GLYPH_BYTES: usize = 24;
const PAGE_BYTES: usize = 108;
const HASH_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphMetrics {
    pub codepoint: char,
    pub page: u16,
    pub uv: [u16; 4],
    pub bearing: [i16; 2],
    pub advance_64: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontTexturePage {
    pub source_path: Box<str>,
    pub source_bytes: u32,
    pub source_sha256: [u8; 32],
    pub pixels_sha256: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub pixels: FontPixels,
}

/// A page's texels: RGBA8, or one coverage byte per texel when every visible texel is white.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontPixels {
    Rgba8(Box<[u8]>),
    Coverage(Box<[u8]>),
}

impl FontPixels {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Rgba8(bytes) | Self::Coverage(bytes) => bytes,
        }
    }

    /// Borrows RGBA texels when this page retains color channels.
    pub fn rgba8(&self) -> Option<&[u8]> {
        match self {
            Self::Rgba8(bytes) => Some(bytes),
            Self::Coverage(_) => None,
        }
    }

    /// One texel as RGBA8; coverage texels are white.
    pub fn texel(&self, index: usize) -> Option<[u8; 4]> {
        match self {
            Self::Rgba8(bytes) => bytes
                .get(index * 4..index * 4 + 4)
                .map(|texel| [texel[0], texel[1], texel[2], texel[3]]),
            Self::Coverage(bytes) => bytes.get(index).map(|&alpha| [255, 255, 255, alpha]),
        }
    }

    /// Coverage-only storage when every visible texel is white, dropping transparent texels'
    /// colour, which nearest sampling never shows.
    fn coverage(&self) -> Option<Self> {
        let rgba8 = self.rgba8()?;
        rgba8
            .chunks_exact(4)
            .all(|texel| texel[3] == 0 || texel[..3] == [255; 3])
            .then(|| Self::Coverage(rgba8.chunks_exact(4).map(|texel| texel[3]).collect()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontCatalogIdentity {
    pub schema: u32,
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
}

/// Outline metrics in atlas pixels, independent of a label's CSS line height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontLineMetrics {
    pub em_64: u32,
    pub ascent_64: u32,
    pub descent_64: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledFontCatalog {
    identity: FontCatalogIdentity,
    glyphs: Box<[GlyphMetrics]>,
    pages: Arc<[FontTexturePage]>,
    /// Drawn size in 1/64 px for glyphs that are not drawn at their texel size.
    draw_sizes_64: Arc<BTreeMap<char, [u32; 2]>>,
    named: Arc<BTreeMap<String, Self>>,
    sizes: Arc<BTreeMap<u32, Self>>,
    linear_sampling: bool,
    rendering: FontRendering,
    line_metrics: Option<FontLineMetrics>,
    kerning_64: Arc<BTreeMap<(char, char), i32>>,
    fallback: Option<Arc<Self>>,
    requests: Option<Arc<FontGlyphRequests>>,
}

pub type RuntimeFontCatalog = CompiledFontCatalog;

impl CompiledFontCatalog {
    /// Decodes a catalog only when it belongs to the exact source manifest
    /// selected by the caller at startup.
    pub fn decode(
        bytes: &[u8],
        expected_source_manifest_sha256: [u8; 32],
    ) -> Result<Self, FontCatalogError> {
        if expected_source_manifest_sha256 == [0; 32] {
            return Err(FontCatalogError::SourceManifestMismatch);
        }
        let envelope = validate_envelope(bytes, expected_source_manifest_sha256)?;
        validate_page_offsets(bytes, envelope)?;
        validate_glyph_records(bytes, envelope)?;

        let pages = decode_pages(bytes, envelope)?;
        let glyphs = decode_glyphs(bytes, envelope, &pages)?;
        Ok(Self {
            identity: FontCatalogIdentity {
                schema: FONT_CARRIER_SCHEMA,
                source_manifest_sha256: expected_source_manifest_sha256,
                carrier_sha256: array_at(bytes, envelope.hash_offset)?,
            },
            glyphs: glyphs.into_boxed_slice(),
            pages: pages.into(),
            draw_sizes_64: Arc::default(),
            named: Arc::default(),
            sizes: Arc::default(),
            linear_sampling: false,
            rendering: FontRendering::Coverage,
            line_metrics: None,
            kerning_64: Arc::default(),
            fallback: None,
            requests: None,
        })
    }

    /// A catalog that also draws `extra` glyphs, which replace same-codepoint entries when
    /// `replace` allows; its identity changes with them so layout caches never alias.
    pub fn with_glyphs(&self, extra: &[crate::SheetGlyph], replace: impl Fn(char) -> bool) -> Self {
        // The glyph table stays sorted by codepoint: merge the few added glyphs into it
        // rather than rebuilding a map of every glyph.
        let mut added: BTreeMap<char, GlyphMetrics> = BTreeMap::new();
        let mut draw_sizes_64 = (*self.draw_sizes_64).clone();
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        for glyph in extra {
            let codepoint = glyph.metrics.codepoint;
            if (added.contains_key(&codepoint) || self.glyph(codepoint).is_some())
                && !replace(codepoint)
            {
                continue;
            }
            added.insert(codepoint, glyph.metrics);
            draw_sizes_64.insert(codepoint, glyph.draw_size_64);
            hash.update(u32::from(codepoint).to_le_bytes());
            hash.update(glyph.metrics.page.to_le_bytes());
            for value in glyph.metrics.uv {
                hash.update(value.to_le_bytes());
            }
            hash.update(glyph.metrics.advance_64.to_le_bytes());
            for value in glyph.metrics.bearing {
                hash.update(value.to_le_bytes());
            }
            for value in glyph.draw_size_64 {
                hash.update(value.to_le_bytes());
            }
        }
        Self {
            identity: FontCatalogIdentity {
                carrier_sha256: hash.finalize().into(),
                ..self.identity
            },
            glyphs: merge_glyphs(&self.glyphs, added),
            pages: Arc::clone(&self.pages),
            draw_sizes_64: Arc::new(draw_sizes_64),
            named: Arc::clone(&self.named),
            sizes: Arc::clone(&self.sizes),
            linear_sampling: self.linear_sampling,
            rendering: self.rendering,
            line_metrics: self.line_metrics,
            kerning_64: Arc::clone(&self.kerning_64),
            fallback: self.fallback.clone(),
            requests: self.requests.clone(),
        }
    }

    /// Selects filtered sampling for a runtime outline raster; decoded carriers remain nearest.
    pub fn with_linear_sampling(mut self) -> Self {
        if !self.linear_sampling {
            let mut hash = Sha256::new();
            hash.update(self.identity.carrier_sha256);
            hash.update(b"linear outline sampling");
            self.identity.carrier_sha256 = hash.finalize().into();
            self.linear_sampling = true;
        }
        self
    }

    pub const fn linear_sampling(&self) -> bool {
        match self.rendering {
            FontRendering::Coverage => self.linear_sampling,
            FontRendering::NativeCoverage => false,
            FontRendering::NativeSdf => true,
        }
    }

    pub fn with_rendering(mut self, rendering: FontRendering) -> Self {
        if self.rendering != rendering {
            let mut hash = Sha256::new();
            hash.update(self.identity.carrier_sha256);
            hash.update(b"runtime font rendering");
            hash.update([rendering.style_flags()]);
            self.identity.carrier_sha256 = hash.finalize().into();
            self.rendering = rendering;
        }
        self
    }

    pub const fn rendering(&self) -> FontRendering {
        self.rendering
    }

    /// Runtime outline faces retain their source baseline and em for semantic text sizing.
    pub fn with_line_metrics(mut self, metrics: FontLineMetrics) -> Result<Self, FontCatalogError> {
        if self.line_metrics == Some(metrics) {
            return Ok(self);
        }
        if metrics.em_64 == 0
            || metrics.em_64 > MAX_FONT_PAGE_SIDE * 64
            || metrics.ascent_64 == 0
            || metrics.ascent_64.saturating_add(metrics.descent_64) > metrics.em_64 * 4
        {
            return Err(invalid_catalog("outline line metrics exceed bounds"));
        }
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        hash.update(metrics.em_64.to_le_bytes());
        hash.update(metrics.ascent_64.to_le_bytes());
        hash.update(metrics.descent_64.to_le_bytes());
        self.identity.carrier_sha256 = hash.finalize().into();
        self.line_metrics = Some(metrics);
        Ok(self)
    }

    pub const fn line_metrics(&self) -> Option<FontLineMetrics> {
        self.line_metrics
    }

    /// Runtime horizontal pair advances in the same atlas units as glyph advances.
    pub fn with_kerning(
        mut self,
        pairs: BTreeMap<(char, char), i32>,
    ) -> Result<Self, FontCatalogError> {
        if pairs.len() > MAX_FONT_KERNING_PAIRS
            || pairs.iter().any(|(&(left, right), &value)| {
                value.unsigned_abs() > MAX_FONT_PAGE_SIDE * 64
                    || self.glyph(left).is_none()
                    || self.glyph(right).is_none()
            })
        {
            return Err(invalid_catalog("outline kerning exceeds bounds"));
        }
        if *self.kerning_64 == pairs {
            return Ok(self);
        }
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        for (&(left, right), &value) in &pairs {
            hash.update(u32::from(left).to_le_bytes());
            hash.update(u32::from(right).to_le_bytes());
            hash.update(value.to_le_bytes());
        }
        self.identity.carrier_sha256 = hash.finalize().into();
        self.kerning_64 = Arc::new(pairs);
        Ok(self)
    }

    pub fn kerning_64(&self, left: char, right: char) -> i32 {
        self.kerning_64.get(&(left, right)).copied().unwrap_or(0)
    }

    /// Keeps white-glyph pages as one coverage byte per texel, a quarter of their RGBA size.
    /// Native distance fields use white RGB throughout, so their linear sampling also permits R8.
    pub fn with_coverage_pages(mut self) -> Self {
        if self.linear_sampling() && self.rendering != FontRendering::NativeSdf {
            return self;
        }
        if self
            .pages
            .iter()
            .all(|page| matches!(page.pixels, FontPixels::Coverage(_)))
        {
            return self;
        }
        let pages = self
            .pages
            .iter()
            .map(|page| {
                if let Some(pixels) = page.pixels.coverage() {
                    FontTexturePage {
                        pixels_sha256: Sha256::digest(pixels.bytes()).into(),
                        pixels,
                        source_path: page.source_path.clone(),
                        source_bytes: page.source_bytes,
                        source_sha256: page.source_sha256,
                        width: page.width,
                        height: page.height,
                    }
                } else {
                    page.clone()
                }
            })
            .collect::<Vec<_>>();
        self.pages = pages.into();
        self
    }

    /// Drawn `[width, height]` in 1/64 px when it differs from the glyph's texel size.
    pub fn draw_size_64(&self, codepoint: char) -> Option<[u32; 2]> {
        self.draw_sizes_64.get(&codepoint).copied()
    }

    pub const fn identity(&self) -> FontCatalogIdentity {
        self.identity
    }

    pub fn glyphs(&self) -> &[GlyphMetrics] {
        &self.glyphs
    }

    pub fn pages(&self) -> &[FontTexturePage] {
        &self.pages
    }

    pub fn glyph(&self, codepoint: char) -> Option<&GlyphMetrics> {
        self.glyphs
            .binary_search_by_key(&codepoint, |glyph| glyph.codepoint)
            .ok()
            .map(|index| &self.glyphs[index])
    }
}

/// `base`, sorted by codepoint, with `added` inserted in order; an added glyph replaces the
/// base glyph of its codepoint.
fn merge_glyphs(base: &[GlyphMetrics], added: BTreeMap<char, GlyphMetrics>) -> Box<[GlyphMetrics]> {
    let mut merged = Vec::with_capacity(base.len() + added.len());
    let mut added = added.into_values().peekable();
    for glyph in base {
        while let Some(next) = added.next_if(|next| next.codepoint < glyph.codepoint) {
            merged.push(next);
        }
        match added.next_if(|next| next.codepoint == glyph.codepoint) {
            Some(replacement) => merged.push(replacement),
            None => merged.push(*glyph),
        }
    }
    merged.extend(added);
    merged.into_boxed_slice()
}

#[derive(Debug, Error)]
pub enum FontCatalogError {
    #[error("font carrier source manifest does not match the required startup provenance")]
    SourceManifestMismatch,
    #[error("font carrier SHA-256 does not match its payload")]
    CarrierHashMismatch,
    #[error("invalid compiled font catalog: {detail}")]
    InvalidCatalog { detail: Box<str> },
    #[error("invalid MCBEFONT1 carrier: {detail}")]
    InvalidCarrier { detail: Box<str> },
}
