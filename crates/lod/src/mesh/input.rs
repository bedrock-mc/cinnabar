//! What a mesh reads: a section and the sections across its faces, and which sections'
//! arrival changes a mesh.

use crate::{
    geometry::{Face, Octant, SectionPos},
    mesh::quad::in_plane_axes,
    section::Section,
};

/// Children touching one face of a section, one per in-plane quadrant.
pub const FACE_CHILDREN: usize = 4;

/// The positions one section's mesh reads across one face; `None` where nothing applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaceSlabs {
    /// The same-level neighbour; `None` past the key range.
    pub same: Option<SectionPos>,
    /// The section one level coarser holding the neighbour, when it is not this section's
    /// own parent (only then can the neighbour region render coarser than this section).
    pub coarser: Option<SectionPos>,
    /// The neighbour's children touching this face, by [`touching_children`] quadrant;
    /// `None` at level 0.
    pub finer: [Option<SectionPos>; FACE_CHILDREN],
}

impl FaceSlabs {
    /// What `pos` reads across `face`.
    #[must_use]
    pub fn of(pos: SectionPos, face: Face) -> Self {
        let same = pos.neighbour(face);
        Self {
            same,
            coarser: same
                .and_then(SectionPos::parent)
                .filter(|&parent| Some(parent) != pos.parent()),
            finer: same.map_or([None; FACE_CHILDREN], |neighbour| {
                touching_children(neighbour, face.opposite())
            }),
        }
    }
}

/// The children of `pos` that touch its `face`, indexed by in-plane quadrant
/// `u_half | v_half << 1` in [`Face::in_plane`] order; all `None` at level 0.
#[must_use]
pub fn touching_children(pos: SectionPos, face: Face) -> [Option<SectionPos>; FACE_CHILDREN] {
    let [u_axis, v_axis] = in_plane_axes(face);
    std::array::from_fn(|quadrant| {
        let mut halves = [false; 3];
        halves[face.axis()] = face.is_positive();
        halves[u_axis] = quadrant & 1 == 1;
        halves[v_axis] = quadrant & 2 == 2;
        pos.child(Octant::from_halves(halves[0], halves[1], halves[2]))
    })
}

/// Every position the mesh of `pos` reads across its faces ([`FaceSlabs::of`] of each
/// face), the inverse of [`dependents`]; `pos` itself is not listed.
pub fn reads(pos: SectionPos) -> impl Iterator<Item = SectionPos> {
    Face::ALL.into_iter().flat_map(move |face| {
        let slabs = FaceSlabs::of(pos, face);
        [slabs.same, slabs.coarser]
            .into_iter()
            .chain(slabs.finer)
            .flatten()
    })
}

/// Every section whose mesh reads `pos` across a face, so a write to `pos` or its arrival
/// changes their meshes: its same-level neighbours, the finer sections whose coarser slab
/// it is and the coarser sections whose finer slab includes it. `pos`'s own mesh is not
/// listed.
#[must_use]
pub fn dependents(pos: SectionPos) -> Vec<SectionPos> {
    let mut out = Vec::new();
    for face in Face::ALL {
        out.extend(pos.neighbour(face));
        for child in touching_children(pos, face).into_iter().flatten() {
            out.extend(child.neighbour(face));
        }
        if let Some(parent) = pos.parent()
            && touching_children(parent, face).contains(&Some(pos))
        {
            out.extend(parent.neighbour(face));
        }
    }
    out
}

/// One section to mesh and what lies across its faces.
#[derive(Debug, Clone, Copy)]
pub struct MeshInput<'a> {
    section: &'a Section,
    faces: [FaceInput<'a>; 6],
}

/// The sections across one face, at the positions [`FaceSlabs::of`] names; `None` where a
/// section is not known yet (or the position does not apply).
#[derive(Debug, Clone, Copy, Default)]
pub struct FaceInput<'a> {
    pub same: Option<&'a Section>,
    pub coarser: Option<&'a Section>,
    pub finer: [Option<&'a Section>; FACE_CHILDREN],
}

