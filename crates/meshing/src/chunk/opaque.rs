use std::cell::OnceCell;

use assets::BlockFlags;

use crate::{
    CubeQuadLayout, DiagnosticGeometryCount, DiagnosticGeometrySummary, Face, PackedQuad,
    PackedQuadLighting, SIDE,
    contributors::{PaletteFacts, PaletteSource, ResolvedPaletteEntry},
};

use super::cube_materials::CubeMaterialResolver;
use super::models::PaletteResolutionContext;

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

/// Cube quads split by draw path, each kept in emission order.
#[derive(Default)]
pub(crate) struct CubeQuadStreams {
    solid: Vec<PackedQuad>,
    solid_lighting: Vec<PackedQuadLighting>,
    solid_counts: [u32; 6],
    two_sided: Vec<PackedQuad>,
    two_sided_lighting: Vec<PackedQuadLighting>,
}

impl CubeQuadStreams {
    /// Solid quads must arrive grouped by face in `Face::ALL` order.
    fn push(&mut self, quad: PackedQuad, lighting: PackedQuadLighting, solid: bool) {
        if solid {
            self.solid_counts[quad.face().index()] += 1;
            self.solid.push(quad);
            self.solid_lighting.push(lighting);
        } else {
            self.two_sided.push(quad);
            self.two_sided_lighting.push(lighting);
        }
    }

    pub(crate) fn finish(self) -> (Vec<PackedQuad>, Vec<PackedQuadLighting>, CubeQuadLayout) {
        let len = self.solid.len() + self.two_sided.len();
        let mut quads = Vec::with_capacity(len);
        let mut lighting = Vec::with_capacity(len);
        for face in CubeQuadLayout::SOLID_FACE_ORDER {
            let start = self.solid_counts[..face.index()].iter().sum::<u32>() as usize;
            let range = start..start + self.solid_counts[face.index()] as usize;
            quads.extend_from_slice(&self.solid[range.clone()]);
            lighting.extend_from_slice(&self.solid_lighting[range]);
        }
        quads.extend(self.two_sided);
        lighting.extend(self.two_sided_lighting);
        (
            quads,
            lighting,
            CubeQuadLayout::from_solid_counts(self.solid_counts),
        )
    }
}

pub(crate) struct CubeMeshOutput<'a> {
    streams: &'a mut CubeQuadStreams,
    diagnostic_geometry: &'a mut DiagnosticGeometryAccumulator,
    materials: &'a [assets::Material],
}

impl<'a> CubeMeshOutput<'a> {
    pub(crate) fn new(
        streams: &'a mut CubeQuadStreams,
        diagnostic_geometry: &'a mut DiagnosticGeometryAccumulator,
        materials: &'a [assets::Material],
    ) -> Self {
        Self {
            streams,
            diagnostic_geometry,
            materials,
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
}

impl VisibilityMasks {
    pub(crate) fn from_facts(facts: &PaletteFacts<'_>) -> Self {
        match &facts.source {
            PaletteSource::Air => Self {
                geometry: AxisColumns::empty(),
                occluders: AxisColumns::empty(),
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
                }
            }
            PaletteSource::Mixed(_) => {
                let mut masks = Self {
                    geometry: AxisColumns::empty(),
                    occluders: AxisColumns::empty(),
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
                        }
                    }
                }
                masks
            }
        }
    }
}

