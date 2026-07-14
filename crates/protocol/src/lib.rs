//! Bedrock 1.26.30 (protocol 1001) packet definitions and codec.

mod codec;
mod login;
mod packet;
mod socket_transport;
mod world;

pub use codec::{ProtocolError, decode_batch, encode};
pub use jolyne::GameData;
pub use login::{LoginSequence, PlaySession};
pub use packet::Packet;
pub use socket_transport::SocketTransport;
pub use valentine::bedrock::context::BedrockSession;
pub use valentine::bedrock::version::v1_26_30::{GAME_VERSION, PROTOCOL_VERSION};
pub use world::{
    BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockUpdateEvent, ChangeDimensionEvent,
    DaylightCycleUpdateEvent, DimensionRange, HASHED_AIR_NETWORK_ID, LevelChunkEvent,
    LevelChunkMode, MAX_BIOME_DEFINITIONS, MAX_BIOME_NAME_BYTES, MAX_BLOCK_LAYERS,
    MAX_SUB_CHUNK_REQUESTS, MovePlayerEvent, PlayerMovementCorrectionEvent, PublisherUpdateEvent,
    SEQUENTIAL_AIR_NETWORK_ID, SetTimeEvent, SubChunkBatchEvent, SubChunkEntryEvent,
    SubChunkResult, SubChunkUnavailable, WeatherChannel, WeatherUpdateEvent, WorldBootstrap,
    WorldEnvironmentBootstrap, WorldEvent, WorldPacketError, air_network_id, into_world_event,
    request_sub_chunk_column, vanilla_dimension_range,
};
