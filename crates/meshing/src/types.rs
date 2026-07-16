use world::SubChunk;

use crate::SIDE;

const CONNECTIVITY_MASK: u64 = (1_u64 << (Face::ALL.len() * Face::ALL.len())) - 1;

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

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    pub(crate) const fn is_negative(self) -> bool {
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

/// One exact visible-quad draw indirection into the packed model-ref stream.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PackedModelDrawRef {
    model_ref_index: u32,
    quad_index: u32,
}

impl PackedModelDrawRef {
    #[must_use]
    pub const fn new(model_ref_index: u32, quad_index: u32) -> Self {
        Self {
            model_ref_index,
            quad_index,
        }
    }

    #[must_use]
    pub const fn words(self) -> [u32; 2] {
        [self.model_ref_index, self.quad_index]
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
/// Word 0 packs local X/Y/Z nibbles, face bits 12..14 (6 and 7 reserved), a
/// falling bit, and signed i8 X/Z flow gradients. Word 1 stores four u8 heights,
/// where 255 is one full block. Vertex order is top NW/NE/SE/SW; bottom
/// NW/SW/SE/NE; -X bottom-N/top-N/top-S/bottom-S; +X
/// bottom-S/top-S/top-N/bottom-N; -Z bottom-E/top-E/top-W/bottom-W; and +Z
/// bottom-W/top-W/top-E/bottom-E. Word 2 stores the selected material in bits
/// 0..30 and the immutable depth-writing route in bit 31; word 3 stores the
/// relative index in the independently allocated liquid-light stream.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PackedLiquidQuad([u32; 4]);

impl PackedLiquidQuad {
    const DEPTH_WRITE_BIT: u32 = 1 << 31;

    #[must_use]
    pub const fn words(self) -> [u32; 4] {
        self.0
    }

    /// Packs one liquid quad. Heights are u8 fixed point with 255 = one block,
    /// ordered top NW/NE/SE/SW; bottom NW/SW/SE/NE; -X
    /// bottom-N/top-N/top-S/bottom-S; +X bottom-S/top-S/top-N/bottom-N; -Z
    /// bottom-E/top-E/top-W/bottom-W; or +Z bottom-W/top-W/top-E/bottom-E.
    /// Raw face encodings 6 and 7 remain reserved.
    #[must_use]
    pub const fn try_pack(
        origin: [u8; 3],
        face: Face,
        heights: [u8; 4],
        material_id: u32,
        lighting_index: u32,
        flow_gradient: [i8; 2],
        falling: bool,
    ) -> Option<Self> {
        if origin[0] >= 16
            || origin[1] >= 16
            || origin[2] >= 16
            || material_id & Self::DEPTH_WRITE_BIT != 0
        {
            return None;
        }
        let geometry = origin[0] as u32
            | ((origin[1] as u32) << 4)
            | ((origin[2] as u32) << 8)
            | ((face as u32) << 12)
            | ((falling as u32) << 15)
            | ((flow_gradient[0] as u8 as u32) << 16)
            | ((flow_gradient[1] as u8 as u32) << 24);
        let corners = heights[0] as u32
            | ((heights[1] as u32) << 8)
            | ((heights[2] as u32) << 16)
            | ((heights[3] as u32) << 24);
        Some(Self([geometry, corners, material_id, lighting_index]))
    }

    #[must_use]
    pub const fn origin(self) -> [u8; 3] {
        [
            (self.0[0] & 15) as u8,
            ((self.0[0] >> 4) & 15) as u8,
            ((self.0[0] >> 8) & 15) as u8,
        ]
    }

    #[must_use]
    pub const fn face(self) -> Face {
        match (self.0[0] >> 12) & 7 {
            0 => Face::NegativeX,
            1 => Face::PositiveX,
            2 => Face::NegativeY,
            3 => Face::PositiveY,
            4 => Face::NegativeZ,
            5 => Face::PositiveZ,
            _ => unreachable!(),
        }
    }

    /// Checks reserved face encodings before accepting raw stream words.
    #[must_use]
    pub const fn try_from_words(words: [u32; 4]) -> Option<Self> {
        if ((words[0] >> 12) & 7) >= 6 {
            None
        } else {
            Some(Self(words))
        }
    }

    #[must_use]
    pub const fn heights(self) -> [u8; 4] {
        [
            self.0[1] as u8,
            (self.0[1] >> 8) as u8,
            (self.0[1] >> 16) as u8,
            (self.0[1] >> 24) as u8,
        ]
    }

    #[must_use]
    pub const fn material_id(self) -> u32 {
        self.0[2] & !Self::DEPTH_WRITE_BIT
    }

    /// Returns whether this record belongs to the opaque depth-writing liquid route.
    #[must_use]
    pub const fn is_depth_writing(self) -> bool {
        self.0[2] & Self::DEPTH_WRITE_BIT != 0
    }

    #[must_use]
    pub(crate) const fn with_depth_write(mut self, enabled: bool) -> Self {
        if enabled {
            self.0[2] |= Self::DEPTH_WRITE_BIT;
        } else {
            self.0[2] &= !Self::DEPTH_WRITE_BIT;
        }
        self
    }

    #[must_use]
    pub const fn lighting_index(self) -> u32 {
        self.0[3]
    }

    #[must_use]
    pub const fn flow_gradient(self) -> [i8; 2] {
        [(self.0[0] >> 16) as u8 as i8, (self.0[0] >> 24) as u8 as i8]
    }

    #[must_use]
    pub const fn is_falling(self) -> bool {
        self.0[0] & (1 << 15) != 0
    }
}

const _: () = assert!(std::mem::size_of::<PackedQuad>() == 8);
const _: () = assert!(std::mem::size_of::<PackedModelRef>() == 16);
const _: () = assert!(std::mem::size_of::<PackedModelDrawRef>() == 8);
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

    pub(crate) fn new(
        origin: [u8; 3],
        face: Face,
        width: u8,
        height: u8,
        material_id: u32,
    ) -> Self {
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

    pub(crate) fn connect_touched_faces(&mut self, touched: u8) {
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

    pub(crate) const fn get(self, face: Face) -> Option<&'a SubChunk> {
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
    pub(crate) cube_streams: Box<CubeStreams>,
    pub(crate) model_refs: Box<[PackedModelRef]>,
    pub(crate) model_lighting: Box<[PackedQuadLighting]>,
    pub(crate) model_draw_refs: Box<ModelDrawRefs>,
    pub(crate) liquid_quads: Box<[PackedLiquidQuad]>,
    pub(crate) liquid_lighting: Box<[PackedQuadLighting]>,
    pub(crate) connectivity: FaceConnectivity,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CubeStreams {
    pub(crate) cube_quads: Box<[PackedQuad]>,
    pub(crate) cube_lighting: Box<[PackedQuadLighting]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ModelDrawRefs {
    pub(crate) opaque: Box<[PackedModelDrawRef]>,
    pub(crate) transparent: Box<[PackedModelDrawRef]>,
}

/// Owned packed streams transferred from worker meshing into the render queue.
pub type ChunkMeshStreams = (
    Box<[PackedQuad]>,
    Box<[PackedQuadLighting]>,
    Box<[PackedModelRef]>,
    Box<[PackedQuadLighting]>,
    Box<[PackedModelDrawRef]>,
    Box<[PackedModelDrawRef]>,
    Box<[PackedLiquidQuad]>,
    Box<[PackedQuadLighting]>,
);

/// Structural rejection for externally assembled chunk mesh streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkMeshStreamError {
    cube_quads: usize,
    cube_lighting: usize,
}

impl ChunkMeshStreamError {
    #[must_use]
    pub const fn cube_quads(self) -> usize {
        self.cube_quads
    }

    #[must_use]
    pub const fn cube_lighting(self) -> usize {
        self.cube_lighting
    }
}

impl ChunkMesh {
    #[must_use]
    pub fn from_streams(
        cube_quads: Vec<PackedQuad>,
        model_refs: Vec<PackedModelRef>,
        model_lighting: Vec<PackedQuadLighting>,
        model_draw_refs: Vec<PackedModelDrawRef>,
        liquid_quads: Vec<PackedLiquidQuad>,
        liquid_lighting: Vec<PackedQuadLighting>,
        connectivity: FaceConnectivity,
    ) -> Self {
        let cube_lighting = vec![crate::lighting::phase26_default_lighting(); cube_quads.len()];
        Self::try_from_streams_with_cube_lighting(
            cube_quads,
            cube_lighting,
            model_refs,
            model_lighting,
            model_draw_refs,
            liquid_quads,
            liquid_lighting,
            connectivity,
        )
        .expect("compatibility cube lighting count is derived from cube quads")
    }

    /// Builds an externally assembled mesh only when every cube quad has one
    /// lighting sidecar in the same immutable order.
    #[allow(
        clippy::too_many_arguments,
        reason = "the method validates the complete independently-owned CPU stream contract"
    )]
    pub fn try_from_streams_with_cube_lighting(
        cube_quads: Vec<PackedQuad>,
        cube_lighting: Vec<PackedQuadLighting>,
        model_refs: Vec<PackedModelRef>,
        model_lighting: Vec<PackedQuadLighting>,
        model_draw_refs: Vec<PackedModelDrawRef>,
        liquid_quads: Vec<PackedLiquidQuad>,
        liquid_lighting: Vec<PackedQuadLighting>,
        connectivity: FaceConnectivity,
    ) -> Result<Self, ChunkMeshStreamError> {
        if cube_quads.len() != cube_lighting.len() {
            return Err(ChunkMeshStreamError {
                cube_quads: cube_quads.len(),
                cube_lighting: cube_lighting.len(),
            });
        }
        Ok(Self {
            cube_streams: Box::new(CubeStreams {
                cube_quads: cube_quads.into_boxed_slice(),
                cube_lighting: cube_lighting.into_boxed_slice(),
            }),
            model_refs: model_refs.into_boxed_slice(),
            model_lighting: model_lighting.into_boxed_slice(),
            model_draw_refs: Box::new(ModelDrawRefs {
                opaque: model_draw_refs.into_boxed_slice(),
                transparent: Box::new([]),
            }),
            liquid_quads: liquid_quads.into_boxed_slice(),
            liquid_lighting: liquid_lighting.into_boxed_slice(),
            connectivity,
        })
    }

    #[must_use]
    pub fn quads(&self) -> &[PackedQuad] {
        self.cube_quads()
    }

    #[must_use]
    pub fn cube_quads(&self) -> &[PackedQuad] {
        &self.cube_streams.cube_quads
    }

    /// CPU-baked lighting in exact one-to-one cube-quad order.
    #[must_use]
    pub fn cube_lighting(&self) -> &[PackedQuadLighting] {
        &self.cube_streams.cube_lighting
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
    pub fn model_draw_refs(&self) -> &[PackedModelDrawRef] {
        &self.model_draw_refs.opaque
    }

    #[must_use]
    pub fn transparent_model_draw_refs(&self) -> &[PackedModelDrawRef] {
        &self.model_draw_refs.transparent
    }

    #[must_use]
    pub fn with_transparent_model_draw_refs(mut self, draw_refs: Vec<PackedModelDrawRef>) -> Self {
        self.model_draw_refs.transparent = draw_refs.into_boxed_slice();
        self
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
        self.cube_streams.cube_quads.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cube_streams.cube_quads.is_empty()
            && self.cube_streams.cube_lighting.is_empty()
            && self.model_refs.is_empty()
            && self.model_lighting.is_empty()
            && self.model_draw_refs.opaque.is_empty()
            && self.model_draw_refs.transparent.is_empty()
            && self.liquid_quads.is_empty()
            && self.liquid_lighting.is_empty()
    }

    #[must_use]
    pub const fn connectivity(&self) -> FaceConnectivity {
        self.connectivity
    }

    #[must_use]
    pub fn into_quads(self) -> Box<[PackedQuad]> {
        self.cube_streams.cube_quads
    }

    #[must_use]
    pub fn into_streams(self) -> ChunkMeshStreams {
        let CubeStreams {
            cube_quads,
            cube_lighting,
        } = *self.cube_streams;
        let ModelDrawRefs {
            opaque,
            transparent,
        } = *self.model_draw_refs;
        (
            cube_quads,
            cube_lighting,
            self.model_refs,
            self.model_lighting,
            opaque,
            transparent,
            self.liquid_quads,
            self.liquid_lighting,
        )
    }
}
