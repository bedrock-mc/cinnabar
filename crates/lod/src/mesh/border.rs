//! Crack border lists: faces on a section face that the same-level neighbour hides but a
//! neighbour region rendered one level coarser or finer shows.

use crate::{
    geometry::{Face, SECTION_SIDE, voxel_index},
    mesh::{
        Bucket, FALLBACK_LIGHT, MeshList,
        class::{AIR, MeshClasses, SEE_THROUGH, SOLID, WATER, face_bucket},
        greedy::{self, Rows},
        group_index,
        input::{FACE_CHILDREN, FaceSlabs, MeshInput},
        padded::{PAD, Padded, axis_stride, padded_index},
        quad::{PAYLOAD_LIGHT_MASK, Quad, in_plane_axes},
    },
    section::Section,
    voxel::{Light, Voxel},
};

/// Section voxels per side of the finer plane: two children per section voxel.
const FINE_SIDE: usize = 2 * SECTION_SIDE;
/// Lists built here, in [`MeshList`] order after the main list.
const LISTS: [MeshList; 2] = [MeshList::ToCoarser, MeshList::ToFiner];

/// Reusable planes of one face's border lists.
pub(super) struct Border {
    /// The coarser neighbour voxel in front of each section voxel of the face, `[v][u]`.
    coarse: Box<[(u32, u8)]>,
    /// The finer neighbour plane, `[V][U]` with two children per section voxel each way.
    fine: Box<[(u32, u8)]>,
    /// Payload of each face in either list per bucket, `[bucket][v][u]`.
    keys: Box<[u32]>,
    /// Face rows per list and bucket.
    rows: [[Rows; 2]; 2],
}

impl Border {
    /// Empty planes, allocated once per mesher.
    pub(super) fn new() -> Self {
        Self {
            coarse: vec![(0, 0); SECTION_SIDE * SECTION_SIDE].into_boxed_slice(),
            fine: vec![(0, 0); FINE_SIDE * FINE_SIDE].into_boxed_slice(),
            keys: vec![0; 2 * SECTION_SIDE * SECTION_SIDE].into_boxed_slice(),
            rows: [[[0; SECTION_SIDE]; 2]; 2],
        }
    }

    /// Bytes held between jobs.
    pub(super) fn heap_bytes(&self) -> usize {
        size_of_val(&*self.coarse) + size_of_val(&*self.fine) + size_of_val(&*self.keys)
    }

    /// Emits the to-coarser and to-finer lists of `face` into `groups`. Nothing is emitted
    /// across an unknown same-level neighbour: the main list already shows every face there.
    /// With `closed` (the exterior fill's closed voxels per padded column), also opens each
    /// padding voxel across `face` whose coarser voxel or any finer child is not solid.
    pub(super) fn lists(
        &mut self,
        padded: &Padded,
        input: &MeshInput<'_>,
        classes: &MeshClasses,
        face: Face,
        groups: &mut [Vec<Quad>],
        closed: Option<&mut [u64]>,
    ) {
        let pos = input.section().pos();
        let slabs = FaceSlabs::of(pos, face);
        let given = input.face(face);
        let coarse = slabs.coarser.is_some();
        let fine = slabs.finer.iter().any(Option::is_some);
        if given.same.is_none() || !(coarse || fine) {
            return;
        }
        if coarse {
            let halves = pos.octant_in_parent();
            let halves = [halves.x(), halves.y(), halves.z()];
            self.load_coarse(face, halves, given.coarser, classes);
        }
        if fine {
            self.load_fine(face, &given.finer, classes);
        }
        let axis = face.axis();
        let [u_axis, v_axis] = in_plane_axes(face);
        if let Some(closed) = closed {
            self.open_padding(face, coarse, fine, closed);
        }
        self.rows = [[[0; SECTION_SIDE]; 2]; 2];
        let ahead = if face.is_positive() {
            axis_stride(axis) as isize
        } else {
            -(axis_stride(axis) as isize)
        };
        for v in 0..SECTION_SIDE {
            for u in 0..SECTION_SIDE {
                let mut at = [1; 3];
                at[axis] = face.plane() + 1;
                at[u_axis] = u + 1;
                at[v_axis] = v + 1;
                let index = padded_index(at[0], at[1], at[2]);
                let (word, class) = (padded.words[index], padded.flags[index]);
                let front = index.wrapping_add_signed(ahead);
                let (front_word, front_class) = (padded.words[front], padded.flags[front]);
                if class == AIR || face_bucket(class, word, front_class, front_word).is_some() {
                    continue;
                }
                let shows =
                    |(other, other_class): (u32, u8)| face_bucket(class, word, other_class, other);
                let to_coarser = if coarse {
                    shows(self.coarse[v * SECTION_SIDE + u])
                } else {
                    None
                };
                let to_finer = if fine {
                    finer_bucket((0..4).map(|child| {
                        let fine_u = 2 * u + (child & 1);
                        let fine_v = 2 * v + (child >> 1);
                        shows(self.fine[fine_v * FINE_SIDE + fine_u])
                    }))
                } else {
                    None
                };
                if to_coarser.is_none() && to_finer.is_none() {
                    continue;
                }
                let light = if front_class == SOLID {
                    brightest_neighbour(padded, index, front)
                } else {
                    front_word & PAYLOAD_LIGHT_MASK
                };
                let cell = v * SECTION_SIDE + u;
                for (list, emits) in [to_coarser, to_finer].into_iter().enumerate() {
                    if let Some(bucket) = emits {
                        let payload = classes.face_word(class, word, bucket);
                        self.keys[bucket.index() * SECTION_SIDE * SECTION_SIDE + cell] =
                            (payload & !PAYLOAD_LIGHT_MASK) | light;
                        self.rows[list][bucket.index()][v] |= 1 << u;
                    }
                }
            }
        }
        for (list_index, list) in LISTS.into_iter().enumerate() {
            for bucket in Bucket::ALL {
                let rows = &mut self.rows[list_index][bucket.index()];
                let plane = SECTION_SIDE * SECTION_SIDE;
                let keys = &self.keys[bucket.index() * plane..][..plane];
                let out = &mut groups[group_index(list, bucket, face)];
                greedy::merge(
                    rows,
                    |u, v| keys[v * SECTION_SIDE + u],
                    |u, v, width, height, payload| {
                        let mut min = [0; 3];
                        min[axis] = face.plane();
                        min[u_axis] = u;
                        min[v_axis] = v;
                        out.push(Quad::merged(face, min, [width, height], payload));
                    },
                );
            }
        }
    }

