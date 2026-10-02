//! Server glyph sheets (`font/glyph_XX.png`), packed into the trailing dynamic pages and
//! layered over the base font for every overridden code point.

use std::sync::Arc;

use assets::{CellGlyph, pack_cells};
use render::UiTexturePage;

use super::{UiPresentationRuntime, dynamic_textures};
use dynamic_textures::FIRST_GLYPH_PAGE;

use render::UI_DYNAMIC_PAGE_SIDE as PAGE_SIDE;

/// The session's glyph cells, cropped from the winning sheets.
#[derive(Debug, Default)]
pub(crate) struct SessionGlyphSheets {
    pub(crate) cells: Vec<CellGlyph>,
    pub(crate) named: std::collections::BTreeMap<String, Vec<CellGlyph>>,
    pub(crate) prepared: std::sync::OnceLock<PreparedGlyphs>,
}

#[derive(Debug)]
pub(crate) struct PreparedGlyphs {
    pages: Vec<UiTexturePage>,
    glyphs: Vec<assets::SheetGlyph>,
    named: std::collections::BTreeMap<String, Vec<assets::SheetGlyph>>,
}

impl SessionGlyphSheets {
    /// Keeps named fonts in the same bounded atlas allocation as the default font.
    pub(crate) fn with_named(
        cells: Vec<CellGlyph>,
        named: std::collections::BTreeMap<String, Vec<CellGlyph>>,
    ) -> Self {
        let sheets = Self {
            cells,
            named,
            prepared: Default::default(),
        };
        let _ = sheets.prepared();
        sheets
    }

    /// Keeps relative atlas pages reusable independently of the carrier page offset.
    fn prepared(&self) -> &PreparedGlyphs {
        self.prepared.get_or_init(|| {
            let mut atlas = pack_cells(&self.cells, 0, PAGE_SIDE, dynamic_textures::GLYPH_PAGES);
            let mut named = std::collections::BTreeMap::new();
            for (name, cells) in &self.named {
                let defaults: std::collections::BTreeMap<_, _> = self
                    .cells
                    .iter()
                    .map(|cell| (cell.codepoint, cell))
                    .collect();
                let metrics: std::collections::BTreeMap<_, _> = atlas
                    .glyphs
                    .iter()
                    .map(|glyph| (glyph.metrics.codepoint, *glyph))
                    .collect();
                let (shared, unique): (Vec<_>, Vec<_>) = cells.iter().partition(|cell| {
                    defaults
                        .get(&cell.codepoint)
                        .is_some_and(|default| *default == *cell)
                });
                let unique: Vec<_> = unique.into_iter().cloned().collect();
                let mut next = pack_cells(
                    &unique,
                    atlas.pages.len() as u16,
                    PAGE_SIDE,
                    dynamic_textures::GLYPH_PAGES.saturating_sub(atlas.pages.len()),
                );
                next.glyphs.extend(
                    shared
                        .into_iter()
                        .filter_map(|cell| metrics.get(&cell.codepoint).copied()),
                );
                atlas.pages.extend(next.pages);
                named.insert(name.clone(), next.glyphs);
            }
            PreparedGlyphs {
                glyphs: atlas.glyphs,
                named,
                pages: atlas
                    .pages
                    .into_iter()
                    .filter_map(|pixels| UiTexturePage::owned([PAGE_SIDE; 2], pixels.into()).ok())
                    .collect(),
            }
        })
    }
}

/// The packed pages for the sheets last seen on the UI runtime.
#[derive(Default)]
pub(super) struct SessionGlyphPages {
    source: Option<Arc<SessionGlyphSheets>>,
    pub(super) pages: Vec<UiTexturePage>,
}

/// Repacks the atlas and swaps the layout font when the runtime's sheet set changes identity.
pub(super) fn observe(
    runtime: &mut UiPresentationRuntime,
    sheets: Option<&Arc<SessionGlyphSheets>>,
) {
    let unchanged = match (&runtime.session_glyphs.source, sheets) {
        (Some(current), Some(next)) => Arc::ptr_eq(current, next),
        (None, None) => true,
        _ => false,
    };
    if unchanged {
        return;
    }
    let first_page = runtime.textures.dynamic_start() + FIRST_GLYPH_PAGE;
    let prepared = sheets.map(|sheets| sheets.prepared());
    let shifted = |glyphs: &[assets::SheetGlyph]| {
        glyphs
            .iter()
            .map(|glyph| {
                let mut glyph = *glyph;
                glyph.metrics.page += first_page as u16;
                glyph
            })
            .collect::<Vec<_>>()
    };
    runtime.font = prepared.map_or_else(
        || runtime.base_font.clone(),
        |atlas| {
            let default = runtime
                .base_font
                .with_glyphs(&shifted(&atlas.glyphs), |_| true);
            let named = atlas
                .named
                .iter()
                .map(|(name, glyphs)| {
                    (
                        name.clone(),
                        runtime.base_font.with_glyphs(&shifted(glyphs), |_| true),
                    )
                })
                .collect();
            Arc::new(default.with_named_fonts(named))
        },
    );
    runtime.nametag_atlas.reset();
    let pages = prepared
        .map(|atlas| atlas.pages.clone())
        .unwrap_or_default();
    runtime.session_glyphs = SessionGlyphPages {
        source: sheets.cloned(),
        pages,
    };
    dynamic_textures::rebuild(runtime);
}

impl SessionGlyphPages {
    /// Texels of UI page `page` when it is one of these glyph pages.
    pub(super) fn page(
        &self,
        runtime_dynamic_start: usize,
        page: usize,
    ) -> Option<super::nametag_atlas::GlyphPage<'_>> {
        let page = self
            .pages
            .get(page.checked_sub(runtime_dynamic_start + FIRST_GLYPH_PAGE)?)?;
        Some(super::nametag_atlas::GlyphPage {
            width: PAGE_SIDE,
            height: PAGE_SIDE,
            rgba8: page.pixels(),
        })
    }
}
