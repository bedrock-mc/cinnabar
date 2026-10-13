//! Solid fill gradients and border colors carried separately from interpolated coverage.

/// UV stores gradient position and fill coverage; model light stores border coverage.
/// Tint stores the first fill stop; overlay stores packed second-stop and border bytes.
pub const UI_STYLE_COVERAGE_PAINT: u16 = 1 << 8;

/// Constant authored colors for a single fill and border coverage mixture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiCoveragePaint {
    pub fill: [[u8; 4]; 2],
    pub border: [u8; 4],
}

impl UiCoveragePaint {
    /// Packs two color bytes per exact float so the vertex layout does not grow.
    pub fn vertex_colors(self) -> ([u8; 4], [f32; 4]) {
        (
            self.fill[0],
            std::array::from_fn(|channel| {
                f32::from(u16::from(self.fill[1][channel]) | (u16::from(self.border[channel]) << 8))
            }),
        )
    }

    /// Recovers the constant paint colors from the tint and packed overlay fields.
    pub fn from_vertex_colors(tint: [u8; 4], overlay: [f32; 4]) -> Self {
        let bytes = overlay.map(|value| value.round() as u16);
        Self {
            fill: [tint, bytes.map(|value| value as u8)],
            border: bytes.map(|value| (value >> 8) as u8),
        }
    }

    /// Applies a normalized opacity to both gradient stops and the border, preserving their RGB.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        for color in self
            .fill
            .iter_mut()
            .chain(std::iter::once(&mut self.border))
        {
            color[3] = (f32::from(color[3]) * opacity).round() as u8;
        }
        self
    }

    /// Evaluates straight gradient RGBA before combining premultiplied fill and border areas.
    pub fn sample(self, position: f32, [fill_area, border_area]: [f32; 2]) -> [f32; 4] {
        let position = position.clamp(0.0, 1.0);
        let fill: [f32; 4] = std::array::from_fn(|channel| {
            (f32::from(self.fill[0][channel])
                + (f32::from(self.fill[1][channel]) - f32::from(self.fill[0][channel])) * position)
                / 255.0
        });
        let border = self.border.map(|value| f32::from(value) / 255.0);
        let fill_alpha = fill[3] * fill_area.clamp(0.0, 1.0);
        let border_alpha = border[3] * border_area.clamp(0.0, 1.0);
        [
            fill[0] * fill_alpha + border[0] * border_alpha,
            fill[1] * fill_alpha + border[1] * border_alpha,
            fill[2] * fill_alpha + border[2] * border_alpha,
            fill_alpha + border_alpha,
        ]
    }
}

#[cfg(test)]
mod tests;
