/// A bounded Bedrock liquid level. Raw values 8..=15 are falling states and
/// retain their raw effective depth while rendering at source surface height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidLevel {
    depth: u8,
    height: u8,
    falling: bool,
}

impl LiquidLevel {
    pub const FULL_HEIGHT: u8 = u8::MAX;

    #[must_use]
    pub const fn from_variant(variant: u32) -> Option<Self> {
        if variant > 15 {
            return None;
        }
        let falling = variant >= 8;
        let depth = (variant & 7) as u8;
        let height = if falling {
            227
        } else {
            (((8 - depth as u16) * 255 + 4) / 9) as u8
        };
        Some(Self {
            depth,
            height,
            falling,
        })
    }

    #[must_use]
    pub const fn depth(self) -> u8 {
        self.depth
    }
    #[must_use]
    pub const fn height(self) -> u8 {
        self.height
    }
    #[must_use]
    pub const fn is_falling(self) -> bool {
        self.falling
    }

    #[must_use]
    pub const fn effective_depth(self) -> u8 {
        if self.falling { 0 } else { self.depth }
    }
}

use assets::{
    BlockFace, BlockFlags, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_WATER_TINT, NetworkIdMode, RuntimeAssets,
    VisualKind,
};
use world::MeshNeighbourhood;

use crate::CameraMedium;
use crate::mesh::{
    BlockClassifier, ContributorResolver, Face, PackedLiquidQuad, PackedQuadLighting,
    ResolvedContributors,
};

const SIDE: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
struct LiquidIdentity([u32; Face::ALL.len()]);

#[derive(Clone, Copy)]
struct LiquidCell {
    identity: LiquidIdentity,
    face_materials: [u32; Face::ALL.len()],
    level: LiquidLevel,
    depth_writing: bool,
}

impl LiquidCell {
    const fn material(self, face: Face) -> u32 {
        self.face_materials[face as usize]
    }

    const fn top_material(self, flowing: bool) -> u32 {
        if flowing {
            self.material(Face::NegativeX)
        } else {
            self.material(Face::PositiveY)
        }
    }
}

struct Sampler<'chunk, 'assets> {
    resolvers: [Option<ContributorResolver<'chunk>>; 27],
    assets: &'assets RuntimeAssets,
}

impl<'chunk, 'assets> Sampler<'chunk, 'assets> {
    fn new(
        classifier: BlockClassifier,
        assets: &'assets RuntimeAssets,
        mode: NetworkIdMode,
        neighbourhood: &MeshNeighbourhood<'chunk>,
    ) -> Self {
        let mut resolvers = std::array::from_fn(|_| None);
        for (offset, chunk) in neighbourhood.liquid_sub_chunks() {
            if let Some(chunk) = chunk {
                resolvers[offset_index(offset)] =
                    Some(ContributorResolver::new(classifier, assets, mode, chunk));
            }
        }
        Self { resolvers, assets }
    }
}

trait LiquidSampler {
    fn assets(&self) -> &RuntimeAssets;

