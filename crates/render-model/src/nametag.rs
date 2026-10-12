//! World-space actor name tags: camera-facing billboards of backing plates and rasterized text,
//! laid out in font pixels as the vanilla name-tag renderer does.
use std::sync::Arc;

/// World size of one font pixel on a tag (vanilla scales the tag by 1.6 / 60).
pub const NAMETAG_BLOCKS_PER_FONT_PIXEL: f32 = 1.6 * (1.0 / 60.0);
pub const NAMETAG_ACOS_LINEAR: f32 = 0.87266463;
pub const NAMETAG_ACOS_CUBIC: f32 = -0.698_131_7;
pub const NAMETAG_HORIZONTAL_ZERO: f32 = 0.0001;
pub const NAMETAG_TEXT_REVERSE_Z_BIAS: i32 = 32;
/// Side of the square RGBA8 text atlas.
pub const NAMETAG_ATLAS_SIDE: u32 = 2048;
/// Most plate and text quads drawn in one frame.
pub const MAX_NAMETAG_RECORDS: usize = 1024;
/// Font-pixel distance between stacked lines; each line's background is exactly this tall.
pub const NAMETAG_LINE_PITCH_PX: f32 = 10.0;
/// Background margin left, right and above a line's centred text, in font pixels.
const NAMETAG_BACKGROUND_PADDING_PX: f32 = 1.0;

/// Left edge of a line centred on the anchor: its width halved with integer truncation, negated.
#[must_use]
pub fn nametag_line_left(width_px: f32) -> f32 {
    -((width_px as u32 / 2) as f32)
}

/// One `[x0, y0, x1, y1]` background per line, each padded around its own centred width and
/// stacked edge to edge. A lone line under two font pixels wide gets none.
pub fn nametag_line_backgrounds(
    widths_px: impl ExactSizeIterator<Item = f32>,
) -> impl Iterator<Item = [f32; 4]> {
    let multiline = widths_px.len() > 1;
    widths_px.enumerate().filter_map(move |(index, width)| {
        let half = -nametag_line_left(width);
        let top = NAMETAG_LINE_PITCH_PX * index as f32 - NAMETAG_BACKGROUND_PADDING_PX;
        (multiline || half > 0.0).then_some([
            -(half + NAMETAG_BACKGROUND_PADDING_PX),
            top,
            half + NAMETAG_BACKGROUND_PADDING_PX,
            top + NAMETAG_LINE_PITCH_PX,
        ])
    })
}

/// One quad of a tag as the GPU reads it: a plate when `uv[2] < 0`, else a text line.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NametagRecord {
    /// World point the tag's font-pixel origin hangs at.
    pub anchor: [f32; 3],
    /// 1 for depth-writing glyphs; 0 for a read-only backing plate.
    pub text: u32,
    /// `[x0, y0, x1, y1]` in font pixels; +x is the viewer's right, +y is down.
    pub rect: [f32; 4],
    /// Normalized atlas rect `[u0, v0, u1, v1]`; `u1 < 0` draws the flat colour.
    pub uv: [f32; 4],
    /// Straight-alpha RGBA multiplier.
    pub color: [f32; 4],
    /// Multiline translation, applied after computing rotation from the base anchor.
    pub line_lift: f32,
    pub padding: [u32; 3],
}

impl NametagRecord {
    /// The record's world quad, using the same native eye-facing transform as the GPU shader.
    /// Returns `None` for degenerate/non-finite remote anchors; local coordinates are font pixels.
    #[must_use]
    pub fn world_corners(&self, eye: [f32; 3]) -> Option<[[f32; 3]; 4]> {
        use glam::{Quat, Vec3};
        let anchor = Vec3::from_array(self.anchor);
        let eye = Vec3::from_array(eye);
        let direction = eye - anchor;
        if !anchor.is_finite()
            || !eye.is_finite()
            || direction == Vec3::ZERO
            || !direction.length_squared().is_finite()
        {
            return None;
        }
        let x = if direction.x == 0.0 {
            NAMETAG_HORIZONTAL_ZERO
        } else {
            direction.x
        };
        let z = if direction.z == 0.0 {
            NAMETAG_HORIZONTAL_ZERO
        } else {
            direction.z
        };
        let horizontal = (x * x + z * z).sqrt();
        let acos = |value: f32| {
            (NAMETAG_ACOS_CUBIC * value * value * value - NAMETAG_ACOS_LINEAR * value)
                + std::f32::consts::FRAC_PI_2
        };
        let yaw = acos(-z / horizontal) * if x > 0.0 { -1.0 } else { 1.0 };
        let pitch_dot =
            (direction.x * (x / horizontal) + direction.z * (z / horizontal)) / direction.length();
        let pitch = acos(pitch_dot) * if direction.y > 0.0 { 1.0 } else { -1.0 };
        let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
        let [x0, y0, x1, y1] = self.rect;
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|[x, y]| {
            (anchor
                + Vec3::Y * self.line_lift
                + rotation * Vec3::new(-x, -y, 0.0) * NAMETAG_BLOCKS_PER_FONT_PIXEL)
                .to_array()
        });
        corners
            .iter()
            .flatten()
            .all(|value| value.is_finite())
            .then_some(corners)
    }
}

/// Immutable pixels for one non-overlapping atlas cell.
#[derive(Clone, Debug, PartialEq)]
pub struct NametagAtlasRect {
    /// Atlas x, y, width and height in texels.
    pub cell: [u32; 4],
    pub rgba8: Arc<[u8]>,
}

impl NametagAtlasRect {
    /// Returns changed cells, including updates missed between render extractions.
    pub fn updates<'a>(
        current: &'a [Self],
        previous: &'a [Self],
    ) -> impl Iterator<Item = &'a Self> {
        current
            .iter()
            .enumerate()
            .filter_map(move |(index, rectangle)| {
                let unchanged = previous.get(index).is_some_and(|old| {
                    old.cell == rectangle.cell && Arc::ptr_eq(&old.rgba8, &rectangle.rgba8)
                });
                (!unchanged).then_some(rectangle)
            })
    }
}

/// This frame's tags for the render world. Records before `see_through` draw over everything;
/// the rest are depth tested.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NametagScene {
    pub records: Vec<NametagRecord>,
    pub see_through: usize,
    /// All live cells, sharing unchanged pixels across publications.
    pub atlas: Arc<[NametagAtlasRect]>,
    pub atlas_revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A short second line gets its own narrow background, not a slab sized to the widest line.
    #[test]
    fn each_line_gets_its_own_background_stacked_without_overlap() {
        let backgrounds: Vec<_> = nametag_line_backgrounds([41.0, 12.0].into_iter()).collect();
        assert_eq!(
            backgrounds,
            [[-21.0, -1.0, 21.0, 9.0], [-7.0, 9.0, 7.0, 19.0]]
        );
        assert_eq!(backgrounds[0][3], backgrounds[1][1]);
    }

    #[test]
    fn single_line_background_keeps_its_padded_half_width() {
        let single: Vec<_> = nametag_line_backgrounds([41.0].into_iter()).collect();
        assert_eq!(single, [[-21.0, -1.0, 21.0, 9.0]]);
        assert_eq!(nametag_line_backgrounds([1.0].into_iter()).count(), 0);
        assert_eq!(nametag_line_backgrounds([1.0, 1.0].into_iter()).count(), 2);
    }
}
