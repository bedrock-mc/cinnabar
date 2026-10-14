//! The per-worker mesher: reusable scratch and the main face list.

use crate::{
    geometry::{Face, SECTION_SIDE},
    mesh::{
        Bucket, ExteriorCulling, GROUP_COUNT, MeshList,
        border::Border,
        class::MeshClasses,
        exterior,
        greedy::{self, Rows},
        group_index,
        input::{MeshInput, WaitingSlabs},
        output::{self, BuildScratch, SectionMesh},
        padded::{COLUMN_BITS, PAD, PAD2, Padded, axis_stride, padded_index},
        quad::{PAYLOAD_LIGHT_MASK, Quad, in_plane_axes},
    },
    voxel::MATERIAL_FIELD,
};

/// Face rows of every layer of one direction, per bucket: `[bucket][layer]`.
type LayerRows = [[Rows; SECTION_SIDE]; 2];

/// Meshes sections one at a time, reusing its scratch (about 0.3 MB) between them; keep
/// one per worker thread.
pub struct Mesher {
    padded: Padded,
    /// Closed voxels per padded column for the exterior fill.
    closed: Box<[u64]>,
    reach: Box<[u64]>,
    rows: Box<LayerRows>,
    x_rows: Box<LayerRows>,
    border: Border,
    groups: Box<[Vec<Quad>; GROUP_COUNT]>,
    build: BuildScratch,
}

impl Default for Mesher {
    fn default() -> Self {
        Self::new()
    }
}

impl Mesher {
    /// A mesher with empty scratch; the first meshes grow its quad buffers.
    #[must_use]
    pub fn new() -> Self {
        let rows = || Box::new([[[0; SECTION_SIDE]; SECTION_SIDE]; 2]);
        Self {
            padded: Padded::new(),
            closed: vec![0; PAD2].into_boxed_slice(),
            reach: vec![0; PAD2].into_boxed_slice(),
            rows: rows(),
            x_rows: rows(),
            border: Border::new(),
            groups: Box::new(std::array::from_fn(|_| Vec::new())),
            build: BuildScratch::default(),
        }
    }

