//! Optional local typography for the explicitly granted personal controls panel.

use std::{fs::File, io::Read, path::Path};

use assets::{
    CellGlyph, FontPixels, FontTexturePage, RuntimeFontCatalog, encode_font_catalog, pack_cells,
};
use sha2::{Digest, Sha256};

pub(super) const FONT_ENV: &str = "CINNABAR_MOD_FONT";
pub(super) const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const ATLAS_SIDE: u32 = render_model::UI_LOCAL_FONT_PAGE_SIDE;
const FONT_EM: f32 = 18.0;
const RASTER_SCALE: u32 = 2;

/// Reads and rasterizes a bounded local source on the registration worker.
pub(crate) fn load(path: &Path) -> Result<RuntimeFontCatalog, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("font source exceeds local byte limit".into());
    }
    rasterize(&bytes)
}

pub(super) fn rasterize(bytes: &[u8]) -> Result<RuntimeFontCatalog, String> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("font source exceeds local byte limit".into());
    }
    rasterize_at(bytes, RASTER_SCALE)
}

fn rasterize_at(bytes: &[u8], raster_scale: u32) -> Result<RuntimeFontCatalog, String> {
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(str::to_owned)?;
    let mut cells = Vec::new();
    let characters = (0x20..=0x7e).filter_map(char::from_u32).chain([
        '\u{b7}', '\u{d7}', '\u{2014}', '\u{2022}', '\u{2039}', '\u{203a}', '\u{2212}', '\u{fffd}',
    ]);
    for codepoint in characters {
        let source = if font.lookup_glyph_index(codepoint) == 0 {
            '?'
        } else {
            codepoint
        };
        let metrics = font.metrics(source, FONT_EM);
        if metrics.width > 64 || metrics.height > 64 || !metrics.advance_width.is_finite() {
            return Err("font glyph dimensions exceed local bounds".into());
        }
        // Empty glyphs keep their advance and get a transparent texel with valid carrier UVs.
        let empty = metrics.width == 0 || metrics.height == 0;
        let logical_size = if empty {
            [1, 1]
        } else {
            [metrics.width as u32, metrics.height as u32]
        };
        let size = if empty {
            [1, 1]
        } else {
            logical_size.map(|size| size * raster_scale)
        };
        let mut rgba8 = vec![255; size[0] as usize * size[1] as usize * 4];
        for pixel in rgba8.chunks_exact_mut(4) {
            pixel[3] = 0;
        }
        if !empty {
            let (dense, alpha) = font.rasterize(source, FONT_EM * raster_scale as f32);
            let left = dense.xmin - metrics.xmin * raster_scale as i32;
            let top = (metrics.ymin + metrics.height as i32) * raster_scale as i32
                - (dense.ymin + dense.height as i32);
            if left < 0
                || top < 0
                || left as usize + dense.width > size[0] as usize
                || top as usize + dense.height > size[1] as usize
            {
                return Err("font raster exceeds logical glyph bounds".into());
            }
            for y in 0..dense.height {
                for x in 0..dense.width {
                    let target =
                        (((top as usize + y) * size[0] as usize + left as usize + x) * 4) + 3;
                    rgba8[target] = alpha[y * dense.width + x];
                }
            }
        }
        cells.push(CellGlyph {
            codepoint,
            size,
            rgba8: rgba8.into(),
            bearing: [
                metrics.xmin as i16,
                -(metrics.ymin + metrics.height as i32) as i16,
            ],
            advance_64: (metrics.advance_width * 64.0)
                .round()
                .clamp(0.0, i16::MAX as f32) as i16,
            draw_size_64: logical_size.map(|value| value * 64),
        });
    }
    let atlas = pack_cells(&cells, 0, ATLAS_SIDE, 1);
    if atlas.glyphs.len() != cells.len() || atlas.pages.len() != 1 {
        return Err("font exceeds local atlas bounds".into());
    }
    let source_hash: [u8; 32] = Sha256::digest(bytes).into();
    let pages = atlas
        .pages
        .into_iter()
        .map(|pixels| FontTexturePage {
            source_path: "font/personal-panel.png".into(),
            source_bytes: bytes.len() as u32,
            source_sha256: source_hash,
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: ATLAS_SIDE,
            height: ATLAS_SIDE,
            pixels: FontPixels::Rgba8(pixels),
        })
        .collect::<Vec<_>>();
    let glyphs = atlas
        .glyphs
        .iter()
        .map(|glyph| glyph.metrics)
        .collect::<Vec<_>>();
    let encoded =
        encode_font_catalog(source_hash, &glyphs, &pages).map_err(|error| error.to_string())?;
    RuntimeFontCatalog::decode(&encoded, source_hash)
        .map(|font| {
            font.with_glyphs(&atlas.glyphs, |_| true)
                .with_linear_sampling()
        })
        .map_err(|error| error.to_string())
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
                .chunks_exact(4)
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
        let native = rasterize_at(&bytes, 1).unwrap();
        let dense = rasterize_at(&bytes, RASTER_SCALE).unwrap();
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
