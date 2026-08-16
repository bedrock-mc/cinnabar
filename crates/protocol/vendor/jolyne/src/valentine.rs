//! Protocol facade for the single Bedrock version supported by `jolyne`.
//!
//! The `valentine::bedrock::version::vX_Y_Z` module selected by the enabled
//! feature is the canonical source. This module keeps the existing flat
//! `jolyne::valentine::*` surface for downstream crates while making the pinned
//! version explicit.

pub use current::*;
#[cfg(feature = "bedrock_1_26_44")]
pub use valentine::bedrock::version::v1_26_44 as current;

// Keep Jolyne's stable facade names while protocolgen exposes shared canonical
// enum names from the version module.
pub type NetworkSettingsPacketCompressionAlgorithm = current::EnumsPacketCompressionAlgorithm;
pub type PlayStatusPacketStatus = current::EnumsPlayStatus;
pub type ServerboundLoadingScreenPacketLoadingScreenPacketType =
    current::EnumsServerboundLoadingScreenPacketType;

use valentine::bedrock::context::BedrockSession;

/// Builds the decode arguments for the pinned protocol version from session state.
///
/// The current generated decoder uses a unit argument type. The session
/// parameter keeps call sites version-agnostic.
pub fn packet_args(_session: &BedrockSession) -> current::McpePacketArgs {
    current::McpePacketArgs
}
