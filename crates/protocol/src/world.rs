use std::sync::Arc;

use jolyne::GameData;
use thiserror::Error;
use valentine::bedrock::version::v1_26_44::LevelChunkPacketView;
use valentine::bedrock::version::v1_26_44::{
    EnumsPlayerRespawnState as RespawnPacketState,
    EnumsRewindType as CorrectPlayerMovePredictionPacketPredictionType,
    EnumsSubChunkPacketPayloadSubChunkRequestResult as SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult,
    GameRule, GameRuleRuleValue, McpePacketData,
};

use crate::{
    ActorPacketError, InventoryPacketError, ItemPacketError, Packet,
    actor::{
        normalize_add_entity, normalize_add_player, normalize_mob_effect, normalize_move_entity,
        normalize_move_entity_delta, normalize_player_list, normalize_remove_entity,
        normalize_set_entity_data, normalize_set_entity_link, normalize_update_attributes,
    },
    audio::{normalize_level_sound, normalize_play_sound, normalize_stop_sound},
    inventory::{
        normalize_armor_equipment, normalize_container_close, normalize_container_data,
        normalize_container_open, normalize_content, normalize_hotbar, normalize_response,
        normalize_slot,
    },
    item::{
        normalize_animate, normalize_animate_entity, normalize_equipment, normalize_item_registry,
    },
    ui::{
        GameModeEvent, UiEvent, UiPacketError, normalize_block_crack, normalize_boss,
        normalize_display_objective, normalize_form, normalize_health, normalize_player_status,
        normalize_remove_objective, normalize_score, normalize_soft_enum, normalize_text,
        normalize_title, normalize_toast,
    },
};

mod events;
mod game_mode;
mod requests;
pub use self::events::{
    BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockEntityUpdateEvent, BlockUpdateEvent,
    ChangeDimensionEvent, ChunkResyncEvent, DaylightCycleUpdateEvent, DimensionRange,
    LevelChunkEvent, LevelChunkMode, MovePlayerEvent, MovePlayerMode, PLAYER_NETWORK_OFFSET,
    PlayerMovementCorrectionEvent, PublisherUpdateEvent, RespawnEvent, STANDING_PLAYER_EYE_HEIGHT,
    SetTimeEvent, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkReplyAdmissionEvent,
    SubChunkResult, SubChunkUnavailable, WeatherChannel, WeatherUpdateEvent, WorldEvent,
    air_network_id, vanilla_dimension_range,
};
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
            local_player_runtime_id: start_game.runtime_id.actor_runtime_id,
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

    #[error("{field} has {bytes} UTF-8 bytes, exceeding {max}")]
    AudioIdentifierTooLong {
        field: &'static str,
        bytes: usize,
        max: usize,
    },

    #[error("{field} is not valid UTF-8")]
    InvalidAudioIdentifierUtf8 { field: &'static str },

    #[error("{field} is non-finite")]
    NonFiniteAudioField { field: &'static str },

    #[error("MovePlayer {field} is non-finite")]
    NonFiniteMovePlayerField { field: &'static str },

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
    InvalidBlockLayer(u32),

    #[error("publisher radius {0} is not a valid unsigned block radius")]
    InvalidPublisherRadius(u32),

    #[error("server-authoritative movement correction tick {0} is outside i64 range")]
    MovementCorrectionTickOutOfRange(u64),

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
        McpePacketData::PlaySoundPacket(packet) => {
            WorldEvent::Audio(normalize_play_sound(*packet)?)
        }
        McpePacketData::StopSoundPacket(packet) => WorldEvent::Audio(normalize_stop_sound(packet)?),
        McpePacketData::LevelSoundEventPacket(packet) => {
            WorldEvent::Audio(normalize_level_sound(*packet)?)
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
            let mode = level_chunk_mode(
                packet.client_request_sub_chunk_limit,
                packet.subchunks_count,
                packet.dimension_id.value,
            )?;
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
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Unknown(0) => {
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
                network_id: packet.block_runtime_id,
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
                network_id: update.runtime_id,
            }));
            updates.extend(extras.into_iter().map(|update| BlockUpdateEvent {
                dimension: current_dimension,
                position: [update.pos.x, update.pos.y, update.pos.z],
                layer: 1,
                network_id: update.runtime_id,
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
            let radius_blocks = packet.newradiusforview;
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
            for (field, value) in [
                ("position x", packet.position.x),
                ("position y", packet.position.y),
                ("position z", packet.position.z),
                ("pitch", packet.rotation.x),
                ("yaw", packet.rotation.y),
                ("head yaw", packet.y_head_rotation),
            ] {
                if !value.is_finite() {
                    return Err(WorldPacketError::NonFiniteMovePlayerField { field });
                }
            }
            let mode = MovePlayerMode::from(packet.position_mode);
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: packet.player_runtime_id.actor_runtime_id,
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

fn level_chunk_mode(
    request_limit: Option<i32>,
    subchunks_count: u32,
    dimension: i32,
) -> Result<LevelChunkMode, WorldPacketError> {
    match request_limit {
        Some(-1) => Ok(LevelChunkMode::LimitlessRequests),
        Some(limit) => Ok(LevelChunkMode::LimitedRequests {
            highest: u16::try_from(limit)
                .map_err(|_| WorldPacketError::InvalidSubChunkCount(limit))?,
        }),
        None => {
            let count = usize::try_from(subchunks_count)
                .map_err(|_| WorldPacketError::InvalidSubChunkCount(i32::MAX))?;
            if count > MAX_SUB_CHUNK_REQUESTS {
                return Err(WorldPacketError::InlineSubChunkCountExceedsDimension {
                    dimension,
                    count,
                    max: MAX_SUB_CHUNK_REQUESTS,
                });
            }
            Ok(LevelChunkMode::Inline { count })
        }
    }
}

pub(crate) fn normalize_borrowed_level_chunk(
    packet: LevelChunkPacketView,
) -> Result<(LevelChunkEvent, bytes::Bytes), WorldPacketError> {
    if packet.cache_enabled {
        return Err(WorldPacketError::CachedChunksUnsupported);
    }
    let mode = level_chunk_mode(
        packet.client_request_sub_chunk_limit,
        packet.subchunks_count,
        packet.dimension_id.value,
    )?;
    let payload = packet.serialized_chunk_data;
    Ok((
        LevelChunkEvent {
            dimension: packet.dimension_id.value,
            x: packet.chunk_position.x,
            z: packet.chunk_position.z,
            mode,
            payload: Vec::new(),
        },
        payload,
    ))
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
    const RETAIL_BIOMES: &str = include_str!("../data/retail_biomes_1_26_40.txt");
    let known_retail = RETAIL_BIOMES
        .lines()
        .any(|identifier| identifier.strip_prefix("minecraft:") == Some(name));
    if known_retail {
        Arc::from(format!("minecraft:{name}"))
    } else {
        Arc::from(name)
    }
}
