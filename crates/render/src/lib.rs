//! Packed chunk meshing and Bevy rendering for the Bedrock client.

mod color;
mod mesh;
mod plugin;

pub use color::debug_color;
pub use mesh::{
    BlockClassifier, ChunkMesh, Face, FaceConnectivity, Neighbourhood, PackedQuad, mesh_sub_chunk,
};
pub use plugin::{
    ChunkRenderInstance, ChunkRenderQueue, ChunkRenderQueueLimits, ChunkUploadAcknowledgement,
    ChunkUploadAcknowledgements, ChunkUploadBudget, ChunkUploadPriority, ChunkUploadToken,
    DebugWorldPlugin,
};
