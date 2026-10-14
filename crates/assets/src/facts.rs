//! Dense block facts shared by registry consumers.

pub mod block;
pub use block::{BlockFact, BlockFacts};

pub(crate) mod family_routing;
mod properties;
mod properties_generated;
mod property_domains_generated;

pub(crate) use properties::decode;
pub use properties::{PropertyDomain, StateValue};
pub use properties_generated::BlockProperty;
