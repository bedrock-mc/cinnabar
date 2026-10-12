//! Resolution of characters a face lacks through its attached fallback.

use super::*;

impl CompiledFontCatalog {
    /// Preserves the primary face's metrics and resolves missing characters through its fallback.
    pub fn glyph_source(&self, ch: char) -> Option<(&Self, GlyphMetrics)> {
        if let Some(glyph) = self.glyph(ch) {
            return Some((self, *glyph));
        }
        let fallback = self.fallback.as_ref()?;
        fallback.glyph(ch).map(|glyph| (&**fallback, *glyph))
    }
}
