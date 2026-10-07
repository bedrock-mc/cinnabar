//! Bedrock 1.26.50 (protocol 2193) packet definitions and codec.

mod actor;
mod audio;
mod blob_cache;
mod block_edit;
mod boss;
mod camera;
mod codec;
mod credits;
mod dimension;
mod disconnect;
mod experience;
mod interaction;
mod inventory;
mod item;
mod item_capacity;
pub mod launcher_control;
mod login;
mod movement;
mod nbt_tree;
mod packet;
mod particle;
mod permissions;
mod primitive_shapes;
mod raw_text;
mod respawn;
mod settings;
mod skin_change;
pub use skin_change::{cape_content_id, player_skin_packet, set_skin_packet_uuid};
mod socket_transport;
pub mod store_control;
mod transfer;
mod translation_parameter;
mod ui;
mod world;
pub mod world_control;

pub use experience::{
    EXPERIENCE_CHANNEL, ExperienceMessage, MAX_EXPERIENCE_ENVELOPE_BYTES, experience_packet,
    is_experience_packet,
};

pub use credits::{ShowCreditsEvent, credits_finished_packet};

pub use dimension::{LoadingScreenPhase, dimension_change_done_packet, loading_screen_packet};

pub use actor::{
    ACTOR_DATA_ID_FLAGS_THIRD, ActorAttribute, ActorAttributeModifier, ActorAttributesUpdateEvent,
    ActorEffectAction, ActorEffectEvent, ActorEvent, ActorIdentifier, ActorIdentifierRegistry,
    ActorInterpolation, ActorKind, ActorLinkEvent, ActorLinkType, ActorMetadata,
    ActorMetadataUpdateEvent, ActorMetadataValue, ActorMoveEvent, ActorPacketError,
    ActorPositionOrigin, ActorProperty, ActorRemoveEvent, ActorSpawnEvent, ActorStatusEvent,
    ActorStatusKind, ActorTakeItemEvent, CAPE_DIMENSIONS, CLASSIC_SKIN_SIDE, CapeImage,
    ITEM_ACTOR_NETWORK_OFFSET, MAX_ACTOR_ATTRIBUTE_MODIFIERS, MAX_ACTOR_ATTRIBUTES,
    MAX_ACTOR_IDENTIFIER_BYTES, MAX_ACTOR_IDENTIFIERS, MAX_ACTOR_LINKS_PER_SPAWN,
    MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_METADATA_NBT_BYTES, MAX_ACTOR_METADATA_STRING_BYTES,
    MAX_ACTOR_NAME_BYTES, MAX_ACTOR_PROPERTIES, MAX_CLASSIC_SKIN_SIDE, MAX_PLAYER_LIST_RECORDS,
    MAX_PLAYER_LIST_SKIN_BYTES, MAX_SKIN_ANIMATION_LAYERS, MAX_SKIN_GEOMETRY_SOURCE_BYTES,
    MAX_STANDARD_SKIN_SIDE, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin,
    PlayerSkinUnavailable, SkinAnimation, SkinAnimationKind, SkinGeometrySource, SkinRgba8,
    StandardSkin, expand_legacy_skin_rgba8, normalize_classic_skin_rgba8,
};
pub use audio::{
    AudioEvent, LevelAudioEvent, LevelEventSound, MAX_AUDIO_IDENTIFIER_BYTES, PlayAudioEvent,
    StopAudioEvent,
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
pub use block_edit::{map_info_request_packet, sign_edit_packet};
pub use boss::boss_registration_response;
pub use camera::{
    CameraAimAssistAction, CameraAimAssistActorPriority, CameraAimAssistCategory,
    CameraAimAssistExclusions, CameraAimAssistItemSetting, CameraAimAssistPreset,
    CameraAimAssistPresetSettings, CameraAimAssistPriorities, CameraAimAssistPriority,
    CameraAimAssistRegistry, CameraAimAssistSettings, CameraAimAssistTargetMode, CameraEase,
    CameraEvent, CameraFadeColor, CameraFadeInstruction, CameraFadeTimes, CameraFovInstruction,
    CameraInstructionEvent, CameraPreset, CameraSetInstruction, CameraShakeAction,
    CameraShakeEvent, CameraShakeType, CameraSpline, CameraSplineInstruction, CameraSplineKind,
    CameraSplineProgressKeyFrame, CameraSplineRotationKeyFrame, CameraSwitchEvent,
    CameraTargetInstruction, MAX_CAMERA_AIM_ASSIST_ENTRIES, MAX_CAMERA_EASE_IDENTIFIER_BYTES,
    MAX_CAMERA_PRESETS, MAX_CAMERA_SPLINE_POINTS, camera_aim_assist_activation_packet,
};
pub use codec::{ProtocolError, decode_batch, encode};
pub use disconnect::ServerDisconnectEvent;
pub use interaction::{
    ActorUseAction, ActorUsePacketError, ActorUseRequest, BlockUsePacketError, BlockUseRequest,
    HeldItemRequest, ItemUseTrigger, PredictedSlotChange, SwingSource, click_air_packet,
    click_block_packet, click_block_transaction_packet, destroy_block_packet,
    is_aim_assist_rotation_action, release_item_packet, start_item_use_on_packet,
    stop_item_use_on_packet, stop_sleeping_packet, swing_arm_packet, use_actor_packet,
};
pub use inventory::recipes::{
    MAX_RECIPE_INGREDIENTS, RECIPE_ANY_AUX, RECIPE_OWNED_BYTES, RecipeCatalog, RecipeDefinition,
    RecipeHandle, RecipeIngredient, RecipeIngredientView, RecipeOutput, RecipeUpdate,
    decode_recipe_update, empty_recipe_extra, recipe_binding_supported, vanilla_tag_contains,
};
pub use inventory::recipes::{
    MultiRecipe, ScreenIngredient, ScreenRecipe, ScreenRecipeKind, ScreenRecipes,
};
pub use inventory::{
    ARMOR_SLOTS, ARMOR_WINDOW_ID, AutoCraftIngredient, CONTAINER_NAME_CREATED_OUTPUT,
    CONTAINER_NAME_HOTBAR, CRAFTING_INPUT_SLOTS, CREATED_OUTPUT_SLOT, ContainerWindow, CraftResult,
    LAST_CONTAINER_NAME, MAX_STACK_REQUEST_ACTIONS, NO_CONTAINER_WINDOW_TYPE, StackItemDescriptor,
    container_window, is_personal_ui_inventory,
};
pub use inventory::{
    BookEdit, MAX_BOOK_PAGE_BYTES, block_pick_request_packet, book_edit_packet,
    crafter_slot_toggle_packet, lectern_update_packet,
};
pub use inventory::{
    CONTAINER_NAME_ARMOR, CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CRAFT_INPUT,
    CONTAINER_NAME_CURSOR, CONTAINER_NAME_DYNAMIC, CONTAINER_NAME_INVENTORY,
    CONTAINER_NAME_LEVEL_ENTITY, CONTAINER_NAME_OFFHAND, CanonicalCell, ContainerCloseEvent,
    ContainerDataEvent, ContainerIdentity, ContainerOpenEvent, InventoryAuthority,
    InventoryContentEvent, InventoryEvent, InventoryPacketError, InventorySlotEvent,
    InventoryTransactionEvent, ItemStackResponseEvent, MAX_CONTAINER_SLOTS, MAX_FILTER_STRINGS,
    MAX_ITEM_NBT_BYTES, MAX_RESPONSE_CONTAINERS, MAX_RESPONSE_NAME_BYTES, MAX_STACK_RESPONSES,
    OFFHAND_WINDOW_ID, PLAYER_INVENTORY_SLOTS, PLAYER_INVENTORY_WINDOW_ID, SelectedSlotEvent,
    SlotIdentity, StackRequestAction, StackRequestContainer, StackRequestSlot, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus, UI_INVENTORY_WINDOW_ID,
    VerifiedNetworkItemStack, container_close_packet, item_stack_request_batch,
    item_stack_request_packet, item_stack_request_packet_filtered, normalize_authority,
    normalize_container_close, normalize_container_data, normalize_container_open,
    normalize_content, normalize_hotbar, normalize_response, normalize_slot, open_inventory_packet,
    personal_craft_content_indices, personal_craft_slot_index, project_container_cell,
    validate_item_nbt_size,
};
pub use inventory::{
    CreativeCategory, CreativeContentEvent, CreativeGroup, CreativeItem, MAX_CREATIVE_GROUPS,
    MAX_CREATIVE_ITEMS,
};
pub use inventory::{
    EnchantOption, EnchantOptionsEvent, MAX_ENCHANT_OPTIONS, OpenCells, UI_SLOT_COUNT,
    WINDOW_TYPE_ANVIL, WINDOW_TYPE_BEACON, WINDOW_TYPE_BLAST_FURNACE, WINDOW_TYPE_BREWING_STAND,
    WINDOW_TYPE_CARTOGRAPHY, WINDOW_TYPE_CONTAINER, WINDOW_TYPE_CRAFTER, WINDOW_TYPE_DISPENSER,
    WINDOW_TYPE_DROPPER, WINDOW_TYPE_ENCHANTMENT, WINDOW_TYPE_FURNACE, WINDOW_TYPE_GRINDSTONE,
    WINDOW_TYPE_HOPPER, WINDOW_TYPE_HORSE, WINDOW_TYPE_LECTERN, WINDOW_TYPE_LOOM,
    WINDOW_TYPE_SMITHING_TABLE, WINDOW_TYPE_SMOKER, WINDOW_TYPE_STONECUTTER, WINDOW_TYPE_WORKBENCH,
    WindowKind, WindowSegment, is_chest_like_name, is_open_window_name, is_result_preview_name,
    normalize_enchant_options, open_cell_request, open_name_first_cell, ui_slot_container_name,
    ui_slot_for_name, ui_slot_request_container,
};
pub use inventory::{
    IngredientObservation, MAX_RECIPE_OBSERVATIONS, RecipeObservation, RecipeObservations,
};
pub use inventory::{MineBlockRequest, MineBlockRequestError};
pub use inventory::{RecipeRegistryError, RecipeRegistrySnapshot};
pub use item::{
    ActorActionEvent, ActorActionKind, ActorHandedness, ArmorEquipmentEvent, EquipmentEvent,
    HOTBAR_SLOT_COUNT, ItemActorEvent, ItemBook, ItemComponents, ItemDisplay, ItemPacketError,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, MAX_ACTION_IDENTIFIER_BYTES,
    MAX_ANIMATE_ENTITY_IDS, MAX_ANIMATION_IDENTIFIER_BYTES, MAX_BOOK_PAGES, MAX_ITEM_EXTRA_BYTES,
    MAX_ITEM_REGISTRY_ENTRIES, NetworkItemStack, item_book, item_bundle_id,
    item_charged_projectile, item_components, item_custom_color, item_display,
    item_enchantment_level, item_extra_damage, item_extra_unbreakable, item_has_enchantment_list,
    item_icon_keys, item_stack_damage, select_hotbar_slot_packet, vanilla_item_registry,
};
pub use item_capacity::{ITEM_DEFAULT_MAX_STACK_SIZE, vanilla_item_capacity};
pub use jolyne::GameData;
pub use jolyne::stream::client::{ClientCape, ClientSkin};
pub use jolyne::stream::{
    ResourcePackArchive, ResourcePackContentKey, ResourcePackHandoff, ResourcePackIdentity,
    ResourcePackStore,
};
pub use jolyne::{GAME_VERSION, PROTOCOL_VERSION};
pub use respawn::{respawn_ready_packet, respawn_request_packet};

/// The vendored wire crates, so no other manifest declares their pinned paths.
#[cfg(feature = "wire-test-support")]
pub mod wire {
    pub use jolyne;
    pub use valentine;
}
pub use login::{LoginSequence, PacketIdTraceSnapshot, PlaySession, network_stack_latency_reply};
pub use movement::{
    BlockAction, BlockActionKind, BlockActions, BlockActionsFull, BlockItemInteraction,
    InteractionEncodeError, MAX_BLOCK_ACTIONS_PER_INPUT, MovementPredictionSync,
    PlayerAuthInputError, PlayerAuthInputInteractions, PlayerAuthInputSnapshot,
    PlayerAuthInputTraceSample, PlayerInputFlags, PlayerInputMode, client_movement_prediction_sync,
    player_auth_input, player_auth_input_trace_sample, player_auth_input_with_interactions,
    player_auth_input_with_mining_request,
};
pub use packet::Packet;
pub use particle::{
    LevelParticleEvent, MAX_PARTICLE_NAME_BYTES, MAX_PARTICLE_VARIABLES_BYTES, ParticleEvent,
    SpawnParticleEffectEvent,
};
pub use permissions::{
    AbilitiesUpdate, AbilityLayerEvidence, AbilityLayersEvidence, MAX_ABILITY_LAYERS,
    decode_abilities_update,
};
pub use raw_text::{
    MAX_RAW_TEXT_COMPONENTS, MAX_RAW_TEXT_DEPTH, MAX_RAW_TEXT_INPUT_BYTES, MAX_RAW_TEXT_NODES,
    MAX_RAW_TEXT_OUTPUT_BYTES, RawTextComponent, RawTextDocument, RawTextResolution,
    RawTextResolver, ResolvedRawText, format_translation, parse_raw_text,
};
pub use render_api::primitive_shapes::{
    PrimitiveShapeChange, PrimitiveShapeData, PrimitiveShapeKind, PrimitiveShapeUpdate,
    PrimitiveShapesEvent, PrimitiveText,
};
pub use settings::request_chunk_radius_packet;
pub use socket_transport::{SocketTransport, bridge_endpoint_path, report_pack_application};
pub use transfer::{MAX_TRANSFER_HOST_BYTES, ServerTransferEvent, ServerTransferRejection};
pub use translation_parameter::localize_parameter_prefix;
pub use ui::{
    BlockCrackAction, BlockCrackEvent, BossAction, BossColor, BossEvent, BossOverlay, BossStyle,
    ChatAutocompleteAction, ChatAutocompleteCatalog, ChatAutocompleteCatalogError,
    ChatAutocompleteCompletion, ChatAutocompleteEvent, ChatPacketError, CommandOutputEvent,
    CommandOutputMessage, CommandParam, CommandParamKind, CommandSpec, CommandTreeEvent,
    CompletionContext, CustomForm, CustomFormElement, CustomFormValue, ElementMenuForm,
    FormButtonImage, FormKind, FormNumber, FormRequestEvent, GameModeEvent, GameModeUpdate,
    HudEvent, HudRules, MAX_BOSS_EVENTS, MAX_CHAT_AUTOCOMPLETE, MAX_CHAT_AUTOCOMPLETE_BYTES,
    MAX_CHAT_PARAMETERS, MAX_COMMAND_OUTPUT_MESSAGES, MAX_FORM_BUTTONS, MAX_FORM_JSON_BYTES,
    MAX_FORM_JSON_DEPTH, MAX_OUTBOUND_CHAT_BYTES, MAX_SCORE_ENTRIES_PER_PACKET, MAX_UI_TEXT_BYTES,
    MenuElement, ModalDialogForm, ModalFormResponseSelection, NPC_DIALOGUE_FORM_ID, NpcButton,
    NpcDialogueForm, NpcRequestKind, ObjectiveEvent, PlayerStatus, RawTextEvent, ScoreAction,
    ScoreEntry, ScoreEvent, ScoreIdentity, ServerFormModel, SleepStatusEvent, TextCategory,
    TextEvent, TextKind, TextMenuForm, TitleAction, TitleEvent, UiEvent, UiPacketError,
    UnsupportedForm, chat_input_packet, chat_text_packet, command_request_packet,
    custom_form_submit_response, modal_form_busy_response, modal_form_cancel_response,
    modal_form_submit_response, npc_request_packet, server_settings_request_packet,
};
pub use valentine::bedrock::context::BedrockSession;
pub use world::{
    ActorBlockSyncMessage, ActorMotionEvent, ActorPropertySyncEvent, BiomeDefinitionEvent,
    BiomeDefinitionsEvent, BlockEntityUpdateEvent, BlockEventEvent, BlockUpdateEvent,
    ChangeDimensionEvent, ChunkResyncEvent, CustomBlock, CustomBlockVisuals, CustomBlocks,
    CustomBox, CustomHashedState, CustomMaterialInstance, CustomPermutation, CustomSelection,
    CustomStateAxis, CustomStateValue, CustomTransformation, CustomVisualComponents,
    DaylightCycleUpdateEvent, DimensionHeightDiagnostic, DimensionRange, GameRulesEvent,
    HASHED_AIR_NETWORK_ID, HeightmapDiagnostic, LevelChunkEvent, LevelChunkMode, MAP_IMAGE_SIDE,
    MAX_BIOME_DEFINITIONS, MAX_BIOME_NAME_BYTES, MAX_BLOCK_LAYERS, MAX_DIMENSION_DEFINITIONS,
    MAX_SUB_CHUNK_REQUESTS, MapDataEvent, MovePlayerEvent, MovePlayerMode,
    MovementCorrectionSubject, NETHER_DIMENSION_ID, OVERWORLD_CLOCK_ID, OVERWORLD_CLOCK_NAME,
    OpenSignEvent, PLAYER_NETWORK_OFFSET, PlayerGameMode, PlayerMovementCorrectionEvent,
    PublisherUpdateEvent, RespawnEvent, SEQUENTIAL_AIR_NETWORK_ID, STANDING_PLAYER_EYE_HEIGHT,
    SetTimeEvent, SubChunkBatchEvent, SubChunkDiagnostic, SubChunkEntryEvent,
    SubChunkReplyAdmissionEvent, SubChunkResult, SubChunkUnavailable, SyncedBlockUpdateEvent,
    WeatherChannel, WeatherUpdateEvent, WorldBootstrap, WorldClockDefinition, WorldClockState,
    WorldClockUpdateEvent, WorldEnvironmentBootstrap, WorldEvent, WorldPacketError, WorldWireError,
    air_network_id, block_name_sort_key, block_state_network_hash, into_world_event, is_hardcore,
    request_sub_chunk_column, rewind_history_size, server_authoritative_block_breaking,
    vanilla_dimension_range,
};

mod movement_transport;
pub use movement_transport::{BatchSendError, InteractionPacketGuard, PhysicsSendIdentity};

mod fast_transfer_action;
pub use fast_transfer_action::FastTransferAction;
