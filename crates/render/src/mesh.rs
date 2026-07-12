use std::collections::VecDeque;

use assets::{
    BlockFace, BlockFlags, DIAGNOSTIC_MATERIAL, NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets,
    VisualKind,
};
use world::{MeshNeighbourhood, PalettedStorage, SubChunk};

const SIDE: usize = 16;
const FULL_COLUMN: u64 = (1_u64 << SIDE) - 1;
const CONNECTIVITY_MASK: u64 = (1_u64 << (Face::ALL.len() * Face::ALL.len())) - 1;

/// Registry facts needed by pure chunk meshing.
///
/// Air cannot be inferred from the numeric value: protocol 1001 may use a
/// sequential runtime ID or a high-bit block-state network hash. Callers must
/// therefore supply the air value advertised by their active registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockClassifier {
    air_network_id: u32,
}

impl BlockClassifier {
    #[must_use]
    pub const fn new(air_network_id: u32) -> Self {
        Self { air_network_id }
    }

    #[must_use]
    pub const fn air_network_id(self) -> u32 {
        self.air_network_id
    }

    #[must_use]
    pub const fn is_air(self, runtime_id: u32) -> bool {
        runtime_id == self.air_network_id
    }
}

/// Axis-aligned face identifiers used by packed quads and cave connectivity.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    NegativeX = 0,
    PositiveX = 1,
    NegativeY = 2,
    PositiveY = 3,
    NegativeZ = 4,
    PositiveZ = 5,
}

impl Face {
    pub const ALL: [Self; 6] = [
        Self::NegativeX,
        Self::PositiveX,
        Self::NegativeY,
        Self::PositiveY,
        Self::NegativeZ,
        Self::PositiveZ,
    ];

    const fn index(self) -> usize {
        self as usize
    }

    const fn is_negative(self) -> bool {
        matches!(self, Self::NegativeX | Self::NegativeY | Self::NegativeZ)
    }
}

/// A vertex-pulled greedy quad encoded as exactly two 32-bit words.
///
/// `geometry` packs local block origin X/Y/Z into bits 0..14 (five bits each),
/// face into bits 15..17, `(width - 1)` into bits 18..21, and
/// `(height - 1)` into bits 22..25. Bits 26..31 are reserved. The second word
/// stores the compact material-table ID resolved from the active asset set.
///
/// Width/height axes are Z/Y for X faces, X/Z for Y faces, and X/Y for Z
/// faces. A vertex shader can reconstruct all four corners from these fields
/// and a face-orientation lookup table.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedQuad {
    geometry: u32,
    material_id: u32,
}

/// One compact reference to an immutable global model template.
///
/// The first word contains the local position and transform selected by the
/// block-state resolver. The remaining words address the template, its first
/// lighting sidecar, and the visible template-quad/variant mask respectively.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PackedModelRef {
    packed_transform: u32,
    template_id: u32,
    lighting_base_index: u32,
    visible_quad_mask: u32,
}

impl PackedModelRef {
    #[must_use]
    pub const fn new(
        packed_transform: u32,
        template_id: u32,
        lighting_base_index: u32,
        visible_quad_mask: u32,
    ) -> Self {
        Self {
            packed_transform,
            template_id,
            lighting_base_index,
            visible_quad_mask,
        }
    }

    #[must_use]
    pub const fn words(self) -> [u32; 4] {
        [
            self.packed_transform,
            self.template_id,
            self.lighting_base_index,
            self.visible_quad_mask,
        ]
    }
}

/// Four face-specific packed light/AO samples in template-quad order.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PackedQuadLighting([u16; 4]);

impl PackedQuadLighting {
    #[must_use]
    pub const fn new(samples: [u16; 4]) -> Self {
        Self(samples)
    }

    #[must_use]
    pub const fn samples(self) -> [u16; 4] {
        self.0
    }
}

/// Fixed-size liquid surface/side geometry record.
///
/// Task 12 assigns the individual fixed-point corner-height, face, flow, and
/// material bit fields. Reserving four words here fixes queue and GPU addressing
/// without prematurely installing a liquid producer.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PackedLiquidQuad([u32; 4]);

impl PackedLiquidQuad {
    #[must_use]
    pub const fn new(words: [u32; 4]) -> Self {
        Self(words)
    }

