//! Pinned outline metrics for the shipped semantic faces.

use crate::{FONT_RASTER_EM_PIXELS, FontLineMetrics};

#[derive(Clone, Copy, Debug)]
pub struct FontFace {
    pub name: &'static str,
    pub manifest: &'static [u8],
    pub units_per_em: u32,
    /// Source hhea ascent and positive descent, in font units.
    pub ascent: u32,
    pub descent: u32,
}

impl FontFace {
    /// Converts the source hhea metrics to the carrier's raster em.
    pub fn line_metrics(self) -> FontLineMetrics {
        let em_64 = FONT_RASTER_EM_PIXELS * 64;
        let fixed = |units| (units * em_64 + self.units_per_em / 2) / self.units_per_em;
        FontLineMetrics {
            em_64,
            ascent_64: fixed(self.ascent),
            descent_64: fixed(self.descent),
        }
    }
}
