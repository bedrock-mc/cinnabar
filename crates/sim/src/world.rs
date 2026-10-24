use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;
use world::{ChunkCollisionRevision, ChunkKey, ChunkStore, SubChunkKey};

use crate::{Aabb, Vec3};

mod bamboo;
mod camera_collision;
mod collision;
mod contracts;
mod current;
mod door;
mod flow;
mod selection;
mod terrain;
pub use camera_collision::CameraShapeCache;
pub use contracts::CollisionWorld;
pub use flow::FlowBlockFacts;
pub use terrain::{
    TERRAIN_EMPTY_SHAPE, TERRAIN_NO_LAYER, TerrainColumn, TerrainGeometry, TerrainSample,
    TerrainSampleError, TerrainSampler, TerrainShape, TerrainShapeBinding, TerrainShapeOverride,
    TerrainState,
};
mod liquid_probe;
pub(crate) use liquid_probe::liquid_probe_bounds;
mod primary_lookup;
mod snapshot;
pub use snapshot::CollisionSnapshot;
mod identity;
mod observation;
mod physics_table;
mod raycast;
mod section;
pub use door::{DoorFacing, DoorState};
mod validate;

pub use raycast::{BLOCK_USE_SUPPORT_DEPTH, BLOCK_USE_SUPPORT_MAX_Y, BlockHit, CameraBlockHit};
use validate::{validate_facts, validate_shapes};

/// Surface friction of a block that declares no friction of its own.
pub const DEFAULT_SURFACE_FRICTION: f64 = 0.6;
/// Maximum width, height, or depth of a collision query in blocks.
pub const MAX_COLLISION_QUERY_EXTENT: f64 = 128.0;
/// Inclusive coordinate bounds checked before scanning collision owner cells.
pub(super) const COLLISION_QUERY_COORDINATE_RANGE: [f64; 2] =
    [i32::MIN as f64 + 1.0, i32::MAX as f64 - 1.0];
