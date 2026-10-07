//! A replaceable private font slot independent of immutable and server-owned UI pages.

use std::sync::{Arc, OnceLock};

use assets::{RuntimeFontCatalog, SheetGlyph};
use render_model::{UI_LOCAL_FONT_PAGE_OFFSET, UI_LOCAL_FONT_PAGE_SIDE, UiTexturePage};

use super::{UiPresentationError, UiPresentationRuntime, session_glyphs};

const MAX_PRIVATE_GLYPHS: usize = 256;

pub(super) struct InstalledFont {
    source: Arc<RuntimeFontCatalog>,
    pub(super) alias: RuntimeFontCatalog,
    pub(super) page: UiTexturePage,
}

pub(super) fn blank_page() -> UiTexturePage {
    static BLANK: OnceLock<UiTexturePage> = OnceLock::new();
    BLANK
        .get_or_init(|| {
            let side = UI_LOCAL_FONT_PAGE_SIDE;
            UiTexturePage::owned([side; 2], vec![0; side as usize * side as usize * 4].into())
                .expect("bounded private-font page has a valid extent")
        })
        .clone()
}

impl UiPresentationRuntime {
    /// Replaces only the personal panel's font; carrier glyphs and session page slots stay fixed.
    /// Sources must be prepared off-thread as a single bounded outline atlas.
    pub fn set_mod_panel_font(
        &mut self,
        font: Option<Arc<RuntimeFontCatalog>>,
    ) -> Result<(), UiPresentationError> {
        let unchanged = match (&self.mod_panel_font, &font) {
            (None, None) => true,
            (Some(current), Some(next)) => current.source.identity() == next.identity(),
            _ => false,
        };
        if unchanged {
            return Ok(());
        }
        let first = self.textures.dynamic_start() + UI_LOCAL_FONT_PAGE_OFFSET;
        let installed = font.map(|source| prepare(source, first)).transpose()?;
        let page = installed
            .as_ref()
            .map_or_else(blank_page, |font| font.page.clone());
        let mut dynamic = self.textures.pages()[self.textures.dynamic_start()..].to_vec();
        dynamic[UI_LOCAL_FONT_PAGE_OFFSET] = page;
        let textures = self
            .textures
            .replace_dynamic(dynamic)
            .map_err(|_| UiPresentationError::InvalidFontTexture)?;
        self.mod_panel_font = installed;
        self.textures = Arc::new(textures);
        self.font = session_glyphs::font(self);
        self.invalidate_mod_panel_font();
        Ok(())
    }
}

fn prepare(
    source: Arc<RuntimeFontCatalog>,
    page: usize,
) -> Result<InstalledFont, UiPresentationError> {
    if source.pages().len() != 1
        || source.glyphs().len() > MAX_PRIVATE_GLYPHS
        || !source.named_fonts().is_empty()
    {
        return Err(UiPresentationError::InvalidFontTexture);
    }
    let page = u16::try_from(page).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    let texture = &source.pages()[0];
    // The reserved slot stays RGBA, so installing a font never moves it to another bucket.
    if [texture.width, texture.height] != [UI_LOCAL_FONT_PAGE_SIDE; 2]
        || texture.pixels.rgba8().is_none()
    {
        return Err(UiPresentationError::InvalidFontTexture);
    }
    if source.glyphs().iter().any(|glyph| {
        glyph.page != 0
            || glyph.uv[0] >= glyph.uv[2]
            || glyph.uv[1] >= glyph.uv[3]
            || u32::from(glyph.uv[2]) > UI_LOCAL_FONT_PAGE_SIDE
            || u32::from(glyph.uv[3]) > UI_LOCAL_FONT_PAGE_SIDE
            || source.draw_size_64(glyph.codepoint).is_some_and(|size| {
                size.into_iter()
                    .any(|side| side > UI_LOCAL_FONT_PAGE_SIDE * 64)
            })
    }) {
        return Err(UiPresentationError::InvalidFontTexture);
    }
    let shifted = source
        .glyphs()
        .iter()
        .map(|glyph| {
            let mut metrics = *glyph;
            metrics.page = page;
            SheetGlyph {
                metrics,
                draw_size_64: source.draw_size_64(glyph.codepoint).unwrap_or([
                    u32::from(glyph.uv[2] - glyph.uv[0]) * 64,
                    u32::from(glyph.uv[3] - glyph.uv[1]) * 64,
                ]),
            }
        })
        .collect::<Vec<_>>();
    let alias = source.with_glyphs(&shifted, |_| true);
    let page = UiTexturePage::font(Arc::clone(&source), 0)
        .map_err(|_| UiPresentationError::InvalidFontTexture)?;
    Ok(InstalledFont {
        source,
        alias,
        page,
    })
}

#[cfg(test)]
mod tests;