    #[must_use]
    pub const fn words(self) -> [u32; 4] {
        self.0
    }
}

const _: () = assert!(std::mem::size_of::<PackedQuad>() == 8);
const _: () = assert!(std::mem::size_of::<PackedModelRef>() == 16);
const _: () = assert!(std::mem::size_of::<PackedQuadLighting>() == 8);
const _: () = assert!(std::mem::size_of::<PackedLiquidQuad>() == 16);

impl PackedQuad {
    const X_SHIFT: u32 = 0;
    const Y_SHIFT: u32 = 5;
    const Z_SHIFT: u32 = 10;
    const FACE_SHIFT: u32 = 15;
    const WIDTH_SHIFT: u32 = 18;
    const HEIGHT_SHIFT: u32 = 22;
    const POSITION_MASK: u32 = 0x1f;
    const EXTENT_MASK: u32 = 0x0f;

    fn new(origin: [u8; 3], face: Face, width: u8, height: u8, material_id: u32) -> Self {
        debug_assert!(origin.into_iter().all(|coordinate| coordinate < SIDE as u8));
        debug_assert!((1..=SIDE as u8).contains(&width));
        debug_assert!((1..=SIDE as u8).contains(&height));

        let geometry = (u32::from(origin[0]) << Self::X_SHIFT)
            | (u32::from(origin[1]) << Self::Y_SHIFT)
            | (u32::from(origin[2]) << Self::Z_SHIFT)
            | ((face as u32) << Self::FACE_SHIFT)
            | (u32::from(width - 1) << Self::WIDTH_SHIFT)
            | (u32::from(height - 1) << Self::HEIGHT_SHIFT);
        Self {
            geometry,
            material_id,
        }
    }

    #[must_use]
    pub const fn origin(&self) -> [u8; 3] {
        [
            ((self.geometry >> Self::X_SHIFT) & Self::POSITION_MASK) as u8,
            ((self.geometry >> Self::Y_SHIFT) & Self::POSITION_MASK) as u8,
            ((self.geometry >> Self::Z_SHIFT) & Self::POSITION_MASK) as u8,
        ]
    }

    #[must_use]
    pub const fn face(&self) -> Face {
        match (self.geometry >> Self::FACE_SHIFT) & 0x07 {
            0 => Face::NegativeX,
            1 => Face::PositiveX,
            2 => Face::NegativeY,
            3 => Face::PositiveY,
            4 => Face::NegativeZ,
            5 => Face::PositiveZ,
            _ => unreachable!(),
        }
    }

    #[must_use]
    pub const fn width(&self) -> u8 {
        (((self.geometry >> Self::WIDTH_SHIFT) & Self::EXTENT_MASK) + 1) as u8
    }

    #[must_use]
    pub const fn height(&self) -> u8 {
        (((self.geometry >> Self::HEIGHT_SHIFT) & Self::EXTENT_MASK) + 1) as u8
    }

    #[must_use]
    pub const fn material_id(&self) -> u32 {
        self.material_id
    }

    /// Raw words ready for upload to a storage buffer.
    #[must_use]
    pub const fn words(&self) -> [u32; 2] {
        [self.geometry, self.material_id]
    }
}

/// Compact 6x6 face-to-face cave connectivity matrix.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FaceConnectivity(u64);

impl FaceConnectivity {
    #[must_use]
    pub const fn none() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn all() -> Self {
        Self(CONNECTIVITY_MASK)
    }

    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn is_connected(self, from: Face, to: Face) -> bool {
        self.0 & (1_u64 << (from.index() * Face::ALL.len() + to.index())) != 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn is_all_connected(self) -> bool {
        self.0 == CONNECTIVITY_MASK
    }

    fn connect_touched_faces(&mut self, touched: u8) {
        for from in Face::ALL {
            if touched & (1 << from.index()) == 0 {
                continue;
            }
            for to in Face::ALL {
                if touched & (1 << to.index()) != 0 {
                    self.0 |= 1_u64 << (from.index() * Face::ALL.len() + to.index());
                }
            }
        }
    }
}

/// The six adjacent sub-chunks used for cross-boundary face culling.
#[derive(Debug, Clone, Copy, Default)]
pub struct Neighbourhood<'a> {
    negative_x: Option<&'a SubChunk>,
    positive_x: Option<&'a SubChunk>,
    negative_y: Option<&'a SubChunk>,
    positive_y: Option<&'a SubChunk>,
    negative_z: Option<&'a SubChunk>,
    positive_z: Option<&'a SubChunk>,
}