/// Maximum number of distinct columns retained by one immutable query identity.
pub const MAX_COLLISION_IDENTITY_CHUNKS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Collision facts read together; zero revisions record absent movement-boundary columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldCollisionIdentity {
    pub registry: CollisionRegistryIdentity,
    pub chunks: Box<[ChunkCollisionRevision]>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldIdentityWire {
    protocol: u32,
    id_space: CollisionIdSpace,
    preg_sha256: [u8; 32],
    chunks: Vec<ChunkRevisionWire>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChunkRevisionWire {
    dimension: i32,
    x: i32,
    z: i32,
    revision: u64,
}

impl Serialize for WorldCollisionIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WorldIdentityWire {
            protocol: self.registry.protocol,
            id_space: self.registry.id_space,
            preg_sha256: self.registry.preg_sha256,
            chunks: self
                .chunks
                .iter()
                .map(|revision| ChunkRevisionWire {
                    dimension: revision.chunk.dimension,
                    x: revision.chunk.x,
                    z: revision.chunk.z,
                    revision: revision.revision,
                })
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorldCollisionIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorldIdentityWire::deserialize(deserializer)?;
        Self::new(
            CollisionRegistryIdentity {
                protocol: wire.protocol,
                id_space: wire.id_space,
                preg_sha256: wire.preg_sha256,
            },
            wire.chunks
                .into_iter()
                .map(|revision| ChunkCollisionRevision {
                    chunk: ChunkKey::new(revision.dimension, revision.x, revision.z),
                    revision: revision.revision,
                }),
        )
        .map_err(D::Error::custom)
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone)]
pub struct CollisionRegistry {
    identity: CollisionRegistryIdentity,
    blocks: Arc<physics_table::PhysicsTable>,
    shape_state_ids: Arc<BTreeMap<[u8; 32], u32>>,
    air_runtime_id: u32,
    collision_halo: [(i32, i32); 3],
    selection_halo: selection::halo::SelectionHalo,
}

#[derive(Debug, Clone)]
struct BlockPhysics {
    actor_facing: Option<u8>,
    actor_visual: Option<Aabb>,
    random_offset: Option<(
        block_transform::random_offset::RandomOffsetComponent,
        [f32; 3],
    )>,
    door: Option<DoorState>,
    flow: Option<FlowBlockFacts>,
    /// Shared among every state registered with bit-identical shapes.
    shapes: Arc<[Aabb]>,
    /// Shapes the interaction ray targets instead of `shapes` (selection boxes).
    pick_shapes: Option<Box<[Aabb]>>,
    pick_rule: assets::block_shapes::PickRule,
    shape_flags: assets::block_shapes::ShapeFlags,
    selection_context: assets::block_shapes::SelectionContext,
    contextual_selection: Box<[Aabb]>,
    outline: Option<Aabb>,
    connected_outline: Option<Aabb>,
    outline_context: assets::block_shapes::OutlineContext,
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
    #[error("runtime ID {runtime_id} contains contradictory authoritative physics facts")]
    ContradictoryFacts { runtime_id: u32 },
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
            blocks: Arc::new(physics_table::PhysicsTable::new(identity.id_space)),
            shape_state_ids: Arc::new(BTreeMap::new()),
            air_runtime_id: 0,
            collision_halo: [(0, 0); 3],
            selection_halo: selection::halo::SelectionHalo::default(),
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
        validate_facts(
            runtime_id,
            &shapes,
            fluid_height_blocks,
            flags,
            surface_response,
        )?;
        for shape in &shapes {
            for (axis, range) in self.collision_halo.iter_mut().enumerate() {
                if shape.max[axis] > 1.0 {
                    range.0 = range.0.min(-1);
                }
                if shape.min[axis] < 0.0 {
                    range.1 = range.1.max(1);
                }
            }
        }
        let table = Arc::make_mut(&mut self.blocks);
        let shapes = table.intern_shapes(shapes);
        table.insert(
            runtime_id,
            BlockPhysics {
                actor_facing: None,
                actor_visual: None,
                random_offset: None,
                door: None,
                flow: None,
                shapes,
                pick_shapes: None,
                pick_rule: assets::block_shapes::PickRule::Always,
                shape_flags: assets::block_shapes::ShapeFlags::empty(),
                selection_context: assets::block_shapes::SelectionContext::Static,
                contextual_selection: Box::new([]),
                outline: None,
                connected_outline: None,
                outline_context: assets::block_shapes::OutlineContext::Static,
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

    /// Drops every registration at or above `first_runtime_id`.
    pub fn remove_runtime_ids_from(&mut self, first_runtime_id: u32) {
        Arc::make_mut(&mut self.blocks).remove_from(first_runtime_id);
    }

    /// Makes the interaction ray target `boxes` (empty: untargetable) instead of the
    /// collision shapes; returns whether `runtime_id` is registered.
    pub fn set_pick_shapes(
        &mut self,
        runtime_id: u32,
        boxes: impl IntoIterator<Item = Aabb>,
    ) -> bool {
        let Some(block) = Arc::make_mut(&mut self.blocks).get_mut(runtime_id) else {
            return false;
        };
        block.pick_shapes = Some(boxes.into_iter().collect());
        true
    }

    /// Attaches compiled pick eligibility to an existing runtime ID.
    pub fn set_pick_rule(&mut self, runtime_id: u32, rule: assets::block_shapes::PickRule) -> bool {
        let Some(block) = Arc::make_mut(&mut self.blocks).get_mut(runtime_id) else {
            return false;
        };
        block.pick_rule = rule;
        true
    }

    /// Drops one registration; returns whether it existed.
    pub fn remove_runtime_id(&mut self, runtime_id: u32) -> bool {
        Arc::make_mut(&mut self.blocks).remove(runtime_id)
    }

    #[must_use]
    pub fn contains_runtime_id(&self, runtime_id: u32) -> bool {
        self.blocks.contains(runtime_id)
    }

    /// Block-local collision boxes of a registered runtime id.
    #[must_use]
    pub fn collision_shapes(&self, runtime_id: u32) -> Option<&[Aabb]> {
        self.physics(runtime_id).map(|physics| &*physics.shapes)
    }

    /// Block-local bounds used by the pick ray and its selection outline.
    #[must_use]
    pub fn selection_shapes(&self, runtime_id: u32) -> Option<&[Aabb]> {
        self.physics(runtime_id)
            .map(|physics| physics.pick_shapes.as_deref().unwrap_or(&physics.shapes))
    }

    /// Surface and fluid movement facts of a registered runtime id.
    #[must_use]
    pub fn movement_facts(&self, runtime_id: u32) -> Option<BlockPhysicsFacts> {
        self.physics(runtime_id).map(|physics| BlockPhysicsFacts {
            friction: physics.friction,
            horizontal_speed_factor: physics.horizontal_speed_factor,
            vertical_speed_factor: physics.vertical_speed_factor,
            fluid_height_blocks: physics.fluid_height_blocks,
            flags: physics.flags,
            surface_response: physics.surface_response,
        })
    }

    fn physics(&self, runtime_id: u32) -> Option<&BlockPhysics> {
        self.blocks.get(runtime_id)
    }
}

/// One emitted collision instance plus its exact palette-cell provenance.
///
/// [`PaletteWorld::collision_boxes`] emits one translated registry shape
/// instance per (block, layer, shape) triple and never merges neighbouring
/// cells into one box, so provenance is exact whenever an entry comes from
/// that adapter. The generic [`CollisionWorld::collision_boxes_with_provenance`]
/// fallback instead reports neither fact (`block: None`, `runtime_id: None`)
/// because a bare-AABB surface cannot know it; consumers must treat missing
/// provenance as unknown rather than re-inferring it from geometry.
///
/// Invariant: `runtime_id` is meaningful only together with `block`; an entry
/// carrying a runtime id without its block cell is treated as unprovenanced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProvenancedCollider {
    /// Translated collider bounds, identical to the same instance's slot in
    /// [`CollisionWorld::collision_boxes`].
    pub aabb: Aabb,
    /// The palette cell whose registered shape produced this collider.
    pub block: Option<[i32; 3]>,
    /// Wire runtime id (widened from the registry's `u32` key space) of the
    /// palette entry on the contributing layer.
    pub runtime_id: Option<u64>,
    /// Whether any layer of `block` is scaffolding, read under the enclosing
    /// collision query's identity. `None` requires a lazy material query when
    /// classification is needed; actors and boundaries have no cell fact.
    pub cell_has_scaffolding: Option<bool>,
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
    #[error("block interaction ray origin is non-finite or outside the supported block range")]
    InvalidRayOrigin,
    #[error("block interaction ray direction must be finite and non-zero")]
    InvalidRayDirection,
    #[error("block interaction ray distance must be finite, positive, and query-bounded")]
    InvalidRayDistance,
    #[error("collision identity is stale or does not cover chunk {chunk:?}")]
    StaleCollisionIdentity { chunk: ChunkKey },
    #[error("block interaction ray exceeded its checked inspected-block bound")]
    RayInspectionLimitExceeded,
}

/// Cells a lenient camera query skipped rather than faulting the whole query.
/// Counts are per scanned cell/layer and saturate; identity-free by design.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LenientSkipCounts {
    pub unknown_runtime_id: u32,
    pub unloaded_chunk: u32,
}

/// Collision boxes from the camera-lenient query plus the cells it skipped.
/// Carries no world identity: the camera never gates authority on it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LenientCollisionBoxes {
    pub value: Vec<Aabb>,
    pub skipped: LenientSkipCounts,
}

