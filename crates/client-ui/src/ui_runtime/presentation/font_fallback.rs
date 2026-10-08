//! Installs off-thread fallback results into stable reserved texture slots.

use super::{UiPresentationRuntime, dynamic_textures, session_glyphs};
use render_model::{MAX_UI_FALLBACK_FONT_PAGES, UI_FALLBACK_FONT_PAGE_OFFSET, UiTexturePage};
use std::sync::Arc;
use view_presentation::ui_atlas::blank_fallback_font_page;

impl UiPresentationRuntime {
    pub(super) fn poll_font_fallback(&mut self) {
        let Some(requests) = self.base_font.glyph_requests().cloned() else {
            return;
        };
        let Some(font) = requests.take_ready() else {
            return;
        };
        if self
            .fallback_font
            .as_ref()
            .is_some_and(|current| current.identity() == font.identity())
        {
            return;
        }
        if font.pages().len() > MAX_UI_FALLBACK_FONT_PAGES {
            return;
        }
        let first = self.textures.dynamic_start() + UI_FALLBACK_FONT_PAGE_OFFSET;
        let Ok(alias) = font.with_page_offset(first as u16) else {
            return;
        };
        let mut pages = Vec::new();
        for index in 0..font.pages().len() {
            let Ok(page) = UiTexturePage::font(Arc::clone(&font), index) else {
                return;
            };
            pages.push(page);
        }
        let mut dynamic = self.textures.pages()[self.textures.dynamic_start()..].to_vec();
        for offset in 0..MAX_UI_FALLBACK_FONT_PAGES {
            dynamic[UI_FALLBACK_FONT_PAGE_OFFSET + offset] = pages
                .get(offset)
                .cloned()
                .unwrap_or_else(blank_fallback_font_page);
        }
        let Ok(textures) = self.textures.replace_dynamic(dynamic) else {
            return;
        };
        self.base_font = Arc::new(
            self.base_font
                .with_named_fallback(Arc::new(alias), requests),
        );
        self.fallback_font = Some(font);
        self.textures = Arc::new(textures);
        self.font = session_glyphs::font(self);
        self.last_frame = None;
        self.nametag_atlas.reset();
        dynamic_textures::rebuild(self);
    }
}

pub(super) fn pages(runtime: &UiPresentationRuntime) -> impl Iterator<Item = UiTexturePage> + '_ {
    (0..MAX_UI_FALLBACK_FONT_PAGES).map(|index| {
        runtime
            .fallback_font
            .as_ref()
            .and_then(|font| {
                (index < font.pages().len())
                    .then(|| UiTexturePage::font(Arc::clone(font), index).ok())
                    .flatten()
            })
            .unwrap_or_else(blank_fallback_font_page)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::{
        FontGlyphRequests, FontLineMetrics, FontPixels, FontRendering, FontTexturePage,
        GlyphMetrics, RuntimeFontCatalog, encode_font_catalog,
    };
    use render_model::UI_FALLBACK_FONT_PAGE_SIDE;
    use sha2::{Digest, Sha256};

    fn fallback(ch: char) -> RuntimeFontCatalog {
        let side = UI_FALLBACK_FONT_PAGE_SIDE;
        let pixels = vec![255; (side * side * 4) as usize].into_boxed_slice();
        let hash = Sha256::digest(&pixels).into();
        let page = FontTexturePage {
            source_path: "font/fixture.png".into(),
            source_bytes: pixels.len() as u32,
            source_sha256: hash,
            pixels_sha256: hash,
            width: side,
            height: side,
            pixels: FontPixels::Rgba8(pixels),
        };
        let glyph = GlyphMetrics {
            codepoint: ch,
            page: 0,
            uv: [0, 0, 4, 8],
            bearing: [0, -8],
            advance_64: 8 * 64,
        };
        let bytes = encode_font_catalog([17; 32], &[glyph], &[page]).unwrap();
        RuntimeFontCatalog::decode(&bytes, [17; 32])
            .unwrap()
            .with_line_metrics(FontLineMetrics {
                em_64: 52 * 64,
                ascent_64: 40 * 64,
                descent_64: 12 * 64,
            })
            .unwrap()
            .with_rendering(FontRendering::NativeSdf)
            .with_coverage_pages()
    }

    #[test]
    fn published_fallback_keeps_static_pages_and_unchanged_payload_ownership() {
        let original = super::super::tests::fixture_font();
        let primary = (*original)
            .clone()
            .with_line_metrics(FontLineMetrics {
                em_64: 32 * 64,
                ascent_64: 24 * 64,
                descent_64: 8 * 64,
            })
            .unwrap();
        let requests = Arc::new(FontGlyphRequests::default());
        let source = original
            .with_named_font("body", &primary)
            .unwrap()
            .with_shared_fallback(&fallback('日'), Arc::clone(&requests))
            .unwrap();
        let mut runtime = UiPresentationRuntime::new(Arc::new(source)).unwrap();
        let static_identity = runtime.textures.static_identity();
        let default_glyph = *runtime.font.glyph('0').unwrap();
        requests.publish(Arc::new(fallback('한')));
        runtime.set_menu_view(None);
        let (_, glyph) = runtime.font.font_named("body").glyph_source('한').unwrap();
        assert_eq!(
            usize::from(glyph.page),
            runtime.textures.dynamic_start() + UI_FALLBACK_FONT_PAGE_OFFSET
        );
        assert_eq!(
            runtime.textures.pages()[usize::from(glyph.page)].format(),
            render_model::UiTextureFormat::Coverage
        );
        assert_eq!(runtime.textures.static_identity(), static_identity);
        assert_eq!(runtime.font.glyph('0'), Some(&default_glyph));
        let textures = Arc::clone(&runtime.textures);
        requests.publish(Arc::clone(runtime.fallback_font.as_ref().unwrap()));
        runtime.set_menu_view(None);
        assert!(Arc::ptr_eq(&textures, &runtime.textures));
        dynamic_textures::rebuild(&mut runtime);
        assert_eq!(runtime.textures.identity(), textures.identity());
    }
}