impl<'a> Neighbourhood<'a> {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            negative_x: None,
            positive_x: None,
            negative_y: None,
            positive_y: None,
            negative_z: None,
            positive_z: None,
        }
    }

    #[must_use]
    pub const fn with_negative_x(mut self, neighbour: &'a SubChunk) -> Self {
        self.negative_x = Some(neighbour);
        self
    }

    #[must_use]
    pub const fn with_positive_x(mut self, neighbour: &'a SubChunk) -> Self {
        self.positive_x = Some(neighbour);
        self
    }

    #[must_use]
    pub const fn with_negative_y(mut self, neighbour: &'a SubChunk) -> Self {
        self.negative_y = Some(neighbour);
        self
    }

    #[must_use]
    pub const fn with_positive_y(mut self, neighbour: &'a SubChunk) -> Self {
        self.positive_y = Some(neighbour);
        self
    }

    #[must_use]
    pub const fn with_negative_z(mut self, neighbour: &'a SubChunk) -> Self {
        self.negative_z = Some(neighbour);
        self
    }

    #[must_use]
    pub const fn with_positive_z(mut self, neighbour: &'a SubChunk) -> Self {
        self.positive_z = Some(neighbour);
        self
    }

    const fn get(self, face: Face) -> Option<&'a SubChunk> {
        match face {
            Face::NegativeX => self.negative_x,
            Face::PositiveX => self.positive_x,
            Face::NegativeY => self.negative_y,
            Face::PositiveY => self.positive_y,
            Face::NegativeZ => self.negative_z,
            Face::PositiveZ => self.positive_z,
        }
    }
}

/// Packed greedy geometry plus visibility metadata for one sub-chunk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkMesh {
    cube_quads: Box<[PackedQuad]>,
    model_refs: Box<[PackedModelRef]>,
    model_lighting: Box<[PackedQuadLighting]>,
    liquid_quads: Box<[PackedLiquidQuad]>,
    liquid_lighting: Box<[PackedQuadLighting]>,
    connectivity: FaceConnectivity,
}

/// Owned packed streams transferred from worker meshing into the render queue.
pub type ChunkMeshStreams = (
    Box<[PackedQuad]>,
    Box<[PackedModelRef]>,
    Box<[PackedQuadLighting]>,
    Box<[PackedLiquidQuad]>,
    Box<[PackedQuadLighting]>,
);

impl ChunkMesh {
    #[must_use]
    pub fn from_streams(
        cube_quads: Vec<PackedQuad>,
        model_refs: Vec<PackedModelRef>,
        model_lighting: Vec<PackedQuadLighting>,
        liquid_quads: Vec<PackedLiquidQuad>,
        liquid_lighting: Vec<PackedQuadLighting>,
        connectivity: FaceConnectivity,
    ) -> Self {
        Self {
            cube_quads: cube_quads.into_boxed_slice(),
            model_refs: model_refs.into_boxed_slice(),
            model_lighting: model_lighting.into_boxed_slice(),
            liquid_quads: liquid_quads.into_boxed_slice(),
            liquid_lighting: liquid_lighting.into_boxed_slice(),
            connectivity,
        }
    }

    #[must_use]
    pub fn quads(&self) -> &[PackedQuad] {
        self.cube_quads()
    }

    #[must_use]
    pub fn cube_quads(&self) -> &[PackedQuad] {
        &self.cube_quads
    }

    #[must_use]
    pub fn model_refs(&self) -> &[PackedModelRef] {
        &self.model_refs
    }

    #[must_use]
    pub fn model_lighting(&self) -> &[PackedQuadLighting] {
        &self.model_lighting
    }

    #[must_use]
    pub fn liquid_quads(&self) -> &[PackedLiquidQuad] {
        &self.liquid_quads
    }

    #[must_use]
    pub fn liquid_lighting(&self) -> &[PackedQuadLighting] {
        &self.liquid_lighting
    }

