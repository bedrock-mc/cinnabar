//! Packed chunk meshing and Bevy rendering for the Bedrock client.

mod actor;
mod actor_render;
mod atmosphere;
mod atmosphere_render;
mod chunk;
mod cloud_config;
mod cloud_render;
mod present_mode;
mod ui;
mod ui_render;
mod visibility_diagnostics;

use meshing::{
    ChunkMesh, PackedBiomeRecord, PackedCloudQuad, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting, mesh_cloud_texture,
};

pub use actor::{
    ACTOR_BONE_MATRIX_BYTES, ActorCullView, ActorDrawFrame, ActorDrawManifestEntry,
    ActorGpuInstance, ActorMainWitness, ActorPresentationGate, ActorPresentedFrameAck,
    ActorRenderFrame, ActorRenderIdentity, ActorRenderInstance, ActorRenderScene,
    ActorRenderSource, ActorRigFrameBuilder, ActorRigGeometry, ActorRigGeometryError,
    ActorRigGeometrySpan, ActorRigRejects, ActorRigRenderFrame, ActorRigRenderInput, ActorRigRoute,
    ActorRigSubmission, ActorRigVertex, ActorRuntimeWitness, ActorSkinPixels, ActorTextureAtlas,
    ActorTexturePixels, DEFAULT_SKIN_PROVENANCE, EntityRigId, MAX_ACTOR_BONE_ARENA_BYTES,
    MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS, MAX_ACTOR_RENDER_DISTANCE_BLOCKS, MAX_ACTOR_RIG_VERTICES,
    MAX_ACTOR_TEXTURE_ATLAS_BYTES, MAX_ACTOR_TEXTURE_ATLAS_SIDE, MAX_RENDER_BONES_PER_ACTOR,
    MAX_RENDERED_PLAYERS, RenderBoneTransform, STANDARD_BIPED_VERTEX_COUNT, STANDARD_SKIN_BYTES,
    STANDARD_SKIN_SIDE, default_actor_skin_rgba8, normalize_actor_skin, pack_actor_textures,
    standard_biped_vertices,
};
pub use actor_render::ActorRenderPlugin;
pub use atmosphere::{
    AtmosphereFrame, AtmosphereTextureAssets, BEDROCK_DAY_TICKS, CLOUD_SCROLL_BLOCKS_PER_TICK,
    CLOUD_TEXTURE_WORLD_PERIOD, MoonPhaseTile, cloud_directional_illuminance, cloud_fog_factor,
    cloud_texture_offset, cloud_weather_colour, moon_phase_tile,
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
    ViewSortGeneration, ViewSortKey, diagnostic_texture_page, greedy_texture_uv,
    plan_texture_mip_uploads, plan_texture_page_bindings, select_animation_frames,
    texture_asset_needs_rebuild, validate_transparent_sort_ref_count,
};
#[cfg(feature = "publication-test-support")]
pub use chunk::{
    PublicationRenderTerminalSnapshot, publication_noop_render_plugin,
    publication_render_terminal_snapshot, settle_publication_noop_frame,
};
pub use cloud_config::{
    CloudCalibrationError, CloudCalibrationHarness, CloudCalibrationRecord, CloudCalibrationReport,
    CloudCoverageSemantics, CloudGeometryDiagnostic, CloudGeometryDiagnosticError,
    CloudMatchingView, CloudQuality, CloudRenderConfig,
};
pub use present_mode::{
    Dx12PresentModePolicy, Dx12PresentModePolicyPlugin, PresentModePreference, PresentModeRemedy,
    resolve_dx12_present_mode_remedy,
};
pub use ui::{
    MAX_UI_BATCHES, MAX_UI_DRAW_BYTES, MAX_UI_INDICES, MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS,
    MAX_UI_TEXTURE_SIDE, MAX_UI_VERTICES, UiRenderBatch, UiRenderInput, UiRenderReject,
    UiRenderRejectReason, UiRenderScene, UiRenderStats, UiRenderStatsSnapshot,
    UiRenderTextureArray, UiRenderVertex, UiScissor,
};
pub use ui_render::UiRenderPlugin;
pub use visibility_diagnostics::{
    ExtractedCameraIdentity, ExtractedViewGenerations, GraphicsAdapterMetadata,
    MAX_VISIBILITY_DIAGNOSTIC_KEYS, OpaqueDrawMode, VisibilityDiagnosticSnapshot,
    VisibilityDiagnostics, VisibilityDiagnosticsInput, VisibilityKeyDelta, VisibilityKeyDigest,
};