    /// Clears the closed bit of every padding voxel across `face` whose loaded coarser voxel
    /// (when `coarse`) or any loaded finer child (when `fine`) is not solid.
    fn open_padding(&self, face: Face, coarse: bool, fine: bool, closed: &mut [u64]) {
        let [u_axis, v_axis] = in_plane_axes(face);
        let shell = if face.is_positive() { PAD - 1 } else { 0 };
        for v in 0..SECTION_SIDE {
            for u in 0..SECTION_SIDE {
                let open_coarse = coarse && self.coarse[v * SECTION_SIDE + u].1 != SOLID;
                let open_fine = fine
                    && (0..4).any(|child| {
                        let fine_u = 2 * u + (child & 1);
                        let fine_v = 2 * v + (child >> 1);
                        self.fine[fine_v * FINE_SIDE + fine_u].1 != SOLID
                    });
                if open_coarse || open_fine {
                    let mut at = [0; 3];
                    at[face.axis()] = shell;
                    at[u_axis] = u + 1;
                    at[v_axis] = v + 1;
                    let index = padded_index(at[0], at[1], at[2]);
                    closed[index / PAD] &= !(1 << (index % PAD));
                }
            }
        }
    }

    /// Loads the coarser section's voxel in front of every section voxel of `face`;
    /// `halves` are this section's octant offsets in its parent, which the neighbour shares
    /// in the plane.
    fn load_coarse(
        &mut self,
        face: Face,
        halves: [usize; 3],
        coarser: Option<&Section>,
        classes: &MeshClasses,
    ) {
        let [u_axis, v_axis] = in_plane_axes(face);
        let half = SECTION_SIDE / 2;
        let touching = face.opposite();
        for coarse_v in 0..half {
            for coarse_u in 0..half {
                let at = touching.voxel_at([
                    halves[u_axis] * half + coarse_u,
                    halves[v_axis] * half + coarse_v,
                ]);
                let value = slab_value(coarser, classes, at);
                for v in 2 * coarse_v..2 * coarse_v + 2 {
                    for u in 2 * coarse_u..2 * coarse_u + 2 {
                        self.coarse[v * SECTION_SIDE + u] = value;
                    }
                }
            }
        }
    }

    /// Loads the touching planes of the neighbour's children across `face`.
    fn load_fine(
        &mut self,
        face: Face,
        children: &[Option<&Section>; FACE_CHILDREN],
        classes: &MeshClasses,
    ) {
        let touching = face.opposite();
        for (quadrant, child) in children.iter().enumerate() {
            let (base_u, base_v) = (
                (quadrant & 1) * SECTION_SIDE,
                (quadrant >> 1) * SECTION_SIDE,
            );
            for v in 0..SECTION_SIDE {
                for u in 0..SECTION_SIDE {
                    self.fine[(base_v + v) * FINE_SIDE + base_u + u] =
                        slab_value(*child, classes, touching.voxel_at([u, v]));
                }
            }
        }
    }
}

/// The bucket of a to-finer face from the buckets it gets against each finer child: opaque
/// when any child shows it opaque, as one face covers the voxel's face; `None` when no child
/// shows it.
fn finer_bucket(children: impl IntoIterator<Item = Option<Bucket>>) -> Option<Bucket> {
    children.into_iter().flatten().min()
}

/// The word and class of the voxel at `[x, y, z]` in `section`, or unknown air.
fn slab_value(
    section: Option<&Section>,
    classes: &MeshClasses,
    [x, y, z]: [usize; 3],
) -> (u32, u8) {
    let word = section.map_or(Voxel::air(FALLBACK_LIGHT).bits(), |section| {
        section.voxels().get(voxel_index(x, y, z)).bits()
    });
    (word, classes.of_word(word))
}

/// Light bits of the brightest see-through padded neighbour of `index` other than `front`,
/// each channel at its maximum; [`FALLBACK_LIGHT`] when there is none.
fn brightest_neighbour(padded: &Padded, index: usize, front: usize) -> u32 {
    let mut light: Option<Light> = None;
    for axis in 0..3 {
        let stride = axis_stride(axis);
        for neighbour in [index - stride, index + stride] {
            if neighbour == front || padded.flags[neighbour] & (AIR | WATER | SEE_THROUGH) == 0 {
                continue;
            }
            let lit = Voxel::from_bits(padded.words[neighbour] & PAYLOAD_LIGHT_MASK)
                .expect("light bits alone are lit air")
                .light();
            light = Some(light.map_or(lit, |best| {
                Light::new(best.sky().max(lit.sky()), best.block().max(lit.block()))
            }));
        }
    }
    Voxel::air(light.unwrap_or(FALLBACK_LIGHT)).bits()
}
