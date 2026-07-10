//! Bedrock 1.26.30 (protocol 1001) packet definitions and codec.

mod codec;
mod packet;

pub use codec::{ProtocolError, decode_batch, encode};
pub use packet::Packet;
pub use valentine::bedrock::context::BedrockSession;
pub use valentine::bedrock::version::v1_26_30::{GAME_VERSION, PROTOCOL_VERSION};
