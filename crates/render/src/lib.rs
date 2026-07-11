//! Packed chunk meshing and Bevy rendering for the Bedrock client.

mod biome;
mod color;
mod mesh;
mod plugin;

pub use biome::PackedBiomeRecord;
pub use color::debug_color;
pub use mesh::{
    BlockClassifier, ChunkMesh, ChunkMeshStreams, Face, FaceConnectivity, Neighbourhood,
    PackedLiquidQuad, PackedModelRef, PackedQuad, PackedQuadLighting, mesh_sub_chunk,
};
pub use plugin::{
    AnimationFrameSample, BiomeTint, ChunkAnimationClock, ChunkBiomeTintIdentity, ChunkBiomeTints,
    ChunkRenderInstance, ChunkRenderQueue, ChunkRenderQueueLimits, ChunkTextureAssetIdentity,
    ChunkTextureAssets, ChunkTextureUploadStats, ChunkUploadAcknowledgement,
    ChunkUploadAcknowledgements, ChunkUploadBudget, ChunkUploadPriority, ChunkUploadToken,
    DebugWorldPlugin, MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90,
    MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270, PresentedFrameAck, PresentedFrameGate,
    RenderViewCohort, TargetRenderExpectation, TextureArrayLimits, TextureLimitError,
    TextureMipUploadPlan, TexturePageBinding, TextureUploadPlanError, diagnostic_texture_page,
    greedy_texture_uv, plan_texture_mip_uploads, plan_texture_page_bindings,
    select_animation_frames, texture_asset_needs_rebuild,
};
