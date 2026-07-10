use std::collections::VecDeque;

use world::SubChunk;

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
/// preserves the complete raw runtime value for debug material lookup.
///
/// Width/height axes are Z/Y for X faces, X/Z for Y faces, and X/Y for Z
/// faces. A vertex shader can reconstruct all four corners from these fields
/// and a face-orientation lookup table.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedQuad {
    geometry: u32,
    runtime_id: u32,
}

impl PackedQuad {
    const X_SHIFT: u32 = 0;
    const Y_SHIFT: u32 = 5;
    const Z_SHIFT: u32 = 10;
    const FACE_SHIFT: u32 = 15;
    const WIDTH_SHIFT: u32 = 18;
    const HEIGHT_SHIFT: u32 = 22;
    const POSITION_MASK: u32 = 0x1f;
    const EXTENT_MASK: u32 = 0x0f;

    fn new(origin: [u8; 3], face: Face, width: u8, height: u8, runtime_id: u32) -> Self {
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
            runtime_id,
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
    pub const fn runtime_id(&self) -> u32 {
        self.runtime_id
    }

    /// Raw words ready for upload to a storage buffer.
    #[must_use]
    pub const fn words(&self) -> [u32; 2] {
        [self.geometry, self.runtime_id]
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
    quads: Box<[PackedQuad]>,
    connectivity: FaceConnectivity,
}

impl ChunkMesh {
    #[must_use]
    pub fn quads(&self) -> &[PackedQuad] {
        &self.quads
    }

    #[must_use]
    pub fn quad_count(&self) -> usize {
        self.quads.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    #[must_use]
    pub const fn connectivity(&self) -> FaceConnectivity {
        self.connectivity
    }

    #[must_use]
    pub fn into_quads(self) -> Box<[PackedQuad]> {
        self.quads
    }
}

/// Greedy-mesh one sub-chunk directly from its packed palette storages.
///
/// Occupancy is represented as three sets of 16x16 `u64` axis columns. Face
/// masks are calculated with shifts/AND-NOT operations, then coplanar runs of
/// equal runtime value are merged before emitting one 8-byte record per quad.
#[must_use]
pub fn mesh_sub_chunk(
    classifier: &BlockClassifier,
    neighbours: &Neighbourhood<'_>,
    sub_chunk: &SubChunk,
) -> ChunkMesh {
    let source = MaterialSource::classify(*classifier, sub_chunk);
    let connectivity = cave_connectivity(*classifier, source);
    if matches!(source, MaterialSource::Air) {
        return ChunkMesh {
            quads: Box::new([]),
            connectivity,
        };
    }

    let occupancy = Occupancy::from_source(*classifier, source);
    let mut quads = Vec::new();
    for face in Face::ALL {
        let columns = exposed_columns(*classifier, *neighbours, face, &occupancy);
        for slice in 0..SIDE {
            let mut rows = [0_u64; SIDE];
            for (v, row) in rows.iter_mut().enumerate() {
                for (u, column) in columns[v].iter().enumerate() {
                    *row |= ((*column >> slice) & 1) << u;
                }
            }
            greedy_slice(*classifier, source, face, slice, &mut rows, &mut quads);
        }
    }

    ChunkMesh {
        quads: quads.into_boxed_slice(),
        connectivity,
    }
}

#[derive(Clone, Copy)]
enum MaterialSource<'a> {
    Air,
    Uniform(u32),
    Mixed(&'a SubChunk),
}

impl<'a> MaterialSource<'a> {
    fn classify(classifier: BlockClassifier, sub_chunk: &'a SubChunk) -> Self {
        for storage in sub_chunk.storages() {
            match storage.uniform_runtime_id() {
                Some(runtime_id) if classifier.is_air(runtime_id) => {}
                Some(runtime_id) => return Self::Uniform(runtime_id),
                None => return Self::Mixed(sub_chunk),
            }
        }
        Self::Air
    }

    fn at(self, classifier: BlockClassifier, x: usize, y: usize, z: usize) -> u32 {
        match self {
            Self::Air => classifier.air_network_id(),
            Self::Uniform(runtime_id) => runtime_id,
            Self::Mixed(sub_chunk) => visible_runtime_id(classifier, sub_chunk, x, y, z),
        }
    }
}

fn visible_runtime_id(
    classifier: BlockClassifier,
    sub_chunk: &SubChunk,
    x: usize,
    y: usize,
    z: usize,
) -> u32 {
    sub_chunk
        .storages()
        .iter()
        .filter_map(|storage| storage.runtime_id(x as u8, y as u8, z as u8))
        .find(|&runtime_id| !classifier.is_air(runtime_id))
        .unwrap_or_else(|| classifier.air_network_id())
}

type Columns = [[u64; SIDE]; SIDE];

struct Occupancy {
    x: Columns,
    y: Columns,
    z: Columns,
}

impl Occupancy {
    fn from_source(classifier: BlockClassifier, source: MaterialSource<'_>) -> Self {
        if matches!(source, MaterialSource::Uniform(_)) {
            return Self {
                x: [[FULL_COLUMN; SIDE]; SIDE],
                y: [[FULL_COLUMN; SIDE]; SIDE],
                z: [[FULL_COLUMN; SIDE]; SIDE],
            };
        }

        let mut occupancy = Self {
            x: [[0; SIDE]; SIDE],
            y: [[0; SIDE]; SIDE],
            z: [[0; SIDE]; SIDE],
        };
        for x in 0..SIDE {
            for y in 0..SIDE {
                for z in 0..SIDE {
                    if classifier.is_air(source.at(classifier, x, y, z)) {
                        continue;
                    }
                    occupancy.x[y][z] |= 1 << x;
                    occupancy.y[x][z] |= 1 << y;
                    occupancy.z[x][y] |= 1 << z;
                }
            }
        }
        occupancy
    }
}

fn exposed_columns(
    classifier: BlockClassifier,
    neighbours: Neighbourhood<'_>,
    face: Face,
    occupancy: &Occupancy,
) -> Columns {
    let neighbour = neighbours
        .get(face)
        .map_or(MaterialSource::Air, |sub_chunk| {
            MaterialSource::classify(classifier, sub_chunk)
        });
    let boundary_bit = if face.is_negative() {
        1_u64
    } else {
        1_u64 << (SIDE - 1)
    };
    let mut exposed = [[0_u64; SIDE]; SIDE];

    for (v, exposed_row) in exposed.iter_mut().enumerate() {
        for (u, exposed_cell) in exposed_row.iter_mut().enumerate() {
            let column = match face {
                Face::NegativeX | Face::PositiveX => occupancy.x[v][u],
                Face::NegativeY | Face::PositiveY => occupancy.y[u][v],
                Face::NegativeZ | Face::PositiveZ => occupancy.z[u][v],
            };
            let mut faces = if face.is_negative() {
                column & !(column << 1)
            } else {
                column & !(column >> 1)
            } & FULL_COLUMN;

            let [x, y, z] = neighbour_boundary_coordinate(face, u, v);
            if !classifier.is_air(neighbour.at(classifier, x, y, z)) {
                faces &= !boundary_bit;
            }
            *exposed_cell = faces;
        }
    }
    exposed
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
    classifier: BlockClassifier,
    source: MaterialSource<'_>,
    face: Face,
    slice: usize,
    rows: &mut [u64; SIDE],
    quads: &mut Vec<PackedQuad>,
) {
    for v in 0..SIDE {
        while rows[v] != 0 {
            let u = rows[v].trailing_zeros() as usize;
            let origin = block_coordinate(face, slice, u, v);
            let runtime_id = source.at(classifier, origin[0], origin[1], origin[2]);

            let shifted = rows[v] >> u;
            let binary_width = (!shifted).trailing_zeros() as usize;
            let binary_width = binary_width.min(SIDE - u);
            let mut width = 1;
            while width < binary_width && {
                let [x, y, z] = block_coordinate(face, slice, u + width, v);
                source.at(classifier, x, y, z) == runtime_id
            } {
                width += 1;
            }

            let span = ((1_u64 << width) - 1) << u;
            let mut height = 1;
            'height: while v + height < SIDE && rows[v + height] & span == span {
                for offset in 0..width {
                    let [x, y, z] = block_coordinate(face, slice, u + offset, v + height);
                    if source.at(classifier, x, y, z) != runtime_id {
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
                runtime_id,
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

fn cave_connectivity(classifier: BlockClassifier, source: MaterialSource<'_>) -> FaceConnectivity {
    match source {
        MaterialSource::Air => return FaceConnectivity::all(),
        MaterialSource::Uniform(_) => return FaceConnectivity::none(),
        MaterialSource::Mixed(_) => {}
    }

    let mut connectivity = FaceConnectivity::none();
    let mut visited = [0_u64; 64];
    let mut queue = VecDeque::new();

    for seed in 0..4096_usize {
        if bit_is_set(&visited, seed) {
            continue;
        }
        let coordinate = coordinate_from_linear(seed);
        if !classifier.is_air(source.at(classifier, coordinate[0], coordinate[1], coordinate[2])) {
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
                    || !classifier.is_air(source.at(
                        classifier,
                        neighbour[0],
                        neighbour[1],
                        neighbour[2],
                    ))
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