    #[must_use]
    pub fn quad_count(&self) -> usize {
        self.cube_quads.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cube_quads.is_empty()
            && self.model_refs.is_empty()
            && self.model_lighting.is_empty()
            && self.liquid_quads.is_empty()
            && self.liquid_lighting.is_empty()
    }

    #[must_use]
    pub const fn connectivity(&self) -> FaceConnectivity {
        self.connectivity
    }

    #[must_use]
    pub fn into_quads(self) -> Box<[PackedQuad]> {
        self.cube_quads
    }

    #[must_use]
    pub fn into_streams(self) -> ChunkMeshStreams {
        (
            self.cube_quads,
            self.model_refs,
            self.model_lighting,
            self.liquid_quads,
            self.liquid_lighting,
        )
    }
}

/// Greedy-mesh one sub-chunk directly from its packed palette storages.
///
/// Occupancy is represented as three sets of 16x16 `u64` axis columns. Face
/// masks are calculated with shifts/AND-NOT operations, then coplanar runs of
/// equal face material are merged before emitting one 8-byte record per quad.
#[must_use]
pub fn mesh_sub_chunk(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbours: &Neighbourhood<'_>,
    sub_chunk: &SubChunk,
) -> ChunkMesh {
    let mut neighbourhood = MeshNeighbourhood::new(sub_chunk);
    for face in Face::ALL {
        if let Some(neighbour) = neighbours.get(face) {
            let _ = neighbourhood.insert(face_offset(face), neighbour);
        }
    }
    mesh_sub_chunk_in_neighbourhood(classifier, visuals, network_id_mode, &neighbourhood)
}

/// Greedy-mesh from the shared bounded 3x3x3 palette-native snapshot.
#[must_use]
pub fn mesh_sub_chunk_in_neighbourhood(
    classifier: &BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
) -> ChunkMesh {
    let sub_chunk = neighbourhood
        .sub_chunk([0, 0, 0])
        .expect("MeshNeighbourhood always contains its center");
    let facts = PaletteFacts::new(*classifier, visuals, network_id_mode, sub_chunk);
    let connectivity = cave_connectivity(&facts);
    if facts.is_air() {
        return ChunkMesh {
            cube_quads: Box::new([]),
            model_refs: Box::new([]),
            model_lighting: Box::new([]),
            liquid_quads: Box::new([]),
            liquid_lighting: Box::new([]),
            connectivity,
        };
    }

    let masks = VisibilityMasks::from_facts(&facts);
    let mut quads = Vec::new();
    for face in Face::ALL {
        let columns = exposed_columns(
            *classifier,
            visuals,
            network_id_mode,
            neighbourhood,
            face,
            &facts,
            &masks,
        );
        for slice in 0..SIDE {
            let mut rows = [0_u64; SIDE];
            for (v, row) in rows.iter_mut().enumerate() {
                for (u, column) in columns[v].iter().enumerate() {
                    *row |= ((*column >> slice) & 1) << u;
                }
            }
            greedy_slice(&facts, face, slice, &mut rows, &mut quads);
        }
    }
    let mut model_refs = Vec::new();
    let mut model_lighting = Vec::new();
    for x in 0..SIDE {
        for y in 0..SIDE {
            for z in 0..SIDE {
                let entry = facts.at(x, y, z);
                if !matches!(entry.kind, VisualKind::Cross | VisualKind::Model)
                    || entry.model_template == NO_MODEL_TEMPLATE
                {
                    continue;
                }
                let Some(template) = visuals.model_templates().get(entry.model_template as usize)
                else {
                    continue;
                };
                if template.quad_count == 0 {
                    continue;
                }
                let Ok(lighting_base_index) = u32::try_from(model_lighting.len()) else {
                    continue;
                };
                let Some(lighting) = crate::lighting::bake_template_lighting(
                    classifier,
                    visuals,
                    network_id_mode,
                    neighbourhood,
                    [x as i32, y as i32, z as i32],
                    entry.model_template,
                ) else {
                    continue;
                };
                let visible_quad_mask = match template.quad_count {
                    0 => 0,
                    32 => u32::MAX,
                    count => (1_u32 << count) - 1,
                };
                model_refs.push(PackedModelRef::new(
                    pack_model_transform(
                        [x as u8, y as u8, z as u8],
                        if entry.kind == VisualKind::Cross {
                            0
                        } else {
                            entry.variant
                        },
                    ),
                    entry.model_template,
                    lighting_base_index,
                    visible_quad_mask,
                ));
                model_lighting.extend(lighting);
            }
        }
    }

    ChunkMesh {
        cube_quads: quads.into_boxed_slice(),
        model_refs: model_refs.into_boxed_slice(),
        model_lighting: model_lighting.into_boxed_slice(),
        liquid_quads: Box::new([]),
        liquid_lighting: Box::new([]),
        connectivity,
    }
}

