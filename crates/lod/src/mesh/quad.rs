//! The two-word quad: geometry and payload, the single source of their bit layouts.
//!
//! Geometry: `x 5 | y 5 | z 5 | face 3 | width - 1 4 | height - 1 4 | spare 6`, from the
//! least significant bit. `x, y, z` is the section-local voxel at the quad's minimum corner;
//! width spans the face's first in-plane axis and height its second, in
//! [`Face::in_plane`] order. Payload: the [`crate::voxel`] word of the face's voxel with its
//! light fields replaced by the light in front of the face. Shader constants are generated
//! from [`QUAD_GEOMETRY_FIELDS`] and [`crate::voxel::VOXEL_FIELDS`].

use crate::{
    bit_field::{BitField, covers_word},
    geometry::{Face, SECTION_SIDE},
    voxel::{BLOCK_LIGHT_FIELD, Light, MATERIAL_FIELD, MaterialId, SKY_LIGHT_FIELD},
};

/// Section-local X of the minimum corner voxel.
pub const QUAD_X_FIELD: BitField = BitField::first("x", 5);
/// Section-local Y of the minimum corner voxel.
pub const QUAD_Y_FIELD: BitField = QUAD_X_FIELD.after("y", 5);
/// Section-local Z of the minimum corner voxel.
pub const QUAD_Z_FIELD: BitField = QUAD_Y_FIELD.after("z", 5);
/// [`Face::index`] of the face direction.
pub const QUAD_FACE_FIELD: BitField = QUAD_Z_FIELD.after("face", 3);
/// Voxels along the first in-plane axis, minus one.
pub const QUAD_WIDTH_FIELD: BitField = QUAD_FACE_FIELD.after("width", 4);
/// Voxels along the second in-plane axis, minus one.
pub const QUAD_HEIGHT_FIELD: BitField = QUAD_WIDTH_FIELD.after("height", 4);
/// Reserved; zero.
pub const QUAD_SPARE_FIELD: BitField = QUAD_HEIGHT_FIELD.after("spare", 6);

/// Every geometry field in bit order; together they cover the word exactly once.
pub const QUAD_GEOMETRY_FIELDS: [BitField; 7] = [
    QUAD_X_FIELD,
    QUAD_Y_FIELD,
    QUAD_Z_FIELD,
    QUAD_FACE_FIELD,
    QUAD_WIDTH_FIELD,
    QUAD_HEIGHT_FIELD,
    QUAD_SPARE_FIELD,
];

const _: () = assert!(
    covers_word(&QUAD_GEOMETRY_FIELDS),
    "quad geometry fields must cover the word exactly once"
);
const _: () = assert!(
    QUAD_X_FIELD.max_value() as usize == SECTION_SIDE - 1
        && QUAD_Y_FIELD.width == QUAD_X_FIELD.width
        && QUAD_Z_FIELD.width == QUAD_X_FIELD.width,
    "coordinates address every section voxel"
);
const _: () = assert!(QUAD_FACE_FIELD.max_value() as usize >= Face::ALL.len() - 1);
const _: () = assert!(QUAD_WIDTH_FIELD.width == QUAD_HEIGHT_FIELD.width);

/// Longest quad side in voxels.
pub const MAX_QUAD_SIDE: usize = QUAD_WIDTH_FIELD.max_value() as usize + 1;

/// The payload bits that carry light: block and sky light, next to each other.
pub const PAYLOAD_LIGHT_MASK: u32 = BLOCK_LIGHT_FIELD.mask() | SKY_LIGHT_FIELD.mask();

const _: () = assert!(
    SKY_LIGHT_FIELD.shift == BLOCK_LIGHT_FIELD.shift + BLOCK_LIGHT_FIELD.width,
    "light is one 8-bit run of the payload"
);

/// One greedy quad: a geometry word and a payload word, in that order in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct Quad {
    geometry: u32,
    payload: u32,
}