    fn contributors(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<ResolvedContributors>;

    fn liquid(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<LiquidCell> {
        let entry = self
            .contributors(neighbourhood, coordinate)?
            .liquid_entry()?;
        let depth_writing = supported_liquid_material_family(self.assets(), entry.faces)?;
        (entry.kind == VisualKind::Liquid).then_some(LiquidCell {
            identity: LiquidIdentity(entry.faces),
            face_materials: entry.faces,
            level: LiquidLevel::from_variant(entry.variant)?,
            depth_writing,
        })
    }

    fn open(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
        contacting_faces: &[Face],
    ) -> bool {
        self.contributors(neighbourhood, coordinate)
            .is_none_or(|contributors| {
                contributors.liquid_entry().is_none()
                    && !contributors.primary_entry().is_some_and(|entry| {
                        entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
                            && contacting_faces.iter().all(|&face| {
                                material_is_opaque(self.assets(), entry.faces[face as usize])
                            })
                    })
            })
    }

    fn solid(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
        contacting_face: Face,
    ) -> bool {
        self.contributors(neighbourhood, coordinate)
            .and_then(ResolvedContributors::primary_entry)
            .is_some_and(|entry| {
                entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
                    && material_is_opaque(self.assets(), entry.faces[contacting_face as usize])
            })
    }
}

impl LiquidSampler for Sampler<'_, '_> {
    fn assets(&self) -> &RuntimeAssets {
        self.assets
    }

    fn contributors(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<ResolvedContributors> {
        let (_, local) = neighbourhood.liquid_block_source(coordinate)?;
        let offset = coordinate
            .map(|value| i8::try_from(value.div_euclid(16)).ok())
            .map(Option::unwrap);
        self.resolvers[offset_index(offset)]
            .as_ref()
            .map(|resolver| resolver.resolve(local))
    }
}

struct DirectSampler<'assets> {
    classifier: BlockClassifier,
    assets: &'assets RuntimeAssets,
    mode: NetworkIdMode,
}

impl LiquidSampler for DirectSampler<'_> {
    fn assets(&self) -> &RuntimeAssets {
        self.assets
    }

    fn contributors(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
    ) -> Option<ResolvedContributors> {
        let (sub_chunk, local) = neighbourhood.liquid_block_source(coordinate)?;
        Some(ContributorResolver::resolve_direct(
            self.classifier,
            self.assets,
            self.mode,
            sub_chunk,
            local,
        ))
    }
}

fn material_is_opaque(assets: &RuntimeAssets, material: u32) -> bool {
    assets.material(material).flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT) == 0
}

const fn opposite_face(face: Face) -> Face {
    match face {
        Face::NegativeX => Face::PositiveX,
        Face::PositiveX => Face::NegativeX,
        Face::NegativeY => Face::PositiveY,
        Face::PositiveY => Face::NegativeY,
        Face::NegativeZ => Face::PositiveZ,
        Face::PositiveZ => Face::NegativeZ,
    }
}

fn horizontal_contacting_faces([x, z]: [i32; 2]) -> ([Face; 2], usize) {
    let mut faces = [Face::PositiveY; 2];
    let mut count = 0;
    if x < 0 {
        faces[count] = Face::PositiveX;
        count += 1;
    } else if x > 0 {
        faces[count] = Face::NegativeX;
        count += 1;
    }
    if z < 0 {
        faces[count] = Face::PositiveZ;
        count += 1;
    } else if z > 0 {
        faces[count] = Face::NegativeZ;
        count += 1;
    }
    (faces, count)
}

fn supported_liquid_material_family(
    assets: &RuntimeAssets,
    materials: [u32; Face::ALL.len()],
) -> Option<bool> {
    if materials
        .into_iter()
        .all(|material| water_material(assets, material))
    {
        Some(false)
    } else if materials
        .into_iter()
        .all(|material| depth_writing_liquid_material(assets, material))
    {
        Some(true)
    } else {
        None
    }
}

fn water_material(assets: &RuntimeAssets, material: u32) -> bool {
    let required = MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT;
    assets.material(material).flags & required == required
}

fn depth_writing_liquid_material(assets: &RuntimeAssets, material: u32) -> bool {
    assets.material(material).flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE != 0
}

/// Resolves the visual medium at a camera-eye position in one packed liquid
/// neighbourhood. Surface height uses the same corner solver as liquid mesh
/// generation, so entering fog matches the rendered water/lava boundary.
#[must_use]
pub fn sample_camera_medium(
    classifier: BlockClassifier,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    local_position: [f32; 3],
) -> CameraMedium {
    if !local_position.iter().all(|value| value.is_finite()) {
        return CameraMedium::Air;
    }
    let block = local_position.map(|value| value.floor() as i32);
    let sampler = DirectSampler {
        classifier,
        assets,
        mode,
    };
    let Some(cell) = sampler.liquid(neighbourhood, block) else {
        return CameraMedium::Air;
    };
    let heights = corner_heights(&sampler, neighbourhood, block, cell.identity);
    let x = local_position[0].rem_euclid(1.0);
    let z = local_position[2].rem_euclid(1.0);
    let surface = triangulated_surface_height(heights, x, z) / 255.0;
    if local_position[1].rem_euclid(1.0) >= surface {
        return CameraMedium::Air;
    }
    if cell.depth_writing {
        CameraMedium::Lava
    } else {
        CameraMedium::Water
    }
}

pub(crate) fn mesh_liquids<S: crate::lighting::MeshLightSampler + ?Sized>(
    classifier: BlockClassifier,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    light_sampler: &S,
) -> (Vec<PackedLiquidQuad>, Vec<PackedQuadLighting>) {
    let center = neighbourhood
        .sub_chunk([0, 0, 0])
        .expect("MeshNeighbourhood always contains its center");
    let contains_liquid = center.storages().iter().any(|storage| {
        storage
            .palette()
            .values()
            .iter()
            .copied()
            .any(|network_value| {
                if classifier.is_air(network_value) {
                    return false;
                }
                let block = assets.resolve(mode, network_value);
                block.kind() == VisualKind::Liquid
                    && supported_liquid_material_family(
                        assets,
                        BlockFace::ALL.map(|face| block.face(face).material_id()),
                    )
                    .is_some()
            })
    });
    if !contains_liquid {
        return (Vec::new(), Vec::new());
    }
    let sampler = Sampler::new(classifier, assets, mode, neighbourhood);
    let mut transparent_quads = Vec::new();
    let mut depth_quads = Vec::new();
    let mut push_quad = |quad: PackedLiquidQuad| {
        if quad.is_depth_writing() {
            depth_quads.push(quad);
        } else {
            transparent_quads.push(quad);
        }
    };
    for x in 0..SIDE {
        for y in 0..SIDE {
            for z in 0..SIDE {
                let block = [x as i32, y as i32, z as i32];
                let Some(cell) = sampler.liquid(neighbourhood, block) else {
                    continue;
                };
                let heights = corner_heights(&sampler, neighbourhood, block, cell.identity);
                let gradient = flow_gradient(&sampler, neighbourhood, block, cell);
                let origin = [x as u8, y as u8, z as u8];
                let above = add(block, [0, 1, 0]);
                if !compatible(&sampler, neighbourhood, above, cell.identity)
                    && !sampler.solid(neighbourhood, above, Face::NegativeY)
                {
                    let material = cell.top_material(gradient != [0, 0]);
                    push_quad(pack(
                        origin,
                        Face::PositiveY,
                        heights,
                        material,
                        gradient,
                        cell.level,
                        cell.depth_writing,
                    ));
                }
                for face in [
                    Face::NegativeX,
                    Face::PositiveX,
                    Face::NegativeZ,
                    Face::PositiveZ,
                ] {
                    let adjacent = add(block, face_offset(face));
                    if compatible(&sampler, neighbourhood, adjacent, cell.identity)
                        || sampler.solid(neighbourhood, adjacent, opposite_face(face))
                    {
                        continue;
                    }
                    let side_heights = match face {
                        Face::NegativeX => [0, heights[0], heights[3], 0],
                        Face::PositiveX => [0, heights[2], heights[1], 0],
                        Face::NegativeZ => [0, heights[1], heights[0], 0],
                        Face::PositiveZ => [0, heights[3], heights[2], 0],
                        _ => unreachable!(),
                    };
                    push_quad(pack(
                        origin,
                        face,
                        side_heights,
                        cell.material(face),
                        gradient,
                        cell.level,
                        cell.depth_writing,
                    ));
                }
                let below = add(block, [0, -1, 0]);
                if !compatible(&sampler, neighbourhood, below, cell.identity)
                    && !sampler.solid(neighbourhood, below, Face::PositiveY)
                {
                    push_quad(pack(
                        origin,
                        Face::NegativeY,
                        [0; 4],
                        cell.material(Face::NegativeY),
                        gradient,
                        cell.level,
                        cell.depth_writing,
                    ));
                }
            }
        }
    }
    transparent_quads.reserve(depth_quads.len());
    transparent_quads.append(&mut depth_quads);
    let mut addressed = Vec::with_capacity(transparent_quads.len());
    let mut lighting = Vec::with_capacity(transparent_quads.len());
    for quad in transparent_quads {
        let index = lighting.len() as u32;
        let block = quad.origin().map(i32::from);
        lighting.push(crate::lighting::bake_quad_lighting_with_sampler(
            &classifier,
            assets,
            mode,
            neighbourhood,
            light_sampler,
            block,
            quad.face(),
            lighting_positions(quad.face(), quad.heights()),
        ));
        addressed.push(
            PackedLiquidQuad::try_pack(
                quad.origin(),
                quad.face(),
                quad.heights(),
                quad.material_id(),
                index,
                quad.flow_gradient(),
                quad.is_falling(),
            )
            .map(|packed| packed.with_depth_write(quad.is_depth_writing()))
            .expect("previously checked liquid record"),
        );
    }
    (addressed, lighting)
}

fn pack(
    origin: [u8; 3],
    face: Face,
    heights: [u8; 4],
    material: u32,
    gradient: [i8; 2],
    level: LiquidLevel,
    depth_writing: bool,
) -> PackedLiquidQuad {
    PackedLiquidQuad::try_pack(
        origin,
        face,
        heights,
        material,
        0,
        gradient,
        level.is_falling(),
    )
    .map(|packed| packed.with_depth_write(depth_writing))
    .expect("local liquid record is bounded")
}

fn flow_gradient<S: LiquidSampler + ?Sized>(
    sampler: &S,
    neighbourhood: &MeshNeighbourhood<'_>,
    block: [i32; 3],
    cell: LiquidCell,
) -> [i8; 2] {
    let current = i16::from(cell.level.effective_depth());
    let mut gradient = [0_i16; 2];
    for offset in [[-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1]] {
        let adjacent = add(block, offset);
        let delta = if let Some(other) = sampler.liquid(neighbourhood, adjacent) {
            (other.identity == cell.identity)
                .then(|| i16::from(other.level.effective_depth()) - current)
        } else if sampler.open(
            neighbourhood,
            adjacent,
            &horizontal_contacting_faces([offset[0], offset[2]]).0[..1],
        ) {
            sampler
                .liquid(neighbourhood, add(adjacent, [0, -1, 0]))
                .filter(|below| below.identity == cell.identity)
                .map(|below| i16::from(below.level.effective_depth()) - current + 8)
        } else {
            None
        };
        if let Some(delta) = delta {
            gradient[0] += (offset[0] as i16) * delta;
            gradient[1] += (offset[2] as i16) * delta;
        }
    }
    [gradient[0] as i8, gradient[1] as i8]
}

fn corner_heights<S: LiquidSampler + ?Sized>(
    sampler: &S,
    neighbourhood: &MeshNeighbourhood<'_>,
    block: [i32; 3],
    identity: LiquidIdentity,
) -> [u8; 4] {
    [
        ([0, 0], [-1, 0], [0, -1], [-1, -1]),
        ([0, 0], [1, 0], [0, -1], [1, -1]),
        ([0, 0], [1, 0], [0, 1], [1, 1]),
        ([0, 0], [-1, 0], [0, 1], [-1, 1]),
    ]
    .map(|(center, a, b, diagonal)| {
        let include_diagonal = compatible(
            sampler,
            neighbourhood,
            add(block, [a[0], 0, a[1]]),
            identity,
        ) || compatible(
            sampler,
            neighbourhood,
            add(block, [b[0], 0, b[1]]),
            identity,
        );
        let samples = [
            Some(center),
            Some(a),
            Some(b),
            include_diagonal.then_some(diagonal),
        ];
        if samples
            .iter()
            .flatten()
            .any(|[x, z]| compatible(sampler, neighbourhood, add(block, [*x, 1, *z]), identity))
        {
            return LiquidLevel::FULL_HEIGHT;
        }
        let mut total = 0_u32;
        let mut weight = 0_u32;
        for [x, z] in samples.into_iter().flatten() {
            let coordinate = add(block, [x, 0, z]);
            if let Some(cell) = sampler.liquid(neighbourhood, coordinate) {
                if cell.identity != identity {
                    continue;
                }
                let sample_weight = if cell.level.height() >= 204 { 10 } else { 1 };
                total += u32::from(cell.level.height()) * sample_weight;
                weight += sample_weight;
            } else {
                let (contacting_faces, count) = horizontal_contacting_faces([x, z]);
                if sampler.open(neighbourhood, coordinate, &contacting_faces[..count]) {
                    weight += 1;
                }
            }
        }
        if weight == 0 {
            0
        } else {
            ((total + weight / 2) / weight) as u8
        }
    })
}

fn compatible<S: LiquidSampler + ?Sized>(
    sampler: &S,
    neighbourhood: &MeshNeighbourhood<'_>,
    coordinate: [i32; 3],
    identity: LiquidIdentity,
) -> bool {
    sampler
        .liquid(neighbourhood, coordinate)
        .is_some_and(|cell| cell.identity == identity)
}

fn triangulated_surface_height(heights: [u8; 4], x: f32, z: f32) -> f32 {
    let [north_west, north_east, south_east, south_west] = heights.map(f32::from);
    if z <= x {
        north_west + x * (north_east - north_west) + z * (south_east - north_east)
    } else {
        north_west + x * (south_east - south_west) + z * (south_west - north_west)
    }
}
const fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

const fn face_offset(face: Face) -> [i32; 3] {
    match face {
        Face::NegativeX => [-1, 0, 0],
        Face::PositiveX => [1, 0, 0],
        Face::NegativeY => [0, -1, 0],
        Face::PositiveY => [0, 1, 0],
        Face::NegativeZ => [0, 0, -1],
        Face::PositiveZ => [0, 0, 1],
    }
}
const fn offset_index([x, y, z]: [i8; 3]) -> usize {
    ((x + 1) as usize) * 9 + ((y + 1) as usize) * 3 + (z + 1) as usize
}

fn lighting_positions(face: Face, heights: [u8; 4]) -> [[i16; 3]; 4] {
    let h = heights.map(i16::from);
    // Packed vertex order is part of the transparent-stream contract:
    // top NW/NE/SE/SW; bottom NW/SW/SE/NE;
    // -X bottom-N/top-N/top-S/bottom-S;
    // +X bottom-S/top-S/top-N/bottom-N;
    // -Z bottom-E/top-E/top-W/bottom-W;
    // +Z bottom-W/top-W/top-E/bottom-E.
    match face {
        Face::PositiveY => [
            [0, h[0], 0],
            [256, h[1], 0],
            [256, h[2], 256],
            [0, h[3], 256],
        ],
        Face::NegativeY => [[0, 0, 0], [0, 0, 256], [256, 0, 256], [256, 0, 0]],
        Face::NegativeX => [[0, h[0], 0], [0, h[1], 0], [0, h[2], 256], [0, h[3], 256]],
        Face::PositiveX => [
            [256, h[0], 256],
            [256, h[1], 256],
            [256, h[2], 0],
            [256, h[3], 0],
        ],
        Face::NegativeZ => [[256, h[0], 0], [256, h[1], 0], [0, h[2], 0], [0, h[3], 0]],
        Face::PositiveZ => [
            [0, h[0], 256],
            [0, h[1], 256],
            [256, h[2], 256],
            [256, h[3], 256],
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::triangulated_surface_height;

    #[test]
    fn camera_surface_matches_the_two_static_index_buffer_triangles() {
        let heights = [0, 255, 0, 0];
        assert_eq!(triangulated_surface_height(heights, 0.75, 0.25), 127.5);
        assert_eq!(triangulated_surface_height(heights, 0.25, 0.75), 0.0);
        assert_eq!(triangulated_surface_height(heights, 0.5, 0.5), 0.0);
    }
}