/// Read-only collision adapter over the palette-packed world store.
pub struct PaletteWorld<'a> {
    read_observation: Option<observation::ReadObservation>,
    unsupported_actor_selections: std::sync::atomic::AtomicU32,
    pick_context: assets::block_shapes::PickContext,
    store: &'a ChunkStore,
    registry: &'a CollisionRegistry,
    dimension: i32,
    unloaded_collision_barriers: bool,
    actor_colliders: Option<std::sync::Arc<[Aabb]>>,
}

/// One internal collision instance before projection onto the public
/// box-only or provenance surfaces.
struct CollisionInstance {
    shape: Aabb,
    block: Option<[i32; 3]>,
    runtime_id: Option<u32>,
    cell_has_scaffolding: Option<bool>,
    /// A powder snow cell's block box, which collides only for some movers.
    powder_snow: bool,
}

/// Colliders for a mover's own move: those every mover meets, and the powder
/// snow cells whose collision depends on the mover.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MoverColliders {
    /// The colliders of [`CollisionWorld::collision_boxes_with_provenance`].
    pub colliders: Vec<ProvenancedCollider>,
    /// Powder snow cells whose block box intersects the query.
    pub powder_snow: Vec<[i32; 3]>,
}

impl CollisionInstance {
    /// Projects the instance onto the public provenance surface.
    fn provenanced(&self) -> ProvenancedCollider {
        ProvenancedCollider {
            aabb: self.shape,
            block: self.block,
            runtime_id: self.runtime_id.map(u64::from),
            cell_has_scaffolding: self.cell_has_scaffolding,
        }
    }
}

