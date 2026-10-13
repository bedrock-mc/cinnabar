//! Fixed-point line geometry shared by layout and height-limited callers.

use super::{MAX_WRAP_LINES, TextError, TextLayoutRequest, units::Units, validate_request};

/// Line dimensions in the same units used to place glyphs.
#[derive(Clone, Copy)]
pub(super) struct LineMetrics {
    pub units: Units,
    pub height_64: i64,
    pub pitch_64: i64,
    pub baseline_64: i64,
}

impl LineMetrics {
    /// Resolves scale quantization, device units and line padding once.
    pub(super) fn new(request: TextLayoutRequest<'_>) -> Result<Self, TextError> {
        let units = Units::select(
            i64::from(request.scale.fixed_1024()),
            request.wrap.device_scale_65536,
            request.font.line_metrics().is_some(),
        );
        let height_64 = units.texels(i64::from(request.line_height_64))?;
        let padding_64 = units.output_to_layout(i64::from(request.wrap.line_padding_64))?;
        Ok(Self {
            units,
            height_64,
            pitch_64: (height_64 + padding_64).max(1),
            baseline_64: units.texels(i64::from(request.baseline_64))?,
        })
    }

    /// Returns a block's output height, with padding only between lines.
    pub(super) fn block_height_64(self, lines: i64) -> Result<i64, TextError> {
        self.units
            .to_output(self.height_64 + (lines - 1) * self.pitch_64)
    }
}

impl TextLayoutRequest<'_> {
    /// Counts whole line boxes within an output height, up to the layout's line limit.
    /// Uses the same fixed-point pitch and final rounding as layout; glyph overhangs
    /// may extend beyond these nominal boxes.
    pub fn max_lines_in_height(self, height_64: u32) -> Result<u16, TextError> {
        validate_request(self)?;
        let metrics = LineMetrics::new(self)?;
        let (mut low, mut high) = (0, MAX_WRAP_LINES as u16);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if metrics.block_height_64(i64::from(middle))? <= i64::from(height_64) {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        Ok(low)
    }
}
