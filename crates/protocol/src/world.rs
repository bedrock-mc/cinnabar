use std::sync::Arc;

use jolyne::GameData;
use thiserror::Error;
use valentine::bedrock::version::v1_26_30::{
    CorrectPlayerMovePredictionPacketPredictionType, GameRuleI32, GameRuleI32Type,
    GameRuleI32Value, GameRuleVarintType, GameRuleVarintValue, LevelEventPacketEvent,
    McpePacketData, MovePlayerPacketMode, StartGamePacketDimension,
    SubChunkEntryWithoutCachingItemResult, SubchunkPacketEntries, SubchunkRequestPacket, Vec3I8,
    Vec3Li,
};

use crate::{
    ActorEvent, ActorPacketError, EquipmentEvent, InventoryEvent, InventoryPacketError,
    ItemActorEvent, ItemPacketError, Packet,
    actor::{
        normalize_add_entity, normalize_add_player, normalize_move_entity,
        normalize_move_entity_delta, normalize_player_list, normalize_remove_entity,
        normalize_set_default_game_type, normalize_set_entity_data, normalize_update_attributes,
        normalize_update_player_game_type,
    },
    inventory::{
        normalize_container_close, normalize_container_data, normalize_container_open,
        normalize_content, normalize_hotbar, normalize_response, normalize_slot,
    },
    item::{
        normalize_animate, normalize_animate_entity, normalize_equipment, normalize_item_registry,
    },
    ui::{
        BlockCrackEvent, UiEvent, UiPacketError, normalize_block_crack, normalize_boss,
        normalize_display_objective, normalize_form, normalize_health, normalize_player_status,
        normalize_remove_objective, normalize_score, normalize_soft_enum, normalize_text,
        normalize_title, normalize_toast,
    },
};

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
/// This matches the generated v1001 packet decoder's collection ceiling, and
/// is repeated here because callers may construct generated packets directly.
pub const MAX_BIOME_DEFINITIONS: usize = 4_096;

/// Maximum UTF-8 bytes accepted for one live biome identifier.
pub const MAX_BIOME_NAME_BYTES: usize = 256;

/// StartGame data reduced to the fields required by the renderer and world streamer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldBootstrap {
    pub dimension: i32,
    pub local_player_runtime_id: u64,
    pub player_position: [f32; 3],
    pub world_spawn_position: [i32; 3],
    pub air_network_id: u32,
    pub block_network_ids_are_hashes: bool,
}