impl<'a> PaletteWorld<'a> {
    /// Borrows palette data with an explicit context for every picking query.
    /// Callers that only query movement may supply the default context.
    ///
    /// Omitting the context is a compile error:
    /// ```compile_fail,E0061
    /// let store = world::ChunkStore::new();
    /// let registry = sim::CollisionRegistry::new();
    /// let view = sim::PaletteWorld::new(&store, &registry, 0);
    /// ```
    #[must_use]
    pub const fn new(
        store: &'a ChunkStore,
        registry: &'a CollisionRegistry,
        dimension: i32,
        pick_context: assets::block_shapes::PickContext,
    ) -> Self {
        Self {
            read_observation: None,
            unsupported_actor_selections: std::sync::atomic::AtomicU32::new(0),
            pick_context,
            store,
            registry,
            dimension,
            unloaded_collision_barriers: false,
            actor_colliders: None,
        }
    }

    /// Adds immutable actor bounds for movement and retains them in prediction snapshots.
    #[must_use]
    pub fn with_actor_colliders(mut self, colliders: impl Into<std::sync::Arc<[Aabb]>>) -> Self {
        self.actor_colliders = Some(colliders.into());
        self
    }

    fn identity_for_chunks(
        &self,
        chunks: impl IntoIterator<Item = ChunkKey>,
    ) -> Result<WorldCollisionIdentity, WorldQueryError> {
        let revisions = chunks
            .into_iter()
            .map(|key| self.column_revision(key))
            .collect::<Result<Vec<_>, _>>()?;
        WorldCollisionIdentity::new(self.registry.identity(), revisions)
    }

    /// The identity of the one column holding `block`, as
    /// [`Self::identity_for_chunks`] would build it.
    fn identity_for_block(
        &self,
        block: [i32; 3],
    ) -> Result<WorldCollisionIdentity, WorldQueryError> {
        let chunk = ChunkKey::new(self.dimension, block[0] >> 4, block[2] >> 4);
        Ok(WorldCollisionIdentity::single(
            self.registry.identity(),
            self.column_revision(chunk)?,
        ))
    }

    /// A loaded column's revision, or the boundary revision of an absent movement column.
    fn column_revision(&self, key: ChunkKey) -> Result<ChunkCollisionRevision, WorldQueryError> {
        self.record_column_read(
            self.store
                .collision_revision(key)
                .or_else(|| self.unloaded_boundary_revision(key))
                .ok_or(WorldQueryError::UnloadedChunk(key)),
        )
    }

    /// Whether every layer of a loaded block is air.
    pub fn is_air(&self, block: [i32; 3]) -> Result<bool, WorldQueryError> {
        Ok(self
            .layers_at(block, &mut section::SectionCache::default())?
            .iter()
            .all(|runtime_id| runtime_id == self.registry.air_runtime_id))
    }
}

