//! Retained debug shapes with stable slots and change-only GPU publications.

mod actors;
mod slots;
mod state;
mod store;
#[cfg(test)]
mod tests;

pub use actors::PrimitiveActor;
pub use slots::PrimitiveSlots;
pub use state::{PrimitiveInstance, PrimitiveMeshKey, PrimitiveState};
pub use store::{
    PrimitiveBatch, PrimitiveShapeStore, PrimitiveTextChange, PrimitiveTextRecord,
    PrimitiveUploadStats,
};
