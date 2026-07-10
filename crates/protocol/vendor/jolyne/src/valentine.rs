//! Protocol facade for the single Bedrock version supported by `jolyne`.
//!
//! `valentine::bedrock::version::v1_26_30` is the canonical source.
//! This module keeps the existing flat `jolyne::valentine::*` surface for
//! downstream crates while making the pinned version explicit.

pub use current::*;
pub use valentine::bedrock::version::v1_26_30 as current;