impl CollisionWorld for PaletteWorld<'_> {
    /// Keeps absent-column material fallbacks confined to neighbouring travel regions.
    fn ensure_movement_origin(&self, feet: Vec3) -> Result<(), WorldQueryError> {
        self.require_movement_origin(feet)
    }

    fn registry_identity(&self) -> CollisionRegistryIdentity {
        self.registry.identity()
    }

    fn liquid_current(
        &self,
        previous_pose: Aabb,
    ) -> Result<Option<CollisionQuery<Vec3>>, WorldQueryError> {
        current::sample(self, previous_pose)
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery {
            value: self.primary_runtime_id(block)? == self.registry.air_runtime_id,
            identity: self.identity_for_block(block)?,
        }))
    }

    fn snapshot(&self) -> Option<CollisionSnapshot> {
        Some(CollisionSnapshot::capture(self))
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut value = Vec::new();
        let identity =
            self.visit_collision_instances(query, &mut |instance: CollisionInstance| {
                if !instance.powder_snow {
                    value.push(instance.shape);
                }
            })?;
        Ok(CollisionQuery { value, identity })
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        let mut value = Vec::new();
        let identity =
            self.visit_collision_instances(query, &mut |instance: CollisionInstance| {
                if !instance.powder_snow {
                    value.push(instance.provenanced());
                }
            })?;
        Ok(CollisionQuery { value, identity })
    }

    /// Reports powder snow cells from the same scan as the ordinary colliders.
    fn collision_boxes_for_mover(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<MoverColliders>, WorldQueryError> {
        let mut value = MoverColliders::default();
        let identity = self.visit_collision_instances(
            query,
            &mut |instance: CollisionInstance| match instance.block {
                Some(block) if instance.powder_snow => value.powder_snow.push(block),
                _ => value.colliders.push(instance.provenanced()),
            },
        )?;
        Ok(CollisionQuery { value, identity })
    }

    /// Collects the same borrowed camera scan for callers that need owned colliders.
    fn collision_boxes_camera_lenient(
        &self,
        query: Aabb,
    ) -> Result<LenientCollisionBoxes, WorldQueryError> {
        let mut value = Vec::new();
        let skipped = self.visit_camera_colliders(query, &mut |shape| value.push(shape))?;
        Ok(LenientCollisionBoxes { value, skipped })
    }

    /// Borrows camera shapes and resolves paired doors on the stack.
    fn visit_collision_boxes_camera_lenient(
        &self,
        query: Aabb,
        visitor: &mut dyn FnMut(Aabb),
    ) -> Result<LenientSkipCounts, WorldQueryError> {
        self.visit_camera_colliders(query, visitor)
    }

    fn camera_segment_entry(
        &self,
        origin: Vec3,
        delta: Vec3,
    ) -> Result<(Option<f64>, LenientSkipCounts), WorldQueryError> {
        self.camera_segment_entry_lenient(origin, delta)
    }

    /// Reads the same layers and availability as `block_physics`, without its owned result.
    fn visit_block_materials(
        &self,
        block: [i32; 3],
        visitor: &mut dyn FnMut(BlockPhysicsFacts),
    ) -> Result<(), WorldQueryError> {
        self.column_revision(ChunkKey::new(self.dimension, block[0] >> 4, block[2] >> 4))?;
        for runtime_id in self
            .layers_at(block, &mut section::SectionCache::default())?
            .iter()
        {
            let facts = self
                .registry
                .movement_facts(runtime_id)
                .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
            visitor(facts);
        }
        Ok(())
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let identity = self.identity_for_block(block)?;
        let layers = self
            .layers_at(block, &mut section::SectionCache::default())?
            .iter()
            .map(|runtime_id| {
                self.registry
                    .movement_facts(runtime_id)
                    .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })
            })
            .collect::<Result<Box<[_]>, WorldQueryError>>()?;
        Ok(BlockPhysicsSample { layers, identity })
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
    let [min_coordinate, max_coordinate] = COLLISION_QUERY_COORDINATE_RANGE;
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

#[cfg(test)]
mod review_default_tests;
