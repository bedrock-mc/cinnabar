/// Exact local-plane to homogeneous clip transform for world-projected UI geometry.
///
/// The matrix is column-major, matching a graphics matrix's column array. Local bounds and
/// glyph quads retain their authored font/geometry units; the GPU performs viewport and near
/// plane clipping and perspective-correct texture interpolation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiWorldProjection {
    pub clip_from_local: [[f32; 4]; 4],
    /// Full viewport extent in logical pixels, before the platform DPI conversion.
    pub viewport_size: [f32; 2],
    /// Compares against the owning view's reverse-Z depth; false uses Always if writing.
    pub depth_test: bool,
    pub depth_write: bool,
    /// Rejects texture texels below half alpha before applying the vertex color alpha.
    pub alpha_test: bool,
}

impl UiWorldProjection {
    pub(super) fn viewport_clip(self) -> Result<crate::UiRect, crate::GeometryError> {
        crate::UiRect::new(
            crate::UiPoint::new(0.0, 0.0)?,
            crate::UiPoint::new(self.viewport_size[0], self.viewport_size[1])?,
        )
    }

    pub(super) fn is_valid(self) -> bool {
        self.clip_from_local
            .iter()
            .flatten()
            .all(|value| value.is_finite())
            && self
                .viewport_size
                .iter()
                .all(|value| value.is_finite() && *value > 0.0)
    }

    pub(super) fn project(self, local: [f32; 2]) -> Option<([f32; 2], f32, f32)> {
        let clip: [f32; 4] = std::array::from_fn(|axis| {
            self.clip_from_local[0][axis] * local[0]
                + self.clip_from_local[1][axis] * local[1]
                + self.clip_from_local[3][axis]
        });
        // Store homogeneous screen coordinates, not divided coordinates. Zero/negative W is
        // legal for geometry crossing the camera and is clipped safely by the rasterizer.
        let screen = [
            (clip[0] + clip[3]) * self.viewport_size[0] * 0.5,
            (clip[3] - clip[1]) * self.viewport_size[1] * 0.5,
        ];
        clip.iter()
            .chain(screen.iter())
            .all(|value| value.is_finite())
            .then_some((screen, clip[2], clip[3]))
    }
}