impl WorldBootstrap {
    #[must_use]
    pub fn from_game_data(game_data: &GameData) -> Self {
        let start_game = &game_data.start_game;
        Self {
            dimension: match start_game.dimension {
                StartGamePacketDimension::Overworld => 0,
                StartGamePacketDimension::Nether => 1,
                StartGamePacketDimension::End => 2,
                StartGamePacketDimension::Unknown(value) => value,
            },
            local_player_runtime_id: start_game.runtime_entity_id as u64,
            player_position: [
                start_game.player_position.x,
                start_game.player_position.y,
                start_game.player_position.z,
            ],
            world_spawn_position: [
                start_game.spawn_position.x,
                start_game.spawn_position.y,
                start_game.spawn_position.z,
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
        let start_game = &game_data.start_game;
        Self {
            initial_time: start_game.current_tick,
            day_cycle_lock_time: start_game.day_cycle_stop_time,
            daylight_cycle_enabled: start_game
                .gamerules
                .iter()
                .find_map(|rule| {
                    if rule.name.eq_ignore_ascii_case("dodaylightcycle")
                        && rule.type_ == GameRuleVarintType::Bool
                        && let Some(GameRuleVarintValue::Bool(enabled)) = &rule.value
                    {
                        Some(*enabled)
                    } else {
                        None
                    }
                })
                .unwrap_or(true),
            rain_level: normalize_weather_level(start_game.rain_level),
            lightning_level: normalize_weather_level(start_game.lightning_level),
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

impl From<MovePlayerPacketMode> for MovePlayerMode {
    fn from(mode: MovePlayerPacketMode) -> Self {
        match mode {
            MovePlayerPacketMode::Normal => Self::Normal,
            MovePlayerPacketMode::Reset => Self::Reset,
            MovePlayerPacketMode::Teleport => Self::Teleport,
            MovePlayerPacketMode::Rotation => Self::Rotation,
            MovePlayerPacketMode::Unknown(value) => Self::Unknown(value),
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
    SubChunks(SubChunkBatchEvent),
    BlockUpdates(Vec<BlockUpdateEvent>),
    BlockEntityUpdate(BlockEntityUpdateEvent),
    ChunkRadiusUpdated(i32),
    PublisherUpdate(PublisherUpdateEvent),
    ChangeDimension(ChangeDimensionEvent),
    MovePlayer(MovePlayerEvent),
    PlayerMovementCorrection(PlayerMovementCorrectionEvent),
    SetTime(SetTimeEvent),
    DaylightCycle(DaylightCycleUpdateEvent),
    Weather(WeatherUpdateEvent),
    Actor(ActorEvent),
    Ui(UiEvent),
    BlockCrack(BlockCrackEvent),
    Equipment(EquipmentEvent),
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
        McpePacketData::PacketText(packet) => WorldEvent::Ui(normalize_text(*packet)?),
        McpePacketData::PacketSetTitle(packet) => WorldEvent::Ui(normalize_title(*packet)?),
        McpePacketData::PacketToastRequest(packet) => WorldEvent::Ui(normalize_toast(packet)?),
        McpePacketData::PacketSetDisplayObjective(packet) => {
            WorldEvent::Ui(normalize_display_objective(*packet)?)
        }
        McpePacketData::PacketRemoveObjective(packet) => {
            WorldEvent::Ui(normalize_remove_objective(packet)?)
        }
        McpePacketData::PacketSetScore(packet) => WorldEvent::Ui(normalize_score(packet)?),
        McpePacketData::PacketBossEvent(packet) => WorldEvent::Ui(normalize_boss(*packet)?),
        McpePacketData::PacketModalFormRequest(packet) => WorldEvent::Ui(normalize_form(packet)?),
        McpePacketData::PacketSetHealth(packet) => WorldEvent::Ui(normalize_health(packet)),
        McpePacketData::PacketPlayStatus(packet) => {
            WorldEvent::Ui(normalize_player_status(packet)?)
        }
        McpePacketData::PacketUpdateSoftEnum(packet) => {
            WorldEvent::Ui(normalize_soft_enum(packet)?)
        }
        McpePacketData::PacketAddEntity(packet) => {
            WorldEvent::Actor(normalize_add_entity(*packet, current_dimension)?)
        }
        McpePacketData::PacketAddPlayer(packet) => {
            WorldEvent::Actor(normalize_add_player(*packet, current_dimension)?)
        }
        McpePacketData::PacketRemoveEntity(packet) => {
            WorldEvent::Actor(normalize_remove_entity(packet, current_dimension))
        }
        McpePacketData::PacketMoveEntity(packet) => {
            WorldEvent::Actor(normalize_move_entity(*packet, current_dimension)?)
        }
        McpePacketData::PacketMoveEntityDelta(packet) => {
            WorldEvent::Actor(normalize_move_entity_delta(*packet, current_dimension)?)
        }
        McpePacketData::PacketSetEntityData(packet) => {
            WorldEvent::Actor(normalize_set_entity_data(*packet, current_dimension)?)
        }
        McpePacketData::PacketUpdateAttributes(packet) => {
            WorldEvent::Actor(normalize_update_attributes(packet, current_dimension)?)
        }
        McpePacketData::PacketPlayerList(packet) => {
            WorldEvent::Actor(normalize_player_list(*packet)?)
        }
        McpePacketData::PacketUpdatePlayerGameType(packet) => {
            WorldEvent::Actor(normalize_update_player_game_type(packet)?)
        }
        McpePacketData::PacketSetDefaultGameType(packet) => {
            WorldEvent::Actor(normalize_set_default_game_type(packet))
        }
        McpePacketData::PacketItemRegistry(packet) => {
            WorldEvent::ItemActor(normalize_item_registry(packet)?)
        }
        McpePacketData::PacketMobEquipment(packet) => {
            WorldEvent::Equipment(normalize_equipment(*packet)?)
        }
        McpePacketData::PacketInventoryContent(packet) => {
            WorldEvent::Inventory(normalize_content(*packet)?)
        }
        McpePacketData::PacketInventorySlot(packet) => {
            WorldEvent::Inventory(normalize_slot(*packet)?)
        }
        McpePacketData::PacketPlayerHotbar(packet) => {
            WorldEvent::Inventory(normalize_hotbar(packet)?)
        }
        McpePacketData::PacketItemStackResponse(packet) => {
            WorldEvent::Inventory(normalize_response(packet)?)
        }
        McpePacketData::PacketContainerOpen(packet) => {
            WorldEvent::Inventory(normalize_container_open(*packet)?)
        }
        McpePacketData::PacketContainerClose(packet) => {
            WorldEvent::Inventory(normalize_container_close(packet)?)
        }
        McpePacketData::PacketContainerSetData(packet) => {
            WorldEvent::Inventory(normalize_container_data(packet)?)
        }
        McpePacketData::PacketAnimate(packet) => WorldEvent::ItemActor(normalize_animate(*packet)?),
        McpePacketData::PacketAnimateEntity(packet) => {
            WorldEvent::ItemActor(normalize_animate_entity(*packet)?)
        }
        McpePacketData::PacketBiomeDefinitionList(packet) => {
            if packet.biome_definitions.len() > MAX_BIOME_DEFINITIONS {
                return Err(WorldPacketError::TooManyBiomeDefinitions {
                    count: packet.biome_definitions.len(),
                    max: MAX_BIOME_DEFINITIONS,
                });
            }
            let mut definitions = Vec::with_capacity(packet.biome_definitions.len());
            for (definition_index, definition) in packet.biome_definitions.into_iter().enumerate() {
                let name = usize::try_from(definition.name_index)
                    .ok()
                    .and_then(|index| packet.string_list.get(index))
                    .ok_or(WorldPacketError::InvalidBiomeNameIndex {
                        index: definition.name_index,
                        string_count: packet.string_list.len(),
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
                    ("snow_foliage", definition.snow_foliage),
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
                    biome_id: (definition.biome_id != u16::MAX).then_some(definition.biome_id),
                    name,
                    temperature: definition.temperature,
                    downfall: definition.downfall,
                    snow_foliage: definition.snow_foliage,
                    map_water_color: definition.map_water_colour as u32,
                });
            }
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from(definitions),
            })
        }
        McpePacketData::PacketLevelChunk(packet) => {
            if packet.blobs.is_some() {
                return Err(WorldPacketError::CachedChunksUnsupported);
            }
            let mode = match packet.sub_chunk_count {
                count if count >= 0 => {
                    let count = count as usize;
                    // Bound by the absolute protocol maximum, not the vanilla
                    // dimension height: custom servers advertise standard
                    // dimension ids with taller-than-vanilla world columns.
                    if count > MAX_SUB_CHUNK_REQUESTS {
                        return Err(WorldPacketError::InlineSubChunkCountExceedsDimension {
                            dimension: packet.dimension,
                            count,
                            max: MAX_SUB_CHUNK_REQUESTS,
                        });
                    }
                    LevelChunkMode::Inline { count }
                }
                -2 => LevelChunkMode::LimitedRequests {
                    highest: packet
                        .highest_subchunk_count
                        .ok_or(WorldPacketError::MissingHighestSubChunk)?,
                },
                -1 => LevelChunkMode::LimitlessRequests,
                count => return Err(WorldPacketError::InvalidSubChunkCount(count)),
            };
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: packet.dimension,
                x: packet.x,
                z: packet.z,
                mode,
                payload: packet.payload,
            })
        }
        McpePacketData::PacketSubchunk(packet) => {
            let SubchunkPacketEntries::SubChunkEntryWithoutCaching(entries) = packet.entries else {
                return Err(WorldPacketError::CachedChunksUnsupported);
            };
            let origin = [packet.origin.x, packet.origin.y, packet.origin.z];
            let mut normalized = Vec::with_capacity(entries.len());
            for entry in entries {
                let offset = [entry.dx, entry.dy, entry.dz];
                let position = checked_sub_chunk_position(origin, offset)?;
                let result = match entry.result {
                    SubChunkEntryWithoutCachingItemResult::Success => SubChunkResult::Success {
                        payload: entry.payload,
                    },
                    SubChunkEntryWithoutCachingItemResult::SuccessAllAir => SubChunkResult::AllAir,
                    SubChunkEntryWithoutCachingItemResult::Undefined => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::Undefined)
                    }
                    SubChunkEntryWithoutCachingItemResult::ChunkNotFound => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound)
                    }
                    SubChunkEntryWithoutCachingItemResult::InvalidDimension => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension)
                    }
                    SubChunkEntryWithoutCachingItemResult::PlayerNotFound => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::PlayerNotFound)
                    }
                    SubChunkEntryWithoutCachingItemResult::YIndexOutOfBounds => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds)
                    }
                    SubChunkEntryWithoutCachingItemResult::Unknown(value) => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::Unknown(value))
                    }
                };
                normalized.push(SubChunkEntryEvent { position, result });
            }
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: packet.dimension,
                entries: normalized,
            })
        }
        McpePacketData::PacketUpdateBlock(packet) => {
            let layer = normalize_layer(packet.layer)?;
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: current_dimension,
                position: [packet.position.x, packet.position.y, packet.position.z],
                layer,
                network_id: packet.block_runtime_id as u32,
            }])
        }
        McpePacketData::PacketUpdateSubchunkBlocks(packet) => {
            let mut updates = Vec::with_capacity(packet.blocks.len() + packet.extra.len());
            updates.extend(packet.blocks.into_iter().map(|update| BlockUpdateEvent {
                dimension: current_dimension,
                position: [update.position.x, update.position.y, update.position.z],
                layer: 0,
                network_id: update.runtime_id as u32,
            }));
            updates.extend(packet.extra.into_iter().map(|update| BlockUpdateEvent {
                dimension: current_dimension,
                position: [update.position.x, update.position.y, update.position.z],
                layer: 1,
                network_id: update.runtime_id as u32,
            }));
            WorldEvent::BlockUpdates(updates)
        }
        McpePacketData::PacketBlockEntityData(packet) => {
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: current_dimension,
                position: [packet.position.x, packet.position.y, packet.position.z],
                nbt: packet.nbt.0.to_vec(),
            })
        }
        McpePacketData::PacketChunkRadiusUpdate(packet) => {
            WorldEvent::ChunkRadiusUpdated(packet.chunk_radius)
        }
        McpePacketData::PacketNetworkChunkPublisherUpdate(packet) => {
            let radius_blocks = u32::try_from(packet.radius)
                .map_err(|_| WorldPacketError::InvalidPublisherRadius(packet.radius))?;
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [
                    packet.coordinates.x,
                    packet.coordinates.y,
                    packet.coordinates.z,
                ],
                radius_blocks,
            })
        }
        McpePacketData::PacketChangeDimension(packet) => {
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: packet.dimension,
                position: [packet.position.x, packet.position.y, packet.position.z],
            })
        }
        McpePacketData::PacketMovePlayer(packet) => {
            let mode = MovePlayerMode::from(packet.mode);
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: packet.runtime_id,
                position: [packet.position.x, packet.position.y, packet.position.z],
                pitch: packet.pitch,
                yaw: packet.yaw,
                head_yaw: packet.head_yaw,
                mode,
                on_ground: packet.on_ground,
                teleported: mode.is_teleport(),
                source_tick: packet.tick,
            })
        }
        McpePacketData::PacketCorrectPlayerMovePrediction(packet) => {
            if packet.prediction_type != CorrectPlayerMovePredictionPacketPredictionType::Player {
                return Ok(None);
            }
            let tick = u64::try_from(packet.tick)
                .map_err(|_| WorldPacketError::NegativeMovementCorrectionTick(packet.tick))?;
            WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
                position: [packet.position.x, packet.position.y, packet.position.z],
                delta: [packet.delta.x, packet.delta.y, packet.delta.z],
                pitch: packet.rotation.x,
                yaw: packet.rotation.z,
                on_ground: packet.on_ground,
                tick,
            })
        }
        McpePacketData::PacketSetTime(packet) => {
            WorldEvent::SetTime(SetTimeEvent { time: packet.time })
        }
        McpePacketData::PacketGameRulesChanged(packet) => {
            let Some(enabled) = daylight_cycle_rule_update(&packet.rules) else {
                return Ok(None);
            };
            WorldEvent::DaylightCycle(DaylightCycleUpdateEvent { enabled })
        }
        McpePacketData::PacketLevelEvent(packet) => {
            if matches!(
                packet.event,
                LevelEventPacketEvent::BlockStartBreak
                    | LevelEventPacketEvent::BlockStopBreak
                    | LevelEventPacketEvent::BlockBreakSpeed
            ) {
                return Ok(Some(WorldEvent::BlockCrack(normalize_block_crack(packet)?)));
            }
            let update = match packet.event {
                LevelEventPacketEvent::StartRain => WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 1.0,
                },
                LevelEventPacketEvent::StopRain => WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 0.0,
                },
                LevelEventPacketEvent::StartThunder => WeatherUpdateEvent {
                    channel: WeatherChannel::Lightning,
                    level: 1.0,
                },
                LevelEventPacketEvent::StopThunder => WeatherUpdateEvent {
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

fn daylight_cycle_rule_update(rules: &[GameRuleI32]) -> Option<bool> {
    rules.iter().find_map(|rule| {
        if rule.name.eq_ignore_ascii_case("dodaylightcycle")
            && rule.type_ == GameRuleI32Type::Bool
            && let Some(GameRuleI32Value::Bool(enabled)) = &rule.value
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
    let known_vanilla = valentine::bedrock::version::v1_26_30::biomes::ALL_BIOMES
        .iter()
        .any(|biome| biome.string_id.strip_prefix("minecraft:") == Some(name));
    if known_vanilla {
        Arc::from(format!("minecraft:{name}"))
    } else {
        Arc::from(name)
    }
}

/// Builds one bounded vertical-column SubChunkRequest.
pub fn request_sub_chunk_column(
    dimension: i32,
    chunk_x: i32,
    chunk_z: i32,
    base_sub_chunk_y: i32,
    count: usize,
) -> Result<Packet, WorldPacketError> {
    if count > MAX_SUB_CHUNK_REQUESTS {
        return Err(WorldPacketError::TooManySubChunkRequests {
            count,
            max: MAX_SUB_CHUNK_REQUESTS,
        });
    }
    let mut requests = Vec::with_capacity(count);
    for offset in 0..count {
        let offset_i32 = i32::try_from(offset).expect("request count is capped at 128");
        base_sub_chunk_y.checked_add(offset_i32).ok_or(
            WorldPacketError::SubChunkRequestYOverflow {
                base_y: base_sub_chunk_y,
                offset,
            },
        )?;
        requests.push(Vec3I8 {
            x: 0,
            y: offset as i8,
            z: 0,
        });
    }
    Ok(SubchunkRequestPacket {
        dimension,
        requests,
        origin: Vec3Li {
            x: chunk_x,
            y: base_sub_chunk_y,
            z: chunk_z,
        },
    }
    .into())
}

fn normalize_layer(layer: i32) -> Result<usize, WorldPacketError> {
    let normalized =
        usize::try_from(layer).map_err(|_| WorldPacketError::InvalidBlockLayer(layer))?;
    if normalized >= MAX_BLOCK_LAYERS {
        return Err(WorldPacketError::InvalidBlockLayer(layer));
    }
    Ok(normalized)
}

fn checked_sub_chunk_position(
    origin: [i32; 3],
    offset: [i8; 3],
) -> Result<[i32; 3], WorldPacketError> {
    let mut position = [0; 3];
    for axis in 0..3 {
        position[axis] = origin[axis]
            .checked_add(i32::from(offset[axis]))
            .ok_or(WorldPacketError::SubChunkPositionOverflow { origin, offset })?;
    }
    Ok(position)
}
