//! Protocol facade for the single Bedrock version supported by `jolyne`.
//!
//! The `valentine::bedrock::version::vX_Y_Z` module selected by the enabled
//! feature is the canonical source. This module keeps the existing flat
//! `jolyne::valentine::*` surface for downstream crates while making the pinned
//! version explicit.

pub use current::*;
#[cfg(feature = "bedrock_1_26_30")]
pub use valentine::bedrock::version::v1_26_30 as current;
#[cfg(all(feature = "bedrock_1_26_40", not(feature = "bedrock_1_26_30")))]
pub use valentine::bedrock::version::v1_26_40 as current;

use valentine::bedrock::context::BedrockSession;

/// Builds the decode arguments for the pinned protocol version from session state.
///
/// Up to 1.26.30 the generated decoder needed the negotiated shield item ID to
/// disambiguate item payloads, so the args carried it. The 1.26.40 generator
/// emits a unit args struct because the shape no longer depends on session
/// state. This helper hides that difference so call sites stay version-agnostic
/// and keep threading the session through.
#[cfg(feature = "bedrock_1_26_30")]
pub fn packet_args(session: &BedrockSession) -> current::McpePacketArgs {
    current::McpePacketArgs::from(session)
}

/// See the `bedrock_1_26_30` variant above.
#[cfg(all(feature = "bedrock_1_26_40", not(feature = "bedrock_1_26_30")))]
pub fn packet_args(_session: &BedrockSession) -> current::McpePacketArgs {
    current::McpePacketArgs
}
