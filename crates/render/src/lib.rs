//! Packed chunk meshing and Bevy rendering for the Bedrock client.

mod atmosphere;
mod atmosphere_render;
mod biome;
mod cloud_config;
mod cloud_mesh;
mod cloud_render;
mod color;
mod lighting;
mod liquid;
mod mesh;
mod plugin;
mod visibility_diagnostics;

pub use atmosphere::{
    AtmosphereFrame, AtmosphereTextureAssets, BEDROCK_DAY_TICKS, CLOUD_SCROLL_BLOCKS_PER_TICK,
    CLOUD_TEXTURE_WORLD_PERIOD, CameraMedium, MoonPhaseTile, cloud_texture_offset, moon_phase_tile,
};
pub use atmosphere_render::AtmospherePlugin;
pub use biome::{
    BIOME_NEIGHBOUR_SLOT_COUNT, MAX_PACKED_BIOME_RECORD_WORDS, PackedBiomeRecord,
    biome_neighbour_index,
};
pub use cloud_config::{
    CloudCalibrationError, CloudCalibrationHarness, CloudCalibrationRecord, CloudCalibrationReport,
    CloudCoverageSemantics, CloudMatchingView, CloudQuality, CloudRenderConfig,
};
pub use cloud_mesh::{
    CLOUD_MASK_SIZE, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CloudFace, CloudMeshError, MAX_CLOUD_BYTES,
    MAX_CLOUD_QUADS, PackedCloudQuad, cloud_instance_origins, mesh_cloud_texture,
};
pub use color::debug_color;
pub use lighting::{
    FullBrightLightSampler, MeshLightSample, MeshLightSampler, PHASE26_BLOCK_LIGHT,
    PHASE26_SKY_LIGHT, bake_quad_lighting, bake_quad_lighting_with_sampler, bake_template_lighting,
    bake_template_lighting_with_sampler, mesh_dependency_mask,
};
pub use liquid::{LiquidLevel, sample_camera_medium};
pub use mesh::{
    BlockClassifier, ChunkMesh, ChunkMeshStreamError, ChunkMeshStreams, ContributorResolver, Face,
    FaceConnectivity, Neighbourhood, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef,
    PackedQuad, PackedQuadLighting, ResolvedContributors, mesh_sub_chunk,
    mesh_sub_chunk_in_neighbourhood, mesh_sub_chunk_in_neighbourhood_with_lighting,
    mesh_sub_chunk_with_lighting,
};
pub use plugin::{
    AnimationFrameSample, BiomeTint, ChunkAnimationClock, ChunkBiomeTintIdentity, ChunkBiomeTints,
    ChunkRenderApplySet, ChunkRenderInstance, ChunkRenderQueue, ChunkRenderQueueLimits,
    ChunkTextureAssetIdentity, ChunkTextureAssets, ChunkTextureUploadStats,
    ChunkUploadAcknowledgement, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken, DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME,
    DebugWorldPlugin, MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90,
    MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270, MAX_MODEL_WITNESS_KEYS,
    MAX_TRANSPARENT_DRAW_REFS, MAX_TRANSPARENT_VIEWS, MAX_TRANSPARENT_WITNESS_KEYS,
    ModelWitnessEvent, ModelWitnessEvidence, ModelWitnessFrameAck, ModelWitnessManifestRecord,
    ModelWitnessRequest, ModelWitnessRequestError, ModelWorkloadCount, ModelWorkloadMetrics,
    ModelWorkloadMetricsSnapshot, PackedTransparentDrawRef, PresentedFrameAck, PresentedFrameGate,
    RenderViewCohort, TRANSPARENT_REF_BUFFER_BYTES, TRANSPARENT_REF_SLOT_BYTES,
    TargetRenderExpectation, TextureArrayLimits, TextureLimitError, TextureMipUploadPlan,
    TexturePageBinding, TextureUploadPlanError, TransparentAllocationIdentity, TransparentDrawArgs,
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
pub use visibility_diagnostics::{
    ExtractedCameraIdentity, ExtractedViewGenerations, GraphicsAdapterMetadata,
    MAX_VISIBILITY_DIAGNOSTIC_KEYS, OpaqueDrawMode, VisibilityDiagnosticSnapshot,
    VisibilityDiagnostics, VisibilityDiagnosticsInput, VisibilityKeyDelta, VisibilityKeyDigest,
};
