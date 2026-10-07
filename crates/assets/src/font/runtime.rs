use super::*;

impl CompiledFontCatalog {
    /// Adds runtime font aliases without changing the pinned carrier format.
    pub fn with_named_fonts(mut self, fonts: BTreeMap<String, Self>) -> Self {
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        for (name, font) in &fonts {
            hash.update(name.as_bytes());
            hash.update(font.identity.carrier_sha256);
        }
        self.identity.carrier_sha256 = hash.finalize().into();
        self.named = Arc::new(fonts);
        self
    }

    /// Attaches runtime pages without replacing default HUD glyphs.
    pub fn with_named_font(&self, name: &str, font: &Self) -> Result<Self, FontCatalogError> {
        self.with_named_font_sizes(name, font, &BTreeMap::new())
    }

    /// Attaches exact raster-size alternatives and rebases every glyph into the combined pages.
    pub fn with_named_font_sizes(
        &self,
        name: &str,
        font: &Self,
        sizes: &BTreeMap<u32, Self>,
    ) -> Result<Self, FontCatalogError> {
        if name.is_empty()
            || name.len() > MAX_FONT_PATH_BYTES
            || sizes.iter().any(|(&size, font)| {
                size == 0
                    || size > MAX_FONT_PAGE_SIDE
                    || font.rendering != FontRendering::NativeCoverage
                    || font
                        .line_metrics
                        .is_none_or(|metrics| metrics.em_64 != size * 64)
            })
        {
            return Err(invalid_catalog("named font sizes exceed catalog bounds"));
        }
        let mut pages = self.pages.to_vec();
        let mut alias = append(font, &mut pages)?;
        let mut alternatives = BTreeMap::new();
        for (&size, font) in sizes {
            let alternative = append(font, &mut pages)?;
            alternatives.insert(size, alternative);
        }
        let mut hash = Sha256::new();
        hash.update(alias.identity.carrier_sha256);
        for (&size, alternative) in &alternatives {
            hash.update(size.to_le_bytes());
            hash.update(alternative.identity.carrier_sha256);
        }
        alias.identity.carrier_sha256 = hash.finalize().into();
        alias.sizes = Arc::new(alternatives);
        let pages: Arc<[FontTexturePage]> = pages.into();
        share_pages(&mut alias, &pages);
        let mut aliases = (*self.named).clone();
        for alias in aliases.values_mut() {
            share_pages(alias, &pages);
        }
        aliases.insert(name.into(), alias);
        let mut result = self.clone();
        result.pages = pages;
        Ok(result.with_named_fonts(aliases))
    }

    pub fn named_fonts(&self) -> &BTreeMap<String, Self> {
        &self.named
    }

    /// Unknown aliases use the default font, as do callers without a font selection.
    pub fn font_named(&self, name: &str) -> &Self {
        self.named.get(name).unwrap_or(self)
    }

    /// Selects the exact requested raster size without allocating during layout.
    pub fn font_named_at_size(&self, name: &str, physical_pixels: f32) -> &Self {
        let font = self.font_named(name);
        if physical_pixels.is_finite() && physical_pixels > 0.0 {
            font.sizes.get(&(physical_pixels as u32)).unwrap_or(font)
        } else {
            font
        }
    }
}

pub(super) fn append(
    font: &CompiledFontCatalog,
    pages: &mut Vec<FontTexturePage>,
) -> Result<CompiledFontCatalog, FontCatalogError> {
    let bytes = pages
        .iter()
        .chain(font.pages.iter())
        .try_fold(0usize, |total, page| {
            total.checked_add(page.pixels.bytes().len())
        });
    if pages.len() + font.pages.len() > MAX_FONT_PAGES
        || bytes.is_none_or(|bytes| bytes > MAX_FONT_DECODED_BYTES)
    {
        return Err(invalid_catalog("named font pages exceed decoded bounds"));
    }
    let offset = u16::try_from(pages.len())
        .map_err(|_| invalid_catalog("named font page offset exceeds bounds"))?;
    let mut alias = font.clone();
    if !alias.sizes.is_empty() {
        return Err(invalid_catalog(
            "nested raster-size alternatives are unsupported",
        ));
    }
    for glyph in alias.glyphs.iter_mut() {
        glyph.page = glyph
            .page
            .checked_add(offset)
            .ok_or_else(|| invalid_catalog("named font page offset exceeds bounds"))?;
    }
    let mut identity = Sha256::new();
    identity.update(font.identity.carrier_sha256);
    identity.update(offset.to_le_bytes());
    alias.identity.carrier_sha256 = identity.finalize().into();
    pages.extend_from_slice(&font.pages);
    Ok(alias)
}

pub(super) fn share_pages(font: &mut CompiledFontCatalog, pages: &Arc<[FontTexturePage]>) {
    font.pages = Arc::clone(pages);
    for alternative in Arc::make_mut(&mut font.sizes).values_mut() {
        alternative.pages = Arc::clone(pages);
    }
}
