use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;
use world::{ChunkCollisionRevision, ChunkKey, ChunkStore, SubChunkKey};

use crate::{Aabb, Vec3};

pub(crate) const DEFAULT_SURFACE_FRICTION: f64 = 0.6;
/// Maximum width, height, or depth of a collision query in blocks.
pub const MAX_COLLISION_QUERY_EXTENT: f64 = 128.0;
/// Maximum number of distinct columns retained by one immutable query identity.
pub const MAX_COLLISION_IDENTITY_CHUNKS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CollisionIdSpace {
    Sequential,
    Hashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionRegistryIdentity {
    pub protocol: u32,
    pub id_space: CollisionIdSpace,
    pub preg_sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldCollisionIdentity {
    pub registry: CollisionRegistryIdentity,
    pub chunks: Box<[ChunkCollisionRevision]>,
}

impl WorldCollisionIdentity {
    pub fn new(
        registry: CollisionRegistryIdentity,
        chunks: impl IntoIterator<Item = ChunkCollisionRevision>,
    ) -> Result<Self, WorldQueryError> {
        let chunks = chunks.into_iter().collect::<BTreeSet<_>>();
        if chunks.len() > MAX_COLLISION_IDENTITY_CHUNKS {
            return Err(WorldQueryError::IdentityChunkLimitExceeded {
                max: MAX_COLLISION_IDENTITY_CHUNKS,
            });
        }
        let mut by_chunk = BTreeMap::new();
        for revision in chunks {
            if let Some(previous) = by_chunk.insert(revision.chunk, revision)
                && previous.revision != revision.revision
            {
                return Err(WorldQueryError::ChunkRevisionConflict {
                    chunk: revision.chunk,
                });
            }
        }
        Ok(Self {
            registry,
            chunks: by_chunk.into_values().collect(),
        })
    }

    pub fn merge(&self, other: &Self) -> Result<Self, WorldQueryError> {
        if self.registry != other.registry {
            return Err(WorldQueryError::RegistryIdentityMismatch);
        }
        Self::new(
            self.registry,
            self.chunks.iter().chain(other.chunks.iter()).copied(),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollisionQuery<T> {
    pub value: T,
    pub identity: WorldCollisionIdentity,
}

impl<T> CollisionQuery<T> {
    /// Builds a registry-neutral empty identity for deterministic fixture worlds.
    #[must_use]
    pub fn synthetic(value: T) -> Self {
        Self {
            value,
            identity: WorldCollisionIdentity::new(
                CollisionRegistryIdentity {
                    protocol: 1001,
                    id_space: CollisionIdSpace::Sequential,
                    preg_sha256: [0; 32],
                },
                [],
            )
            .expect("empty collision identity is bounded"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BlockPhysicsFlags(u8);

impl BlockPhysicsFlags {
    pub const CLIMBABLE: Self = Self(1 << 0);
    pub const WATER: Self = Self(1 << 1);
    pub const LAVA: Self = Self(1 << 2);
    pub const COBWEB: Self = Self(1 << 3);
    pub const POWDER_SNOW: Self = Self(1 << 4);
    pub const SCAFFOLDING: Self = Self(1 << 5);
    pub const PASSABLE: Self = Self(1 << 6);
    pub const KNOWN_BITS: u8 = (1 << 7) - 1;

    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::KNOWN_BITS == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SurfaceResponse {
    #[default]
    None,
    Slime,
    Bed,
    Honey,
    SoulSand,
    BubbleUp,
    BubbleDown,
}

impl SurfaceResponse {
    #[must_use]
    pub const fn from_primitive(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Slime),
            2 => Some(Self::Bed),
            3 => Some(Self::Honey),
            4 => Some(Self::SoulSand),
            5 => Some(Self::BubbleUp),
            6 => Some(Self::BubbleDown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockPhysicsFacts {
    pub friction: f64,
    pub horizontal_speed_factor: f64,
    pub vertical_speed_factor: f64,
    pub fluid_height_blocks: f64,
    pub flags: BlockPhysicsFlags,
    pub surface_response: SurfaceResponse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockPhysicsSample {
    pub layers: Box<[BlockPhysicsFacts]>,
    pub identity: WorldCollisionIdentity,
}

impl BlockPhysicsSample {
    #[must_use]
    pub fn primary(&self) -> &BlockPhysicsFacts {
        self.layers
            .first()
            .expect("every block physics sample contains an explicit primary layer")
    }
}

/// Runtime-ID keyed authoritative movement facts in local block coordinates.
#[derive(Debug)]
pub struct CollisionRegistry {
    identity: CollisionRegistryIdentity,
    blocks: BTreeMap<u32, BlockPhysics>,
    air_runtime_id: u32,
}

#[derive(Debug)]
struct BlockPhysics {
    shapes: Box<[Aabb]>,
    friction: f64,
    horizontal_speed_factor: f64,
    vertical_speed_factor: f64,
    fluid_height_blocks: f64,
    flags: BlockPhysicsFlags,
    surface_response: SurfaceResponse,
}

impl Default for CollisionRegistry {
    fn default() -> Self {
        Self::with_identity(CollisionRegistryIdentity {
            protocol: 1001,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [0; 32],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("runtime ID {runtime_id} has a non-finite or non-positive {field}")]
    InvalidScalar {
        runtime_id: u32,
        field: &'static str,
    },
    #[error("runtime ID {runtime_id} collision shape {shape_index} is non-finite or inverted")]
    InvalidShape { runtime_id: u32, shape_index: usize },
    #[error(
        "runtime ID {runtime_id} collision shape {shape_index} exceeds the one-block local query halo"
    )]
    ShapeOutsideLocalHalo { runtime_id: u32, shape_index: usize },
    #[error("runtime ID {runtime_id} has unknown physics flag bits {bits:#04x}")]
    InvalidFlags { runtime_id: u32, bits: u8 },
    #[error("runtime ID {runtime_id} has unknown surface response {value}")]
    InvalidSurfaceResponse { runtime_id: u32, value: u8 },
}

impl CollisionRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_identity(identity: CollisionRegistryIdentity) -> Self {
        Self {
            identity,
            blocks: BTreeMap::new(),
            air_runtime_id: 0,
        }
    }

    #[must_use]
    pub const fn identity(&self) -> CollisionRegistryIdentity {
        self.identity
    }

    pub fn set_air_runtime_id(&mut self, runtime_id: u32) {
        self.air_runtime_id = runtime_id;
    }

    pub fn register(
        &mut self,
        runtime_id: u32,
        boxes: impl IntoIterator<Item = Aabb>,
    ) -> Result<(), RegistryError> {
        self.register_physics(
            runtime_id,
            boxes,
            DEFAULT_SURFACE_FRICTION,
            1.0,
            1.0,
            0.0,
            BlockPhysicsFlags::default(),
            SurfaceResponse::None,
        )
    }

    pub fn register_with_friction(
        &mut self,
        runtime_id: u32,
        boxes: impl IntoIterator<Item = Aabb>,
        friction: f64,
    ) -> Result<(), RegistryError> {
        self.register_physics(
            runtime_id,
            boxes,
            friction,
            1.0,
            1.0,
            0.0,
            BlockPhysicsFlags::default(),
            SurfaceResponse::None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_physics(
        &mut self,
        runtime_id: u32,
        boxes: impl IntoIterator<Item = Aabb>,
        friction: f64,
        horizontal_speed_factor: f64,
        vertical_speed_factor: f64,
        fluid_height_blocks: f64,
        flags: BlockPhysicsFlags,
        surface_response: SurfaceResponse,
    ) -> Result<(), RegistryError> {
        for (field, value, allow_zero) in [
            ("friction", friction, false),
            ("horizontal speed factor", horizontal_speed_factor, false),
            ("vertical speed factor", vertical_speed_factor, false),
            ("fluid height", fluid_height_blocks, true),
        ] {
            if !value.is_finite() || value < 0.0 || (!allow_zero && value == 0.0) {
                return Err(RegistryError::InvalidScalar { runtime_id, field });
            }
        }
        let shapes = boxes.into_iter().collect::<Vec<_>>();
        validate_shapes(runtime_id, &shapes)?;
        self.blocks.insert(
            runtime_id,
            BlockPhysics {
                shapes: shapes.into_boxed_slice(),
                friction,
                horizontal_speed_factor,
                vertical_speed_factor,
                fluid_height_blocks,
                flags,
                surface_response,
            },
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_primitives(
        &mut self,
        runtime_id: u32,
        boxes: impl IntoIterator<Item = Aabb>,
        friction: f64,
        horizontal_speed_factor: f64,
        vertical_speed_factor: f64,
        fluid_height_blocks: f64,
        flags: u8,
        surface_response: u8,
    ) -> Result<(), RegistryError> {
        let flags = BlockPhysicsFlags::from_bits(flags).ok_or(RegistryError::InvalidFlags {
            runtime_id,
            bits: flags,
        })?;
        let surface_response = SurfaceResponse::from_primitive(surface_response).ok_or(
            RegistryError::InvalidSurfaceResponse {
                runtime_id,
                value: surface_response,
            },
        )?;
        self.register_physics(
            runtime_id,
            boxes,
            friction,
            horizontal_speed_factor,
            vertical_speed_factor,
            fluid_height_blocks,
            flags,
            surface_response,
        )
    }

    fn physics(&self, runtime_id: u32) -> Option<&BlockPhysics> {
        self.blocks.get(&runtime_id)
    }
}

fn validate_shapes(runtime_id: u32, shapes: &[Aabb]) -> Result<(), RegistryError> {
    for (shape_index, shape) in shapes.iter().enumerate() {
        let coordinates = [
            shape.min.x,
            shape.min.y,
            shape.min.z,
            shape.max.x,
            shape.max.y,
            shape.max.z,
        ];
        if !coordinates.into_iter().all(f64::is_finite)
            || shape.min.x > shape.max.x
            || shape.min.y > shape.max.y
            || shape.min.z > shape.max.z
        {
            return Err(RegistryError::InvalidShape {
                runtime_id,
                shape_index,
            });
        }
        if shape.min.x < -1.0
            || shape.min.y < -1.0
            || shape.min.z < -1.0
            || shape.max.x > 2.0
            || shape.max.y > 2.0
            || shape.max.z > 2.0
        {
            return Err(RegistryError::ShapeOutsideLocalHalo {
                runtime_id,
                shape_index,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldQueryError {
    #[error("collision query contains non-finite or inverted bounds")]
    InvalidBounds,
    #[error("collision query coordinate is outside the i32 block range")]
    CoordinateOutOfRange,
    #[error("collision query exceeds the maximum bounded extent")]
    QueryExtentExceeded,
    #[error("collision identity exceeds the maximum of {max} chunks")]
    IdentityChunkLimitExceeded { max: usize },
    #[error("collision identities use different registries")]
    RegistryIdentityMismatch,
    #[error("collision identities disagree about chunk {chunk:?}")]
    ChunkRevisionConflict { chunk: ChunkKey },
    #[error("chunk {0:?} has not received a complete LevelChunk")]
    UnloadedChunk(ChunkKey),
    #[error("runtime ID {runtime_id} at {block:?} has no authoritative physics metadata")]
    UnknownRuntimeId { runtime_id: u32, block: [i32; 3] },
}

pub trait CollisionWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError>;

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: DEFAULT_SURFACE_FRICTION,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: BlockPhysicsFlags::default(),
                surface_response: SurfaceResponse::None,
            }]),
            identity: WorldCollisionIdentity::new(
                CollisionRegistryIdentity {
                    protocol: 1001,
                    id_space: CollisionIdSpace::Sequential,
                    preg_sha256: [0; 32],
                },
                [],
            )
            .expect("empty collision identity is bounded"),
        })
    }
}

/// Read-only collision adapter over the palette-packed world store.
pub struct PaletteWorld<'a> {
    store: &'a ChunkStore,
    registry: &'a CollisionRegistry,
    dimension: i32,
}

impl<'a> PaletteWorld<'a> {
    #[must_use]
    pub const fn new(
        store: &'a ChunkStore,
        registry: &'a CollisionRegistry,
        dimension: i32,
    ) -> Self {
        Self {
            store,
            registry,
            dimension,
        }
    }

    fn identity_for_chunks(
        &self,
        chunks: impl IntoIterator<Item = ChunkKey>,
    ) -> Result<WorldCollisionIdentity, WorldQueryError> {
        let revisions = chunks
            .into_iter()
            .map(|key| {
                self.store
                    .collision_revision(key)
                    .ok_or(WorldQueryError::UnloadedChunk(key))
            })
            .collect::<Result<Vec<_>, _>>()?;
        WorldCollisionIdentity::new(self.registry.identity(), revisions)
    }

    fn runtime_ids_at(&self, block: [i32; 3]) -> Result<Vec<u32>, WorldQueryError> {
        let [x, y, z] = block;
        let chunk = ChunkKey::new(self.dimension, x >> 4, z >> 4);
        if !self.store.is_chunk_loaded(chunk) {
            return Err(WorldQueryError::UnloadedChunk(chunk));
        }
        let key = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
        let Some(sub_chunk) = self.store.sub_chunk(key) else {
            return Ok(vec![self.registry.air_runtime_id]);
        };
        let ids = (0..sub_chunk.storages().len())
            .map(|layer| {
                sub_chunk
                    .runtime_id(
                        layer,
                        x.rem_euclid(16) as u8,
                        y.rem_euclid(16) as u8,
                        z.rem_euclid(16) as u8,
                    )
                    .expect("validated palette storage resolves every local coordinate")
            })
            .collect::<Vec<_>>();
        Ok(if ids.is_empty() {
            vec![self.registry.air_runtime_id]
        } else {
            ids
        })
    }
}

impl CollisionWorld for PaletteWorld<'_> {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        validate_collision_query(query)?;
        if query.min == query.max {
            return Ok(CollisionQuery {
                value: Vec::new(),
                identity: WorldCollisionIdentity::new(self.registry.identity(), [])?,
            });
        }
        let grown = query.grown(1.0);
        let min = block_floor(grown.min)?;
        let max = block_ceil(grown.max)?;
        let chunks = (min[0] >> 4..=max[0] >> 4)
            .flat_map(|x| {
                (min[2] >> 4..=max[2] >> 4).map(move |z| ChunkKey::new(self.dimension, x, z))
            })
            .collect::<Vec<_>>();
        let identity = self.identity_for_chunks(chunks)?;

        let mut result = Vec::new();
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                for y in min[1]..=max[1] {
                    let block = [x, y, z];
                    let block_offset = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                    for runtime_id in self.runtime_ids_at(block)? {
                        let physics = self
                            .registry
                            .physics(runtime_id)
                            .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
                        result.extend(
                            physics
                                .shapes
                                .iter()
                                .copied()
                                .map(|shape| shape.translated(block_offset))
                                .filter(|shape| shape.intersects(query)),
                        );
                    }
                }
            }
        }
        Ok(CollisionQuery {
            value: result,
            identity,
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let chunk = ChunkKey::new(self.dimension, block[0] >> 4, block[2] >> 4);
        let identity = self.identity_for_chunks([chunk])?;
        let layers = self
            .runtime_ids_at(block)?
            .into_iter()
            .map(|runtime_id| {
                let physics = self
                    .registry
                    .physics(runtime_id)
                    .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
                Ok(BlockPhysicsFacts {
                    friction: physics.friction,
                    horizontal_speed_factor: physics.horizontal_speed_factor,
                    vertical_speed_factor: physics.vertical_speed_factor,
                    fluid_height_blocks: physics.fluid_height_blocks,
                    flags: physics.flags,
                    surface_response: physics.surface_response,
                })
            })
            .collect::<Result<Vec<_>, WorldQueryError>>()?;
        Ok(BlockPhysicsSample {
            layers: layers.into_boxed_slice(),
            identity,
        })
    }
}

pub(crate) fn validate_collision_query(query: Aabb) -> Result<(), WorldQueryError> {
    let coordinates = [
        query.min.x,
        query.min.y,
        query.min.z,
        query.max.x,
        query.max.y,
        query.max.z,
    ];
    if !coordinates.into_iter().all(f64::is_finite)
        || query.min.x > query.max.x
        || query.min.y > query.max.y
        || query.min.z > query.max.z
    {
        return Err(WorldQueryError::InvalidBounds);
    }
    let min_coordinate = f64::from(i32::MIN) + 1.0;
    let max_coordinate = f64::from(i32::MAX) - 1.0;
    if coordinates
        .into_iter()
        .any(|value| value < min_coordinate || value > max_coordinate)
    {
        return Err(WorldQueryError::CoordinateOutOfRange);
    }
    let extent = query.max - query.min;
    if !extent.is_finite()
        || extent.x > MAX_COLLISION_QUERY_EXTENT
        || extent.y > MAX_COLLISION_QUERY_EXTENT
        || extent.z > MAX_COLLISION_QUERY_EXTENT
    {
        return Err(WorldQueryError::QueryExtentExceeded);
    }
    Ok(())
}

fn block_floor(value: Vec3) -> Result<[i32; 3], WorldQueryError> {
    convert_block_coords(value, f64::floor)
}

fn block_ceil(value: Vec3) -> Result<[i32; 3], WorldQueryError> {
    convert_block_coords(value, f64::ceil)
}

fn convert_block_coords(
    value: Vec3,
    round: impl Fn(f64) -> f64,
) -> Result<[i32; 3], WorldQueryError> {
    let values = [round(value.x), round(value.y), round(value.z)];
    if values
        .into_iter()
        .any(|value| value < f64::from(i32::MIN) || value > f64::from(i32::MAX))
    {
        return Err(WorldQueryError::CoordinateOutOfRange);
    }
    Ok(values.map(|value| value as i32))
}