impl Quad {
    /// A quad of `face` from section-local voxel `min`, `size` voxels along the face's two
    /// in-plane axes ([`Face::in_plane`] order), with `payload`.
    ///
    /// # Panics
    ///
    /// When a coordinate lies outside the section or a side is outside
    /// `1..=MAX_QUAD_SIDE`.
    #[must_use]
    pub fn new(face: Face, min: [usize; 3], size: [usize; 2], payload: u32) -> Self {
        assert!(
            min.iter().all(|&value| value < SECTION_SIDE),
            "quad corner {min:?} lies outside the section"
        );
        assert!(
            size.iter().all(|&side| (1..=MAX_QUAD_SIDE).contains(&side)),
            "quad size {size:?} outside 1..={MAX_QUAD_SIDE}"
        );
        Self::merged(face, min, size, payload)
    }

    /// [`Self::new`] for the greedy merge, whose corners and sizes are in range by
    /// construction; checked in debug builds only.
    pub(super) fn merged(face: Face, min: [usize; 3], size: [usize; 2], payload: u32) -> Self {
        debug_assert!(min.iter().all(|&value| value < SECTION_SIDE));
        debug_assert!(size.iter().all(|&side| (1..=MAX_QUAD_SIDE).contains(&side)));
        Self {
            geometry: QUAD_X_FIELD.place(min[0] as u32)
                | QUAD_Y_FIELD.place(min[1] as u32)
                | QUAD_Z_FIELD.place(min[2] as u32)
                | QUAD_FACE_FIELD.place(face.index() as u32)
                | QUAD_WIDTH_FIELD.place(size[0] as u32 - 1)
                | QUAD_HEIGHT_FIELD.place(size[1] as u32 - 1),
            payload,
        }
    }

    /// The geometry word.
    #[must_use]
    pub const fn geometry(self) -> u32 {
        self.geometry
    }

    /// The payload word.
    #[must_use]
    pub const fn payload(self) -> u32 {
        self.payload
    }

    /// The face direction.
    #[must_use]
    pub const fn face(self) -> Face {
        Face::ALL[QUAD_FACE_FIELD.read(self.geometry) as usize]
    }

    /// The section-local voxel at the minimum corner.
    #[must_use]
    pub const fn origin(self) -> [usize; 3] {
        [
            QUAD_X_FIELD.read(self.geometry) as usize,
            QUAD_Y_FIELD.read(self.geometry) as usize,
            QUAD_Z_FIELD.read(self.geometry) as usize,
        ]
    }

    /// Voxels along the two in-plane axes.
    #[must_use]
    pub const fn size(self) -> [usize; 2] {
        [
            QUAD_WIDTH_FIELD.read(self.geometry) as usize + 1,
            QUAD_HEIGHT_FIELD.read(self.geometry) as usize + 1,
        ]
    }

    /// The face's material.
    #[must_use]
    pub const fn material(self) -> MaterialId {
        MaterialId::new(MATERIAL_FIELD.read(self.payload)).expect("the field holds an id")
    }

    /// The light in front of the face.
    #[must_use]
    pub const fn light(self) -> Light {
        Light::new(
            SKY_LIGHT_FIELD.read(self.payload) as u8,
            BLOCK_LIGHT_FIELD.read(self.payload) as u8,
        )
    }

    /// The voxels the quad's faces belong to, as section-local `min` (inclusive) and `max`
    /// (exclusive) corners.
    #[must_use]
    pub fn voxel_box(self) -> ([usize; 3], [usize; 3]) {
        let [x, y, z] = self.origin();
        let [width, height] = self.size();
        // Constant indices per axis keep the corners in registers.
        let max = match QUAD_FACE_FIELD.read(self.geometry) >> 1 {
            0 => [x + 1, y + width, z + height],
            1 => [x + width, y + 1, z + height],
            _ => [x + width, y + height, z + 1],
        };
        ([x, y, z], max)
    }

