//! Optional local typography for the explicitly granted personal controls panel.

use std::path::Path;

use assets::RuntimeFontCatalog;

/// Optional font selected for the personal controls panel.
pub const FONT_ENV: &str = "CINNABAR_MOD_FONT";
/// Upper bound on the local font source read by the registration worker.
pub const MAX_SOURCE_BYTES: usize = mod_api::MAX_PANEL_FONT_BYTES;
const ATLAS_SIDE: u32 = render_model::UI_LOCAL_FONT_PAGE_SIDE;
const FONT_EM: f32 = 18.0;
const RASTER_SCALE: u32 = 2;

/// Reads and rasterizes a bounded local source on the registration worker.
pub fn load(path: &Path) -> Result<RuntimeFontCatalog, String> {
    let bytes = mod_host::local_files::read(path, MAX_SOURCE_BYTES)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    rasterize(&bytes)
}

/// Rasterizes a bounded in-memory font source at the panel density.
pub fn rasterize(bytes: &[u8]) -> Result<RuntimeFontCatalog, String> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("font source exceeds local byte limit".into());
    }
    assets::rasterize_outline_font(bytes, FONT_EM, RASTER_SCALE, ATLAS_SIDE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn invalid_and_oversized_optional_fonts_fail_before_attachment() {
        assert!(rasterize(b"not a font").is_err());
        assert!(
            rasterize(&vec![0; MAX_SOURCE_BYTES + 1])
                .unwrap_err()
                .contains("byte limit")
        );
        let path =
            std::env::temp_dir().join(format!("cinnabar-optional-font-{}.ttf", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len((MAX_SOURCE_BYTES + 1) as u64).unwrap();
        assert!(load(&path).unwrap_err().contains("byte limit"));
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn supplied_font_fixture_is_antialiased_and_fits_one_private_page() {
        let Some(path) = std::env::var_os(FONT_ENV) else {
            eprintln!(
                "skipping supplied_font_fixture_is_antialiased_and_fits_one_private_page: fixture unavailable; set CINNABAR_MOD_FONT to a local outline font"
            );
            return;
        };
        let font = load(Path::new(&path)).unwrap();
        assert!(font.linear_sampling());
        assert_eq!(font.pages().len(), 1);
        assert_eq!(
            [font.pages()[0].width, font.pages()[0].height],
            [ATLAS_SIDE; 2]
        );
        assert!(font.glyph('A').is_some_and(|glyph| glyph.advance_64 > 0));
        let space = font.glyph(' ').unwrap();
        assert!(space.advance_64 > 0);
        assert_eq!(
            [space.uv[2] - space.uv[0], space.uv[3] - space.uv[1]],
            [1; 2]
        );
        let texel = (usize::from(space.uv[1]) * ATLAS_SIDE as usize + usize::from(space.uv[0])) * 4;
        assert_eq!(font.pages()[0].pixels.bytes()[texel + 3], 0);
        let attached = font
            .with_named_font(ui::mod_panel::FONT_NAME, &font)
            .unwrap();
        assert_eq!(attached.glyph('A').unwrap().page, 0);
        assert_eq!(
            attached
                .font_named(ui::mod_panel::FONT_NAME)
                .glyph('A')
                .unwrap()
                .page,
            1
        );
        assert!(
            font.pages()[0]
                .pixels
                .bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| (1..255).contains(&pixel[3]))
        );
    }

    #[test]
    fn supplied_font_density_changes_raster_without_changing_logical_layout() {
        let Some(path) = std::env::var_os(FONT_ENV) else {
            eprintln!(
                "skipping supplied_font_density_changes_raster_without_changing_logical_layout: fixture unavailable; set CINNABAR_MOD_FONT to a local outline font"
            );
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let native = assets::rasterize_outline_font(&bytes, FONT_EM, 1, ATLAS_SIDE).unwrap();
        let dense =
            assets::rasterize_outline_font(&bytes, FONT_EM, RASTER_SCALE, ATLAS_SIDE).unwrap();
        for codepoint in "Ag1 Cinnaroids".chars() {
            let native_glyph = native.glyph(codepoint).unwrap();
            let dense_glyph = dense.glyph(codepoint).unwrap();
            assert_eq!(native_glyph.bearing, dense_glyph.bearing);
            assert_eq!(native_glyph.advance_64, dense_glyph.advance_64);
            assert_eq!(
                native.draw_size_64(codepoint),
                dense.draw_size_64(codepoint)
            );
            if codepoint != ' ' {
                assert_eq!(
                    dense_glyph.uv[2] - dense_glyph.uv[0],
                    (native_glyph.uv[2] - native_glyph.uv[0]) * RASTER_SCALE as u16
                );
            }
        }
        let mut cache = ui::TextLayoutCache::new(8, 64 * 1024);
        for scale in [1.0, 1.5, 2.0] {
            let request = |font| ui::TextLayoutRequest {
                text: "Ag1 Cinnaroids",
                style: ui::TextStyle::default(),
                width_64: 400 * 64,
                line_height_64: ui::TEXT_LINE_HEIGHT_64,
                baseline_64: ui::TEXT_BASELINE_64,
                scale: ui::UiScale::new_display(scale).unwrap(),
                font,
                wrap: Default::default(),
            };
            let native_layout = cache.layout(request(&native)).unwrap();
            let dense_layout = cache.layout(request(&dense)).unwrap();
            assert_eq!(native_layout.size_64(), dense_layout.size_64());
            for (native, dense) in native_layout.glyphs().iter().zip(dense_layout.glyphs()) {
                assert_eq!(native.bounds_64, dense.bounds_64);
            }
            assert!(!Arc::ptr_eq(&native_layout, &dense_layout));
        }
    }
}