#[derive(Clone, Copy)]
struct ResolvedPaletteEntry {
    flags: BlockFlags,
    faces: [u32; Face::ALL.len()],
    kind: VisualKind,
    model_template: u32,
    variant: u32,
}

impl ResolvedPaletteEntry {
    const AIR: Self = Self {
        flags: BlockFlags::AIR,
        faces: [DIAGNOSTIC_MATERIAL; Face::ALL.len()],
        kind: VisualKind::Invisible,
        model_template: NO_MODEL_TEMPLATE,
        variant: 0,
    };
    const DIAGNOSTIC: Self = Self {
        flags: BlockFlags::empty(),
        faces: [DIAGNOSTIC_MATERIAL; Face::ALL.len()],
        kind: VisualKind::Diagnostic,
        model_template: NO_MODEL_TEMPLATE,
        variant: 0,
    };

    const fn emits_cube_geometry(self) -> bool {
        self.flags.contains(BlockFlags::CUBE_GEOMETRY)
            || matches!(self.kind, VisualKind::Diagnostic)
    }
}

struct StoragePaletteFacts<'a> {
    storage: &'a PalettedStorage,
    entries: Box<[ResolvedPaletteEntry]>,
}

enum PaletteSource<'a> {
    Air,
    Uniform(ResolvedPaletteEntry),
    Mixed(Box<[StoragePaletteFacts<'a>]>),
}

/// Block flags and six-face materials parallel to storage palettes, never to
/// the 4,096 voxel positions.
struct PaletteFacts<'a> {
    source: PaletteSource<'a>,
}

impl<'a> PaletteFacts<'a> {
    fn new(
        classifier: BlockClassifier,
        visuals: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
        sub_chunk: &'a SubChunk,
    ) -> Self {
        for storage in sub_chunk.storages() {
            match storage.uniform_runtime_id() {
                Some(network_value) if classifier.is_air(network_value) => {}
                Some(network_value) => {
                    return Self {
                        source: PaletteSource::Uniform(resolve_palette_entry(
                            classifier,
                            visuals,
                            network_id_mode,
                            network_value,
                        )),
                    };
                }
                None => return Self::mixed(classifier, visuals, network_id_mode, sub_chunk),
            }
        }

        Self {
            source: PaletteSource::Air,
        }
    }

