//! Cube-quad stream order that lets the renderer cull back faces by whole direction.

use std::ops::Range;

use assets::{MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_TWO_SIDED, Material};

use crate::{Face, PackedQuad, SIDE};

/// A set of [`Face`]s, one bit per `Face as usize`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FaceMask(u8);

impl FaceMask {
    pub const ALL: Self = Self(0b11_1111);

    #[must_use]
    pub const fn contains(self, face: Face) -> bool {
        self.0 & (1 << face as u8) != 0
    }

    #[must_use]
    pub const fn with(self, face: Face) -> Self {
        Self(self.0 | (1 << face as u8))
    }
}

/// Faces of which some quad in the sub-chunk at block `origin` can show its front to a
/// perspective eye at `camera`; a camera inside an axis slab keeps both faces of that axis.
#[must_use]
pub fn sub_chunk_facing_faces(origin: [i32; 3], camera: [f64; 3]) -> FaceMask {
    if !camera.iter().all(|value| value.is_finite()) {
        return FaceMask::ALL;
    }
    const AXES: [(Face, Face); 3] = [
        (Face::NegativeX, Face::PositiveX),
        (Face::NegativeY, Face::PositiveY),
        (Face::NegativeZ, Face::PositiveZ),
    ];
    let mut facing = FaceMask::default();
    for (axis, (negative, positive)) in AXES.into_iter().enumerate() {
        let min = f64::from(origin[axis]);
        // Negative faces lie on planes min..max-1 and positive faces on min+1..max.
        if camera[axis] < min + SIDE as f64 {
            facing = facing.with(negative);
        }
        if camera[axis] > min {
            facing = facing.with(positive);
        }
    }
    facing
}

/// Whether every positional variant of material `id` is opaque and single-sided, so
/// back-face culling replaces both of its fragment discards.
#[must_use]
pub fn is_single_sided_opaque(materials: &[Material], id: u32) -> bool {
    let gated = |material: &Material| {
        material.flags & (MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_TWO_SIDED) != 0
    };
    let Some(base) = materials.get(id as usize) else {
        return false;
    };
    let start = base.variation_start as usize;
    let variants = start..start.saturating_add(base.variation_count as usize);
    !gated(base)
        && materials
            .get(variants)
            .is_some_and(|variants| !variants.iter().any(gated))
}

/// Partition of a sub-chunk's cube quads: single-sided opaque quads in one run per face,
/// ordered by [`CubeQuadLayout::SOLID_FACE_ORDER`], then every quad that keeps the
/// alpha-tested two-sided path. The default has no solid quads, which draws exactly as
/// a single two-sided stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CubeQuadLayout {
    solid_ends: [u32; 6],
}

impl CubeQuadLayout {
    /// Half of the camera octants select one contiguous run in this order, none in `Face::ALL`.
    pub const SOLID_FACE_ORDER: [Face; 6] = [
        Face::NegativeX,
        Face::NegativeY,
        Face::NegativeZ,
        Face::PositiveX,
        Face::PositiveY,
        Face::PositiveZ,
    ];

    /// Layout for `counts` solid quads per face, indexed by `Face as usize`.
    #[must_use]
    pub fn from_solid_counts(counts: [u32; 6]) -> Self {
        let mut end = 0_u32;
        Self {
            solid_ends: Self::SOLID_FACE_ORDER.map(|face| {
                end = end.saturating_add(counts[face as usize]);
                end
            }),
        }
    }

    #[must_use]
    pub const fn solid_len(self) -> u32 {
        self.solid_ends[5]
    }

    #[must_use]
    pub fn solid_range(self, face: Face) -> Range<u32> {
        let slot = Self::slot(face);
        let start = if slot == 0 {
            0
        } else {
            self.solid_ends[slot - 1]
        };
        start..self.solid_ends[slot]
    }

    /// Merged contiguous local ranges of the solid runs whose face is in `facing`.
    pub fn solid_runs(self, facing: FaceMask) -> impl Iterator<Item = Range<u32>> {
        let mut runs = Self::SOLID_FACE_ORDER
            .into_iter()
            .filter(move |&face| facing.contains(face))
            .map(move |face| self.solid_range(face))
            .filter(|range| !range.is_empty())
            .peekable();
        std::iter::from_fn(move || {
            let mut run = runs.next()?;
            while let Some(next) = runs.next_if(|next| next.start == run.end) {
                run.end = next.end;
            }
            Some(run)
        })
    }

