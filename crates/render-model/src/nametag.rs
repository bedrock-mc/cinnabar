//! World-space actor name tags: screen-aligned billboards of backing plates and rasterized text,
//! laid out in font pixels as the vanilla name-tag renderer does.
use std::sync::Arc;

/// Java label scale in blocks per font design pixel; debug text shares this scale.
pub const NAMETAG_BLOCKS_PER_FONT_PIXEL: f32 = 1.6 * (1.0 / 60.0);
/// Atlas coverage must exceed this value before label opacity is applied.
pub const NAMETAG_ALPHA_CUTOFF: f32 = 0.1;
/// Opacity for depth-tested sneaking text.
pub const NAMETAG_DIM_TEXT_ALPHA: f32 = 32.0 / 255.0;
pub const NAMETAG_TEXT_REVERSE_Z_BIAS: i32 = 32;
/// Side of the square RGBA8 text atlas.
pub const NAMETAG_ATLAS_SIDE: u32 = 2048;
/// Most plate and text quads drawn in one frame.
pub const MAX_NAMETAG_RECORDS: usize = 1024;
/// Font-pixel distance between stacked lines, shared with debug text.
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
    line_backgrounds(widths_px, NAMETAG_LINE_PITCH_PX - 1.0, true)
}

/// Java label plates have one pixel of side padding and vertical bounds -1 through 8.
/// Multiline tags retain the shared line pitch and one centered plate per line.
pub fn java_nametag_line_backgrounds(
    widths_px: impl ExactSizeIterator<Item = f32>,
) -> impl Iterator<Item = [f32; 4]> {
    line_backgrounds(widths_px, 8.0, false)
}

/// Builds centered line plates with the requested bottom bound and narrow-line policy.
fn line_backgrounds(
    widths_px: impl ExactSizeIterator<Item = f32>,
    bottom_px: f32,
    skip_narrow_single: bool,
) -> impl Iterator<Item = [f32; 4]> {
    let multiline = widths_px.len() > 1;
    widths_px.enumerate().filter_map(move |(index, width)| {
        let half = -nametag_line_left(width);
        let top = NAMETAG_LINE_PITCH_PX * index as f32 - NAMETAG_BACKGROUND_PADDING_PX;
        (!skip_narrow_single || multiline || half > 0.0).then_some([
            -(half + NAMETAG_BACKGROUND_PADDING_PX),
            top,
            half + NAMETAG_BACKGROUND_PADDING_PX,
            NAMETAG_LINE_PITCH_PX * index as f32 + bottom_px,
        ])
    })
}

/// One quad of a tag as the GPU reads it: a plate when `uv[2] < 0`, else a text line.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NametagRecord {
    /// World point the tag's font-pixel origin hangs at.
    pub anchor: [f32; 3],
    /// 1 for glyphs, which write depth only in the tested pass; 0 for a backing plate.
    pub text: u32,
    /// `[x0, y0, x1, y1]` in font pixels; +x is the viewer's right, +y is down.
    pub rect: [f32; 4],
    /// Normalized atlas rect `[u0, v0, u1, v1]`; `u1 < 0` draws the flat colour.
    pub uv: [f32; 4],
    /// Straight-alpha RGBA multiplier.
    pub color: [f32; 4],
    /// Multiline world-Y translation, independent of the camera rotation.
    pub line_lift: f32,
    /// Nonzero when this quad tests world depth; tested glyphs also write depth.
    pub depth_tested: u32,
    pub padding: [u32; 2],
}

impl NametagRecord {
    /// Evaluates a software label fragment, rejecting low atlas coverage before color opacity.
    /// Flat plates ignore the supplied texel; sampled RGB is already in the desired color space.
    /// Fully transparent cosmetic fragments leave both scene color and alpha untouched.
    #[must_use]
    pub fn fragment_color(&self, texel: [f32; 4]) -> Option<[f32; 4]> {
        let texel = if self.uv[2] < 0.0 { [1.0; 4] } else { texel };
        if texel[3] <= NAMETAG_ALPHA_CUTOFF {
            return None;
        }
        let color = std::array::from_fn(|channel| self.color[channel] * texel[channel]);
        (color[3] > 0.0).then_some(color)
    }

