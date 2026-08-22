//! Bedrock 1.26.44 (protocol 2168) packet definitions and codec.

mod actor;
mod audio;
mod blob_cache;
mod codec;
mod interaction;
mod inventory;
mod item;
mod login;
mod movement;
mod packet;
mod raw_text;
mod socket_transport;
mod ui;
mod world;

pub use actor::{
    ActorAttribute, ActorAttributeModifier, ActorAttributesUpdateEvent, ActorEffectAction,
    ActorEffectEvent, ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorMetadata,
    ActorMetadataUpdateEvent, ActorMetadataValue, ActorMoveEvent, ActorPacketError,
    ActorPositionOrigin, ActorProperty, ActorRemoveEvent, ActorSpawnEvent,
    MAX_ACTOR_ATTRIBUTE_MODIFIERS, MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_IDENTIFIER_BYTES,
    MAX_ACTOR_LINKS_PER_SPAWN, MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_METADATA_NBT_BYTES,
    MAX_ACTOR_METADATA_STRING_BYTES, MAX_ACTOR_NAME_BYTES, MAX_ACTOR_PROPERTIES,
    MAX_PLAYER_LIST_RECORDS, MAX_PLAYER_LIST_SKIN_BYTES, MAX_STANDARD_SKIN_SIDE, PlayerListEntry,
    PlayerListUpdateEvent, PlayerSkin, PlayerSkinUnavailable, StandardSkin,
};
pub use audio::{
    AudioEvent, LevelAudioEvent, MAX_AUDIO_IDENTIFIER_BYTES, PlayAudioEvent, StopAudioEvent,
};
pub use blob_cache::{
    BlobCacheError, BlobCacheLimits, BlobCacheReady, BlobCacheResolver, BlobCacheStats,
    BlobCacheStatus, CLIENT_BLOB_CACHE_TRIM_FLOOR_BYTES, CLIENT_BLOB_CACHE_TRIM_TRIGGER_BYTES,
    ClientBlobCache, MAX_CLIENT_BLOB_HASHES_PER_PACKET, MAX_CLIENT_BLOB_ORDINARY_READY_BYTES,
    MAX_CLIENT_BLOB_ORDINARY_READY_EVENTS, MAX_CLIENT_BLOB_PENDING_BYTES,
    MAX_CLIENT_BLOB_PENDING_TRANSACTIONS, MAX_CLIENT_BLOB_READY_BYTES,
    MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES, MAX_CLIENT_BLOB_RECOVERY_READY_EVENTS,
    MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION, client_blob_hash,
};
pub use codec::{ProtocolError, decode_batch, encode};
pub use interaction::{
    ActorUseAction, ActorUsePacketError, ActorUseRequest, BlockUsePacketError, BlockUseRequest,
    click_block_packet, destroy_block_packet, use_actor_packet,
};
pub use inventory::{
    ContainerCloseEvent, ContainerDataEvent, ContainerIdentity, ContainerOpenEvent,
    InventoryAuthority, InventoryContentEvent, InventoryEvent, InventoryPacketError,
    InventorySlotEvent, ItemStackResponseEvent, MAX_CONTAINER_SLOTS, MAX_ITEM_NBT_BYTES,
    MAX_RESPONSE_CONTAINERS, MAX_RESPONSE_NAME_BYTES, MAX_STACK_RESPONSES, PLAYER_INVENTORY_SLOTS,
    SelectedSlotEvent, SlotIdentity, StackRequestAction, StackRequestContainer, StackRequestSlot,
    StackResponse, StackResponseContainer, StackResponseSlot, StackResponseStatus,
    VerifiedNetworkItemStack, container_close_packet, item_stack_request_packet,
    normalize_authority, normalize_container_close, normalize_container_data,
    normalize_container_open, normalize_content, normalize_hotbar, normalize_response,
    normalize_slot, validate_item_nbt_size,
};
pub use item::{
    ActorActionEvent, ActorActionKind, ActorHandedness, ArmorEquipmentEvent, EquipmentEvent,
    HOTBAR_SLOT_COUNT, ItemActorEvent, ItemPacketError, ItemRegistryEntry, ItemRegistryEvent,
    ItemRegistryVersion, MAX_ACTION_IDENTIFIER_BYTES, MAX_ANIMATE_ENTITY_IDS,
    MAX_ANIMATION_IDENTIFIER_BYTES, MAX_ITEM_EXTRA_BYTES, MAX_ITEM_REGISTRY_ENTRIES,
    NetworkItemStack, item_stack_damage, select_hotbar_slot_packet, vanilla_item_registry,
};
pub use jolyne::GameData;
pub use jolyne::stream::{ResourcePackArchive, ResourcePackContentKey, ResourcePackHandoff};
pub use login::{LoginSequence, PacketIdTraceSnapshot, PlaySession};
pub use movement::{
    PlayerAuthInputError, PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode,
    player_auth_input,
};
pub use packet::Packet;
pub use raw_text::{
    MAX_RAW_TEXT_COMPONENTS, MAX_RAW_TEXT_DEPTH, MAX_RAW_TEXT_INPUT_BYTES, MAX_RAW_TEXT_NODES,
    MAX_RAW_TEXT_OUTPUT_BYTES, RawTextComponent, RawTextDocument, RawTextResolution,
    RawTextResolver, ResolvedRawText, parse_raw_text,
};
pub use socket_transport::{SocketTransport, bridge_endpoint_path};
pub use ui::{
    BlockCrackAction, BlockCrackEvent, BossAction, BossColor, BossEvent, BossOverlay, BossStyle,
    ChatAutocompleteAction, ChatAutocompleteCatalog, ChatAutocompleteCatalogError,
    ChatAutocompleteCompletion, ChatAutocompleteEvent, ChatPacketError, CommandOutputEvent,
    CommandOutputMessage, FormRequestEvent, GameModeEvent, GameModeUpdate, HudEvent,
    MAX_BOSS_EVENTS, MAX_CHAT_AUTOCOMPLETE, MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_CHAT_PARAMETERS,
    MAX_COMMAND_OUTPUT_MESSAGES, MAX_FORM_JSON_BYTES, MAX_OUTBOUND_CHAT_BYTES,
    MAX_SCORE_ENTRIES_PER_PACKET, MAX_UI_TEXT_BYTES, ObjectiveEvent, PlayerStatus, RawTextEvent,
    ScoreAction, ScoreEntry, ScoreEvent, ScoreIdentity, TextCategory, TextEvent, TextKind,
    TitleAction, TitleEvent, UiEvent, UiPacketError, chat_input_packet, chat_text_packet,
};
pub use valentine::bedrock::context::BedrockSession;
pub use valentine::bedrock::version::v1_26_44::{GAME_VERSION, PROTOCOL_VERSION};
pub use world::{
    ActorMotionEvent, BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockEntityUpdateEvent,
    BlockUpdateEvent, ChangeDimensionEvent, ChunkResyncEvent, DaylightCycleUpdateEvent,
    DimensionRange, HASHED_AIR_NETWORK_ID, LevelChunkEvent, LevelChunkMode, MAX_BIOME_DEFINITIONS,
    MAX_BIOME_NAME_BYTES, MAX_BLOCK_LAYERS, MAX_SUB_CHUNK_REQUESTS, MovePlayerEvent,
    MovePlayerMode, PLAYER_NETWORK_OFFSET, PlayerGameMode, PlayerMovementCorrectionEvent,
    PublisherUpdateEvent, RespawnEvent, SEQUENTIAL_AIR_NETWORK_ID, STANDING_PLAYER_EYE_HEIGHT,
    SetTimeEvent, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkReplyAdmissionEvent,
    SubChunkResult, SubChunkUnavailable, WeatherChannel, WeatherUpdateEvent, WorldBootstrap,
    WorldEnvironmentBootstrap, WorldEvent, WorldPacketError, WorldWireError, air_network_id,
    into_world_event, request_sub_chunk_column, vanilla_dimension_range,
};