impl<'a> MeshInput<'a> {
    /// Pairs `section` with the sections across its faces, by [`Face::index`].
    ///
    /// # Panics
    ///
    /// When a given section is not at the position [`FaceSlabs::of`] names for it.
    #[must_use]
    pub fn new(section: &'a Section, faces: [FaceInput<'a>; 6]) -> Self {
        for face in Face::ALL {
            let slabs = FaceSlabs::of(section.pos(), face);
            let given = &faces[face.index()];
            let check = |given: Option<&Section>, expected: Option<SectionPos>, what: &str| {
                if let Some(given) = given {
                    assert_eq!(
                        Some(given.pos()),
                        expected,
                        "{what} section across {face:?} of {:?}",
                        section.pos()
                    );
                }
            };
            check(given.same, slabs.same, "same-level");
            check(given.coarser, slabs.coarser, "coarser");
            for (child, expected) in given.finer.iter().zip(slabs.finer) {
                check(*child, expected, "finer");
            }
        }
        Self { section, faces }
    }

    /// Reads every position [`FaceSlabs::of`] names through `lookup`, which returns `None`
    /// for sections not known yet.
    #[must_use]
    pub fn from_lookup(
        section: &'a Section,
        mut lookup: impl FnMut(SectionPos) -> Option<&'a Section>,
    ) -> Self {
        let faces = Face::ALL.map(|face| {
            let slabs = FaceSlabs::of(section.pos(), face);
            FaceInput {
                same: slabs.same.and_then(&mut lookup),
                coarser: slabs.coarser.and_then(&mut lookup),
                finer: slabs.finer.map(|pos| pos.and_then(&mut lookup)),
            }
        });
        Self::new(section, faces)
    }

    /// The section to mesh.
    #[must_use]
    pub fn section(&self) -> &'a Section {
        self.section
    }

    /// The sections across `face`.
    #[must_use]
    pub fn face(&self, face: Face) -> &FaceInput<'a> {
        &self.faces[face.index()]
    }

    /// Applicable positions not given; the mesh treats them as unknown space.
    #[must_use]
    pub fn waiting(&self) -> WaitingSlabs {
        let mut waiting = WaitingSlabs::NONE;
        for face in Face::ALL {
            let slabs = FaceSlabs::of(self.section.pos(), face);
            let given = self.face(face);
            let bit = 1 << face.index();
            if slabs.same.is_some() && given.same.is_none() {
                waiting.same |= bit;
            }
            if slabs.coarser.is_some() && given.coarser.is_none() {
                waiting.coarser |= bit;
            }
            for quadrant in 0..FACE_CHILDREN {
                if slabs.finer[quadrant].is_some() && given.finer[quadrant].is_none() {
                    waiting.finer |= 1 << (face.index() * FACE_CHILDREN + quadrant);
                }
            }
        }
        waiting
    }
}

/// Positions a mesh read as unknown space; their arrival should mesh the section again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WaitingSlabs {
    same: u8,
    coarser: u8,
    finer: u32,
}

impl WaitingSlabs {
    /// Nothing missing.
    pub const NONE: Self = Self {
        same: 0,
        coarser: 0,
        finer: 0,
    };

    /// True when the mesh read every applicable position.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.same == 0 && self.coarser == 0 && self.finer == 0
    }

    /// The missing positions of the mesh of `pos`.
    pub fn positions(self, pos: SectionPos) -> impl Iterator<Item = SectionPos> {
        Face::ALL.into_iter().flat_map(move |face| {
            let slabs = FaceSlabs::of(pos, face);
            let bit = 1 << face.index();
            let same = (self.same & bit != 0).then_some(slabs.same).flatten();
            let coarser = (self.coarser & bit != 0).then_some(slabs.coarser).flatten();
            let finer = (0..FACE_CHILDREN).filter_map(move |quadrant| {
                let bit = 1 << (face.index() * FACE_CHILDREN + quadrant);
                (self.finer & bit != 0)
                    .then_some(slabs.finer[quadrant])
                    .flatten()
            });
            same.into_iter().chain(coarser).chain(finer)
        })
    }
}
