//! Vendor-independent world event types.
//!
//! Split out of `world.rs` so that module stays the normalisation logic rather
//! than the data model it produces; the two grew past the crate's file-size
//! budget together.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_40::MovePlayerPacketPositionMode;

use crate::{
    ActorEffectEvent, ActorEvent, ActorLinkEvent, ArmorEquipmentEvent, BlockCrackEvent,
    EquipmentEvent, InventoryEvent, ItemActorEvent, UiEvent,
};

use super::{HASHED_AIR_NETWORK_ID, SEQUENTIAL_AIR_NETWORK_ID};

/// Vertical sub-chunk span for one dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimensionRange {
    pub base_sub_chunk_y: i32,
    pub sub_chunk_count: usize,
}

/// Phase-zero dimension ranges matching the vanilla Bedrock dimensions.
#[must_use]
pub const fn vanilla_dimension_range(dimension: i32) -> Option<DimensionRange> {
    match dimension {
        0 => Some(DimensionRange {
            base_sub_chunk_y: -4,
            sub_chunk_count: 24,
        }),
        1 => Some(DimensionRange {
            base_sub_chunk_y: 0,
            sub_chunk_count: 8,
        }),
        2 => Some(DimensionRange {
            base_sub_chunk_y: 0,
            sub_chunk_count: 16,
        }),
        _ => None,
    }
}

