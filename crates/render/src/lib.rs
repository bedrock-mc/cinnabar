//! Packed chunk meshing and Bevy rendering for the Bedrock client.

mod actor;
mod actor_render;
mod atmosphere;
mod atmosphere_render;
mod chunk;
mod cloud_config;
mod cloud_render;
mod visibility_diagnostics;

use meshing::{
    ChunkMesh, PackedBiomeRecord, PackedCloudQuad, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting, mesh_cloud_texture,
};

pub use actor::{
    ACTOR_INTERPOLATION_DELAY_SECONDS, ActorRenderFrame, ActorRenderInstance, ActorRenderScene,
    ActorRenderSource, ActorSkinPixels, DEFAULT_SKIN_PROVENANCE, MAX_RENDERED_PLAYERS,
    STANDARD_BIPED_VERTEX_COUNT, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE, standard_biped_vertices,
};
pub use actor_render::ActorRenderPlugin;
pub use atmosphere::{
    AtmosphereFrame, AtmosphereTextureAssets, BEDROCK_DAY_TICKS, CLOUD_SCROLL_BLOCKS_PER_TICK,
    CLOUD_TEXTURE_WORLD_PERIOD, MoonPhaseTile, cloud_texture_offset, moon_phase_tile,
};
pub use atmosphere_render::AtmospherePlugin;
pub use chunk::{
    AnimationFrameSample, BiomeTint, ChunkAnimationClock, ChunkBiomeTints, ChunkRenderApplySet,
    ChunkRenderInstance, ChunkRenderPlugin, ChunkRenderQueue, ChunkRenderQueueLimits,
    ChunkTextureAssetIdentity, ChunkTextureAssets, ChunkTextureUploadStats,
    ChunkUploadAcknowledgement, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken, DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME,
    MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180,
    MATERIAL_UV_ROTATE_270, MAX_MODEL_WITNESS_KEYS, MAX_TRANSPARENT_DRAW_REFS,
    MAX_TRANSPARENT_VIEWS, MAX_TRANSPARENT_WITNESS_KEYS, ModelWitnessEvent, ModelWitnessEvidence,
    ModelWitnessFrameAck, ModelWitnessManifestRecord, ModelWitnessRequest,
    ModelWitnessRequestError, ModelWorkloadCount, ModelWorkloadMetrics,
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
pub use cloud_config::{
    CloudCalibrationError, CloudCalibrationHarness, CloudCalibrationRecord, CloudCalibrationReport,
    CloudCoverageSemantics, CloudMatchingView, CloudQuality, CloudRenderConfig,
};
pub use visibility_diagnostics::{
    ExtractedCameraIdentity, ExtractedViewGenerations, GraphicsAdapterMetadata,
    MAX_VISIBILITY_DIAGNOSTIC_KEYS, OpaqueDrawMode, VisibilityDiagnosticSnapshot,
    VisibilityDiagnostics, VisibilityDiagnosticsInput, VisibilityKeyDelta, VisibilityKeyDigest,
};
