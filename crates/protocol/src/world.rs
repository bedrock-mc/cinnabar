use std::sync::Arc;

use jolyne::GameData;
use thiserror::Error;
use valentine::bedrock::version::v1_26_40::{
    CorrectPlayerMovePredictionPacketPredictionType, GameRule, GameRuleRuleValue, McpePacketData,
    MovePlayerPacketPositionMode, RespawnPacketState,
    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult,
};

use crate::{
    ActorEffectEvent, ActorEvent, ActorLinkEvent, ActorPacketError, ArmorEquipmentEvent,
    EquipmentEvent, InventoryEvent, InventoryPacketError, ItemActorEvent, ItemPacketError, Packet,
    actor::{
        normalize_add_entity, normalize_add_player, normalize_mob_effect, normalize_move_entity,
        normalize_move_entity_delta, normalize_player_list, normalize_remove_entity,
        normalize_set_entity_data, normalize_set_entity_link, normalize_update_attributes,
    },
    inventory::{
        normalize_armor_equipment, normalize_container_close, normalize_container_data,
        normalize_container_open, normalize_content, normalize_hotbar, normalize_response,
        normalize_slot,
    },
    item::{
        normalize_animate, normalize_animate_entity, normalize_equipment, normalize_item_registry,
    },
    ui::{
        BlockCrackEvent, GameModeEvent, UiEvent, UiPacketError, normalize_block_crack,
        normalize_boss, normalize_display_objective, normalize_form, normalize_health,
        normalize_player_status, normalize_remove_objective, normalize_score, normalize_soft_enum,
        normalize_text, normalize_title, normalize_toast,
    },
};

mod game_mode;
mod requests;
pub use self::game_mode::PlayerGameMode;
pub use self::requests::request_sub_chunk_column;
use self::requests::{checked_sub_chunk_position, normalize_layer};

/// Sequential palette state ID generated for `minecraft:air` in 1.26.30.
pub const SEQUENTIAL_AIR_NETWORK_ID: u32 = 12_530;

/// Canonical block-state network hash for `minecraft:air`.
pub const HASHED_AIR_NETWORK_ID: u32 = 0xdbf4_4120;

/// Client safety limit for block storage layers in update packets.
pub const MAX_BLOCK_LAYERS: usize = 16;

/// Maximum Y offsets emitted in one column SubChunkRequest.
pub const MAX_SUB_CHUNK_REQUESTS: usize = 128;

/// Maximum live biome definitions retained from one server packet.
///
/// This matched the 1.26.30 generated decoder's own collection ceiling. The
/// 1.26.40 generated crate emits no collection ceilings at all (see the module
/// header of `tests/world_collection_bounds.rs`), so this is now the only bound
/// applied to the list and it must stay enforced here.
pub const MAX_BIOME_DEFINITIONS: usize = 4_096;

/// Maximum UTF-8 bytes accepted for one live biome identifier.
pub const MAX_BIOME_NAME_BYTES: usize = 256;

// LevelEvent ids. 1.26.40 stopped modelling LevelEventPacket's event as a
// generated enum (`LevelEventPacket.event_id` is a bare varint32), so the ids
// this crate reacts to are pinned here from gophertunnel
// `minecraft/protocol/packet/level_event.go` @ be6713da4dc051a4197f897d04835e89e9c54321.
/// `LevelEventStartRaining`.
pub(crate) const LEVEL_EVENT_START_RAINING: i32 = 3001;
/// `LevelEventStartThunderstorm`.
pub(crate) const LEVEL_EVENT_START_THUNDERSTORM: i32 = 3002;
/// `LevelEventStopRaining`.
pub(crate) const LEVEL_EVENT_STOP_RAINING: i32 = 3003;
/// `LevelEventStopThunderstorm`.
pub(crate) const LEVEL_EVENT_STOP_THUNDERSTORM: i32 = 3004;
/// `LevelEventStartBlockCracking`.
pub(crate) const LEVEL_EVENT_START_BLOCK_CRACKING: i32 = 3600;
/// `LevelEventStopBlockCracking`.
pub(crate) const LEVEL_EVENT_STOP_BLOCK_CRACKING: i32 = 3601;
/// `LevelEventUpdateBlockCracking`.
pub(crate) const LEVEL_EVENT_UPDATE_BLOCK_CRACKING: i32 = 3602;

