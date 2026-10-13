//! What lies across a section's faces, so the mip can judge exposure on full neighbour data.

use crate::{
    classify::{MaterialClasses, voxel_class},
    geometry::{Face, SECTION_SIDE, SectionPos},
    storage::DenseSection,
};

const _: () = assert!(SECTION_SIDE == u32::BITS as usize);

/// Which voxels of a neighbour's touching plane expose their neighbours
/// ([`crate::classify::BlockClass::exposes_neighbours`]). Bit `u` of row `v` holds in-plane
/// position `[u, v]` from [`Face::in_plane`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FacePlane {
    rows: [u32; SECTION_SIDE],
}

impl FacePlane {
    /// A neighbour known to hold only air.
    pub const EXPOSING: Self = Self {
        rows: [u32::MAX; SECTION_SIDE],
    };
    /// A neighbour whose touching plane is closed everywhere.
    pub const CLOSED: Self = Self {
        rows: [0; SECTION_SIDE],
    };

    /// The plane of `neighbour`, the section across `face` of the one being mipped, that
    /// touches that face.
    #[must_use]
    pub fn across(face: Face, neighbour: &DenseSection, classes: &impl MaterialClasses) -> Self {
        let touching = face.opposite();
        let mut plane = Self::CLOSED;
        for v in 0..SECTION_SIDE {
            for u in 0..SECTION_SIDE {
                let [x, y, z] = touching.voxel_at([u, v]);
                if voxel_class(classes, neighbour.get(x, y, z)).exposes_neighbours() {
                    plane.rows[v] |= 1 << u;
                }
            }
        }
        plane
    }

    /// True when the neighbour voxel at in-plane `[u, v]` exposes; panics outside the plane.
    #[must_use]
    pub fn exposes(&self, [u, v]: [usize; 2]) -> bool {
        assert!(u < SECTION_SIDE, "in-plane u {u}");
        self.rows[v] >> u & 1 == 1
    }

    /// Sets in-plane `[u, v]`; panics outside the plane.
    pub fn set(&mut self, [u, v]: [usize; 2], exposes: bool) {
        assert!(u < SECTION_SIDE, "in-plane u {u}");
        if exposes {
            self.rows[v] |= 1 << u;
        } else {
            self.rows[v] &= !(1 << u);
        }
    }
}

/// The touching planes of a section's six neighbours, by [`Face::index`]; `None` where the
/// neighbour is unknown. The mip then falls back to its provisional rule for that face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Neighbourhood {
    faces: [Option<FacePlane>; 6],
}

impl Neighbourhood {
    /// Every neighbour unknown.
    pub const UNKNOWN: Self = Self { faces: [None; 6] };

    /// The planes of `pos`'s neighbours that `lookup` returns, at the same level. Callers
    /// that know a missing neighbour is empty set [`FacePlane::EXPOSING`] afterwards.
    #[must_use]
    pub fn of_section<'a>(
        pos: SectionPos,
        classes: &impl MaterialClasses,
        lookup: impl Fn(SectionPos) -> Option<&'a DenseSection>,
    ) -> Self {
        Self {
            faces: Face::ALL.map(|face| {
                let neighbour = lookup(pos.neighbour(face)?)?;
                Some(FacePlane::across(face, neighbour, classes))
            }),
        }
    }

    /// The plane across `face`, if known.
    #[must_use]
    pub fn get(&self, face: Face) -> Option<&FacePlane> {
        self.faces[face.index()].as_ref()
    }

    /// Records or forgets the plane across `face`.
    pub fn set(&mut self, face: Face, plane: Option<FacePlane>) {
        self.faces[face.index()] = plane;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fixtures::{blocks, fixture_classifier},
        voxel::{Light, MaterialId, Voxel},
    };

    fn stone() -> Voxel {
        Voxel::new(
            MaterialId::new(blocks::STONE).expect("material"),
            Light::DARK,
            crate::voxel::FULL_COVERAGE,
            false,
        )
    }

    #[test]
    fn planes_read_the_neighbour_side_that_touches_the_face() {
        let classes = fixture_classifier();
        // Stone everywhere except air on the neighbour's X = 0 plane at (y 3, z 9).
        let mut neighbour = DenseSection::filled(stone(), 0);
        neighbour.set(0, 3, 9, Voxel::AIR);
        let plane = FacePlane::across(Face::PosX, &neighbour, &classes);
        assert!(plane.exposes([3, 9]));
        assert!(!plane.exposes([9, 3]));
        assert_eq!(
            FacePlane::across(Face::NegX, &neighbour, &classes),
            FacePlane::CLOSED,
            "the far side of the neighbour is closed"
        );
        let mut expected = FacePlane::CLOSED;
        expected.set([3, 9], true);
        assert_eq!(plane, expected);
        expected.set([3, 9], false);
        assert_eq!(expected, FacePlane::CLOSED);
        assert!(FacePlane::EXPOSING.exposes([31, 31]));
    }

    #[test]
    fn neighbourhoods_look_up_same_level_neighbours() {
        let classes = fixture_classifier();
        let pos = SectionPos::new(2, 0, 0, 0).expect("pos");
        let air = DenseSection::filled(Voxel::AIR, 0);
        let above = pos.neighbour(Face::PosY).expect("above");
        let found = Neighbourhood::of_section(pos, &classes, |at| (at == above).then_some(&air));
        for face in Face::ALL {
            let expected = (face == Face::PosY).then_some(&FacePlane::EXPOSING);
            assert_eq!(found.get(face), expected, "{face:?}");
        }
        let mut known = Neighbourhood::UNKNOWN;
        known.set(Face::PosY, Some(FacePlane::EXPOSING));
        assert_eq!(known, found);
    }
}
