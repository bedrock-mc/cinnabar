use std::sync::Arc;

use jolyne::GameData;
use thiserror::Error;
use valentine::bedrock::version::v1_26_51::{
    EnumsSubChunkPacketPayloadSubChunkRequestResult as SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult,
    GameRule, GameRuleRuleValue, McpePacketData,
};

use crate::{
    ActorPacketError, InventoryPacketError, ItemPacketError, Packet,
    actor::{
        normalize_add_entity, normalize_add_item_actor, normalize_add_player, normalize_mob_effect,
        normalize_move_entity, normalize_move_entity_delta, normalize_player_list,
        normalize_remove_entity, normalize_set_entity_data, normalize_set_entity_link,
        normalize_update_attributes,
    },
    audio::{
        normalize_level_event_sound, normalize_level_sound, normalize_play_sound,
        normalize_stop_sound,
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
        GameModeEvent, UiEvent, UiPacketError, normalize_available_commands, normalize_block_crack,
        normalize_boss, normalize_display_objective, normalize_form, normalize_health,
        normalize_player_status, normalize_remove_objective, normalize_score, normalize_soft_enum,
        normalize_text, normalize_title, normalize_toast,
    },
};

mod biomes;
mod block_side;
mod block_updates;
mod clocks;
mod custom_blocks;
mod diagnostics;
mod dimension;
mod environment;
mod events;
mod game_mode;
mod game_rules;
mod level_chunk;
mod requests;

pub use self::clocks::{
    OVERWORLD_CLOCK_ID, OVERWORLD_CLOCK_NAME, WorldClockDefinition, WorldClockState,
    WorldClockUpdateEvent,
};
pub use self::custom_blocks::{
    CustomBlock, CustomBlockVisuals, CustomBlocks, CustomBox, CustomHashedState,
    CustomMaterialInstance, CustomPermutation, CustomSelection, CustomStateAxis, CustomStateValue,
    CustomTransformation, CustomVisualComponents, block_name_sort_key, block_state_network_hash,
};
pub use self::diagnostics::{DimensionHeightDiagnostic, HeightmapDiagnostic, SubChunkDiagnostic};
pub use self::environment::WorldEnvironmentBootstrap;
pub use self::events::{
    ActorBlockSyncMessage, ActorMotionEvent, ActorPropertySyncEvent, BiomeDefinitionEvent,
    BiomeDefinitionsEvent, BlockEntityUpdateEvent, BlockEventEvent, BlockUpdateEvent,
    ChangeDimensionEvent, ChunkResyncEvent, DaylightCycleUpdateEvent, DimensionRange,
    GameRulesEvent, LevelChunkEvent, LevelChunkMode, MAP_IMAGE_SIDE, MAX_ACTOR_PROPERTY_SYNC_BYTES,
    MapDataEvent, MovePlayerEvent, MovePlayerMode, MovementCorrectionSubject, NETHER_DIMENSION_ID,
    OpenSignEvent, PLAYER_NETWORK_OFFSET, PlayerMovementCorrectionEvent, PublisherUpdateEvent,
    RespawnEvent, STANDING_PLAYER_EYE_HEIGHT, SetTimeEvent, SubChunkBatchEvent, SubChunkEntryEvent,
    SubChunkReplyAdmissionEvent, SubChunkResult, SubChunkUnavailable, SyncedBlockUpdateEvent,
    WeatherChannel, WeatherUpdateEvent, WorldEvent, air_network_id, vanilla_dimension_range,
};
pub use self::game_mode::PlayerGameMode;
use self::game_rules::{daylight_cycle_rule_update, hud_rules, weather_cycle_rule_update};
use self::level_chunk::level_chunk_mode;
pub(crate) use self::level_chunk::normalize_borrowed_level_chunk;
use self::requests::checked_sub_chunk_position;
pub use self::requests::request_sub_chunk_column;
use biomes::canonical_biome_name;

/// Sequential palette state ID generated for `minecraft:air` in 1.26.30.
pub const SEQUENTIAL_AIR_NETWORK_ID: u32 = 12_530;

