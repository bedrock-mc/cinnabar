use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::{self, PacketId, StartGame};
use thiserror::Error;

use crate::{
    ActorPacketError, InventoryPacketError, ItemActorEvent, ItemPacketError, Packet,
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
        normalize_actor_attack, normalize_animate, normalize_animate_entity, normalize_equipment,
        normalize_item_registry,
    },
    packet::GameHeader,
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
mod motion;
mod requests;

pub use self::clocks::{
    OVERWORLD_CLOCK_ID, OVERWORLD_CLOCK_NAME, WorldClockDefinition, WorldClockState,
    WorldClockUpdateEvent,
};
pub use self::custom_blocks::{
    CustomBlock, CustomBlockPhysics, CustomBlockVisuals, CustomBlocks, CustomBox, CustomCollision,
    CustomHashedState, CustomMaterialInstance, CustomPermutation, CustomPhysicalComponents,
    CustomPlacement, CustomSelection, CustomStateAxis, CustomStateValue, CustomTransformation,
    CustomVisualComponents, block_name_sort_key, block_state_network_hash,
};
pub use self::diagnostics::{DimensionHeightDiagnostic, HeightmapDiagnostic, SubChunkDiagnostic};
pub use self::environment::WorldEnvironmentBootstrap;
pub use self::events::{
    ActorBlockSyncMessage, ActorMotionEvent, ActorPropertySyncEvent, BiomeDefinitionEvent,
    BiomeDefinitionsEvent, BlockEntityUpdateEvent, BlockEventEvent, BlockUpdateEvent,
    ChangeDimensionEvent, ChunkResyncEvent, DaylightCycleUpdateEvent, DimensionRange,
    GameRulesEvent, LevelChunkEvent, LevelChunkMode, MAP_IMAGE_SIDE, MAX_ACTOR_PROPERTY_SYNC_BYTES,
    MapDataEvent, MovePlayerEvent, MovePlayerMode, MovementCorrectionSubject, MovementEffectEvent,
    MovementEffectKind, NETHER_DIMENSION_ID, OpenSignEvent, PLAYER_NETWORK_OFFSET,
    PlayerMovementCorrectionEvent, PublisherUpdateEvent, RespawnEvent, STANDING_PLAYER_EYE_HEIGHT,
    SetTimeEvent, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkReplyAdmissionEvent,
    SubChunkResult, SubChunkUnavailable, SyncedBlockUpdateEvent, WeatherChannel,
    WeatherUpdateEvent, WorldEvent, WorldSpawnEvent, air_network_id, vanilla_dimension_range,
};
pub use self::game_mode::PlayerGameMode;
use self::game_rules::{
    crafting_rules, daylight_cycle_rule_update, death_rules, hud_rules, weather_cycle_rule_update,
};
pub(crate) use self::level_chunk::normalize_level_chunk_payload;
pub use self::requests::{request_sub_chunk_column, request_sub_chunks};

use protocol_ids::LevelEvent;

/// Client safety limit for block storage layers in update packets.
pub const MAX_BLOCK_LAYERS: usize = 16;

/// Maximum dimension definitions retained from one server packet or session.
pub const MAX_DIMENSION_DEFINITIONS: usize = 64;

/// Maximum live biome definitions retained from one server packet.
///
/// The generated decoder bounds the list only by its input, so this is the only element bound.
pub const MAX_BIOME_DEFINITIONS: usize = 4_096;

