//! Packed chunk meshing and Bevy rendering for the Bedrock client.
#[cfg(test)]
#[path = "../tests/it/support/gpu_snapshot.rs"]
mod gpu_snapshot;
mod lighting;
mod lightmap;
#[cfg(test)]
mod shader_test_support;
pub use lighting::{WorldFullbright, WorldLighting};
pub use lightmap::{LightmapInputs, darkness_pulse};
pub use render_api::fancy_actor_shade;

mod aim_assist;
pub use aim_assist::{
    AIM_ASSIST_TEXTURES, AimAssistHighlight, AimAssistHighlightPlugin, AimAssistHighlightScene,
    AimAssistTexture,
};
mod actor;
mod actor_render;
#[cfg(test)]
mod alloc_count;
mod atmosphere;
mod atmosphere_render;
mod block_entity;
mod celestial;
mod chunk;
mod cloud_config;
mod cloud_render;
mod device_poll;
pub use cloud_render::CloudVisibility;
mod dropped_item;
mod enhanced;
mod entity_shadow_render;
pub use enhanced::{EnhancedRenderPlugin, EnhancedRendering, MAX_SHADOW_CASCADES};
pub use entity_shadow_render::{EntityShadowRenderPlugin, EntityShadowScene};
mod gpu_timing;
pub use gpu_timing::{GpuFrameTimes, GpuTimingPlugin};

mod dropped_item_render;
mod hand_rig_render;
mod lightning;
mod lightning_render;
mod media;
pub use media::MediaTexture;
mod media_screen;
pub use media_screen::{
    MAX_MEDIA_SCREENS, MediaFrame, MediaScreen, MediaScreenScene, media_screen_axes,
};
mod material_shader;
mod mod_render;
pub use mod_render::{MAX_BLOCK_HIGHLIGHTS, ModPassLabel, ModRenderPlugin, ModRenderScene};
pub mod motion_blur;
mod nametag_render;
pub use nametag_render::NametagSceneResource;
mod native_sunlight;
mod native_trig;
pub use native_sunlight::AtmosphereViewInputs;
mod panorama;
mod panorama_render;
mod particle_render;
mod pipeline_warmup;
#[cfg(test)]
#[path = "pipeline_warmup/app_tests.rs"]
mod pipeline_warmup_app_tests;
pub use pipeline_warmup::PipelineWarmupReadiness;
mod present_mode;
mod primitive_shapes;
pub use primitive_shapes::{PrimitiveShapesRenderPlugin, PrimitiveShapesScene};
mod runtime_profile;
mod runtime_profile_slow;
mod runtime_profile_trace;
mod scene_sampling;
mod scene_target;
mod screen_fire;
mod screen_overlay;
mod screen_overlay_portal;
mod screen_overlay_render;
mod shader_safety;
#[cfg(test)]
#[path = "../tests/it/support/shader_source.rs"]
mod shader_source;
mod surface_lifecycle;
mod ui_render;
#[cfg(all(test, target_os = "macos"))]
mod upload_allocation_tests;
mod upload_staging;
mod viewmodel;
mod viewmodel_render;

pub use hand_rig_render::{
    HAND_ITEM_LAYER_FLAG, HAND_OFFHAND_LAYER_FLAG, HandItemAlphaMode, HandItemAtlas, HandRigLight,
    HandRigRenderPlugin, HandRigScene,
};
pub use particle_render::{
    ParticleGpuFrame, ParticleRenderPlugin, ParticleSimulation, particle_view,
    update_particle_frame,
};
pub use viewmodel::{
    MAX_VIEWMODEL_DEPTH_BYTES, ViewmodelCompletionGate, ViewmodelGeometry, ViewmodelMode,
    ViewmodelScene, ViewmodelSkin, ViewmodelToken, viewmodel_depth_bytes,
};
pub use viewmodel_render::ViewmodelRenderPlugin;
mod visibility_diagnostics;
mod weather;
mod weather_render;

use meshing::{
    ChunkMesh, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad,
    PackedQuadLighting,
};

