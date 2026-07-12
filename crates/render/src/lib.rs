//! Packed chunk meshing and Bevy rendering for the Bedrock client.

mod biome;
mod color;
mod lighting;
mod liquid;
mod mesh;
mod plugin;

pub use biome::PackedBiomeRecord;
pub use color::debug_color;
pub use lighting::{
    PHASE26_BLOCK_LIGHT, PHASE26_SKY_LIGHT, bake_quad_lighting, bake_template_lighting,
    mesh_dependency_mask,
};
pub use liquid::LiquidLevel;
pub use mesh::{
    BlockClassifier, ChunkMesh, ChunkMeshStreams, ContributorResolver, Face, FaceConnectivity,
    Neighbourhood, PackedLiquidQuad, PackedModelRef, PackedQuad, PackedQuadLighting,
    ResolvedContributors, mesh_sub_chunk, mesh_sub_chunk_in_neighbourhood,
};
pub use plugin::{
    AnimationFrameSample, BiomeTint, ChunkAnimationClock, ChunkBiomeTintIdentity, ChunkBiomeTints,
    ChunkRenderInstance, ChunkRenderQueue, ChunkRenderQueueLimits, ChunkTextureAssetIdentity,
    ChunkTextureAssets, ChunkTextureUploadStats, ChunkUploadAcknowledgement,
    ChunkUploadAcknowledgements, ChunkUploadBudget, ChunkUploadPriority, ChunkUploadToken,
    DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME, DebugWorldPlugin, MATERIAL_UV_REFLECT_U,
    MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270,
    MAX_TRANSPARENT_DRAW_REFS, MAX_TRANSPARENT_VIEWS, MAX_TRANSPARENT_WITNESS_KEYS,
    PackedTransparentDrawRef, PresentedFrameAck, PresentedFrameGate, RenderViewCohort,
    TRANSPARENT_REF_BUFFER_BYTES, TRANSPARENT_REF_SLOT_BYTES, TargetRenderExpectation,
    TextureArrayLimits, TextureLimitError, TextureMipUploadPlan, TexturePageBinding,
    TextureUploadPlanError, TransparentAllocationIdentity, TransparentDrawArgs,
    TransparentOrderedSnapshot, TransparentSortCandidate, TransparentSortError,
    TransparentSortJobGate, TransparentSortMetrics, TransparentSortMetricsSnapshot,
    TransparentSortResult, TransparentSortState, TransparentUploadBatch, TransparentWitnessEvent,
    TransparentWitnessEvidence, TransparentWitnessIncompleteEvent, TransparentWitnessRequest,
    TransparentWitnessRequestError, TransparentWitnessStageEvent, TransparentWitnessStageRecord,
    ViewSortGeneration, ViewSortKey, diagnostic_texture_page,
    direct_transparent_draw_args_for_test, greedy_texture_uv, mdi_transparent_draw_args_for_test,
    plan_texture_mip_uploads, plan_texture_page_bindings, select_animation_frames,
    sort_transparent_candidates_for_test, texture_asset_needs_rebuild,
    validate_transparent_sort_ref_count,
};
