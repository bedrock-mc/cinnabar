use assets::{BlockFlags, NetworkIdMode, RuntimeAssets};
use world::MeshNeighbourhood;

use crate::{
    BlockClassifier, DiagnosticGeometryCount, DiagnosticGeometrySummary, Face, PackedQuad,
    PackedQuadLighting, SIDE,
    contributors::{PaletteFacts, PaletteSource, ResolvedPaletteEntry},
};

#[derive(Default)]
pub(crate) struct DiagnosticGeometryAccumulator {
    counts: std::collections::BTreeMap<(Option<u32>, u32), u32>,
    omitted_identity_count: u32,
    omitted_quad_count: u64,
}

impl DiagnosticGeometryAccumulator {
    fn record(&mut self, entry: ResolvedPaletteEntry) {
        let key = (entry.sequential_id, entry.network_value);
        if let Some(count) = self.counts.get_mut(&key) {
            *count = count.saturating_add(1);
        } else if self.counts.len() < world::MAX_PALETTE_ENTRIES {
            self.counts.insert(key, 1);
        } else {
            self.omitted_identity_count = self.omitted_identity_count.saturating_add(1);
            self.omitted_quad_count = self.omitted_quad_count.saturating_add(1);
        }
    }

    pub(crate) fn finish(self) -> DiagnosticGeometrySummary {
        let mut summary = DiagnosticGeometrySummary::from_counts(self.counts.into_iter().map(
            |((sequential_id, network_id), quad_count)| {
                DiagnosticGeometryCount::new(sequential_id, network_id, quad_count)
            },
        ));
        summary.add_omitted(self.omitted_identity_count, self.omitted_quad_count);
        summary
    }
}

pub(crate) struct CubeMeshOutput<'a> {
    quads: &'a mut Vec<PackedQuad>,
    lighting: &'a mut Vec<PackedQuadLighting>,
    diagnostic_geometry: &'a mut DiagnosticGeometryAccumulator,
}

impl<'a> CubeMeshOutput<'a> {
    pub(crate) fn new(
        quads: &'a mut Vec<PackedQuad>,
        lighting: &'a mut Vec<PackedQuadLighting>,
        diagnostic_geometry: &'a mut DiagnosticGeometryAccumulator,
    ) -> Self {
        Self {
            quads,
            lighting,
            diagnostic_geometry,
        }
    }
}

type Columns = [[u64; SIDE]; SIDE];
const FULL_COLUMN: u64 = (1_u64 << SIDE) - 1;

struct AxisColumns {
    x: Columns,
    y: Columns,
    z: Columns,
}

impl AxisColumns {
    const fn empty() -> Self {
        Self {
            x: [[0; SIDE]; SIDE],
            y: [[0; SIDE]; SIDE],
            z: [[0; SIDE]; SIDE],
        }
    }

    const fn full() -> Self {
        Self {
            x: [[FULL_COLUMN; SIDE]; SIDE],
            y: [[FULL_COLUMN; SIDE]; SIDE],
            z: [[FULL_COLUMN; SIDE]; SIDE],
        }
    }

    fn set(&mut self, x: usize, y: usize, z: usize) {
        self.x[y][z] |= 1 << x;
        self.y[x][z] |= 1 << y;
        self.z[x][y] |= 1 << z;
    }

    const fn column(&self, face: Face, u: usize, v: usize) -> u64 {
        match face {
            Face::NegativeX | Face::PositiveX => self.x[v][u],
            Face::NegativeY | Face::PositiveY => self.y[u][v],
            Face::NegativeZ | Face::PositiveZ => self.z[u][v],
        }
    }
}

pub(crate) struct VisibilityMasks {
    geometry: AxisColumns,
    occluders: AxisColumns,
    leaves: AxisColumns,
}

impl VisibilityMasks {
    pub(crate) fn from_facts(facts: &PaletteFacts<'_>) -> Self {
        match &facts.source {
            PaletteSource::Air => Self {
                geometry: AxisColumns::empty(),
                occluders: AxisColumns::empty(),
                leaves: AxisColumns::empty(),
            },
            PaletteSource::Uniform(contributors) => {
                let entry = contributors.geometry_entry();
                Self {
                    geometry: if entry.emits_cube_geometry() {
                        AxisColumns::full()
                    } else {
                        AxisColumns::empty()
                    },
                    occluders: if entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE) {
                        AxisColumns::full()
                    } else {
                        AxisColumns::empty()
                    },
                    leaves: if entry.flags.contains(BlockFlags::LEAF_MODEL) {
                        AxisColumns::full()
                    } else {
                        AxisColumns::empty()
                    },
                }
            }
            PaletteSource::Mixed(_) => {
                let mut masks = Self {
                    geometry: AxisColumns::empty(),
                    occluders: AxisColumns::empty(),
                    leaves: AxisColumns::empty(),
                };
                for x in 0..SIDE {
                    for y in 0..SIDE {
                        for z in 0..SIDE {
                            let entry = facts.at(x, y, z);
                            if entry.emits_cube_geometry() {
                                masks.geometry.set(x, y, z);
                            }
                            if entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE) {
                                masks.occluders.set(x, y, z);
                            }
                            if entry.flags.contains(BlockFlags::LEAF_MODEL) {
                                masks.leaves.set(x, y, z);
                            }
                        }
                    }
                }
                masks
            }
        }
    }
}