pub use actor::{
    ACTOR_BONE_MATRIX_BYTES, ACTOR_CANDIDATE_RADIUS_BLOCKS, ACTOR_GPU_INSTANCE_WORDS,
    ACTOR_LAYER_BODY, ActorArtworkLocation, ActorArtworkPageId, ActorArtworkPages, ActorCullView,
    ActorDrawFrame, ActorDrawManifestEntry, ActorGlint, ActorGpuInstance, ActorMainWitness,
    ActorMaterial, ActorPipelineReadiness, ActorPresentationGate, ActorPresentedFrameAck,
    ActorRenderFrame, ActorRenderIdentity, ActorRenderInstance, ActorRenderScene,
    ActorRenderSource, ActorRigFrameBuilder, ActorRigGeometrySpan, ActorRigRejects,
    ActorRigRenderFrame, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
    ActorRuntimeWitness, ActorSkinResidency, ActorTexturePage, EquipmentRaster, IDENTITY_UV_ANIM,
    MAX_ACTOR_BONE_ARENA_BYTES, MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS,
    MAX_ACTOR_RENDER_DISTANCE_BLOCKS, MAX_ACTOR_RENDER_INSTANCES, MAX_ACTOR_TEXTURE_PAGES,
    ResidentSkin, actor_bounds_are_visible, actor_rig_submission_is_visible, pack_actor_light,
    pack_overlay_rgba8, pack_skin_slot,
};
pub use actor_render::ActorRenderPlugin;
pub use atmosphere::{
    AtmosphereFrame, AtmosphereTextureAssets, BEDROCK_DAY_TICKS, CLOUD_ALPHA,
    CLOUD_SCROLL_BLOCKS_PER_TICK, CLOUD_TEXTURE_WORLD_PERIOD, MoonPhaseTile,
    PROVISIONAL_BOSS_DARKEN_SKY_STRENGTH, PROVISIONAL_BOSS_WORLD_FOG_END_BLOCKS,
    PROVISIONAL_BOSS_WORLD_FOG_START_BLOCKS, SkyKind, cloud_colour, cloud_distance_fade,
    cloud_face_shade, cloud_texture_offset, cloud_weather_colour, moon_phase_tile,
};
pub use atmosphere_render::AtmospherePlugin;
pub use block_entity::{
    AtlasRect, BLOCK_ENTITY_VERTEX_WORDS, BLOCK_SELECTION_VERTICES_PER_EDGE, BannerLayer,
    BannerModel, BannerMount, BeaconModel, BedModel, BellAttachment, BellModel, BlockEntityAtlas,
    BlockEntityAtlasImage, BlockEntityFrame, BlockEntityKind, BlockEntityLight,
    BlockEntityRenderPlugin, BlockEntityScene, BlockEntitySubmission, BlockEntityVertex,
    BlockSelectionFrame, BlockSelectionTarget, ChestModel, ChestPair, ChestVariant, ConduitModel,
    CopperAge, CrackInstance, CrackQuad, CrackShape, CrystalBeamModel, DRAGON_DEATH_BLEND,
    DecoratedPotModel, DragonDeathModel, Facing, ItemFrameModel, MAX_BANNER_LAYERS,
    MAX_BLOCK_ENTITY_VERTICES, Oxidation, SPAWNER_MOBS, SceneClock, ShulkerModel, SignFace,
    SignModel, SignMount, SkullKind, SkullModel, SkullMount, SpawnerModel, StaticItemPlacement,
    StaticItemPlacements, StatueModel, StatuePose, TEXT_CELL, TEXT_SLOT_COUNT, TextureRef,
    bed_color, block_matrix, crack_shape_from_template, crack_texture_name, floor_yaw_degrees,
    item_frame_item_transform, lid_angle_radians, matrix_rows, pattern_texture, sherd_pattern,
    skull_geometry, swing_degrees,
};
pub use celestial::{
    NIGHT_SKY_TRANSFER, celestial_angle, day_plateau, daylight, lightmap_sky_darken,
    star_brightness, sun_direction, sunrise_band,
};
pub use chunk::required_vertex_storage_buffers;
pub use chunk::{
    AnimationFrameSample, BiomeTint, ChunkAnimationClock, ChunkBiomeTints, ChunkRenderApplySet,
    ChunkRenderInstance, ChunkRenderPlugin, ChunkRenderQueue, ChunkRenderQueueLimits,
    ChunkTextureAssetIdentity, ChunkTextureAssets, ChunkTextureReload, ChunkTextureUploadStats,
    ChunkUploadAcknowledgement, ChunkUploadAcknowledgements, ChunkUploadBudget,
    ChunkUploadPriority, ChunkUploadToken, DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME,
    MATERIAL_UV_REFLECT_U, MATERIAL_UV_REFLECT_V, MATERIAL_UV_ROTATE_90, MATERIAL_UV_ROTATE_180,
    MATERIAL_UV_ROTATE_270, MAX_MODEL_WITNESS_KEYS, MAX_TRANSPARENT_DRAW_REFS,
    MAX_TRANSPARENT_VIEWS, MAX_TRANSPARENT_WITNESS_KEYS, ModelWitnessEvent, ModelWitnessEvidence,
    ModelWitnessFrameAck, ModelWitnessManifestRecord, ModelWitnessRequest,
    ModelWitnessRequestError, ModelWorkloadMetrics, PackedTransparentDrawRef, PresentedFrameAck,
    PresentedFrameGate, RenderViewCohort, TRANSPARENT_REF_BUFFER_BYTES, TRANSPARENT_REF_SLOT_BYTES,
    TargetRenderExpectation, TextureArrayLimits, TextureLimitError, TextureMipUploadPlan,
    TexturePageBinding, TextureUploadPlanError, TransparentAllocationIdentity, TransparentDrawArgs,
    TransparentOrderedSnapshot, TransparentSortError, TransparentSortJobGate,
    TransparentSortMetrics, TransparentSortResult, TransparentSortState, TransparentUploadBatch,
    TransparentWitnessEvent, TransparentWitnessEvidence, TransparentWitnessIncompleteEvent,
    TransparentWitnessRequest, TransparentWitnessRequestError, TransparentWitnessStageEvent,
    TransparentWitnessStageRecord, ViewSortGeneration, ViewSortKey, diagnostic_texture_page,
    greedy_texture_uv, plan_texture_mip_uploads, plan_texture_page_bindings,
    select_animation_frames, texture_asset_needs_rebuild, validate_transparent_sort_ref_count,
};
#[cfg(feature = "publication-test-support")]
pub use chunk::{
    PublicationRenderTerminalSnapshot, publication_noop_render_plugin,
    publication_render_terminal_snapshot, settle_publication_noop_frame,
};
pub use cloud_config::{
    CloudCalibrationError, CloudCalibrationHarness, CloudCalibrationRecord, CloudCalibrationReport,
    CloudCoverageSemantics, CloudGeometryDiagnostic, CloudGeometryDiagnosticError,
    CloudMatchingView, CloudQuality, CloudRenderConfig, adjusted_cloud_distance_blocks,
    adjusted_player_render_distance_blocks,
};
pub use dropped_item::{
    DroppedItemInstance, DroppedItemModel, DroppedItemScene, DroppedItemShape,
    DroppedItemSpawnPose, ItemMeshVertex, MAX_DROPPED_ITEM_INSTANCES, MAX_DYNAMIC_ITEM_VERTICES,
    MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE, TerrainItemInstance, TerrainItemTransition, WHITE_LAYER,
    dropped_item_transform, native_dropped_item_transform, rope_color, rope_point, rope_ribbon,
};
pub use dropped_item_render::DroppedItemRenderPlugin;
pub use lightning::{
    BoltRecord, BoltSegment, LIGHTNING_FLASH_SECONDS, LIGHTNING_HEIGHT, LightningScene,
    MAX_BOLT_RECORDS, MAX_LIGHTNING_BOLTS, lightning_bolt_segments, lightning_flash_level,
    push_bolt_records,
};
pub use panorama::{PANORAMA_WGSL, PanoramaScene};
pub use panorama_render::PanoramaRenderPlugin;
pub use present_mode::{
    Dx12PresentModePolicy, Dx12PresentModePolicyPlugin, PresentModePreference, PresentModeRemedy,
    resolve_dx12_present_mode_remedy,
};
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
pub use screen_overlay_render::ScreenOverlayRenderPlugin;
pub use ui_render::{
    UiGlintSettings, UiRenderPlugin, UiRenderSceneResource, UiRenderStatsResource,
};
pub use visibility_diagnostics::{
    ExtractedViewGenerations, MAX_VISIBILITY_DIAGNOSTIC_KEYS, VisibilityDiagnostics,
    VisibilityDiagnosticsInput,
};
pub use weather::{
    ColumnSample, ColumnSampler, LAYERS_PER_KIND, MAX_PRECIPITATION_LAYERS, OCCLUSION_BLOCKED,
    OCCLUSION_OPEN, OCCLUSION_SIDE, OcclusionGrid, PARTICLE_BOX, PARTICLE_MESH_QUADS,
    PARTICLE_POOL, PRECIPITATION_LEVEL_PER_SECOND, PRECIPITATION_LEVEL_PER_TICK,
    PRECIPITATION_SAMPLE_OFFSETS, PRECIPITATION_TICKS_PER_SECOND, Precipitation,
    PrecipitationLayerRecord, PrecipitationMix, PrecipitationParams, PrecipitationScene,
    PrecipitationSim, RAIN_PARAMS, RainSplashQueue, SNOW_PARAMS, WeatherTextureAssets,
    altitude_adjusted_temperature, approach_level, average_precipitation, classify_precipitation,
    column_heights, particle_mesh, particles_per_layer, pick_rain_splashes,
    precipitation_forward_offset,
};

mod opaque_phase;
pub(crate) use opaque_phase::install_opaque_phase_reset;
mod stars;

#[cfg(test)]
mod queue_review_support;