    fn mixed(
        classifier: BlockClassifier,
        visuals: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
        sub_chunk: &'a SubChunk,
    ) -> Self {
        let storages = sub_chunk
            .storages()
            .iter()
            .map(|storage| StoragePaletteFacts {
                storage,
                entries: storage
                    .palette()
                    .values()
                    .iter()
                    .copied()
                    .map(|network_value| {
                        resolve_palette_entry(classifier, visuals, network_id_mode, network_value)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            source: PaletteSource::Mixed(storages),
        }
    }

    const fn is_air(&self) -> bool {
        matches!(self.source, PaletteSource::Air)
    }

    fn at(&self, x: usize, y: usize, z: usize) -> ResolvedPaletteEntry {
        match &self.source {
            PaletteSource::Air => ResolvedPaletteEntry::AIR,
            PaletteSource::Uniform(entry) => *entry,
            PaletteSource::Mixed(storages) => {
                for storage in storages {
                    let Some(index) = packed_palette_index(storage.storage, x, y, z) else {
                        return ResolvedPaletteEntry::DIAGNOSTIC;
                    };
                    let Some(&entry) = storage.entries.get(index) else {
                        return ResolvedPaletteEntry::DIAGNOSTIC;
                    };
                    if !entry.flags.contains(BlockFlags::AIR) {
                        return entry;
                    }
                }
                ResolvedPaletteEntry::AIR
            }
        }
    }
}

fn resolve_palette_entry(
    classifier: BlockClassifier,
    visuals: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    network_value: u32,
) -> ResolvedPaletteEntry {
    if classifier.is_air(network_value) {
        return ResolvedPaletteEntry::AIR;
    }

    let block = visuals.resolve(network_id_mode, network_value);
    let mut flags = block.flags();
    flags.remove(BlockFlags::AIR);
    let faces = if flags.contains(BlockFlags::CUBE_GEOMETRY) {
        Face::ALL.map(|face| block.face(block_face(face)).material_id())
    } else {
        flags.remove(BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL);
        [DIAGNOSTIC_MATERIAL; Face::ALL.len()]
    };
    ResolvedPaletteEntry {
        flags,
        faces,
        kind: block.kind(),
        model_template: block.model_template().unwrap_or(NO_MODEL_TEMPLATE),
        variant: block.variant(),
    }
}

const fn pack_model_transform(local: [u8; 3], transform: u32) -> u32 {
    (local[0] as u32)
        | ((local[1] as u32) << 4)
        | ((local[2] as u32) << 8)
        | ((transform & 0x000f_ffff) << 12)
}

fn packed_palette_index(storage: &PalettedStorage, x: usize, y: usize, z: usize) -> Option<usize> {
    if x >= SIDE || y >= SIDE || z >= SIDE {
        return None;
    }
    if storage.bits_per_index() == 0 {
        return Some(0);
    }

    let linear = (x << 8) | (z << 4) | y;
    let bits = usize::from(storage.bits_per_index());
    let values_per_word = 32 / bits;
    let word = *storage.packed_words().get(linear / values_per_word)?;
    let shift = (linear % values_per_word) * bits;
    let mask = (1_u32 << storage.bits_per_index()) - 1;
    Some(((word >> shift) & mask) as usize)
}

const fn block_face(face: Face) -> BlockFace {
    match face {
        Face::NegativeX => BlockFace::West,
        Face::PositiveX => BlockFace::East,
        Face::NegativeY => BlockFace::Down,
        Face::PositiveY => BlockFace::Up,
        Face::NegativeZ => BlockFace::North,
        Face::PositiveZ => BlockFace::South,
    }
}

type Columns = [[u64; SIDE]; SIDE];

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

struct VisibilityMasks {
    geometry: AxisColumns,
    occluders: AxisColumns,
    leaves: AxisColumns,
}

impl VisibilityMasks {
    fn from_facts(facts: &PaletteFacts<'_>) -> Self {
        match &facts.source {
            PaletteSource::Air => Self {
                geometry: AxisColumns::empty(),
                occluders: AxisColumns::empty(),
                leaves: AxisColumns::empty(),
            },
            PaletteSource::Uniform(entry) => Self {
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
            },
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

fn exposed_columns(
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

const fn face_offset(face: Face) -> [i8; 3] {
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

const fn connectivity_open(entry: ResolvedPaletteEntry) -> bool {
    !entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
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

fn greedy_slice(
    facts: &PaletteFacts<'_>,
    face: Face,
    slice: usize,
    rows: &mut [u64; SIDE],
    quads: &mut Vec<PackedQuad>,
) {
    for v in 0..SIDE {
        while rows[v] != 0 {
            let u = rows[v].trailing_zeros() as usize;
            let origin = block_coordinate(face, slice, u, v);
            let material_id = facts.at(origin[0], origin[1], origin[2]).faces[face.index()];

            let shifted = rows[v] >> u;
            let binary_width = (!shifted).trailing_zeros() as usize;
            let binary_width = binary_width.min(SIDE - u);
            let mut width = 1;
            while width < binary_width && {
                let [x, y, z] = block_coordinate(face, slice, u + width, v);
                facts.at(x, y, z).faces[face.index()] == material_id
            } {
                width += 1;
            }

            let span = ((1_u64 << width) - 1) << u;
            let mut height = 1;
            'height: while v + height < SIDE && rows[v + height] & span == span {
                for offset in 0..width {
                    let [x, y, z] = block_coordinate(face, slice, u + offset, v + height);
                    if facts.at(x, y, z).faces[face.index()] != material_id {
                        break 'height;
                    }
                }
                height += 1;
            }

            for row in &mut rows[v..v + height] {
                *row &= !span;
            }
            quads.push(PackedQuad::new(
                origin.map(|coordinate| coordinate as u8),
                face,
                width as u8,
                height as u8,
                material_id,
            ));
        }
    }
}

const fn block_coordinate(face: Face, slice: usize, u: usize, v: usize) -> [usize; 3] {
    match face {
        Face::NegativeX | Face::PositiveX => [slice, v, u],
        Face::NegativeY | Face::PositiveY => [u, slice, v],
        Face::NegativeZ | Face::PositiveZ => [u, v, slice],
    }
}

fn cave_connectivity(facts: &PaletteFacts<'_>) -> FaceConnectivity {
    match &facts.source {
        PaletteSource::Air => return FaceConnectivity::all(),
        PaletteSource::Uniform(entry) => {
            return if connectivity_open(*entry) {
                FaceConnectivity::all()
            } else {
                FaceConnectivity::none()
            };
        }
        PaletteSource::Mixed(_) => {}
    }

    let mut connectivity = FaceConnectivity::none();
    let mut visited = [0_u64; 64];
    let mut queue = VecDeque::new();

    for seed in 0..4096_usize {
        if bit_is_set(&visited, seed) {
            continue;
        }
        let coordinate = coordinate_from_linear(seed);
        if !connectivity_open(facts.at(coordinate[0], coordinate[1], coordinate[2])) {
            continue;
        }

        set_bit(&mut visited, seed);
        queue.push_back(seed as u16);
        let mut touched = 0_u8;

        while let Some(linear) = queue.pop_front() {
            let [x, y, z] = coordinate_from_linear(usize::from(linear));
            touched |= touched_faces(x, y, z);

            for neighbour in adjacent_coordinates(x, y, z).into_iter().flatten() {
                let neighbour_linear = linear_from_coordinate(neighbour);
                if bit_is_set(&visited, neighbour_linear)
                    || !connectivity_open(facts.at(neighbour[0], neighbour[1], neighbour[2]))
                {
                    continue;
                }
                set_bit(&mut visited, neighbour_linear);
                queue.push_back(neighbour_linear as u16);
            }
        }
        connectivity.connect_touched_faces(touched);
    }
    connectivity
}

const fn touched_faces(x: usize, y: usize, z: usize) -> u8 {
    let mut touched = 0_u8;
    if x == 0 {
        touched |= 1 << Face::NegativeX.index();
    }
    if x == SIDE - 1 {
        touched |= 1 << Face::PositiveX.index();
    }
    if y == 0 {
        touched |= 1 << Face::NegativeY.index();
    }
    if y == SIDE - 1 {
        touched |= 1 << Face::PositiveY.index();
    }
    if z == 0 {
        touched |= 1 << Face::NegativeZ.index();
    }
    if z == SIDE - 1 {
        touched |= 1 << Face::PositiveZ.index();
    }
    touched
}

fn adjacent_coordinates(x: usize, y: usize, z: usize) -> [Option<[usize; 3]>; 6] {
    [
        (x > 0).then_some([x.saturating_sub(1), y, z]),
        (x + 1 < SIDE).then_some([x + 1, y, z]),
        (y > 0).then_some([x, y.saturating_sub(1), z]),
        (y + 1 < SIDE).then_some([x, y + 1, z]),
        (z > 0).then_some([x, y, z.saturating_sub(1)]),
        (z + 1 < SIDE).then_some([x, y, z + 1]),
    ]
}

const fn linear_from_coordinate([x, y, z]: [usize; 3]) -> usize {
    (x << 8) | (z << 4) | y
}

const fn coordinate_from_linear(linear: usize) -> [usize; 3] {
    [linear >> 8, linear & 0x0f, (linear >> 4) & 0x0f]
}

fn bit_is_set(bits: &[u64; 64], index: usize) -> bool {
    bits[index / 64] & (1_u64 << (index % 64)) != 0
}

fn set_bit(bits: &mut [u64; 64], index: usize) {
    bits[index / 64] |= 1_u64 << (index % 64);
}
