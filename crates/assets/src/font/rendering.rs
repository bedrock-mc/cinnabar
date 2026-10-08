/// Text shader bits shared by catalog metadata and draw submission.
pub const FONT_STYLE_COVERAGE_GAMMA: u8 = 32;
pub const FONT_STYLE_SDF: u8 = 64;

/// Runtime text coverage interpretation; serialized carriers retain ordinary coverage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontRendering {
    #[default]
    Coverage,
    NativeCoverage,
    NativeSdf,
}

impl FontRendering {
    pub const fn style_flags(self) -> u8 {
        match self {
            Self::Coverage => 0,
            Self::NativeCoverage => FONT_STYLE_COVERAGE_GAMMA,
            Self::NativeSdf => FONT_STYLE_COVERAGE_GAMMA | FONT_STYLE_SDF,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog,
    };

    #[test]
    fn native_modes_change_cache_identity_sampling_and_survive_aliases() {
        let pixels = vec![255; 4].into_boxed_slice();
        let page = FontTexturePage {
            source_path: "font/face.png".into(),
            source_bytes: 4,
            source_sha256: [1; 32],
            pixels_sha256: sha2::Sha256::digest(&pixels).into(),
            width: 1,
            height: 1,
            pixels: FontPixels::Rgba8(pixels),
        };
        let glyph = GlyphMetrics {
            codepoint: '\u{fffd}',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        };
        let bytes = encode_font_catalog([2; 32], &[glyph], &[page]).unwrap();
        let base = RuntimeFontCatalog::decode(&bytes, [2; 32]).unwrap();
        let pixel = base
            .clone()
            .with_linear_sampling()
            .with_rendering(FontRendering::NativeCoverage);
        let sdf = base
            .clone()
            .with_rendering(FontRendering::NativeSdf)
            .with_coverage_pages();
        assert!(!pixel.linear_sampling());
        assert!(sdf.linear_sampling());
        assert_ne!(base.identity(), pixel.identity());
        assert_ne!(pixel.identity(), sdf.identity());
        assert_eq!(
            sdf.identity(),
            sdf.clone()
                .with_rendering(FontRendering::NativeSdf)
                .identity()
        );
        assert!(matches!(sdf.pages()[0].pixels, FontPixels::Coverage(_)));
        let combined = base.with_named_font("native", &sdf).unwrap();
        assert_eq!(combined.rendering(), FontRendering::Coverage);
        assert_eq!(
            combined.font_named("native").rendering(),
            FontRendering::NativeSdf
        );
        assert_eq!(
            sdf.with_glyphs(&[], |_| false).rendering(),
            FontRendering::NativeSdf
        );
    }

    use sha2::Digest;
}