pub(crate) fn exposed_columns(
    classifier: BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    face: Face,
    facts: &PaletteFacts<'_>,
    masks: &VisibilityMasks,
) -> Columns {
    let neighbour = neighbourhood
        .sub_chunk(face_offset(face))
        .map(|sub_chunk| PaletteFacts::new(classifier, visuals, network_id_mode, sub_chunk));
    let boundary_bit = if face.is_negative() {
        1_u64
    } else {
        1_u64 << (SIDE - 1)
    };
    let mut exposed = [[0_u64; SIDE]; SIDE];

    for (v, exposed_row) in exposed.iter_mut().enumerate() {
        for (u, exposed_cell) in exposed_row.iter_mut().enumerate() {
            let geometry_column = masks.geometry.column(face, u, v);
            let occluder_column = masks.occluders.column(face, u, v);
            let leaf_column = masks.leaves.column(face, u, v);
            let neighbour_occluders = if face.is_negative() {
                occluder_column << 1
            } else {
                occluder_column >> 1
            };
            let neighbour_leaves = if face.is_negative() {
                leaf_column << 1
            } else {
                leaf_column >> 1
            };
            let leaf_pairs = leaf_column & neighbour_leaves;
            let mut faces = geometry_column & !neighbour_occluders & !leaf_pairs & FULL_COLUMN;

            if faces & boundary_bit != 0 {
                let slice = if face.is_negative() { 0 } else { SIDE - 1 };
                let [source_x, source_y, source_z] = block_coordinate(face, slice, u, v);
                let source = facts.at(source_x, source_y, source_z);
                let neighbour = neighbour
                    .as_ref()
                    .map_or(ResolvedPaletteEntry::AIR, |facts| {
                        let [x, y, z] = neighbour_boundary_coordinate(face, u, v);
                        facts.at(x, y, z)
                    });
                if culls_face(source.flags, neighbour.flags) {
                    faces &= !boundary_bit;
                }
            }
            *exposed_cell = faces;
        }
    }
    exposed
}

pub(crate) const fn face_offset(face: Face) -> [i8; 3] {
    match face {
        Face::NegativeX => [-1, 0, 0],
        Face::PositiveX => [1, 0, 0],
        Face::NegativeY => [0, -1, 0],
        Face::PositiveY => [0, 1, 0],
        Face::NegativeZ => [0, 0, -1],
        Face::PositiveZ => [0, 0, 1],
    }
}

const fn culls_face(source: BlockFlags, neighbour: BlockFlags) -> bool {
    neighbour.contains(BlockFlags::OCCLUDES_FULL_FACE)
        || (source.contains(BlockFlags::LEAF_MODEL) && neighbour.contains(BlockFlags::LEAF_MODEL))
}

const fn neighbour_boundary_coordinate(face: Face, u: usize, v: usize) -> [usize; 3] {
    match face {
        Face::NegativeX => [SIDE - 1, v, u],
        Face::PositiveX => [0, v, u],
        Face::NegativeY => [u, SIDE - 1, v],
        Face::PositiveY => [u, 0, v],
        Face::NegativeZ => [u, v, SIDE - 1],
        Face::PositiveZ => [u, v, 0],
    }
}

pub(crate) fn greedy_slice(
    facts: &PaletteFacts<'_>,
    face: Face,
    slice: usize,
    rows: &mut [u64; SIDE],
    lighting_scratch: &[PackedQuadLighting; SIDE * SIDE],
    output: &mut CubeMeshOutput<'_>,
) {
    for v in 0..SIDE {
        while rows[v] != 0 {
            let u = rows[v].trailing_zeros() as usize;
            let origin = block_coordinate(face, slice, u, v);
            let origin_entry = facts.at(origin[0], origin[1], origin[2]);
            let material_id = origin_entry.faces[face.index()];
            let lighting = lighting_scratch[v * SIDE + u];

            let shifted = rows[v] >> u;
            let binary_width = (!shifted).trailing_zeros() as usize;
            let binary_width = binary_width.min(SIDE - u);
            let mut width = 1;
            while width < binary_width && {
                let [x, y, z] = block_coordinate(face, slice, u + width, v);
                same_greedy_identity(origin_entry, facts.at(x, y, z), face)
                    && lighting_scratch[v * SIDE + u + width] == lighting
            } {
                width += 1;
            }

            let span = ((1_u64 << width) - 1) << u;
            let mut height = 1;
            'height: while v + height < SIDE && rows[v + height] & span == span {
                for offset in 0..width {
                    let [x, y, z] = block_coordinate(face, slice, u + offset, v + height);
                    if !same_greedy_identity(origin_entry, facts.at(x, y, z), face) {
                        break 'height;
                    }
                    if lighting_scratch[(v + height) * SIDE + u + offset] != lighting {
                        break 'height;
                    }
                }
                height += 1;
            }

            for row in &mut rows[v..v + height] {
                *row &= !span;
            }
            output.quads.push(PackedQuad::new(
                origin.map(|coordinate| coordinate as u8),
                face,
                width as u8,
                height as u8,
                material_id,
            ));
            output.lighting.push(lighting);
            if material_id == assets::DIAGNOSTIC_MATERIAL {
                output.diagnostic_geometry.record(origin_entry);
            }
        }
    }
}

fn same_greedy_identity(
    origin: ResolvedPaletteEntry,
    candidate: ResolvedPaletteEntry,
    face: Face,
) -> bool {
    let origin_material = origin.faces[face.index()];
    origin_material == candidate.faces[face.index()]
        && (origin_material != assets::DIAGNOSTIC_MATERIAL
            || (origin.network_value == candidate.network_value
                && origin.sequential_id == candidate.sequential_id))
}

pub(crate) const fn block_coordinate(face: Face, slice: usize, u: usize, v: usize) -> [usize; 3] {
    match face {
        Face::NegativeX | Face::PositiveX => [slice, v, u],
        Face::NegativeY | Face::PositiveY => [u, slice, v],
        Face::NegativeZ | Face::PositiveZ => [u, v, slice],
    }
}