/// Returns the raw network value that represents air for this StartGame mode.
#[must_use]
pub const fn air_network_id(block_network_ids_are_hashes: bool) -> u32 {
    if block_network_ids_are_hashes {
        HASHED_AIR_NETWORK_ID
    } else {
        SEQUENTIAL_AIR_NETWORK_ID
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelChunkMode {
    Inline { count: usize },
    LimitedRequests { highest: u16 },
    LimitlessRequests,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelChunkEvent {
    pub dimension: i32,
    pub x: i32,
    pub z: i32,
    pub mode: LevelChunkMode,
    pub payload: Vec<u8>,
}

/// Requests fresh, ordinary SubChunk data after one cached transaction was abandoned.
///
/// This is recovery control data, not substitute chunk content. The world streamer routes it
/// through the normal bounded request and retry scheduler. When `requested_sub_chunk_ys` is
/// present, it names the exact absolute section Ys to request for this column and takes
/// precedence over `requested_sub_chunks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkResyncEvent {
    pub dimension: i32,
    pub x: i32,
    pub z: i32,
    /// `None` requests the dimension's full vanilla vertical range.
    pub requested_sub_chunks: Option<usize>,
    /// Exact absolute section Ys to request for this column.
    pub requested_sub_chunk_ys: Option<Vec<i32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubChunkUnavailable {
    Undefined,
    ChunkNotFound,
    InvalidDimension,
    PlayerNotFound,
    YIndexOutOfBounds,
    Unknown(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubChunkResult {
    Success { payload: Vec<u8> },
    AllAir,
    Unavailable(SubChunkUnavailable),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubChunkEntryEvent {
    /// Absolute sub-chunk coordinates in X/Y/Z order.
    pub position: [i32; 3],
    pub result: SubChunkResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubChunkBatchEvent {
    pub dimension: i32,
    pub entries: Vec<SubChunkEntryEvent>,
}
/// Admission for a cached SubChunk response retained by the blob resolver.
///
/// This event carries no payload and does not mutate world state. It only
/// lets the client-world retry scheduler retire the exact response deadlines
/// while the reconstructed SubChunks event waits behind unresolved blobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubChunkReplyAdmissionEvent {
    pub dimension: i32,
    /// Absolute sub-chunk coordinates in X/Y/Z order.
    pub positions: Vec<[i32; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdateEvent {
    pub dimension: i32,
    /// Absolute block coordinates in X/Y/Z order.
    pub position: [i32; 3],
    pub layer: usize,
    pub network_id: u32,
}

/// One live block-entity NBT replacement from packet 56.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntityUpdateEvent {
    pub dimension: i32,
    /// Absolute block coordinates in X/Y/Z order.
    pub position: [i32; 3],
    /// Exact validated-by-Valentine NetworkLittleEndian NBT bytes. The world
    /// worker applies the stricter client limits before storage.
    pub nbt: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublisherUpdateEvent {
    /// Absolute block coordinates in X/Y/Z order.
    pub center: [i32; 3],
    pub radius_blocks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeDimensionEvent {
    pub dimension: i32,
    pub position: [f32; 3],
}

/// One server-driven local-player respawn phase.
///
/// The wire state and runtime ID are retained even when semantically unusual;
/// every well-formed respawn packet changes local position authority and must
/// reach the app instead of being silently dropped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RespawnEvent {
    pub position: [f32; 3],
    pub state: u8,
    pub runtime_entity_id: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MovePlayerEvent {
    pub runtime_id: u64,
    pub position: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub mode: MovePlayerMode,
    pub on_ground: bool,
    pub teleported: bool,
    pub source_tick: i64,
}

/// Visual eye height of a standing player above its feet.
pub const STANDING_PLAYER_EYE_HEIGHT: f32 = 1.62;

/// Bedrock's standing-player network-position offset for movement packets.
///
/// This is deliberately distinct from [`STANDING_PLAYER_EYE_HEIGHT`]. Actor
/// spawns use a feet origin, while player and actor-absolute movement positions
/// include a pose-specific protocol offset; sleeping is resolved from retained
/// actor metadata by the client world.
pub const PLAYER_NETWORK_OFFSET: f32 = 1.62001;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovePlayerMode {
    #[default]
    Normal,
    Reset,
    Teleport,
    Rotation,
    Unknown(u8),
}

impl MovePlayerMode {
    #[must_use]
    pub const fn is_teleport(self) -> bool {
        matches!(self, Self::Teleport)
    }
}

impl From<MovePlayerPacketPositionMode> for MovePlayerMode {
    fn from(mode: MovePlayerPacketPositionMode) -> Self {
        // 1.26.40 renames the mode variants but keeps the wire values, so this
        // stays a name-only remap: gophertunnel packet/move_player.go pins
        // MoveModeNormal=0, MoveModeReset=1, MoveModeTeleport=2,
        // MoveModeRotation=3, which are Normal/Respawn/Teleport/OnlyHeadRot here.
        match mode {
            MovePlayerPacketPositionMode::Normal => Self::Normal,
            MovePlayerPacketPositionMode::Respawn => Self::Reset,
            MovePlayerPacketPositionMode::Teleport => Self::Teleport,
            MovePlayerPacketPositionMode::OnlyHeadRot => Self::Rotation,
            MovePlayerPacketPositionMode::Unknown(value) => Self::Unknown(value),
        }
    }
}

/// One server world-clock update.
///
/// The signed Bedrock time is retained exactly. Interpreting negative values or
/// mapping ticks to a visual day cycle belongs to the app-owned clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTimeEvent {
    pub time: i32,
}

/// One runtime update to the world's daylight-cycle switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaylightCycleUpdateEvent {
    pub enabled: bool,
}

/// Weather channel targeted by a normalized level event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherChannel {
    Rain,
    Lightning,
}

/// One normalized weather-channel target from a Bedrock level event.
///
/// Start events target `1.0`; stop events target `0.0`. LevelEvent's integer
/// data is not an intensity and is intentionally excluded from this contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherUpdateEvent {
    pub channel: WeatherChannel,
    pub level: f32,
}

/// One server-authoritative correction for the local player's predicted movement.
///
/// Unlike [`MovePlayerEvent`], this packet carries no runtime ID: Bedrock sends it
/// directly to the player whose prediction is being corrected.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerMovementCorrectionEvent {
    pub position: [f32; 3],
    pub delta: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub on_ground: bool,
    pub tick: u64,
}

/// One live biome definition reduced to the fields required by tint lookup.
///
/// `biome_id` preserves the unsigned wire value except that `0xffff` is the
/// vanilla name-resolved sentinel and is represented as `None`.
/// Dragonfly's chunk palettes contain the separate stable `EncodeBiome()`
/// value; neither definition packet order nor `name_index` is that palette ID.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeDefinitionEvent {
    pub biome_id: Option<u16>,
    pub name: Arc<str>,
    pub temperature: f32,
    pub downfall: f32,
    pub snow_foliage: f32,
    pub map_water_color: u32,
}

/// Bounded, packet-order-preserving live biome definition snapshot.
///
/// Packet order is retained for deterministic diagnostics only. It must never
/// be treated as the runtime biome registry order.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeDefinitionsEvent {
    pub definitions: Arc<[BiomeDefinitionEvent]>,
}

/// Small, vendor-independent world events consumed by the Bevy app.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldEvent {
    BiomeDefinitions(BiomeDefinitionsEvent),
    LevelChunk(LevelChunkEvent),
    ChunkResync(ChunkResyncEvent),
    /// Confirms retained cached SubChunk replies before reconstruction.
    SubChunkReplyAdmission(SubChunkReplyAdmissionEvent),
    SubChunks(SubChunkBatchEvent),
    BlockUpdates(Vec<BlockUpdateEvent>),
    BlockEntityUpdate(BlockEntityUpdateEvent),
    ChunkRadiusUpdated(i32),
    PublisherUpdate(PublisherUpdateEvent),
    ChangeDimension(ChangeDimensionEvent),
    Respawn(RespawnEvent),
    MovePlayer(MovePlayerEvent),
    PlayerMovementCorrection(PlayerMovementCorrectionEvent),
    SetTime(SetTimeEvent),
    DaylightCycle(DaylightCycleUpdateEvent),
    Weather(WeatherUpdateEvent),
    Actor(ActorEvent),
    ActorEffect(ActorEffectEvent),
    ActorLink(ActorLinkEvent),
    Ui(UiEvent),
    BlockCrack(BlockCrackEvent),
    Equipment(EquipmentEvent),
    // Boxed: five item stacks would otherwise dominate every WorldEvent.
    ArmorEquipment(Box<ArmorEquipmentEvent>),
    Inventory(InventoryEvent),
    ItemActor(ItemActorEvent),
}
