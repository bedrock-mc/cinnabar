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