    /// Builds the screen-aligned world quad from the camera's world rotation, as in the shader.
    /// Local X follows camera-right and local Y follows camera-down, in font pixels.
    /// Returns `None` for non-finite geometry or a camera rotation that is not normalized.
    #[must_use]
    pub fn world_corners(&self, camera_rotation: glam::Quat) -> Option<[[f32; 3]; 4]> {
        use glam::Vec3;
        let anchor = Vec3::from_array(self.anchor);
        if !anchor.is_finite() || !camera_rotation.is_finite() || !camera_rotation.is_normalized() {
            return None;
        }
        let right = camera_rotation * Vec3::X;
        let up = camera_rotation * Vec3::Y;
        let [x0, y0, x1, y1] = self.rect;
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|[x, y]| {
            (anchor
                + Vec3::Y * self.line_lift
                + (right * x - up * y) * NAMETAG_BLOCKS_PER_FONT_PIXEL)
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

/// This frame's complete labels in draw order, with depth selection on each record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NametagScene {
    pub records: Vec<NametagRecord>,
    /// All live cells, sharing unchanged pixels across publications.
    pub atlas: Arc<[NametagAtlasRect]>,
    pub atlas_revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{EulerRot, Quat, Vec3};

    #[test]
    fn tag_plane_matches_the_camera_at_center_and_frustum_edges() {
        let record = NametagRecord {
            rect: [-20.0, -5.0, 20.0, 5.0],
            line_lift: 0.25,
            ..Default::default()
        };
        let eye = Vec3::new(3.0, 2.0, -1.0);
        let edge = (std::f32::consts::FRAC_PI_4).tan() * 0.95;
        for camera_rotation in [
            Quat::IDENTITY,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Quat::from_rotation_y(std::f32::consts::PI),
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 0.3),
            Quat::from_euler(EulerRot::YXZ, -2.1, 1.2, -0.6),
        ] {
            let forward = camera_rotation * Vec3::NEG_Z;
            for depth in [1.0, 8.0] {
                for offset in [
                    Vec3::NEG_Z,
                    Vec3::new(edge, 0.0, -1.0),
                    Vec3::new(-edge, edge, -1.0),
                    Vec3::new(edge, -edge, -1.0),
                ] {
                    let tag = NametagRecord {
                        anchor: (eye + camera_rotation * (offset * depth)).to_array(),
                        ..record
                    };
                    let corners = tag
                        .world_corners(camera_rotation)
                        .unwrap()
                        .map(Vec3::from_array);
                    let right = corners[1] - corners[0];
                    let down = corners[3] - corners[0];
                    let normal = right.cross(down).normalize();
                    assert!(
                        (normal - forward).length() < 1e-5,
                        "rotation {camera_rotation:?}, offset {offset:?}, depth {depth}: normal {normal:?}"
                    );
                    for corner in corners {
                        assert!((corner - corners[0]).dot(forward).abs() < 1e-5);
                    }
                    assert!((right.normalize() - camera_rotation * Vec3::X).length() < 1e-5);
                    assert!((down.normalize() + camera_rotation * Vec3::Y).length() < 1e-5);
                }
            }
        }
    }

    #[test]
    fn screen_alignment_rejects_invalid_geometry_and_camera_rotations() {
        let record = NametagRecord {
            rect: [0.0, 0.0, 1.0, 1.0],
            ..Default::default()
        };
        // A tag at the camera position still has a defined screen-aligned plane.
        assert!(record.world_corners(Quat::IDENTITY).is_some());
        for rotation in [
            Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            Quat::from_xyzw(0.0, 0.0, 0.0, 2.0),
            Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0),
            Quat::from_xyzw(0.0, f32::INFINITY, 0.0, 1.0),
        ] {
            assert!(record.world_corners(rotation).is_none());
        }
        for invalid in [
            NametagRecord {
                anchor: [f32::NAN; 3],
                ..record
            },
            NametagRecord {
                rect: [f32::INFINITY; 4],
                ..record
            },
            NametagRecord {
                line_lift: f32::NAN,
                ..record
            },
        ] {
            assert!(invalid.world_corners(Quat::IDENTITY).is_none());
        }
    }

    #[test]
    fn transparent_cosmetics_discard_without_rejecting_faint_visible_fragments() {
        for uv in [[0., 0., -1., -1.], [0., 0., 1., 1.]] {
            let invisible = NametagRecord {
                uv,
                color: [0.; 4],
                ..Default::default()
            };
            assert_eq!(invisible.fragment_color([1.; 4]), None);
            let faint = NametagRecord {
                color: [0., 0., 0., 0.01],
                ..invisible
            };
            assert_eq!(faint.fragment_color([1.; 4]), Some(faint.color));
        }
    }

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

    #[test]
    fn java_label_scale_and_plate_bounds_match_the_font_pixel_contract() {
        let record = NametagRecord {
            rect: [0.0, 0.0, 1.0, 1.0],
            ..Default::default()
        };
        let corners = record
            .world_corners(Quat::IDENTITY)
            .unwrap()
            .map(Vec3::from_array);
        assert_eq!(corners[1] - corners[0], Vec3::new(0.026_666_67, 0.0, 0.0));
        assert_eq!(corners[3] - corners[0], Vec3::new(0.0, -0.026_666_67, 0.0));
        let plates: Vec<_> = java_nametag_line_backgrounds([41.0, 12.0].into_iter()).collect();
        assert_eq!(plates, [[-21.0, -1.0, 21.0, 8.0], [-7.0, 9.0, 7.0, 18.0]]);
        assert_eq!(
            java_nametag_line_backgrounds([1.0].into_iter()).collect::<Vec<_>>(),
            [[-1.0, -1.0, 1.0, 8.0]]
        );
    }
}