    /// The quad's rectangle in section-local voxel-corner coordinates (`0..=SECTION_SIDE`):
    /// flat along the face's axis, on the voxel side the face points to.
    #[must_use]
    pub fn bounds(self) -> ([usize; 3], [usize; 3]) {
        let (mut min, mut max) = self.voxel_box();
        let face = self.face();
        let axis = face.axis();
        let plane = min[axis] + usize::from(face.is_positive());
        min[axis] = plane;
        max[axis] = plane;
        (min, max)
    }
}

/// The union of the [`Quad::bounds`] of `quads`, which all face `face`; `None` when there
/// are none.
pub fn face_bounds(face: Face, quads: &[Quad]) -> Option<([usize; 3], [usize; 3])> {
    let mut low = [SECTION_SIDE; 3];
    let mut high = [0; 3];
    for quad in quads {
        let (min, max) = quad.voxel_box();
        low = [low[0].min(min[0]), low[1].min(min[1]), low[2].min(min[2])];
        high = [
            high[0].max(max[0]),
            high[1].max(max[1]),
            high[2].max(max[2]),
        ];
    }
    if quads.is_empty() {
        return None;
    }
    // Faces lie on the far side of their voxels when positive, the near side otherwise.
    let axis = face.axis();
    if face.is_positive() {
        low[axis] += 1;
    } else {
        high[axis] -= 1;
    }
    Some((low, high))
}

/// The in-plane axes of each normal axis; the fourth row covers the unused face values.
const IN_PLANE_AXES: [[usize; 2]; 4] = [[1, 2], [0, 2], [0, 1], [0, 1]];

/// The two axes of [`Face::in_plane`], in its order.
#[must_use]
pub const fn in_plane_axes(face: Face) -> [usize; 2] {
    IN_PLANE_AXES[face.axis()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::Voxel;

    #[test]
    fn layout_matches_the_documented_widths() {
        let widths: Vec<_> = QUAD_GEOMETRY_FIELDS
            .iter()
            .map(|field| (field.name, field.width))
            .collect();
        assert_eq!(
            widths,
            [
                ("x", 5),
                ("y", 5),
                ("z", 5),
                ("face", 3),
                ("width", 4),
                ("height", 4),
                ("spare", 6),
            ]
        );
        assert_eq!(MAX_QUAD_SIDE, 16);
        assert_eq!(PAYLOAD_LIGHT_MASK.count_ones(), 8);
    }

    #[test]
    fn fields_round_trip_at_their_extremes() {
        let material = MaterialId::LAST_BLOCK;
        let payload = Voxel::new(material, Light::new(15, 3), 7, true).bits();
        for face in Face::ALL {
            let quad = Quad::new(face, [31, 0, 17], [16, 1], payload);
            assert_eq!(quad.face(), face);
            assert_eq!(quad.origin(), [31, 0, 17]);
            assert_eq!(quad.size(), [16, 1]);
            assert_eq!(quad.material(), material);
            assert_eq!(quad.light(), Light::new(15, 3));
            assert_eq!(quad.geometry() & QUAD_SPARE_FIELD.mask(), 0);
        }
    }

    #[test]
    fn bounds_sit_on_the_face_plane() {
        let quad = Quad::new(Face::PosX, [31, 4, 8], [2, 3], 1);
        assert_eq!(quad.voxel_box(), ([31, 4, 8], [32, 6, 11]));
        assert_eq!(quad.bounds(), ([32, 4, 8], [32, 6, 11]));
        let quad = Quad::new(Face::NegY, [0, 0, 0], [16, 16], 1);
        assert_eq!(quad.bounds(), ([0, 0, 0], [16, 0, 16]));
    }

    #[test]
    #[should_panic(expected = "quad size")]
    fn oversized_quads_are_rejected() {
        let _ = Quad::new(Face::PosY, [0, 0, 0], [17, 1], 1);
    }
}
