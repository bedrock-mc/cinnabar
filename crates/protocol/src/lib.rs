//! Bedrock 1.26.30 (protocol 1001) packet definitions and codec.

mod actor;
mod codec;
mod login;
mod movement;
mod packet;
mod socket_transport;
mod world;

pub use actor::{
    ActorAttribute, ActorAttributeModifier, ActorAttributesUpdateEvent, ActorEvent, ActorKind,
    ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, ActorMoveEvent, ActorPacketError,
    ActorProperty, ActorRemoveEvent, ActorSpawnEvent, MAX_ACTOR_ATTRIBUTE_MODIFIERS,
    MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_IDENTIFIER_BYTES, MAX_ACTOR_METADATA_ENTRIES,
    MAX_ACTOR_METADATA_NBT_BYTES, MAX_ACTOR_METADATA_STRING_BYTES, MAX_ACTOR_NAME_BYTES,
    MAX_ACTOR_PROPERTIES, MAX_PLAYER_LIST_RECORDS, MAX_PLAYER_LIST_SKIN_BYTES,
    MAX_STANDARD_SKIN_SIDE, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin,
    PlayerSkinUnavailable, StandardSkin,
};
pub use codec::{ProtocolError, decode_batch, encode};
pub use jolyne::GameData;
pub use login::{LoginSequence, PlaySession};
pub use movement::{
    PlayerAuthInputError, PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode,
    player_auth_input,
};
pub use packet::Packet;
pub use socket_transport::SocketTransport;
pub use valentine::bedrock::context::BedrockSession;
pub use valentine::bedrock::version::v1_26_30::{GAME_VERSION, PROTOCOL_VERSION};
pub use world::{
    BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockEntityUpdateEvent, BlockUpdateEvent,
    ChangeDimensionEvent, DaylightCycleUpdateEvent, DimensionRange, HASHED_AIR_NETWORK_ID,
    LevelChunkEvent, LevelChunkMode, MAX_BIOME_DEFINITIONS, MAX_BIOME_NAME_BYTES, MAX_BLOCK_LAYERS,
    MAX_SUB_CHUNK_REQUESTS, MovePlayerEvent, MovePlayerMode, PlayerMovementCorrectionEvent,
    PublisherUpdateEvent, SEQUENTIAL_AIR_NETWORK_ID, SetTimeEvent, SubChunkBatchEvent,
    SubChunkEntryEvent, SubChunkResult, SubChunkUnavailable, WeatherChannel, WeatherUpdateEvent,
    WorldBootstrap, WorldEnvironmentBootstrap, WorldEvent, WorldPacketError, air_network_id,
    into_world_event, request_sub_chunk_column, vanilla_dimension_range,
};