/// StartGame data reduced to the fields required by the renderer and world streamer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldBootstrap {
    pub dimension: i32,
    pub local_player_runtime_id: u64,
    /// StartGame's unique (persistent) local-player entity id, required to
    /// recognize the local rider in SetActorLink events.
    pub local_player_unique_id: i64,
    pub player_position: [f32; 3],
    pub world_spawn_position: [i32; 3],
    pub air_network_id: u32,
    pub block_network_ids_are_hashes: bool,
}

impl WorldBootstrap {
    #[must_use]
    pub fn from_game_data(game_data: &GameData) -> Self {
        let start_game = &game_data.start_game;
        let settings = &start_game.settings;
        Self {
            // 1.26.40 stopped naming the vanilla dimension ids in an enum and
            // moved the field into LevelSettings' spawn block. gophertunnel
            // packet/start_game.go writes `Dimension int32` (Varint32) right
            // after `UserDefinedBiomeName`, which is exactly
            // `settings.spawn_settings.dimension` here, so the raw id is used.
            dimension: settings.spawn_settings.dimension,
            local_player_runtime_id: start_game.runtime_id.actor_runtime_id as u64,
            local_player_unique_id: start_game.entity_id.actor_unique_id,
            player_position: [
                start_game.position.x,
                start_game.position.y,
                start_game.position.z,
            ],
            world_spawn_position: [
                settings.default_spawn_block_position.x,
                settings.default_spawn_block_position.y,
                settings.default_spawn_block_position.z,
            ],
            air_network_id: air_network_id(start_game.block_network_ids_are_hashes),
            block_network_ids_are_hashes: start_game.block_network_ids_are_hashes,
        }
    }
}

/// Initial clock and weather state retained from StartGame.
///
/// This is separate from [`WorldBootstrap`] so existing world-stream
/// construction remains independent of the later app-owned atmosphere state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldEnvironmentBootstrap {
    /// StartGame's current absolute world tick.
    pub initial_time: i64,
    /// StartGame's cycle lock tick, used only when the daylight cycle is disabled.
    pub day_cycle_lock_time: i32,
    /// Whether the world clock advances between server-authored time updates.
    pub daylight_cycle_enabled: bool,
    /// Initial rain intensity clamped to the closed unit interval.
    pub rain_level: f32,
    /// Initial lightning intensity clamped to the closed unit interval.
    pub lightning_level: f32,
}

impl WorldEnvironmentBootstrap {
    #[must_use]
    pub fn from_game_data(game_data: &GameData) -> Self {
        let settings = &game_data.start_game.settings;
        Self {
            // gophertunnel packet/start_game.go writes `Time int64`; the
            // generated field is u64 over the same eight little-endian bytes.
            initial_time: game_data.start_game.level_current_time as i64,
            day_cycle_lock_time: settings.day_cycle_stop_time,
            // StartGame and GameRulesChanged now carry the same `GameRule`
            // type, so the two rule scans collapse into one helper.
            daylight_cycle_enabled: daylight_cycle_rule_update(&settings.rule_data.rules_list)
                .unwrap_or(true),
            rain_level: normalize_weather_level(settings.rain_level),
            lightning_level: normalize_weather_level(settings.lightning_level),
        }
    }
}