    /// Bytes of scratch held between sections, including grown quad buffers.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.padded.heap_bytes()
            + size_of_val(&*self.closed)
            + size_of_val(&*self.reach)
            + 2 * size_of::<LayerRows>()
            + self.border.heap_bytes()
            + self.build.heap_bytes()
            + self
                .groups
                .iter()
                .map(|group| group.capacity() * size_of::<Quad>())
                .sum::<usize>()
    }

    /// Meshes `input` into its main and border lists, grouped and cut into clusters. Runs
    /// without allocating beyond the returned mesh once the quad buffers have grown.
    pub fn mesh(
        &mut self,
        input: &MeshInput<'_>,
        classes: &MeshClasses,
        culling: ExteriorCulling,
    ) -> SectionMesh {
        for group in self.groups.iter_mut() {
            group.clear();
        }
        let section = input.section();
        // An all-air section shows no face whatever lies around it.
        let mut waiting = WaitingSlabs::NONE;
        if !section.is_all_air() {
            waiting = input.waiting();
            self.padded.gather(input, classes);
            let level = section.pos().level();
            let cull = culling == ExteriorCulling::SectionLocal;
            if cull {
                self.closed.copy_from_slice(&self.padded.masks.solid);
            }
            // The border lists open the padding wherever a coarser or finer neighbour is
            // see-through, before the fill reads it.
            for face in Face::ALL {
                let closed = cull.then_some(&mut self.closed[..]);
                self.border.lists(
                    &self.padded,
                    input,
                    classes,
                    face,
                    &mut self.groups[..],
                    closed,
                );
            }
            if cull {
                exterior::fill(&self.closed, level, &mut self.reach);
            }
            self.main_faces::<0>(classes, cull);
            self.main_faces::<1>(classes, cull);
            self.main_faces::<2>(classes, cull);
            self.main_faces::<3>(classes, cull);
            self.main_faces::<4>(classes, cull);
            self.main_faces::<5>(classes, cull);
        }
        output::build(
            section.pos(),
            section.biomes(),
            &self.groups,
            waiting,
            &mut self.build,
        )
    }

    /// Emits the main list of face [`Face::ALL`]`[FACE]`, layer by layer from the far side
    /// to the near side as seen from in front of the faces.
    fn main_faces<const FACE: usize>(&mut self, classes: &MeshClasses, culled: bool) {
        // A constant face per instance keeps the strides and shifts below constant.
        let face = Face::ALL[FACE];
        let shown = self.face_rows::<FACE>(culled);
        let axis = face.axis();
        let [u_axis, v_axis] = in_plane_axes(face);
        let (step_u, step_v, step_layer) =
            (axis_stride(u_axis), axis_stride(v_axis), axis_stride(axis));
        let front = if face.is_positive() {
            step_layer as isize
        } else {
            -(step_layer as isize)
        };
        let (words, flags) = (&self.padded.words, &self.padded.flags);
        for step in 0..SECTION_SIDE {
            let layer = if face.is_positive() {
                step
            } else {
                SECTION_SIDE - 1 - step
            };
            let origin = padded_index(1, 1, 1) + layer * step_layer;
            for bucket in Bucket::ALL {
                if !shown[bucket.index()] {
                    continue;
                }
                let rows = &mut self.rows[bucket.index()][layer];
                if rows.iter().all(|&row| row == 0) {
                    continue;
                }
                let key = |u: usize, v: usize| {
                    let at = origin + u * step_u + v * step_v;
                    let lit = words[at.wrapping_add_signed(front)];
                    let word = match bucket {
                        Bucket::Opaque => words[at],
                        Bucket::Translucent => classes.face_word(flags[at], words[at], bucket),
                    };
                    (word & !PAYLOAD_LIGHT_MASK) | (lit & PAYLOAD_LIGHT_MASK)
                };
                let out = &mut self.groups[group_index(MeshList::Main, bucket, face)];
                greedy::merge(rows, key, |u, v, width, height, payload| {
                    let mut min = [0; 3];
                    min[axis] = layer;
                    min[u_axis] = u;
                    min[v_axis] = v;
                    out.push(Quad::merged(face, min, [width, height], payload));
                });
            }
        }
    }

    /// Fills the face rows of every layer of face [`Face::ALL`]`[FACE]` per bucket; returns
    /// which buckets have any face.
    fn face_rows<const FACE: usize>(&mut self, culled: bool) -> [bool; 2] {
        let face = Face::ALL[FACE];
        let masks = &self.padded.masks;
        let words = &self.padded.words;
        let reach = &self.reach;
        // The front voxel's column and its bit offset within it.
        let (column_step, shift): (isize, i32) = match face {
            Face::NegX => (0, -1),
            Face::PosX => (0, 1),
            Face::NegY => (-(PAD as isize), 0),
            Face::PosY => (PAD as isize, 0),
            Face::NegZ => (-1, 0),
            Face::PosZ => (1, 0),
        };
        let word_step = column_step * PAD as isize + shift as isize;
        let front = |mask: &[u64], column: usize| {
            let bits = mask[column.wrapping_add_signed(column_step)];
            match shift {
                1 => bits >> 1,
                -1 => (bits << 1) & COLUMN_BITS,
                _ => bits,
            }
        };
        const BORDER_X: u64 = 1 | 1 << (PAD - 1);
        let mut shown = [false; 2];
        let rows = if face.axis() == 0 {
            &mut self.x_rows
        } else {
            &mut self.rows
        };
        for y in 0..SECTION_SIDE {
            for z in 0..SECTION_SIDE {
                let column = (y + 1) * PAD + z + 1;
                let solid_front = front(&masks.solid, column);
                let mut see_through = masks.see_through[column] & !solid_front;
                let mut same = see_through & front(&masks.see_through, column) & !BORDER_X;
                while same != 0 {
                    let x = same.trailing_zeros() as usize;
                    same &= same - 1;
                    let at = column * PAD + x;
                    let ahead = at.wrapping_add_signed(word_step);
                    if MATERIAL_FIELD.read(words[at]) == MATERIAL_FIELD.read(words[ahead]) {
                        see_through &= !(1 << x);
                    }
                }
                let air_front = front(&masks.air, column);
                // A waterlogged voxel shows its water's surface against air.
                let surface = masks.wet[column] & air_front;
                see_through &= !surface;
                let mut opaque = (masks.solid[column] & !solid_front) | see_through;
                let dry_front = front(&masks.see_through, column) & !front(&masks.wet, column);
                let mut water = (masks.water[column] & (air_front | dry_front)) | surface;
                if culled {
                    let open = front(reach, column);
                    opaque &= open;
                    water &= open;
                }
                let opaque = (opaque >> 1) as u32;
                let water = (water >> 1) as u32;
                shown[0] |= opaque != 0;
                shown[1] |= water != 0;
                // X faces collect [z][y] runs along X and are transposed below.
                let (layer, v) = if face.axis() == 1 { (y, z) } else { (z, y) };
                rows[0][layer][v] = opaque;
                rows[1][layer][v] = water;
            }
        }
        if face.axis() == 0 {
            for (bucket, &any) in shown.iter().enumerate() {
                if !any {
                    continue;
                }
                for z in 0..SECTION_SIDE {
                    // [z][y] bits x become [x] bits y: rows of layer x at v = z.
                    let mut runs = self.x_rows[bucket][z];
                    greedy::transpose(&mut runs);
                    for (x, row) in runs.into_iter().enumerate() {
                        self.rows[bucket][x][z] = row;
                    }
                }
            }
        }
        shown
    }
}