    /// This layout when `quads` honour it under `materials`, otherwise the all-two-sided default.
    #[must_use]
    pub fn checked(self, quads: &[PackedQuad], materials: &[Material]) -> Self {
        let honoured = quads.len() >= self.solid_len() as usize
            && Self::SOLID_FACE_ORDER.into_iter().all(|face| {
                let range = self.solid_range(face);
                quads[range.start as usize..range.end as usize]
                    .iter()
                    .all(|quad| {
                        quad.face() == face && is_single_sided_opaque(materials, quad.material_id())
                    })
            });
        if honoured { self } else { Self::default() }
    }

    const fn slot(face: Face) -> usize {
        match face {
            Face::NegativeX => 0,
            Face::NegativeY => 1,
            Face::NegativeZ => 2,
            Face::PositiveX => 3,
            Face::PositiveY => 4,
            Face::PositiveZ => 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn faces(mask: FaceMask) -> Vec<Face> {
        Face::ALL
            .into_iter()
            .filter(|&face| mask.contains(face))
            .collect()
    }

    #[test]
    fn camera_outside_the_bounds_selects_one_face_per_axis() {
        let facing = sub_chunk_facing_faces([16, -32, 0], [40.0, -50.0, 8.5]);
        assert_eq!(
            faces(facing),
            [
                Face::PositiveX,
                Face::NegativeY,
                Face::NegativeZ,
                Face::PositiveZ
            ]
        );
        let corner = sub_chunk_facing_faces([0, 0, 0], [-1.0, 17.0, -0.5]);
        assert_eq!(
            faces(corner),
            [Face::NegativeX, Face::PositiveY, Face::NegativeZ]
        );
    }

    #[test]
    fn camera_inside_the_bounds_keeps_every_face() {
        assert_eq!(
            sub_chunk_facing_faces([0, 64, 0], [3.25, 70.0, 15.9]),
            FaceMask::ALL
        );
    }

    #[test]
    fn camera_on_a_bounding_plane_drops_only_the_faces_it_cannot_see() {
        // On the max plane every negative face is behind or edge-on to the eye.
        let max = sub_chunk_facing_faces([0, 0, 0], [16.0, 8.0, 8.0]);
        assert!(!max.contains(Face::NegativeX) && max.contains(Face::PositiveX));
        let min = sub_chunk_facing_faces([0, 0, 0], [0.0, 8.0, 8.0]);
        assert!(min.contains(Face::NegativeX) && !min.contains(Face::PositiveX));
    }

    #[test]
    fn non_finite_camera_keeps_every_face() {
        assert_eq!(
            sub_chunk_facing_faces([0, 0, 0], [f64::NAN, 0.0, 0.0]),
            FaceMask::ALL
        );
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)] // A one-element list of draw runs is the expectation.
    fn runs_merge_adjacent_slots_and_skip_empty_ones() {
        // Slots: -X 2, -Y 0, -Z 3, +X 1, +Y 4, +Z 0.
        let layout = CubeQuadLayout::from_solid_counts([2, 1, 0, 4, 3, 0]);
        assert_eq!(layout.solid_len(), 10);
        let all = layout.solid_runs(FaceMask::ALL).collect::<Vec<_>>();
        assert_eq!(all, [0..10]);
        let octant = FaceMask::default()
            .with(Face::NegativeX)
            .with(Face::PositiveY)
            .with(Face::NegativeZ);
        assert_eq!(layout.solid_runs(octant).collect::<Vec<_>>(), [0..5, 6..10]);
        assert_eq!(layout.solid_runs(FaceMask::default()).count(), 0);
    }

    #[test]
    fn any_gated_variant_keeps_the_material_two_sided() {
        let solid = Material::unvaried();
        let cutout = Material {
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
            ..solid
        };
        let varied = Material {
            variation_start: 1,
            variation_count: 2,
            ..solid
        };
        assert!(is_single_sided_opaque(&[varied, solid, solid], 0));
        assert!(!is_single_sided_opaque(&[varied, solid, cutout], 0));
        assert!(!is_single_sided_opaque(&[varied, solid], 0));
        assert!(!is_single_sided_opaque(&[solid], 1));
    }
}