pub(crate) fn exposed_columns<'a>(
    context: PaletteResolutionContext<'_, 'a>,
    face: Face,
    masks: &VisibilityMasks,
    neighbour_facts: &[OnceCell<PaletteFacts<'a>>; Face::ALL.len()],
    leaves: &super::leaves::LeafOcclusion<'_, 'a>,
) -> Columns {
    let neighbour = context
        .neighbourhood
        .sub_chunk(face_offset(face))
        .map(|sub_chunk| {
            neighbour_facts[face.index()].get_or_init(|| {
                PaletteFacts::new(
                    context.classifier,
                    context.visuals,
                    context.network_id_mode,
                    sub_chunk,
                )
            })
        });
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
            let neighbour_occluders = if face.is_negative() {
                occluder_column << 1
            } else {
                occluder_column >> 1
            };
            let mut faces = geometry_column & !neighbour_occluders & FULL_COLUMN;

            if faces & boundary_bit != 0 {
                let neighbour = neighbour
                    .as_ref()
                    .map_or(ResolvedPaletteEntry::AIR, |facts| {
                        let [x, y, z] = neighbour_boundary_coordinate(face, u, v);
                        facts.at(x, y, z)
                    });
                if culls_face(neighbour.flags) {
                    faces &= !boundary_bit;
                }
            }
            // Native leaf adjacency is directional, not the full-solid mask.
            // Do not emit both coincident planes when cutout material is two-sided.
            let mut candidates = faces;
            while candidates != 0 {
                let slice = candidates.trailing_zeros() as usize;
                candidates &= candidates - 1;
                if leaves.culls_face(block_coordinate(face, slice, u, v), face) {
                    faces &= !(1_u64 << slice);
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

const fn culls_face(neighbour: BlockFlags) -> bool {
    neighbour.contains(BlockFlags::OCCLUDES_FULL_FACE)
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
    resolver: &CubeMaterialResolver<'_, '_, '_>,
    face: Face,
    slice: usize,
    rows: &mut [u64; SIDE],
    lighting_scratch: &[PackedQuadLighting; SIDE * SIDE],
    output: &mut CubeMeshOutput<'_>,
) {
    let facts = resolver.facts;
    for v in 0..SIDE {
        while rows[v] != 0 {
            let u = rows[v].trailing_zeros() as usize;
            let origin = block_coordinate(face, slice, u, v);
            let origin_entry = facts.at(origin[0], origin[1], origin[2]);
            let material_id = resolver.face_material(origin, origin_entry, face);
            let lighting = lighting_scratch[v * SIDE + u];
            let material = output.materials[material_id as usize];
            // Each isotropic face owns its block-position hash. Merging even
            // identical material/light records would repeat one block's UVs.
            let positional = material.variation_count > 1
                || material.flags & assets::MATERIAL_FLAG_ISOTROPIC != 0
                // Native leaf faces have block-local rotations and clamped
                // atlas edges. A merged quad would stretch/clamp its mask.
                || material.flags & assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR != 0;

            let shifted = rows[v] >> u;
            let binary_width = (!shifted).trailing_zeros() as usize;
            let binary_width = binary_width.min(SIDE - u);
            let mut width = 1;
            while !positional && width < binary_width && {
                let [x, y, z] = block_coordinate(face, slice, u + width, v);
                let candidate = facts.at(x, y, z);
                let candidate_material = resolver.face_material([x, y, z], candidate, face);
                same_greedy_identity(origin_entry, candidate, material_id, candidate_material)
                    && lighting_scratch[v * SIDE + u + width] == lighting
            } {
                width += 1;
            }

            let span = ((1_u64 << width) - 1) << u;
            let mut height = 1;
            'height: while !positional && v + height < SIDE && rows[v + height] & span == span {
                for offset in 0..width {
                    let [x, y, z] = block_coordinate(face, slice, u + offset, v + height);
                    let candidate = facts.at(x, y, z);
                    let candidate_material = resolver.face_material([x, y, z], candidate, face);
                    if !same_greedy_identity(
                        origin_entry,
                        candidate,
                        material_id,
                        candidate_material,
                    ) {
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
            output.streams.push(
                PackedQuad::new(
                    origin.map(|coordinate| coordinate as u8),
                    face,
                    width as u8,
                    height as u8,
                    material_id,
                ),
                lighting,
                crate::is_single_sided_opaque(output.materials, material_id),
            );
            if material_id == assets::DIAGNOSTIC_MATERIAL {
                output.diagnostic_geometry.record(origin_entry);
            }
        }
    }
}

fn same_greedy_identity(
    origin: ResolvedPaletteEntry,
    candidate: ResolvedPaletteEntry,
    origin_material: u32,
    candidate_material: u32,
) -> bool {
    origin_material == candidate_material
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
