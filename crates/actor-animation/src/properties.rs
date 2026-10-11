//! Synced actor property definitions, which the property queries read by name.
use std::sync::Arc;

/// How a property's stored number reads: enums store the index of their value name.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyKind {
    Number,
    Enum(Arc<[Arc<str>]>),
}

/// One property of an actor type, in wire index order.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyDefinition {
    pub name: Arc<str>,
    pub kind: PropertyKind,
    /// Stored number an actor reads before the server sets it (enums: the value index).
    pub default: f32,
}