fn normalize_weather_level(level: f32) -> f32 {
    if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldPacketError {
    #[error(transparent)]
    Actor(#[from] ActorPacketError),

    #[error(transparent)]
    Ui(#[from] UiPacketError),

    #[error(transparent)]
    Item(#[from] ItemPacketError),

    #[error(transparent)]
    Inventory(#[from] InventoryPacketError),

    #[error("BiomeDefinitionList has {count} definitions, exceeding {max}")]
    TooManyBiomeDefinitions { count: usize, max: usize },

    #[error("biome definition name index {index} is outside string table of length {string_count}")]
    InvalidBiomeNameIndex { index: i16, string_count: usize },

    #[error("biome name has {bytes} UTF-8 bytes, exceeding {max}")]
    BiomeNameTooLong { bytes: usize, max: usize },

    #[error("biome definition {definition} has non-finite {field}")]
    NonFiniteBiomeClimate {
        definition: usize,
        field: &'static str,
    },

    #[error("unsupported LevelChunk sub-chunk count {0}")]
    InvalidSubChunkCount(i32),

    /// Unreachable since 1.26.40.
    ///
    /// The limit is no longer gated behind a `-2` SubChunkCount sentinel that a
    /// server could set without supplying the value; gophertunnel
    /// `packet/level_chunk.go` models it as `SubChunkLimit Optional[int32]`, so
    /// "request mode" and "limit present" are the same bit. Retained so the
    /// public error surface does not change.
    #[error("limited LevelChunk omitted HighestSubChunk")]
    MissingHighestSubChunk,

    #[error("inline LevelChunk count {count} exceeds dimension {dimension} maximum {max}")]
    InlineSubChunkCountExceedsDimension {
        dimension: i32,
        count: usize,
        max: usize,
    },

    #[error("client cache chunk blobs are disabled in the phase-zero client")]
    CachedChunksUnsupported,

    #[error("sub-chunk origin {origin:?} plus offset {offset:?} overflows i32")]
    SubChunkPositionOverflow { origin: [i32; 3], offset: [i8; 3] },

    #[error("block update layer {0} is outside 0..{MAX_BLOCK_LAYERS}")]
    InvalidBlockLayer(i32),

    #[error("publisher radius {0} is not a valid unsigned block radius")]
    InvalidPublisherRadius(i32),

    #[error("server-authoritative movement correction tick {0} is negative")]
    NegativeMovementCorrectionTick(i64),

    #[error("SubChunkRequest has {count} offsets, exceeding {max}")]
    TooManySubChunkRequests { count: usize, max: usize },

    #[error("SubChunkRequest base Y {base_y} plus offset {offset} overflows i32")]
    SubChunkRequestYOverflow { base_y: i32, offset: usize },
}

/// Converts a generated packet into the bounded world surface used by the app.
/// Packets unrelated to world streaming return `Ok(None)`.
pub fn into_world_event(
    packet: Packet,
    current_dimension: i32,
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let event = match packet.data {
        McpePacketData::TextPacket(packet) => WorldEvent::Ui(normalize_text(*packet)?),
        McpePacketData::CommandOutputPacket(packet) => {
            WorldEvent::Ui(crate::ui::normalize_command_output(*packet)?)
        }
        McpePacketData::SetTitlePacket(packet) => WorldEvent::Ui(normalize_title(*packet)?),
        McpePacketData::ToastRequestPacket(packet) => WorldEvent::Ui(normalize_toast(packet)?),
        McpePacketData::SetDisplayObjectivePacket(packet) => {
            WorldEvent::Ui(normalize_display_objective(*packet)?)
        }
        McpePacketData::RemoveObjectivePacket(packet) => {
            WorldEvent::Ui(normalize_remove_objective(packet)?)
        }
        McpePacketData::SetScorePacket(packet) => WorldEvent::Ui(normalize_score(packet)?),
        McpePacketData::BossEventPacket(packet) => WorldEvent::Ui(normalize_boss(*packet)?),
        McpePacketData::ModalFormRequestPacket(packet) => WorldEvent::Ui(normalize_form(packet)?),
        McpePacketData::SetHealthPacket(packet) => WorldEvent::Ui(normalize_health(packet)),
        McpePacketData::PlayStatusPacket(packet) => {
            WorldEvent::Ui(normalize_player_status(packet)?)
        }
        McpePacketData::UpdateSoftEnumPacket(packet) => {
            WorldEvent::Ui(normalize_soft_enum(packet)?)
        }
        McpePacketData::AddActorPacket(packet) => {
            WorldEvent::Actor(normalize_add_entity(*packet, current_dimension)?)
        }
        McpePacketData::AddPlayerPacket(packet) => {
            WorldEvent::Actor(normalize_add_player(*packet, current_dimension)?)
        }
        McpePacketData::RemoveActorPacket(packet) => {
            WorldEvent::Actor(normalize_remove_entity(packet, current_dimension))
        }
        McpePacketData::MoveActorAbsolutePacket(packet) => {
            WorldEvent::Actor(normalize_move_entity(*packet, current_dimension)?)
        }
        McpePacketData::MoveActorDeltaPacket(packet) => {
            WorldEvent::Actor(normalize_move_entity_delta(*packet, current_dimension)?)
        }
        McpePacketData::SetActorDataPacket(packet) => {
            WorldEvent::Actor(normalize_set_entity_data(*packet, current_dimension)?)
        }
        McpePacketData::UpdateAttributesPacket(packet) => {
            WorldEvent::Actor(normalize_update_attributes(packet, current_dimension)?)
        }
        McpePacketData::PlayerListPacket(packet) => {
            WorldEvent::Actor(normalize_player_list(packet)?)
        }
        McpePacketData::ItemRegistryPacket(packet) => {
            WorldEvent::ItemActor(normalize_item_registry(packet)?)
        }
        McpePacketData::MobEquipmentPacket(packet) => {
            WorldEvent::Equipment(normalize_equipment(*packet)?)
        }
        McpePacketData::MobArmorEquipmentPacket(packet) => {
            WorldEvent::ArmorEquipment(Box::new(normalize_armor_equipment(*packet)?))
        }
        McpePacketData::MobEffectPacket(packet) => {
            WorldEvent::ActorEffect(normalize_mob_effect(*packet, current_dimension)?)
        }
        McpePacketData::SetActorLinkPacket(packet) => {
            WorldEvent::ActorLink(normalize_set_entity_link(*packet, current_dimension))
        }
        McpePacketData::SetPlayerGameTypePacket(packet) => {
            WorldEvent::Ui(UiEvent::GameMode(GameModeEvent {
                update: PlayerGameMode::update_from_game_mode(packet.player_game_type),
            }))
        }
        McpePacketData::SetDefaultGameTypePacket(packet) => {
            WorldEvent::Ui(UiEvent::DefaultGameMode(GameModeEvent {
                update: PlayerGameMode::update_from_default_game_mode(packet.default_game_type),
            }))
        }
        McpePacketData::InventoryContentPacket(packet) => {
            WorldEvent::Inventory(normalize_content(*packet)?)
        }
        McpePacketData::InventorySlotPacket(packet) => {
            WorldEvent::Inventory(normalize_slot(*packet)?)
        }
        McpePacketData::PlayerHotbarPacket(packet) => {
            WorldEvent::Inventory(normalize_hotbar(packet)?)
        }
        McpePacketData::ItemStackResponsePacket(packet) => {
            WorldEvent::Inventory(normalize_response(packet)?)
        }
        McpePacketData::ContainerOpenPacket(packet) => {
            WorldEvent::Inventory(normalize_container_open(*packet)?)
        }
        McpePacketData::ContainerClosePacket(packet) => {
            WorldEvent::Inventory(normalize_container_close(packet)?)
        }
        McpePacketData::ContainerSetDataPacket(packet) => {
            WorldEvent::Inventory(normalize_container_data(packet)?)
        }
        McpePacketData::AnimatePacket(packet) => WorldEvent::ItemActor(normalize_animate(*packet)?),
        McpePacketData::AnimateEntityPacket(packet) => {
            WorldEvent::ItemActor(normalize_animate_entity(*packet)?)
        }
        McpePacketData::BiomeDefinitionListPacket(packet) => {
            // 1.26.40 renames the packet's two collections and splits each
            // entry into a `key` (the string-table index) plus a `value`
            // payload. The wire is unchanged: gophertunnel protocol/biome.go
            // writes Int16 NameIndex, Int16 BiomeID, then the climate floats.
            let string_list = packet.stringlist.strings;
            let biome_definitions = packet.mapof_biomenamestodata;
            if biome_definitions.len() > MAX_BIOME_DEFINITIONS {
                return Err(WorldPacketError::TooManyBiomeDefinitions {
                    count: biome_definitions.len(),
                    max: MAX_BIOME_DEFINITIONS,
                });
            }
            let mut definitions = Vec::with_capacity(biome_definitions.len());
            for (definition_index, definition) in biome_definitions.into_iter().enumerate() {
                // The generated key is u16 while gophertunnel declares the same
                // two bytes as a signed Int16, so it is reinterpreted here to
                // keep out-of-range indices reported exactly as before.
                let name_index = definition.key as i16;
                let definition = definition.value;
                let name = usize::try_from(name_index)
                    .ok()
                    .and_then(|index| string_list.get(index))
                    .ok_or(WorldPacketError::InvalidBiomeNameIndex {
                        index: name_index,
                        string_count: string_list.len(),
                    })?;
                if name.len() > MAX_BIOME_NAME_BYTES {
                    return Err(WorldPacketError::BiomeNameTooLong {
                        bytes: name.len(),
                        max: MAX_BIOME_NAME_BYTES,
                    });
                }
                for (field, value) in [
                    ("temperature", definition.temperature),
                    ("downfall", definition.downfall),
                    ("snow_foliage", definition.foliagesnow),
                ] {
                    if !value.is_finite() {
                        return Err(WorldPacketError::NonFiniteBiomeClimate {
                            definition: definition_index,
                            field,
                        });
                    }
                }
                let name = canonical_biome_name(name);
                definitions.push(BiomeDefinitionEvent {
                    biome_id: (definition.id != u16::MAX).then_some(definition.id),
                    name,
                    temperature: definition.temperature,
                    downfall: definition.downfall,
                    snow_foliage: definition.foliagesnow,
                    map_water_color: definition.mapwatercolor_argb as u32,
                });
            }
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from(definitions),
            })
        }
        McpePacketData::LevelChunkPacket(packet) => {
            // gophertunnel packet/level_chunk.go writes BlobHashes
            // unconditionally now, so the presence of hashes no longer marks a
            // cached transfer. `CacheEnabled` is the authoritative gate.
            if packet.cache_enabled {
                return Err(WorldPacketError::CachedChunksUnsupported);
            }
            // The old `-1` / `-2` sentinels folded into SubChunkCount are gone.
            // gophertunnel reads SubChunkCount as a Varuint32 (and rejects
            // values above 64), then reads `SubChunkLimit Optional[int32]`.
            // Presence of the limit is what selects client-request mode; its
            // documented `-1` value means "no limit".
            let mode = match packet.client_request_sub_chunk_limit {
                Some(-1) => LevelChunkMode::LimitlessRequests,
                Some(limit) => LevelChunkMode::LimitedRequests {
                    highest: u16::try_from(limit)
                        .map_err(|_| WorldPacketError::InvalidSubChunkCount(limit))?,
                },
                None => {
                    // SubChunkCount is unsigned on the wire but decoded into an
                    // i32, so anything above i32::MAX still surfaces as negative.
                    if packet.subchunks_count < 0 {
                        return Err(WorldPacketError::InvalidSubChunkCount(
                            packet.subchunks_count,
                        ));
                    }
                    let count = packet.subchunks_count as usize;
                    // Bound by the absolute protocol maximum, not the vanilla
                    // dimension height: custom servers advertise standard
                    // dimension ids with taller-than-vanilla world columns.
                    if count > MAX_SUB_CHUNK_REQUESTS {
                        return Err(WorldPacketError::InlineSubChunkCountExceedsDimension {
                            dimension: packet.dimension_id.value,
                            count,
                            max: MAX_SUB_CHUNK_REQUESTS,
                        });
                    }
                    LevelChunkMode::Inline { count }
                }
            };
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: packet.dimension_id.value,
                x: packet.chunk_position.x,
                z: packet.chunk_position.z,
                mode,
                payload: packet.serialized_chunk_data,
            })
        }
        McpePacketData::SubChunkPacket(packet) => {
            // The 1.26.30 split between cached and non-cached entry lists is
            // gone: gophertunnel protocol/sub_chunk.go models one SubChunkEntry
            // with `BlobHash Optional[uint64]`, and packet/sub_chunk.go keeps
            // the packet-level CacheEnabled flag as the mode switch.
            if packet.cache_enabled {
                return Err(WorldPacketError::CachedChunksUnsupported);
            }
            let origin = [
                packet.center_pos.subchunk_position_x,
                packet.center_pos.subchunk_position_y,
                packet.center_pos.subchunk_position_z,
            ];
            let mut normalized = Vec::with_capacity(packet.sub_chunk_data.len());
            for entry in packet.sub_chunk_data {
                let offset = [
                    entry.sub_chunk_pos_offset.subchunk_offset_x,
                    entry.sub_chunk_pos_offset.subchunk_offset_y,
                    entry.sub_chunk_pos_offset.subchunk_offset_z,
                ];
                let position = checked_sub_chunk_position(origin, offset)?;
                // Result names were realigned onto the vanilla SubChunkResult
                // constants (gophertunnel protocol/sub_chunk.go): Undefined=0,
                // Success=1, ChunkNotFound=2, InvalidDimension=3,
                // PlayerNotFound=4, IndexOutOfBounds=5, SuccessAllAir=6.
                let result = match entry.sub_chunk_request_result {
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Success => {
                        SubChunkResult::Success {
                            // RawPayload is Optional now; a Success entry
                            // without one carries no sub-chunk bytes, which is
                            // the same empty payload 1.26.30 would have decoded.
                            payload: entry.serialized_sub_chunk.unwrap_or_default(),
                        }
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::SuccessAllAir => {
                        SubChunkResult::AllAir
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Undefined => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::Undefined)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::LevelChunkDoesntExist => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::WrongDimension => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::PlayerDoesntExist => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::PlayerNotFound)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::IndexOutOfBounds => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Unknown(value) => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::Unknown(value))
                    }
                };
                normalized.push(SubChunkEntryEvent { position, result });
            }
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: packet.dimension_type.value,
                entries: normalized,
            })
        }
        McpePacketData::UpdateBlockPacket(packet) => {
            let layer = normalize_layer(packet.layer)?;
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: current_dimension,
                position: [
                    packet.block_position.x,
                    packet.block_position.y,
                    packet.block_position.z,
                ],
                layer,
                network_id: packet.block_runtime_id as u32,
            }])
        }
        McpePacketData::UpdateSubChunkBlocksPacket(packet) => {
            // The two block lists moved into a nested `blocks_changed` struct;
            // gophertunnel packet/update_sub_chunk_blocks.go still writes
            // Blocks (layer 0) then Extra (layer 1) back to back.
            let standards = packet.blocks_changed.blocks_changed_standards;
            let extras = packet.blocks_changed.blocks_changed_extras;
            let mut updates = Vec::with_capacity(standards.len() + extras.len());
            updates.extend(standards.into_iter().map(|update| BlockUpdateEvent {
                dimension: current_dimension,
                position: [update.pos.x, update.pos.y, update.pos.z],
                layer: 0,
                network_id: update.runtime_id as u32,
            }));
            updates.extend(extras.into_iter().map(|update| BlockUpdateEvent {
                dimension: current_dimension,
                position: [update.pos.x, update.pos.y, update.pos.z],
                layer: 1,
                network_id: update.runtime_id as u32,
            }));
            WorldEvent::BlockUpdates(updates)
        }
        McpePacketData::BlockActorDataPacket(packet) => {
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: current_dimension,
                position: [
                    packet.block_position.x,
                    packet.block_position.y,
                    packet.block_position.z,
                ],
                nbt: packet.actor_data_tags.0.to_vec(),
            })
        }
        McpePacketData::ChunkRadiusUpdatedPacket(packet) => {
            WorldEvent::ChunkRadiusUpdated(packet.chunk_radius)
        }
        McpePacketData::NetworkChunkPublisherUpdatePacket(packet) => {
            let radius_blocks = u32::try_from(packet.newradiusforview).map_err(|_| {
                WorldPacketError::InvalidPublisherRadius(packet.newradiusforview)
            })?;
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [
                    packet.newpositionforview.x,
                    packet.newpositionforview.y,
                    packet.newpositionforview.z,
                ],
                radius_blocks,
            })
        }
        McpePacketData::ChangeDimensionPacket(packet) => {
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: packet.dimension_id.value,
                position: [packet.position.x, packet.position.y, packet.position.z],
            })
        }
        McpePacketData::RespawnPacket(packet) => WorldEvent::Respawn(RespawnEvent {
            position: [packet.position.x, packet.position.y, packet.position.z],
            // The state byte is typed now; gophertunnel packet/respawn.go pins
            // SearchingForSpawn=0, ReadyToSpawn=1, ClientReadyToSpawn=2, and
            // this event deliberately keeps the raw wire value.
            state: match packet.state {
                RespawnPacketState::SearchingForSpawn => 0,
                RespawnPacketState::ReadyToSpawn => 1,
                RespawnPacketState::ClientReadyToSpawn => 2,
                RespawnPacketState::Unknown(value) => value,
            },
            runtime_entity_id: packet.player_runtime_id.actor_runtime_id,
        }),
        McpePacketData::MovePlayerPacket(packet) => {
            let mode = MovePlayerMode::from(packet.position_mode);
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: packet.player_runtime_id.actor_runtime_id as u64,
                position: [packet.position.x, packet.position.y, packet.position.z],
                // gophertunnel packet/move_player.go writes Pitch then Yaw as
                // two float32s, which the generated crate models as a Vec2
                // whose second component is `y`, not `z`.
                pitch: packet.rotation.x,
                yaw: packet.rotation.y,
                head_yaw: packet.y_head_rotation,
                mode,
                on_ground: packet.on_ground,
                teleported: mode.is_teleport(),
                source_tick: packet.tick.inputtick,
            })
        }
        McpePacketData::CorrectPlayerMovePredictionPacket(packet) => {
            if packet.prediction_type != CorrectPlayerMovePredictionPacketPredictionType::Player {
                return Ok(None);
            }
            let tick = packet.tick.inputtick;
            let tick = u64::try_from(tick)
                .map_err(|_| WorldPacketError::NegativeMovementCorrectionTick(tick))?;
            WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
                position: [packet.pos.x, packet.pos.y, packet.pos.z],
                delta: [packet.pos_delta.x, packet.pos_delta.y, packet.pos_delta.z],
                // Vec2's components are (x, y) in 1.26.40; gophertunnel
                // packet/correct_player_move_prediction.go writes Rotation as
                // one Vec2 of (pitch, yaw).
                pitch: packet.rotation.x,
                yaw: packet.rotation.y,
                on_ground: packet.on_ground,
                tick,
            })
        }
        McpePacketData::SetTimePacket(packet) => {
            WorldEvent::SetTime(SetTimeEvent { time: packet.time })
        }
        McpePacketData::GameRulesChangedPacket(packet) => {
            let Some(enabled) = daylight_cycle_rule_update(&packet.rule_data.rules_list) else {
                return Ok(None);
            };
            WorldEvent::DaylightCycle(DaylightCycleUpdateEvent { enabled })
        }
        McpePacketData::LevelEventPacket(packet) => {
            if matches!(
                packet.event_id,
                LEVEL_EVENT_START_BLOCK_CRACKING
                    | LEVEL_EVENT_STOP_BLOCK_CRACKING
                    | LEVEL_EVENT_UPDATE_BLOCK_CRACKING
            ) {
                return Ok(Some(WorldEvent::BlockCrack(normalize_block_crack(packet)?)));
            }
            let update = match packet.event_id {
                LEVEL_EVENT_START_RAINING => WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 1.0,
                },
                LEVEL_EVENT_STOP_RAINING => WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 0.0,
                },
                LEVEL_EVENT_START_THUNDERSTORM => WeatherUpdateEvent {
                    channel: WeatherChannel::Lightning,
                    level: 1.0,
                },
                LEVEL_EVENT_STOP_THUNDERSTORM => WeatherUpdateEvent {
                    channel: WeatherChannel::Lightning,
                    level: 0.0,
                },
                _ => return Ok(None),
            };
            WorldEvent::Weather(update)
        }
        _ => return Ok(None),
    };
    Ok(Some(event))
}

