use std::{collections::BTreeMap, str, sync::Arc};

use sha2::{Digest, Sha256};
use thiserror::Error;

mod fallback;
mod rendering;
mod runtime;
pub use fallback::FontGlyphRequests;
pub use rendering::{FONT_STYLE_COVERAGE_GAMMA, FONT_STYLE_SDF, FontRendering};

pub const FONT_CARRIER_MAGIC: [u8; 9] = *b"MCBEFONT1";
pub const FONT_CARRIER_SCHEMA: u32 = 1;
pub const MAX_FONT_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_FONT_PAGES: usize = 256;
pub const MAX_FONT_GLYPHS: usize = 65_536;
pub const MAX_FONT_KERNING_PAIRS: usize = 65_536;
pub const MAX_FONT_PAGE_SIDE: u32 = 4_096;
pub const MAX_FONT_PATH_BYTES: usize = 512;
/// Em height used to compile the shipped outline font carriers.
pub const FONT_RASTER_EM_PIXELS: u32 = 18;
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

    /// The RGBA8 texels an encoder or hash needs; coverage pages no longer have them.
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
        let mut glyphs: BTreeMap<char, GlyphMetrics> = self
            .glyphs
            .iter()
            .map(|glyph| (glyph.codepoint, *glyph))
            .collect();
        let mut draw_sizes_64 = (*self.draw_sizes_64).clone();
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        for glyph in extra {
            let codepoint = glyph.metrics.codepoint;
            if glyphs.contains_key(&codepoint) && !replace(codepoint) {
                continue;
            }
            glyphs.insert(codepoint, glyph.metrics);
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
            glyphs: glyphs.into_values().collect(),
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
        let pages = self
            .pages
            .iter()
            .map(|page| FontTexturePage {
                pixels: page
                    .pixels
                    .coverage()
                    .unwrap_or_else(|| page.pixels.clone()),
                ..page.clone()
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

#[derive(Clone, Copy)]
struct Envelope {
    glyph_count: usize,
    page_count: usize,
    glyph_offset: usize,
    page_offset: usize,
    paths_offset: usize,
    pixels_offset: usize,
    hash_offset: usize,
}

pub fn encode_font_catalog(
    source_manifest_sha256: [u8; 32],
    glyphs: &[GlyphMetrics],
    pages: &[FontTexturePage],
) -> Result<Box<[u8]>, FontCatalogError> {
    validate_catalog(source_manifest_sha256, glyphs, pages)?;
    let glyph_offset = HEADER_BYTES;
    let page_offset = checked_add(
        glyph_offset,
        checked_mul(glyphs.len(), GLYPH_BYTES, invalid_catalog)?,
        invalid_catalog,
    )?;
    let paths_offset = checked_add(
        page_offset,
        checked_mul(pages.len(), PAGE_BYTES, invalid_catalog)?,
        invalid_catalog,
    )?;
    let paths_bytes = pages.iter().try_fold(0usize, |total, page| {
        checked_add(total, page.source_path.len(), invalid_catalog)
    })?;
    let pixels_offset = checked_add(paths_offset, paths_bytes, invalid_catalog)?;
    let pixels_bytes = pages.iter().try_fold(0usize, |total, page| {
        checked_add(total, page.pixels.bytes().len(), invalid_catalog)
    })?;
    let hash_offset = checked_add(pixels_offset, pixels_bytes, invalid_catalog)?;
    let total_bytes = checked_add(hash_offset, HASH_BYTES, invalid_catalog)?;
    if total_bytes > MAX_FONT_CARRIER_BYTES {
        return Err(invalid_catalog("font carrier exceeds its byte bound"));
    }

    let mut bytes = Vec::with_capacity(total_bytes);
    bytes.extend_from_slice(&FONT_CARRIER_MAGIC);
    push_u32(&mut bytes, FONT_CARRIER_SCHEMA);
    push_u32(
        &mut bytes,
        u32::try_from(glyphs.len()).map_err(|_| invalid_catalog("glyph count overflow"))?,
    );
    push_u32(
        &mut bytes,
        u32::try_from(pages.len()).map_err(|_| invalid_catalog("page count overflow"))?,
    );
    bytes.extend_from_slice(&source_manifest_sha256);
    for offset in [
        glyph_offset,
        page_offset,
        paths_offset,
        pixels_offset,
        hash_offset,
    ] {
        push_u64(&mut bytes, offset)?;
    }
    bytes.resize(HEADER_BYTES, 0);

    for glyph in glyphs {
        push_u32(&mut bytes, glyph.codepoint as u32);
        push_u16(&mut bytes, glyph.page);
        push_u16(&mut bytes, 0);
        for coordinate in glyph.uv {
            push_u16(&mut bytes, coordinate);
        }
        for bearing in glyph.bearing {
            push_i16(&mut bytes, bearing);
        }
        push_i16(&mut bytes, glyph.advance_64);
        push_i16(&mut bytes, 0);
    }

    let mut path_cursor = paths_offset;
    let mut pixel_cursor = pixels_offset;
    for page in pages {
        push_u64(&mut bytes, path_cursor)?;
        push_u32(
            &mut bytes,
            u32::try_from(page.source_path.len())
                .map_err(|_| invalid_catalog("font path length overflow"))?,
        );
        push_u32(&mut bytes, page.width);
        push_u32(&mut bytes, page.height);
        push_u32(&mut bytes, page.source_bytes);
        push_u64(&mut bytes, pixel_cursor)?;
        push_u64(&mut bytes, page.pixels.bytes().len())?;
        bytes.extend_from_slice(&page.source_sha256);
        bytes.extend_from_slice(&page.pixels_sha256);
        push_u32(&mut bytes, 0);
        path_cursor = checked_add(path_cursor, page.source_path.len(), invalid_catalog)?;
        pixel_cursor = checked_add(pixel_cursor, page.pixels.bytes().len(), invalid_catalog)?;
    }
    for page in pages {
        bytes.extend_from_slice(page.source_path.as_bytes());
    }
    for page in pages {
        bytes.extend_from_slice(page.pixels.bytes());
    }
    debug_assert_eq!(bytes.len(), hash_offset);
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    Ok(bytes.into_boxed_slice())
}

fn validate_catalog(
    source_manifest_sha256: [u8; 32],
    glyphs: &[GlyphMetrics],
    pages: &[FontTexturePage],
) -> Result<(), FontCatalogError> {
    if source_manifest_sha256 == [0; 32] {
        return Err(invalid_catalog("source manifest SHA-256 is zero"));
    }
    if pages.is_empty() || pages.len() > MAX_FONT_PAGES {
        return Err(invalid_catalog("font page count is outside its bound"));
    }
    if glyphs.is_empty() || glyphs.len() > MAX_FONT_GLYPHS {
        return Err(invalid_catalog("font glyph count is outside its bound"));
    }
    let mut total_source_bytes = 0u64;
    let mut total_decoded_bytes = 0usize;
    let mut previous_page: Option<(&str, [u8; 32])> = None;
    for page in pages {
        validate_source_path(&page.source_path).map_err(invalid_catalog)?;
        let key = (page.source_path.as_ref(), page.source_sha256);
        if previous_page.is_some_and(|previous| previous >= key) {
            return Err(invalid_catalog(
                "font pages are not strictly source-ordered",
            ));
        }
        if page.source_bytes == 0
            || page.source_sha256 == [0; 32]
            || page.pixels_sha256 == [0; 32]
            || page.width == 0
            || page.height == 0
            || page.width > MAX_FONT_PAGE_SIDE
            || page.height > MAX_FONT_PAGE_SIDE
        {
            return Err(invalid_catalog(
                "font page identity or dimensions are invalid",
            ));
        }
        let expected_pixels = pixel_length(page.width, page.height).map_err(invalid_catalog)?;
        let rgba8 = page
            .pixels
            .rgba8()
            .ok_or_else(|| invalid_catalog("coverage font pages cannot be encoded"))?;
        if rgba8.len() != expected_pixels || Sha256::digest(rgba8).as_slice() != page.pixels_sha256
        {
            return Err(invalid_catalog("font page pixels are invalid"));
        }
        total_source_bytes = total_source_bytes
            .checked_add(u64::from(page.source_bytes))
            .ok_or_else(|| invalid_catalog("font source-byte total overflow"))?;
        total_decoded_bytes = total_decoded_bytes
            .checked_add(rgba8.len())
            .ok_or_else(|| invalid_catalog("font decoded-byte total overflow"))?;
        previous_page = Some(key);
    }
    if total_source_bytes > MAX_FONT_SOURCE_BYTES || total_decoded_bytes > MAX_FONT_DECODED_BYTES {
        return Err(invalid_catalog(
            "font page bytes exceed their aggregate bound",
        ));
    }

    let mut previous_codepoint = None;
    for glyph in glyphs {
        let codepoint = glyph.codepoint as u32;
        if previous_codepoint.is_some_and(|previous| previous >= codepoint) {
            return Err(invalid_catalog(
                "font glyphs are not strictly codepoint-ordered",
            ));
        }
        let page = pages
            .get(usize::from(glyph.page))
            .ok_or_else(|| invalid_catalog("font glyph references an absent page"))?;
        validate_uv(glyph.uv, page.width, page.height).map_err(invalid_catalog)?;
        previous_codepoint = Some(codepoint);
    }
    Ok(())
}

fn validate_envelope(
    bytes: &[u8],
    expected_source_manifest_sha256: [u8; 32],
) -> Result<Envelope, FontCatalogError> {
    if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_FONT_CARRIER_BYTES {
        return Err(invalid_carrier("carrier byte length is outside its bound"));
    }
    if bytes.get(..9) != Some(FONT_CARRIER_MAGIC.as_slice()) {
        return Err(invalid_carrier("invalid MCBEFONT1 magic"));
    }
    if u32_at(bytes, 9)? != FONT_CARRIER_SCHEMA {
        return Err(invalid_carrier("unsupported MCBEFONT1 schema"));
    }
    let glyph_count = usize::try_from(u32_at(bytes, 13)?)
        .map_err(|_| invalid_carrier("glyph count exceeds platform"))?;
    let page_count = usize::try_from(u32_at(bytes, 17)?)
        .map_err(|_| invalid_carrier("page count exceeds platform"))?;
    if glyph_count == 0
        || glyph_count > MAX_FONT_GLYPHS
        || page_count == 0
        || page_count > MAX_FONT_PAGES
    {
        return Err(invalid_carrier("carrier counts are outside their bounds"));
    }
    let source_manifest_sha256 = array_at(bytes, 21)?;
    if source_manifest_sha256 != expected_source_manifest_sha256 {
        return Err(FontCatalogError::SourceManifestMismatch);
    }
    if bytes[93..HEADER_BYTES] != [0; 3] {
        return Err(invalid_carrier("carrier header reserved bytes are nonzero"));
    }
    let envelope = Envelope {
        glyph_count,
        page_count,
        glyph_offset: usize_at(bytes, 53)?,
        page_offset: usize_at(bytes, 61)?,
        paths_offset: usize_at(bytes, 69)?,
        pixels_offset: usize_at(bytes, 77)?,
        hash_offset: usize_at(bytes, 85)?,
    };
    let expected_page_offset = checked_add(
        HEADER_BYTES,
        checked_mul(glyph_count, GLYPH_BYTES, invalid_carrier)?,
        invalid_carrier,
    )?;
    let expected_paths_offset = checked_add(
        expected_page_offset,
        checked_mul(page_count, PAGE_BYTES, invalid_carrier)?,
        invalid_carrier,
    )?;
    if envelope.glyph_offset != HEADER_BYTES
        || envelope.page_offset != expected_page_offset
        || envelope.paths_offset != expected_paths_offset
        || envelope.paths_offset > envelope.pixels_offset
        || envelope.pixels_offset > envelope.hash_offset
        || checked_add(envelope.hash_offset, HASH_BYTES, invalid_carrier)? != bytes.len()
    {
        return Err(invalid_carrier("carrier section offsets are noncanonical"));
    }
    let actual_digest = Sha256::digest(&bytes[..envelope.hash_offset]);
    if actual_digest.as_slice() != &bytes[envelope.hash_offset..] {
        return Err(FontCatalogError::CarrierHashMismatch);
    }
    Ok(envelope)
}

fn validate_page_offsets(bytes: &[u8], envelope: Envelope) -> Result<(), FontCatalogError> {
    let mut expected_path_offset = envelope.paths_offset;
    let mut expected_pixel_offset = envelope.pixels_offset;
    let mut total_source_bytes = 0u64;
    let mut previous_page: Option<(&str, [u8; 32])> = None;
    for index in 0..envelope.page_count {
        let base = record_offset(envelope.page_offset, index, PAGE_BYTES)?;
        let path_offset = usize_at(bytes, base)?;
        let path_length = usize::try_from(u32_at(bytes, base + 8)?)
            .map_err(|_| invalid_carrier("font path length exceeds platform"))?;
        let width = u32_at(bytes, base + 12)?;
        let height = u32_at(bytes, base + 16)?;
        let source_bytes = u32_at(bytes, base + 20)?;
        let pixel_offset = usize_at(bytes, base + 24)?;
        let pixel_length = usize_at(bytes, base + 32)?;
        let source_sha256 = array_at(bytes, base + 40)?;
        // The envelope seals the pixels; their digests are checked when encoding.
        let pixels_sha256: [u8; 32] = array_at(bytes, base + 72)?;
        if u32_at(bytes, base + 104)? != 0
            || path_offset != expected_path_offset
            || pixel_offset != expected_pixel_offset
            || width == 0
            || height == 0
            || width > MAX_FONT_PAGE_SIDE
            || height > MAX_FONT_PAGE_SIDE
            || source_bytes == 0
            || source_sha256 == [0; 32]
            || pixels_sha256 == [0; 32]
            || pixel_length != pixel_length_for_carrier(width, height)?
        {
            return Err(invalid_carrier("font page descriptor is invalid"));
        }
        let path_end = checked_add(path_offset, path_length, invalid_carrier)?;
        let pixel_end = checked_add(pixel_offset, pixel_length, invalid_carrier)?;
        if path_length == 0
            || path_length > MAX_FONT_PATH_BYTES
            || path_end > envelope.pixels_offset
            || pixel_end > envelope.hash_offset
        {
            return Err(invalid_carrier("font page range is outside its section"));
        }
        let path = str::from_utf8(&bytes[path_offset..path_end])
            .map_err(|_| invalid_carrier("font page path is not UTF-8"))?;
        validate_source_path(path).map_err(invalid_carrier)?;
        let key = (path, source_sha256);
        if previous_page.is_some_and(|previous| previous >= key) {
            return Err(invalid_carrier(
                "font pages are not strictly source-ordered",
            ));
        }
        total_source_bytes = total_source_bytes
            .checked_add(u64::from(source_bytes))
            .ok_or_else(|| invalid_carrier("font source-byte total overflow"))?;
        expected_path_offset = path_end;
        expected_pixel_offset = pixel_end;
        previous_page = Some(key);
    }
    if expected_path_offset != envelope.pixels_offset
        || expected_pixel_offset != envelope.hash_offset
        || total_source_bytes > MAX_FONT_SOURCE_BYTES
        || envelope.hash_offset - envelope.pixels_offset > MAX_FONT_DECODED_BYTES
    {
        return Err(invalid_carrier("font page aggregate ranges are invalid"));
    }
    Ok(())
}

fn validate_glyph_records(bytes: &[u8], envelope: Envelope) -> Result<(), FontCatalogError> {
    let mut previous = None;
    for index in 0..envelope.glyph_count {
        let base = record_offset(envelope.glyph_offset, index, GLYPH_BYTES)?;
        let codepoint = u32_at(bytes, base)?;
        if char::from_u32(codepoint).is_none()
            || previous.is_some_and(|previous| previous >= codepoint)
            || usize::from(u16_at(bytes, base + 4)?) >= envelope.page_count
            || u16_at(bytes, base + 6)? != 0
            || i16_at(bytes, base + 22)? != 0
        {
            return Err(invalid_carrier("font glyph record is invalid"));
        }
        previous = Some(codepoint);
    }
    Ok(())
}

fn decode_pages(
    bytes: &[u8],
    envelope: Envelope,
) -> Result<Vec<FontTexturePage>, FontCatalogError> {
    let mut pages = Vec::with_capacity(envelope.page_count);
    for index in 0..envelope.page_count {
        let base = record_offset(envelope.page_offset, index, PAGE_BYTES)?;
        let path_offset = usize_at(bytes, base)?;
        let path_length = usize::try_from(u32_at(bytes, base + 8)?)
            .map_err(|_| invalid_carrier("font path length exceeds platform"))?;
        let pixel_offset = usize_at(bytes, base + 24)?;
        let pixel_length = usize_at(bytes, base + 32)?;
        pages.push(FontTexturePage {
            source_path: str::from_utf8(&bytes[path_offset..path_offset + path_length])
                .map_err(|_| invalid_carrier("font page path is not UTF-8"))?
                .into(),
            source_bytes: u32_at(bytes, base + 20)?,
            source_sha256: array_at(bytes, base + 40)?,
            pixels_sha256: array_at(bytes, base + 72)?,
            width: u32_at(bytes, base + 12)?,
            height: u32_at(bytes, base + 16)?,
            pixels: FontPixels::Rgba8(bytes[pixel_offset..pixel_offset + pixel_length].into()),
        });
    }
    Ok(pages)
}

fn decode_glyphs(
    bytes: &[u8],
    envelope: Envelope,
    pages: &[FontTexturePage],
) -> Result<Vec<GlyphMetrics>, FontCatalogError> {
    let mut glyphs = Vec::with_capacity(envelope.glyph_count);
    for index in 0..envelope.glyph_count {
        let base = record_offset(envelope.glyph_offset, index, GLYPH_BYTES)?;
        let page = u16_at(bytes, base + 4)?;
        let uv = [
            u16_at(bytes, base + 8)?,
            u16_at(bytes, base + 10)?,
            u16_at(bytes, base + 12)?,
            u16_at(bytes, base + 14)?,
        ];
        let texture = &pages[usize::from(page)];
        validate_uv(uv, texture.width, texture.height).map_err(invalid_carrier)?;
        glyphs.push(GlyphMetrics {
            codepoint: char::from_u32(u32_at(bytes, base)?)
                .ok_or_else(|| invalid_carrier("invalid glyph codepoint"))?,
            page,
            uv,
            bearing: [i16_at(bytes, base + 16)?, i16_at(bytes, base + 18)?],
            advance_64: i16_at(bytes, base + 20)?,
        });
    }
    Ok(glyphs)
}

fn validate_source_path(path: &str) -> Result<(), Box<str>> {
    if path.is_empty()
        || path.len() > MAX_FONT_PATH_BYTES
        || !path.starts_with("font/")
        || !path.ends_with(".png")
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err("font source path is unsafe or noncanonical".into());
    }
    Ok(())
}

fn validate_uv(uv: [u16; 4], width: u32, height: u32) -> Result<(), Box<str>> {
    if uv[0] >= uv[2] || uv[1] >= uv[3] || u32::from(uv[2]) > width || u32::from(uv[3]) > height {
        return Err("font glyph UV rectangle is invalid".into());
    }
    Ok(())
}

fn pixel_length(width: u32, height: u32) -> Result<usize, Box<str>> {
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "font pixel length overflow".into())
}

fn pixel_length_for_carrier(width: u32, height: u32) -> Result<usize, FontCatalogError> {
    pixel_length(width, height).map_err(invalid_carrier)
}

fn record_offset(start: usize, index: usize, width: usize) -> Result<usize, FontCatalogError> {
    checked_add(
        start,
        checked_mul(index, width, invalid_carrier)?,
        invalid_carrier,
    )
}

fn checked_add(
    left: usize,
    right: usize,
    error: fn(&'static str) -> FontCatalogError,
) -> Result<usize, FontCatalogError> {
    left.checked_add(right)
        .ok_or_else(|| error("font carrier offset overflow"))
}

fn checked_mul(
    left: usize,
    right: usize,
    error: fn(&'static str) -> FontCatalogError,
) -> Result<usize, FontCatalogError> {
    left.checked_mul(right)
        .ok_or_else(|| error("font carrier length overflow"))
}

fn invalid_catalog(detail: impl Into<Box<str>>) -> FontCatalogError {
    FontCatalogError::InvalidCatalog {
        detail: detail.into(),
    }
}

fn invalid_carrier(detail: impl Into<Box<str>>) -> FontCatalogError {
    FontCatalogError::InvalidCarrier {
        detail: detail.into(),
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i16(bytes: &mut Vec<u8>, value: i16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: usize) -> Result<(), FontCatalogError> {
    bytes.extend_from_slice(
        &u64::try_from(value)
            .map_err(|_| invalid_catalog("font carrier offset exceeds u64"))?
            .to_le_bytes(),
    );
    Ok(())
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, FontCatalogError> {
    Ok(u16::from_le_bytes(array_at(bytes, offset)?))
}

fn i16_at(bytes: &[u8], offset: usize) -> Result<i16, FontCatalogError> {
    Ok(i16::from_le_bytes(array_at(bytes, offset)?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, FontCatalogError> {
    Ok(u32::from_le_bytes(array_at(bytes, offset)?))
}

fn usize_at(bytes: &[u8], offset: usize) -> Result<usize, FontCatalogError> {
    usize::try_from(u64::from_le_bytes(array_at(bytes, offset)?))
        .map_err(|_| invalid_carrier("font carrier offset exceeds platform"))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], FontCatalogError> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| invalid_carrier("font carrier field offset overflow"))?;
    bytes
        .get(offset..end)
        .ok_or_else(|| invalid_carrier("truncated font carrier field"))?
        .try_into()
        .map_err(|_| invalid_carrier("invalid font carrier field"))
}