/// Maximum UTF-8 bytes accepted for one live biome identifier.
pub const MAX_BIOME_NAME_BYTES: usize = 256;

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
    pub(crate) fn from_start_game(start_game: &StartGame) -> Self {
        let settings = &start_game.settings;
        let spawn = &settings.default_spawn_block_position;
        Self {
            // gophertunnel packet/start_game.go writes `Dimension int32` (Varint32) right after
            // `UserDefinedBiomeName`, which is `settings.spawn_settings.dimension` here.
            dimension: settings.spawn_settings.dimension.0,
            local_player_runtime_id: start_game.runtime_id.0,
            local_player_unique_id: start_game.entity_id.0,
            player_position: start_game.position.to_array(),
            world_spawn_position: [spawn.x.0, spawn.y.0, spawn.z.0],
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

    #[error("camera {field} is unusable")]
    InvalidCameraField { field: &'static str },

    #[error("camera {field} has {count} entries, exceeding {max}")]
    CameraCollectionTooLarge {
        field: &'static str,
        count: usize,
        max: usize,
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

    #[error("SubChunkRequest position {position:?} is outside the offsets from {center:?}")]
    SubChunkRequestOutOfRange {
        center: [i32; 3],
        position: [i32; 3],
    },
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

/// Defines world routing and dispatch from one table, so play decodes exactly the packets that
/// normalization consumes.
macro_rules! world_packets {
    ($header:ident, $dimension:ident; $($variant:ident($packet:pat) => $handler:expr,)+) => {
        /// Whether play decodes `id` for world normalization; other packets stay undecoded.
        pub(crate) fn routes_world_packet(id: PacketId) -> bool {
            matches!(id, $(PacketId::$variant)|+)
        }

        /// Normalizes a decoded payload and its header as [`into_world_event`] does, for
        /// callers that hold the payload unboxed.
        pub(crate) fn dispatch(
            data: packets::Packet,
            $header: GameHeader,
            $dimension: i32,
        ) -> Result<Option<WorldEvent>, WorldPacketError> {
            let event = match data {
                $(packets::Packet::$variant($packet) => $handler,)+
                _ => return Ok(None),
            };
            Ok(Some(event))
        }
    };
}

/// Converts a generated packet into the bounded world surface used by the app.
/// Packets unrelated to world streaming return `Ok(None)`.
pub fn into_world_event(
    packet: Packet,
    current_dimension: i32,
) -> Result<Option<WorldEvent>, WorldPacketError> {
    dispatch(*packet.data, packet.header, current_dimension)
}

world_packets! {
    header, current_dimension;
    UpdateClientInputLocks(packet) => WorldEvent::Camera(crate::CameraEvent::InputLocks(
        crate::camera::ClientInputLocks::from_bits(packet.input_lock_component_data.0),
    )),
    ShowCredits(packet) => {
        let Some(event) = crate::credits::normalize(&packet) else {
            return Ok(None);
        };
        WorldEvent::Ui(UiEvent::ShowCredits(event))
    },
    ScriptMessage(message) => {
        if header.from_subclient != 0 || header.to_subclient != 0 {
            return Ok(None);
        }
        return Ok(crate::experience::normalize(message).map(WorldEvent::Experience));
    },
    UpdateAbilities(packet) => {
        WorldEvent::Abilities(crate::permissions::normalize_abilities(packet.data))
    },
    Text(packet) => WorldEvent::Ui(normalize_text(packet)?),
    DeathInfo(packet) => WorldEvent::Ui(crate::ui::normalize_death_info(packet)?),
    CommandOutput(packet) => WorldEvent::Ui(crate::ui::normalize_command_output(packet)?),
    SetTitle(packet) => WorldEvent::Ui(normalize_title(*packet)?),
    ToastRequest(packet) => WorldEvent::Ui(normalize_toast(packet)?),
    SetDisplayObjective(packet) => WorldEvent::Ui(normalize_display_objective(packet)?),
    RemoveObjective(packet) => WorldEvent::Ui(normalize_remove_objective(packet)?),
    SetScore(packet) => WorldEvent::Ui(normalize_score(packet)?),
    BossEvent(packet) => WorldEvent::Ui(normalize_boss(packet)?),
    ModalFormRequest(packet) => WorldEvent::Ui(normalize_form(packet)?),
    ServerSettingsResponse(packet) => {
        WorldEvent::Ui(crate::ui::normalize_server_settings(packet)?)
    },
    NpcDialogue(packet) => WorldEvent::Ui(crate::ui::normalize_npc_dialogue(packet)?),
    SetHealth(packet) => WorldEvent::Ui(normalize_health(packet)),
    PlayStatus(packet) => WorldEvent::Ui(normalize_player_status(packet)?),
    UpdateSoftEnum(packet) => WorldEvent::Ui(normalize_soft_enum(packet)?),
    AvailableCommands(packet) => WorldEvent::Ui(normalize_available_commands(*packet)),
    AddActor(packet) => WorldEvent::Actor(normalize_add_entity(*packet, current_dimension)?),
    AddPainting(packet) => {
        WorldEvent::Actor(crate::actor::normalize_add_painting(packet, current_dimension)?)
    },
    AddPlayer(packet) => WorldEvent::Actor(normalize_add_player(*packet, current_dimension)?),
    RemoveActor(packet) => WorldEvent::Actor(normalize_remove_entity(packet, current_dimension)),
    MoveActorAbsolute(packet) => {
        WorldEvent::Actor(normalize_move_entity(packet, current_dimension)?)
    },
    MoveActorDelta(packet) => {
        WorldEvent::Actor(normalize_move_entity_delta(packet, current_dimension)?)
    },
    SetActorData(packet) => {
        WorldEvent::Actor(normalize_set_entity_data(packet, current_dimension)?)
    },
    UpdateAttributes(packet) => {
        WorldEvent::Actor(normalize_update_attributes(packet, current_dimension)?)
    },
    PlayerList(packet) => WorldEvent::Actor(normalize_player_list(packet)?),
    PlayerSkin(packet) => WorldEvent::Actor(crate::actor::normalize_skin_update(packet)),
    AddItemActor(packet) => {
        WorldEvent::Actor(normalize_add_item_actor(packet, current_dimension)?)
    },
    AvailableActorIdentifiers(packet) => WorldEvent::Actor(crate::ActorEvent::Identifiers(
        crate::actor::identifiers::normalize(&packet.identifier_list.0),
    )),
    TakeItemActor(packet) => WorldEvent::Actor(crate::actor::normalize_take_item_actor(packet)),
    ActorEvent(packet) => {
        if packet.event_id == bedrock_protocol_1_26_51::enums::ActorEventType::StartAttacking {
            WorldEvent::ItemActor(normalize_actor_attack(packet.target_runtime_id)?)
        } else {
            let Some(event) = crate::actor::normalize_actor_event(packet) else {
                return Ok(None);
            };
            WorldEvent::Actor(event)
        }
    },
    ItemRegistry(packet) => {
        WorldEvent::ItemActor(ItemActorEvent::Registry(normalize_item_registry(packet)?))
    },
    MobEquipment(packet) => WorldEvent::Equipment(normalize_equipment(packet)?),
    MobArmorEquipment(packet) => {
        WorldEvent::ArmorEquipment(Box::new(normalize_armor_equipment(packet)?))
    },
    MobEffect(packet) => WorldEvent::ActorEffect(normalize_mob_effect(*packet, current_dimension)?),
    SyncActorProperty(packet) => {
        let data = &packet.property_data.0;
        if data.len() > MAX_ACTOR_PROPERTY_SYNC_BYTES {
            return Ok(None);
        }
        WorldEvent::ActorPropertySync(ActorPropertySyncEvent {
            data: Arc::from(&data[..]),
        })
    },
    SetActorLink(packet) => {
        WorldEvent::ActorLink(normalize_set_entity_link(packet, current_dimension))
    },
    SetPlayerGameType(packet) => WorldEvent::Ui(UiEvent::GameMode(GameModeEvent {
        update: PlayerGameMode::update_from_game_mode(packet.player_game_type),
    })),
    UpdatePlayerGameType(packet) => WorldEvent::Ui(game_mode::targeted_update(packet)),
    SetDefaultGameType(packet) => WorldEvent::Ui(UiEvent::DefaultGameMode(GameModeEvent {
        update: PlayerGameMode::update_from_game_mode(packet.default_game_type),
    })),
    InventoryContent(packet) => WorldEvent::Inventory(normalize_content(packet)?),
    CreativeContent(packet) => {
        let Some(event) = crate::inventory::normalize_creative_content(packet)? else {
            return Ok(None);
        };
        WorldEvent::Inventory(event)
    },
    InventorySlot(packet) => WorldEvent::Inventory(normalize_slot(packet)?),
    InventoryTransaction(packet) => {
        let Some(event) = crate::inventory::normalize_transaction(packet) else {
            return Ok(None);
        };
        WorldEvent::Inventory(event)
    },
    PlayerHotbar(packet) => WorldEvent::Inventory(normalize_hotbar(packet)?),
    ItemStackResponse(packet) => WorldEvent::Inventory(normalize_response(packet)?),
    ContainerOpen(packet) => WorldEvent::Inventory(normalize_container_open(packet)?),
    ContainerClose(packet) => WorldEvent::Inventory(normalize_container_close(packet)?),
    ContainerRegistryCleanup(packet) => {
        WorldEvent::Inventory(crate::inventory::normalize_container_registry_cleanup(packet)?)
    },
    ContainerSetData(packet) => WorldEvent::Inventory(normalize_container_data(packet)?),
    UnlockedRecipes(packet) => WorldEvent::Inventory(crate::InventoryEvent::RecipeUnlocks(
        crate::inventory::recipes::normalize_recipe_unlocks(packet)
    )),
    SetPlayerInventoryOptions(packet) => {
        WorldEvent::Inventory(crate::inventory::normalize_inventory_options(packet))
    },
    SetPlayerFurnaceOptions(packet) => {
        let Some(event) = crate::inventory::normalize_furnace_options(packet) else {
            return Ok(None);
        };
        WorldEvent::Inventory(event)
    },
    PlayerEnchantOptions(packet) => {
        WorldEvent::Inventory(crate::inventory::normalize_enchant_options(packet)?)
    },
    Animate(packet) => WorldEvent::ItemActor(normalize_animate(packet)?),
    AnimateEntity(packet) => WorldEvent::ItemActor(normalize_animate_entity(packet)?),
    PlaySound(packet) => WorldEvent::Audio(normalize_play_sound(*packet)?),
    StopSound(packet) => WorldEvent::Audio(normalize_stop_sound(packet)?),
    LevelSoundEvent(packet) => WorldEvent::Audio(normalize_level_sound(*packet)?),
    Camera(packet) => WorldEvent::Camera(crate::camera::normalize_switch(packet)),
    CameraPresets(packet) => WorldEvent::Camera(crate::camera::normalize_presets(packet)?),
    CameraSpline(packet) => WorldEvent::Camera(crate::camera::normalize_registry(packet)?),
    CameraAimAssist(packet) => WorldEvent::Camera(crate::camera::normalize_settings(packet)?),
    CameraAimAssistPresets(packet) => {
        WorldEvent::Camera(crate::camera::normalize_aim_presets(packet)?)
    },
    CameraAimAssistActorPriority(packet) => {
        WorldEvent::Camera(crate::camera::normalize_actor_priorities(packet)?)
    },
    CameraShake(packet) => WorldEvent::Camera(crate::camera::normalize_shake(packet)?),
    CameraInstruction(packet) => {
        WorldEvent::Camera(crate::camera::normalize_instruction(packet.camera_instruction)?)
    },
    BiomeDefinitionList(packet) => {
        WorldEvent::BiomeDefinitions(biomes::normalize_biome_definitions(packet)?)
    },
    LevelChunk(packet) => WorldEvent::LevelChunk(level_chunk::normalize_level_chunk(packet)?),
    SubChunk(packet) => WorldEvent::SubChunks(level_chunk::normalize_sub_chunks(packet)?),
    DimensionData(packet) => {
        WorldEvent::DimensionHeights(diagnostics::normalize_dimension_data(packet))
    },
    UpdateBlock(packet) => block_updates::normalize_update(packet, current_dimension)?,
    UpdateBlockSynced(packet) => {
        let Some(event) = block_updates::normalize_synced(packet, current_dimension) else {
            return Ok(None);
        };
        event
    },
    UpdateSubChunkBlocks(packet) => block_updates::normalize_sub_chunk(packet, current_dimension),
    BlockActorData(packet) => WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
        dimension: current_dimension,
        position: [
            packet.block_position.x.0,
            packet.block_position.y.0,
            packet.block_position.z.0,
        ],
        nbt: packet.actor_data_tags.0.to_vec(),
    }),
    ClientboundMapItemData(packet) => {
        let Some(event) = block_side::normalize_map_data(&packet) else {
            return Ok(None);
        };
        WorldEvent::MapData(event)
    },
    OpenSign(packet) => {
        WorldEvent::OpenSign(block_side::normalize_open_sign(&packet, current_dimension))
    },
    BlockEvent(packet) => {
        WorldEvent::BlockEvent(block_side::normalize_block_event(&packet, current_dimension))
    },
    ChunkRadiusUpdated(packet) => {
        if packet.chunk_radius.0 > 0 {
            bridge::join_profile::JoinProfile::current()
                .mark_once(bridge::join_profile::Milestone::ChunkRadiusReply);
        }
        WorldEvent::ChunkRadiusUpdated(packet.chunk_radius.0)
    },
    NetworkChunkPublisherUpdate(packet) => {
        let center = &packet.new_position_for_view;
        WorldEvent::PublisherUpdate(PublisherUpdateEvent {
            center: [center.x.0, center.y.0, center.z.0],
            radius_blocks: packet.new_radius_for_view.0,
        })
    },
    ChangeDimension(packet) => {
        WorldEvent::ChangeDimension(dimension::normalize_change_dimension(&packet))
    },
    PlayerAction(action) => {
        return Ok(crate::dimension::normalize_ack(
            &action,
            header.from_subclient,
            header.to_subclient,
        ));
    },
    Respawn(packet) => WorldEvent::Respawn(dimension::normalize_respawn(&packet)),
    MovePlayer(packet) => WorldEvent::MovePlayer(motion::normalize_move_player(&packet)?),
    NetworkStackLatency(packet) => {
        if !packet.is_from_server {
            return Ok(None);
        }
        WorldEvent::NetworkStackLatency(packet.creation_time.0)
    },
    SetActorMotion(packet) => {
        let Some(event) = motion::normalize_actor_motion(&packet) else {
            return Ok(None);
        };
        WorldEvent::ActorMotion(event)
    },
    MovementEffect(packet) => WorldEvent::MovementEffect(motion::normalize_movement_effect(&packet)),
    CorrectPlayerMovePrediction(packet) => {
        let Some(event) = motion::normalize_correction(&packet) else {
            return Ok(None);
        };
        WorldEvent::PlayerMovementCorrection(event)
    },
    SetTime(packet) => WorldEvent::SetTime(SetTimeEvent { time: packet.time.0 }),
    SetSpawnPosition(packet) => {
        // Player respawn points are not carried; compasses only read the world spawn.
        if packet.spawn_position_type != bedrock_protocol_1_26_51::enums::SpawnPositionType::Worldspawn {
            return Ok(None);
        }
        WorldEvent::WorldSpawn(WorldSpawnEvent {
            position: [
                packet.block_position.x.0,
                packet.block_position.y.0,
                packet.block_position.z.0,
            ],
        })
    },
    SyncWorldClocks(packet) => {
        let updates = clocks::normalize_world_clocks(packet.data);
        if updates.is_empty() {
            return Ok(None);
        }
        WorldEvent::WorldClocks(updates)
    },
    GameRulesChanged(packet) => {
        let rules = &packet.rule_data.rules_list;
        let daylight_cycle =
            daylight_cycle_rule_update(rules).map(|enabled| DaylightCycleUpdateEvent { enabled });
        let weather_cycle = weather_cycle_rule_update(rules);
        let hud = hud_rules(rules);
        let death = death_rules(rules);
        let crafting = crafting_rules(rules);
        if daylight_cycle.is_none()
            && weather_cycle.is_none()
            && hud.is_empty()
            && death.is_empty()
            && crafting.is_empty()
        {
            return Ok(None);
        }
        WorldEvent::GameRules(GameRulesEvent {
            daylight_cycle,
            weather_cycle,
            hud,
            death,
            crafting,
        })
    },
    LevelEventGeneric(packet) => {
        if packet.event_id.0 != LevelEvent::SleepingPlayers.wire_value() {
            return Ok(None);
        }
        // The event data is loose tags; wrap them in a root compound as vanilla decodes it.
        let tags = &packet.ctd.0;
        let mut nbt = Vec::with_capacity(tags.len() + 3);
        nbt.extend_from_slice(&[0x0a, 0x00]);
        nbt.extend_from_slice(tags);
        nbt.push(0x00);
        WorldEvent::Ui(UiEvent::SleepStatus(crate::SleepStatusEvent {
            nbt: Arc::from(nbt),
        }))
    },
    LevelEvent(packet) => {
        let Some(event) = normalize_level_event(packet)? else {
            return Ok(None);
        };
        event
    },
    PrimitiveShapes(packet) => {
        WorldEvent::PrimitiveShapes(crate::primitive_shapes::normalize(packet))
    },
    SpawnParticleEffect(packet) => match crate::particle::normalize_spawn(packet) {
        Some(event) => WorldEvent::Particle(event),
        None => return Ok(None),
    },
}

/// Block cracking, level sounds, particles and weather share the LevelEvent packet.
fn normalize_level_event(
    packet: packets::LevelEvent,
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let event_id = packet.event_id.0;
    if matches!(
        LevelEvent::from_wire(event_id),
        Some(
            LevelEvent::StartBlockCracking
                | LevelEvent::StopBlockCracking
                | LevelEvent::UpdateBlockCracking
        )
    ) {
        return Ok(Some(WorldEvent::BlockCrack(normalize_block_crack(packet)?)));
    }
    if let Some(event) = normalize_level_event_sound(&packet) {
        return Ok(Some(WorldEvent::Audio(event)));
    }
    if let Some(event) = crate::particle::normalize_level_event(&packet) {
        return Ok(Some(WorldEvent::Particle(event)));
    }
    let (channel, level) = match LevelEvent::from_wire(event_id) {
        Some(LevelEvent::StartRaining) => (WeatherChannel::Rain, 1.0),
        Some(LevelEvent::StopRaining) => (WeatherChannel::Rain, 0.0),
        Some(LevelEvent::StartThunderstorm) => (WeatherChannel::Lightning, 1.0),
        Some(LevelEvent::StopThunderstorm) => (WeatherChannel::Lightning, 0.0),
        _ => return Ok(None),
    };
    Ok(Some(WorldEvent::Weather(WeatherUpdateEvent {
        channel,
        level,
    })))
}