/// Reads the authoritative `doDaylightCycle` switch from a rule list.
///
/// 1.26.40 collapses the 1.26.30 `GameRuleI32` / `GameRuleVarint` pair (and
/// their separate `type_` discriminants) into one `GameRule` whose value is a
/// tagged union, so the redundant "declared type matches the value arm" check
/// the old modelling required is gone: a non-boolean rule simply cannot decode
/// into `GameRuleRuleValue::Bool`.
fn daylight_cycle_rule_update(rules: &[GameRule]) -> Option<bool> {
    rules.iter().find_map(|rule| {
        if rule.rule_name.eq_ignore_ascii_case("dodaylightcycle")
            && let GameRuleRuleValue::Bool(enabled) = &rule.rule_value
        {
            Some(*enabled)
        } else {
            None
        }
    })
}

fn canonical_biome_name(name: &str) -> Arc<str> {
    if name.contains(':') {
        return Arc::from(name);
    }
    // Content registries stay on v1_26_30. The 1.26.40 crate is generated from
    // the Endstone dump, which is wire-only and ships no biome/block/item/state
    // tables; the prismarine-derived 1.26.30 crate remains the only source for
    // this data. Biome string IDs are stable across the two versions, so this is
    // a data pin, not a protocol dependency.
    let known_vanilla = valentine::bedrock::version::v1_26_30::biomes::ALL_BIOMES
        .iter()
        .any(|biome| biome.string_id.strip_prefix("minecraft:") == Some(name));
    if known_vanilla {
        Arc::from(format!("minecraft:{name}"))
    } else {
        Arc::from(name)
    }
}
