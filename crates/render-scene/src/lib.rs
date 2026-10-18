//! Render submission resources shared by presentation and GPU plugins.
pub mod actor;
pub mod aim_assist;
pub mod atmosphere;
pub mod block_entity;
pub mod chunk;
pub mod cloud_config;
pub mod cloud_visibility;
pub mod dropped_item;
pub mod enhanced;
pub mod entity_shadow;
pub mod far_terrain;
pub mod formats;
pub mod gpu_times;
pub mod hand_rig;
pub mod lighting;
pub mod lightning;
pub mod media_screen;
pub mod mod_scene;
pub mod model_resources;
pub mod motion_blur;
pub mod nametag;
pub mod panorama;
pub mod particles;
pub mod primitive_shapes;
pub mod readiness;
pub mod runtime_profile;
#[cfg(any(test, feature = "alloc-stats"))]
mod runtime_profile_alloc;
pub mod runtime_profile_slow;
pub mod runtime_profile_trace;
pub mod runtime_profile_uploads;
pub mod screen_fire;
pub mod screen_overlay;
pub mod screen_overlay_portal;
pub mod skybox;
pub mod startup_presentation;
pub mod terrain_publication;
pub mod ui;
pub mod ui_glint;
pub mod viewmodel;
pub mod visibility_diagnostics;
pub mod weather;
pub use actor::pipeline_readiness::ActorPipelineReadiness;
pub use actor::presentation::{
    ActorDrawFrame, ActorPresentationGate, ActorPresentedFrameAck,
    MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS,
};
pub use actor::witness::{ActorMainWitness, ActorRuntimeWitness};
pub use actor::{ActorRenderFrame, ActorRenderInstance, ActorRenderScene, ActorRenderSource};
pub use aim_assist::{
    AIM_ASSIST_TEXTURES, AimAssistHighlight, AimAssistHighlightScene, AimAssistTexture,
};
pub use atmosphere::{
    AtmosphereFrame, AtmosphereTextureAssets, CLOUD_ALPHA, CLOUD_SCROLL_BLOCKS_PER_TICK,
    CLOUD_TEXTURE_WORLD_PERIOD, MoonPhaseTile, PROVISIONAL_BOSS_DARKEN_SKY_STRENGTH,
    PROVISIONAL_BOSS_WORLD_FOG_END_BLOCKS, PROVISIONAL_BOSS_WORLD_FOG_START_BLOCKS, SkyKind,
    cloud_colour, cloud_distance_fade, cloud_face_shade, cloud_texture_offset,
    cloud_weather_colour, moon_phase_tile,
};
pub use block_entity::{
    BlockEntityFrameResource, BlockEntitySceneResource, BlockSelectionFrameResource,
    StaticItemPlacementsResource,
};
pub use chunk::api::{
    ChunkUploadAcknowledgement, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken, PresentedFrameAck, PresentedFrameGate, RenderViewCohort,
    TargetRenderExpectation,
};
pub use chunk::biome_tints::{
    BiomeTint, ChunkBiomeTints, MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V,
    MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180, MATERIAL_UV_ROTATE_270,
};
pub use chunk::frame_evidence::{MAX_MODEL_WITNESS_KEYS, MAX_TRANSPARENT_WITNESS_KEYS};
pub use chunk::instance::ChunkRenderInstance;
pub use chunk::metrics::{ModelWorkloadMetrics, TransparentSortMetrics};
pub use chunk::model_witness::{
    ModelWitnessEvent, ModelWitnessEvidence, ModelWitnessFrameAck, ModelWitnessManifestRecord,
    ModelWitnessRequest, ModelWitnessRequestError,
};
pub use chunk::queue::{ChunkRenderQueue, ChunkRenderQueueLimits};
pub use chunk::schedule::ChunkRenderApplySet;
pub use chunk::texture_reload::{ChunkTexturePreparation, ChunkTextureReload};
pub use chunk::textures::{
    AnimationFrameSample, ChunkAnimationClock, ChunkTextureAssetIdentity, ChunkTextureAssets,
    ChunkTextureBindingIdentity, EnhancedTextureAssets, TextureArrayLimits, TextureLimitError,
    TextureMipUploadPlan, TexturePageBinding, TextureUploadPlanError, diagnostic_texture_page,
    greedy_texture_uv, plan_texture_mip_uploads, plan_texture_page_bindings,
    select_animation_frames, texture_asset_needs_rebuild,
};
pub use chunk::transparent_witness::{
    TransparentWitnessEvent, TransparentWitnessEvidence, TransparentWitnessIncompleteEvent,
    TransparentWitnessRequest, TransparentWitnessRequestError, TransparentWitnessStageEvent,
    TransparentWitnessStageRecord,
};
pub use cloud_config::{
    CloudCalibrationError, CloudCalibrationHarness, CloudCalibrationRecord, CloudCalibrationReport,
    CloudCoverageSemantics, CloudGeometryDiagnostic, CloudGeometryDiagnosticError,
    CloudMatchingView, CloudRenderConfig, adjusted_cloud_distance_blocks,
    adjusted_player_render_distance_blocks,
};
pub use cloud_visibility::CloudVisibility;
pub use dropped_item::DroppedItemScene;
pub use enhanced::EnhancedRendering;
pub use entity_shadow::EntityShadowScene;
pub use far_terrain::{FarTerrainResidency, FarTerrainScene};
pub use gpu_times::GpuFrameTimes;
pub use hand_rig::HandRigScene;
pub use lighting::{WorldFullbright, WorldLighting};
mod java_world;
pub use java_world::JavaWorldVisuals;
pub use lightning::{
    BoltRecord, BoltSegment, LIGHTNING_FLASH_SECONDS, LIGHTNING_HEIGHT, LightningScene,
    MAX_BOLT_RECORDS, MAX_LIGHTNING_BOLTS, lightning_bolt_segments, lightning_flash_level,
    push_bolt_records,
};
pub use media_screen::{
    MAX_MEDIA_SCREENS, MediaFrame, MediaScreen, MediaScreenScene, media_screen_axes,
};
pub use mod_scene::ModRenderScene;
pub use model_resources::{ActorArtworkResource, ViewmodelGeometryResource};
pub use nametag::NametagSceneResource;
pub use panorama::PanoramaScene;
pub use particles::{ParticleGpuFrame, ParticleSimulation, particle_view, update_particle_frame};
pub use primitive_shapes::PrimitiveShapesScene;
pub use readiness::PipelineWarmupReadiness;
pub use runtime_profile::{
    RuntimeStage, RuntimeStageProfileSnapshot, RuntimeStageProfiler, RuntimeStageSample,
    RuntimeStageSpans, begin_stage_span, end_stage_span,
};
pub use runtime_profile_slow::{FrameBudgets, SlowFrameCounts};
pub use screen_fire::ScreenFireTexture;
pub use screen_overlay::{
    MAX_SCREEN_OVERLAY_LAYERS, SCREEN_OVERLAY_TEXTURE_SIDE, ScreenOverlayKind, ScreenOverlayLayer,
    ScreenOverlayScene, ScreenOverlayTextures,
};
pub use skybox::SkyboxSettings;
pub use ui::{UiRenderSceneResource, UiRenderStatsResource};
pub use ui_glint::{UiGlintSettings, UiItemGlintTexture};
pub use viewmodel::{
    MAX_VIEWMODEL_DEPTH_BYTES, ViewmodelCompletionGate, ViewmodelMode, ViewmodelScene,
    ViewmodelToken, viewmodel_depth_bytes,
};
pub use visibility_diagnostics::{
    ExtractedViewGenerations, MAX_VISIBILITY_DIAGNOSTIC_KEYS, VisibilityDiagnostics,
    VisibilityDiagnosticsInput,
};
pub use weather::{
    PrecipitationMixResource, PrecipitationScene, RainSplashQueue, WeatherTextureAssets,
};

pub use formats::{SCENE_COLOR_FORMAT, SCENE_HDR_FORMAT};
pub use motion_blur::CameraMotionBlur;

pub use enhanced::MAX_SHADOW_CASCADES;

pub mod native_sunlight;
pub use native_sunlight::AtmosphereViewInputs;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;
