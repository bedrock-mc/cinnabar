//! Pinned outline metrics for the shipped semantic faces.

use crate::FontLineMetrics;

/// Font units in one source-grid texel of every shipped pixel face.
const PIXEL_GRID_UNITS: u32 = 64;

/// Added ASCII left bearings in atlas texels; closing applies to `)` and `}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiBearing {
    pub regular: i16,
    pub closing: i16,
}

impl AsciiBearing {
    /// Returns the added bearing for printable ASCII, leaving other source glyphs alone.
    pub const fn offset(self, codepoint: char) -> i16 {
        match codepoint {
            ')' | '}' => self.closing,
            '!'..='~' => self.regular,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FontFace {
    pub name: &'static str,
    pub manifest: &'static [u8],
    pub units_per_em: u32,
    /// Semantic space width in thousandths of an em; None retains the source advance.
    pub space_em_1000: Option<u32>,
    pub ascii_bearing: Option<AsciiBearing>,
    /// Source hhea ascent and positive descent, in font units.
    pub ascent: u32,
    pub descent: u32,
}

impl FontFace {
    /// Raster em that puts every 64-unit source-grid texel on one atlas texel.
    pub const fn raster_em_pixels(self) -> u32 {
        self.units_per_em / PIXEL_GRID_UNITS
    }

    /// Converts the semantic space width to the carrier's fixed-point pen units.
    pub const fn space_advance_64(self) -> Option<i16> {
        match self.space_em_1000 {
            Some(width) => Some(((width * self.raster_em_pixels() * 64 + 500) / 1000) as i16),
            None => None,
        }
    }

    /// Converts the source hhea metrics to the carrier's raster em.
    pub fn line_metrics(self) -> FontLineMetrics {
        let em_64 = self.raster_em_pixels() * 64;
        let fixed = |units| (units * em_64 + self.units_per_em / 2) / self.units_per_em;
        FontLineMetrics {
            em_64,
            ascent_64: fixed(self.ascent),
            descent_64: fixed(self.descent),
        }
    }
}
