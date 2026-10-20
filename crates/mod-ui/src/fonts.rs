//! Shared typography for authored mod surfaces, independent of server glyph overrides.

use assets::RuntimeFontCatalog;
use std::sync::{Arc, OnceLock};

/// Adds the shipped Inter face while preserving all existing default and named fonts.
/// Rasterization happens once during UI initialization, before any frame is painted.
pub fn install(base: Arc<RuntimeFontCatalog>) -> Result<Arc<RuntimeFontCatalog>, String> {
    if base
        .named_fonts()
        .contains_key(ui::mod_panel::SANS_FONT_NAME)
    {
        return Ok(base);
    }
    static SANS: OnceLock<Result<RuntimeFontCatalog, String>> = OnceLock::new();
    let sans = SANS.get_or_init(|| {
        assets::rasterize_outline_font(
            include_bytes!("../../../assets/fonts/InterVariable.ttf"),
            18.0,
            2,
            render_model::UI_LOCAL_FONT_PAGE_SIDE,
        )
    });
    let sans = sans.as_ref().map_err(Clone::clone)?;
    base.with_named_font(ui::mod_panel::SANS_FONT_NAME, sans)
        .map(Arc::new)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_sans_has_antialiased_texels_and_is_installed_only_once() {
        let source = assets::rasterize_outline_font(
            include_bytes!("../../../assets/fonts/InterVariable.ttf"),
            18.0,
            2,
            render_model::UI_LOCAL_FONT_PAGE_SIDE,
        )
        .unwrap();
        let base = Arc::new(source);
        let font = install(Arc::clone(&base)).unwrap();
        assert_eq!(font.glyph('A'), base.glyph('A'));
        let sans = font.font_named(ui::mod_panel::SANS_FONT_NAME);
        assert!(sans.linear_sampling());
        for character in ['A', 'g', 'y'] {
            let glyph = sans.glyph(character).unwrap();
            let page = &sans.pages()[glyph.page as usize];
            let [left, top, right, bottom] = glyph.uv.map(usize::from);
            for y in top..bottom {
                for x in left..right {
                    if x == left || x == right - 1 || y == top || y == bottom - 1 {
                        assert_eq!(
                            page.pixels.texel(y * page.width as usize + x).unwrap()[3],
                            0,
                            "filtered glyph {character} needs transparent room around its ink"
                        );
                    }
                }
            }
        }
        for page in sans.pages() {
            for index in 0..(page.width as usize * page.height as usize) {
                assert_eq!(
                    page.pixels.texel(index).unwrap()[..3],
                    [255; 3],
                    "filtered monochrome glyphs need white transparent gutters"
                );
            }
        }
        assert!(sans.pages().iter().any(|page| {
            (0..(page.width as usize * page.height as usize))
                .any(|index| (1..255).contains(&page.pixels.texel(index).unwrap()[3]))
        }));
        assert!(Arc::ptr_eq(&font, &install(Arc::clone(&font)).unwrap()));
    }
}