/// Canonical block-state network hash for `minecraft:air`.
pub const HASHED_AIR_NETWORK_ID: u32 = 0xdbf4_4120;

/// Client safety limit for block storage layers in update packets.
pub const MAX_BLOCK_LAYERS: usize = 16;

/// Maximum Y offsets emitted in one column SubChunkRequest.
pub const MAX_SUB_CHUNK_REQUESTS: usize = 128;

/// Maximum dimension definitions retained from one server packet or session.
pub const MAX_DIMENSION_DEFINITIONS: usize = 64;

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
/// `LevelEventSleepingPlayers`, sent as a LevelEventGeneric.
const LEVEL_EVENT_SLEEPING_PLAYERS: i32 = 9801;
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

/// The explicit StartGame block-breaking negotiation, separate from whether
/// a caller currently has sufficient authority to mine any particular block.
#[must_use]
pub fn server_authoritative_block_breaking(game_data: &GameData) -> bool {
    game_data
        .start_game
        .movement_settings
        .server_authoritative_block_breaking
}

/// StartGame's `RewindHistorySize`, the retained prediction window in ticks.
#[must_use]
pub fn rewind_history_size(game_data: &GameData) -> i32 {
    game_data.start_game.movement_settings.rewind_history_size
}

/// Whether StartGame declares a hardcore world.
#[must_use]
pub fn is_hardcore(game_data: &GameData) -> bool {
    game_data.start_game.settings.is_hardcore
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

/// Provenance-preserving failures in an otherwise decoded world packet body.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldWireError {
    #[error(transparent)]
    Actor(ActorPacketError),

    #[error(transparent)]
    Item(ItemPacketError),

    #[error(transparent)]
    Inventory(InventoryPacketError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldPacketError {
    #[error("malformed inner world packet wire: {0}")]
    Wire(WorldWireError),

    #[error(transparent)]
    Actor(ActorPacketError),

    #[error(transparent)]
    Ui(#[from] UiPacketError),

    #[error(transparent)]
    Item(ItemPacketError),

    #[error(transparent)]
    Inventory(InventoryPacketError),

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

    #[error("camera {field} is non-finite")]
    NonFiniteCameraField { field: &'static str },

    #[error("camera {field} has {bytes} UTF-8 bytes, exceeding {max}")]
    CameraIdentifierTooLong {
        field: &'static str,
        bytes: usize,
        max: usize,
    },

    #[error("camera spline instructions are recognized but not normalized")]
    UnsupportedCameraSpline,

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

impl From<ActorPacketError> for WorldPacketError {
    fn from(error: ActorPacketError) -> Self {
        if matches!(
            &error,
            ActorPacketError::InvalidAbsoluteMoveRuntimeId
                | ActorPacketError::InvalidAbsoluteMoveLength { .. }
                | ActorPacketError::Item(
                    ItemPacketError::InvalidItemNbt | ItemPacketError::MalformedWire
                )
        ) {
            Self::Wire(WorldWireError::Actor(error))
        } else {
            Self::Actor(error)
        }
    }
}

impl From<ItemPacketError> for WorldPacketError {
    fn from(error: ItemPacketError) -> Self {
        if matches!(
            error,
            ItemPacketError::InvalidItemNbt | ItemPacketError::MalformedWire
        ) {
            Self::Wire(WorldWireError::Item(error))
        } else {
            Self::Item(error)
        }
    }
}

impl From<InventoryPacketError> for WorldPacketError {
    fn from(error: InventoryPacketError) -> Self {
        if matches!(
            error,
            InventoryPacketError::MalformedWire
                | InventoryPacketError::InvalidItemNbt
                | InventoryPacketError::InvalidItemExtra
        ) {
            Self::Wire(WorldWireError::Inventory(error))
        } else {
            Self::Inventory(error)
        }
    }
}

/// Converts a generated packet into the bounded world surface used by the app.
/// Packets unrelated to world streaming return `Ok(None)`.
pub fn into_world_event(
    packet: Packet,
    current_dimension: i32,
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let event = match packet.data {
        McpePacketData::ShowCreditsPacket(packet) => {
            let Some(event) = crate::credits::normalize(&packet) else {
                return Ok(None);
            };
            WorldEvent::Ui(UiEvent::ShowCredits(event))
        }
        McpePacketData::ScriptMessagePacket(message) => {
            if packet.header.from_subclient != 0 || packet.header.to_subclient != 0 {
                return Ok(None);
            }
            return Ok(crate::experience::normalize(message).map(WorldEvent::Experience));
        }
        McpePacketData::UpdateAbilitiesPacket(packet) => {
            WorldEvent::Abilities(crate::permissions::normalize_abilities(packet.data))
        }
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
        McpePacketData::ServerSettingsResponsePacket(packet) => {
            WorldEvent::Ui(crate::ui::normalize_server_settings(packet)?)
        }
        McpePacketData::NpcDialoguePacket(packet) => {
            WorldEvent::Ui(crate::ui::normalize_npc_dialogue(*packet)?)
        }
        McpePacketData::SetHealthPacket(packet) => WorldEvent::Ui(normalize_health(packet)),
        McpePacketData::PlayStatusPacket(packet) => {
            WorldEvent::Ui(normalize_player_status(packet)?)
        }
        McpePacketData::UpdateSoftEnumPacket(packet) => {
            WorldEvent::Ui(normalize_soft_enum(packet)?)
        }
        McpePacketData::AvailableCommandsPacket(packet) => {
            WorldEvent::Ui(normalize_available_commands(*packet))
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
        McpePacketData::PlayerSkinPacket(packet) => {
            WorldEvent::Actor(crate::actor::normalize_skin_update(*packet))
        }
        McpePacketData::AddItemActorPacket(packet) => {
            WorldEvent::Actor(normalize_add_item_actor(*packet, current_dimension)?)
        }
        McpePacketData::TakeItemActorPacket(packet) => {
            WorldEvent::Actor(crate::actor::normalize_take_item_actor(packet))
        }
        McpePacketData::ActorEventPacket(packet) => {
            let Some(event) = crate::actor::normalize_actor_event(*packet) else {
                return Ok(None);
            };
            WorldEvent::Actor(event)
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
        McpePacketData::SyncActorPropertyPacket(packet) => {
            let data = &packet.property_data.0;
            if data.len() > MAX_ACTOR_PROPERTY_SYNC_BYTES {
                return Ok(None);
            }
            WorldEvent::ActorPropertySync(ActorPropertySyncEvent {
                data: Arc::from(&data[..]),
            })
        }
        McpePacketData::SetActorLinkPacket(packet) => {
            WorldEvent::ActorLink(normalize_set_entity_link(*packet, current_dimension))
        }
        McpePacketData::SetPlayerGameTypePacket(packet) => {
            WorldEvent::Ui(UiEvent::GameMode(GameModeEvent {
                update: PlayerGameMode::update_from_game_mode(packet.player_game_type),
            }))
        }
        McpePacketData::UpdatePlayerGameTypePacket(packet) => {
            WorldEvent::Ui(game_mode::targeted_update(packet))
        }
        McpePacketData::SetDefaultGameTypePacket(packet) => {
            WorldEvent::Ui(UiEvent::DefaultGameMode(GameModeEvent {
                update: PlayerGameMode::update_from_default_game_mode(packet.default_game_type),
            }))
        }
        McpePacketData::InventoryContentPacket(packet) => {
            WorldEvent::Inventory(normalize_content(*packet)?)
        }
        McpePacketData::CreativeContentPacket(packet) => {
            let Some(event) = crate::inventory::normalize_creative_content(packet)? else {
                return Ok(None);
            };
            WorldEvent::Inventory(event)
        }
        McpePacketData::InventorySlotPacket(packet) => {
            WorldEvent::Inventory(normalize_slot(*packet)?)
        }
        McpePacketData::InventoryTransactionPacket(packet) => {
            let Some(event) = crate::inventory::normalize_transaction(*packet) else {
                return Ok(None);
            };
            WorldEvent::Inventory(event)
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
        McpePacketData::PlayerEnchantOptionsPacket(packet) => {
            WorldEvent::Inventory(crate::inventory::normalize_enchant_options(packet)?)
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
        McpePacketData::CameraPacket(packet) => {
            WorldEvent::Camera(crate::camera::normalize_switch(packet))
        }
        McpePacketData::CameraPresetsPacket(packet) => {
            WorldEvent::Camera(crate::camera::normalize_presets(packet))
        }
        McpePacketData::CameraShakePacket(packet) => {
            WorldEvent::Camera(crate::camera::normalize_shake(*packet)?)
        }
        McpePacketData::CameraInstructionPacket(packet) => WorldEvent::Camera(
            crate::camera::normalize_instruction(packet.camera_instruction)?,
        ),
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
                // Climate is optional on the wire. Ignore an unusable optional
                // field instead of rejecting a well-framed biome definition.
                let max_snow_accumulation = definition
                    .chunkgendata
                    .as_ref()
                    .and_then(|generation| generation.climate.as_ref())
                    .map(|climate| climate.snowaccumulationmax)
                    .filter(|value| value.is_finite());
                definitions.push(BiomeDefinitionEvent {
                    biome_id: (definition.id != u16::MAX).then_some(definition.id),
                    name,
                    temperature: definition.temperature,
                    downfall: definition.downfall,
                    snow_foliage: definition.foliagesnow,
                    max_snow_accumulation,
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
                let diagnostics = Some(SubChunkDiagnostic::from_entry(&entry));
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
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Successallair => {
                        SubChunkResult::AllAir
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Unknown(0) => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::Undefined)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Levelchunkdoesntexist => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Wrongdimension => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Playerdoesntexist => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::PlayerNotFound)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Indexoutofbounds => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds)
                    }
                    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Unknown(value) => {
                        SubChunkResult::Unavailable(SubChunkUnavailable::Unknown(value))
                    }
                };
                normalized.push(SubChunkEntryEvent {
                    position,
                    result,
                    diagnostics,
                });
            }
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: packet.dimension_type.value,
                entries: normalized,
            })
        }
        McpePacketData::DimensionDataPacket(packet) => WorldEvent::DimensionHeights(
            packet
                .definitions
                .into_iter()
                .take(MAX_DIMENSION_DEFINITIONS)
                .map(|entry| DimensionHeightDiagnostic {
                    name: Arc::from(entry.key),
                    dimension: entry.value.dimension_type.value,
                    minimum_y: entry.value.minimum_y,
                    height_range: entry.value.height_range,
                    generator: diagnostics::dimension_generator_id(entry.value.generator_type),
                })
                .collect(),
        ),
        packet @ (McpePacketData::UpdateBlockPacket(_)
        | McpePacketData::UpdateBlockSyncedPacket(_)
        | McpePacketData::UpdateSubChunkBlocksPacket(_)) => {
            return block_updates::normalize(packet, current_dimension);
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
        McpePacketData::ClientboundMapItemDataPacket(packet) => {
            let Some(event) = block_side::normalize_map_data(&packet) else {
                return Ok(None);
            };
            WorldEvent::MapData(event)
        }
        McpePacketData::OpenSignPacket(packet) => {
            WorldEvent::OpenSign(block_side::normalize_open_sign(&packet, current_dimension))
        }
        McpePacketData::BlockEventPacket(packet) => WorldEvent::BlockEvent(
            block_side::normalize_block_event(&packet, current_dimension),
        ),
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
            WorldEvent::ChangeDimension(dimension::normalize_change_dimension(&packet))
        }
        McpePacketData::PlayerActionPacket(action) => {
            return Ok(crate::dimension::normalize_ack(
                &action,
                packet.header.from_subclient,
                packet.header.to_subclient,
            ));
        }
        McpePacketData::RespawnPacket(packet) => {
            WorldEvent::Respawn(dimension::normalize_respawn(&packet))
        }
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
        McpePacketData::NetworkStackLatencyPacket(packet) => {
            if !packet.is_from_server {
                return Ok(None);
            }
            WorldEvent::NetworkStackLatency(packet.creation_time)
        }
        McpePacketData::SetActorMotionPacket(packet) => {
            let motion = [packet.motion.x, packet.motion.y, packet.motion.z];
            if motion.iter().any(|value| !value.is_finite()) {
                // A well-formed impulse with non-finite components cannot
                // enter prediction; skip the whole packet instead of guessing
                // a clamp. Truncation and other wire failures stay fatal.
                return Ok(None);
            }
            WorldEvent::ActorMotion(ActorMotionEvent {
                actor_runtime_id: packet.target_runtime_id.actor_runtime_id,
                motion,
                tick: packet.tick.inputtick,
            })
        }
        McpePacketData::CorrectPlayerMovePredictionPacket(packet) => {
            let delta = [packet.pos_delta.x, packet.pos_delta.y, packet.pos_delta.z];
            // A well-formed correction whose velocity record or rotation is not
            // finite cannot enter prediction or camera state; skip the whole
            // packet instead of guessing a clamp, exactly like SetActorMotion.
            // The raw position is exempt: non-finite positions keep flowing so
            // downstream resolution applies its documented sentinel recovery.
            // Protocol 2168 carries no shape/mode field; prediction_type names
            // the rewind subject and every subject is retained here.
            if delta.iter().any(|value| !value.is_finite())
                || !packet.rotation.x.is_finite()
                || !packet.rotation.y.is_finite()
            {
                return Ok(None);
            }
            WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
                position: [packet.pos.x, packet.pos.y, packet.pos.z],
                delta,
                // Vec2's components are (x, y) in 1.26.40; gophertunnel
                // packet/correct_player_move_prediction.go writes Rotation as
                // one Vec2 of (pitch, yaw).
                pitch: packet.rotation.x,
                yaw: packet.rotation.y,
                subject: MovementCorrectionSubject::from(packet.prediction_type),
                on_ground: packet.on_ground,
                tick: packet.tick.inputtick,
            })
        }
        McpePacketData::SetTimePacket(packet) => {
            WorldEvent::SetTime(SetTimeEvent { time: packet.time })
        }
        McpePacketData::SyncWorldClocksPacket(packet) => {
            let updates = clocks::normalize_world_clocks(packet.data);
            if updates.is_empty() {
                return Ok(None);
            }
            WorldEvent::WorldClocks(updates)
        }
        McpePacketData::GameRulesChangedPacket(packet) => {
            let rules = &packet.rule_data.rules_list;
            let daylight_cycle = daylight_cycle_rule_update(rules)
                .map(|enabled| DaylightCycleUpdateEvent { enabled });
            let weather_cycle = weather_cycle_rule_update(rules);
            let hud = hud_rules(rules);
            if daylight_cycle.is_none() && weather_cycle.is_none() && hud.is_empty() {
                return Ok(None);
            }
            WorldEvent::GameRules(GameRulesEvent {
                daylight_cycle,
                weather_cycle,
                hud,
            })
        }
        McpePacketData::LevelEventGenericPacket(packet) => {
            if packet.event_id != LEVEL_EVENT_SLEEPING_PLAYERS {
                return Ok(None);
            }
            // The event data is loose tags; wrap them in a root compound as vanilla decodes it.
            let mut nbt = Vec::with_capacity(packet.__ctd__.0.len() + 3);
            nbt.extend_from_slice(&[0x0a, 0x00]);
            nbt.extend_from_slice(&packet.__ctd__.0);
            nbt.push(0x00);
            WorldEvent::Ui(UiEvent::SleepStatus(crate::SleepStatusEvent {
                nbt: Arc::from(nbt),
            }))
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
            if let Some(event) = normalize_level_event_sound(&packet) {
                return Ok(Some(WorldEvent::Audio(event)));
            }
            if let Some(event) = crate::particle::normalize_level_event(&packet) {
                return Ok(Some(WorldEvent::Particle(event)));
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
        McpePacketData::PrimitiveShapesPacket(packet) => {
            WorldEvent::PrimitiveShapes(crate::primitive_shapes::normalize(packet))
        }
        McpePacketData::SpawnParticleEffectPacket(packet) => {
            match crate::particle::normalize_spawn(*packet) {
                Some(event) => WorldEvent::Particle(event),
                None => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(event))
}
