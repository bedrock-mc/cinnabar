//! Packed chunk meshing and Bevy rendering for the Bedrock client.
mod lighting;
mod lightmap;
pub use lighting::WorldLighting;
pub use lightmap::{LightmapInputs, darkness_pulse};

mod actor;
mod actor_render;
mod atmosphere;
mod atmosphere_render;
mod block_entity;
mod celestial;
mod chunk;
mod cloud_config;
mod cloud_render;
pub use cloud_render::CloudVisibility;
mod dropped_item;
mod enhanced;
pub use enhanced::{EnhancedRenderPlugin, EnhancedRendering, MAX_SHADOW_CASCADES};
mod dropped_item_render;
mod hand_rig_render;
mod item_geometry;
mod lightning;
mod lightning_render;
mod media;
pub use media::MediaTexture;
mod nametag;
mod nametag_render;
mod panorama;
mod panorama_render;
mod particles;
mod present_mode;
mod runtime_profile;
mod runtime_profile_trace;
mod screen_overlay;
mod screen_overlay_render;
mod ui;
mod ui_textures;

pub use ui_textures::{
    MAX_UI_ART_PAGES, MAX_UI_DYNAMIC_PAGES, MAX_UI_MODEL_ATLAS_PAGES, MAX_UI_TEXTURE_BUCKETS,
    UI_ART_PAGE_SIDE, UI_DYNAMIC_PAGE_SIDE, UI_MODEL_ATLAS_PAGE_OFFSET, UI_MODEL_ATLAS_SIDE,
    UI_PLAYER_SKIN_PAGE_OFFSET, UI_SESSION_ICON_PAGE_OFFSET, UiTextureBucket, UiTextureCatalog,
    UiTextureLocation, UiTexturePage, UiTexturePlan,
};
mod ui_render;
mod viewmodel;
mod viewmodel_render;

pub use hand_rig_render::{
    HAND_ITEM_LAYER_FLAG, HAND_OFFHAND_LAYER_FLAG, HandItemAtlas, HandRigLight,
    HandRigRenderPlugin, HandRigScene,
};
pub use particles::{
    ATLAS_SIDE as PARTICLE_ATLAS_SIDE, DrawLists as ParticleDrawLists,
    EmptyWorld as EmptyParticleWorld, Fluid as ParticleFluid, LevelParticle, MAX_LIVE_PARTICLES,
    ParticleGpuFrame, ParticleInstance, ParticleRenderPlugin, ParticleSound, ParticleSystem,
    ParticleView, ParticleWorld, SpawnRequest, TileRequest, block_break_request,
    block_crack_request, classify_level_event, is_particle_level_event, item_icon_request,
    named_request, parse_molang_variables, particle_view, terrain_request, update_particle_frame,
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
    ChunkMesh, PackedBiomeRecord, PackedCloudQuad, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting, mesh_cloud_texture,
};

pub use actor::{
    ACTOR_BONE_MATRIX_BYTES, ACTOR_CANDIDATE_RADIUS_BLOCKS, ACTOR_GPU_INSTANCE_WORDS,
    ACTOR_LAYER_BODY, ActorArtworkLocation, ActorArtworkPages, ActorCullView, ActorDrawFrame,
    ActorDrawManifestEntry, ActorGpuInstance, ActorMainWitness, ActorPresentationGate,
    ActorPresentedFrameAck, ActorRenderFrame, ActorRenderIdentity, ActorRenderInstance,
    ActorRenderScene, ActorRenderSource, ActorRigFrameBuilder, ActorRigGeometry,
    ActorRigGeometryError, ActorRigGeometrySpan, ActorRigRejects, ActorRigRenderFrame,
    ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, ActorRigVertex, ActorRuntimeWitness,
    ActorSkinPixels, ActorTexturePage, ActorVertex, DEFAULT_PLAYER_SKIN_PATH,
    DEFAULT_SKIN_PROVENANCE, EntityRigId, EquipmentRaster, IDENTITY_UV_ANIM,
    MAX_ACTOR_BONE_ARENA_BYTES, MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_PRESENTED_ACKNOWLEDGEMENTS,
    MAX_ACTOR_RENDER_DISTANCE_BLOCKS, MAX_ACTOR_RENDER_INSTANCES, MAX_ACTOR_RIG_VERTICES,
    MAX_ACTOR_TEXTURE_PAGES, MAX_RENDER_BONES_PER_ACTOR, MAX_RENDERED_PLAYERS, RenderBoneTransform,
    STANDARD_BIPED_VERTEX_COUNT, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE, UNIT_AXIS_SCALE,
    actor_bounds_are_visible, actor_rig_submission_is_visible, attachable_geometry,
    default_actor_skin_rgba8, entity_geometry, equipment_geometry, equipment_rig_id,
    extruded_sprite_vertices, find_geometry_index, geometry_bone_names, geometry_bone_pivots,
    held_sprite_vertices, install_default_player_skin, item_mesh_rig_id, layer_geometry_rig_id,
    normalize_actor_skin, normalize_actor_skin_cached, pack_actor_light, pack_equipment_rig_id,
    pack_overlay_rgba8, pack_rig_id, skin_geometry, skin_rig_id, skull_geometry,
    standard_biped_overlay_vertices, standard_biped_vertices, textured_cube_vertices,
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
    AtlasRect, BLOCK_ENTITY_VERTEX_WORDS, BannerLayer, BannerModel, BannerMount, BeaconModel,
    BedModel, BellAttachment, BellModel, BlockEntityAtlas, BlockEntityAtlasImage, BlockEntityFrame,
    BlockEntityKind, BlockEntityRenderPlugin, BlockEntityScene, BlockEntitySubmission,
    BlockEntityVertex, BlockSelectionFrame, BlockSelectionTarget, ChestModel, ChestPair,
    ChestVariant, ConduitModel, CopperAge, CrackInstance, CrackQuad, CrackShape, DecoratedPotModel,
    Facing, ItemFrameModel, MAX_BANNER_LAYERS, MAX_BLOCK_ENTITY_VERTICES, Oxidation, SPAWNER_MOBS,
    SceneClock, ShulkerModel, SignFace, SignModel, SignMount, SkullKind, SkullModel, SkullMount,
    SpawnerModel, StaticItemPlacement, StaticItemPlacements, StatueModel, StatuePose, TEXT_CELL,
    TEXT_SLOT_COUNT, TextureRef, banner_color, bed_color, block_matrix, crack_shape_from_template,
    crack_texture_name, floor_yaw_degrees, item_frame_item_transform, lid_angle_radians,
    matrix_rows, pattern_texture, sherd_pattern, shulker_color_from_block_name, swing_degrees,
};
pub use celestial::{
    NIGHT_SKY_TRANSFER, celestial_angle, day_plateau, daylight, fog_brightness, star_brightness,
    sun_direction, sunrise_band,
};
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
pub use dropped_item::{
    DroppedItemCube, DroppedItemInstance, DroppedItemModel, DroppedItemScene, DroppedItemShape,
    DroppedItemSpawnPose, DroppedItemSprite, ItemMeshVertex, MAX_DROPPED_ITEM_INSTANCES,
    MAX_DYNAMIC_ITEM_VERTICES, MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE, OPAQUE_WHITE, WHITE_LAYER,
    dropped_item_transform, native_dropped_item_transform, rope_color, rope_point, rope_ribbon,
};
pub use dropped_item_render::DroppedItemRenderPlugin;
pub use lightning::{
    BoltRecord, BoltSegment, LIGHTNING_FLASH_SECONDS, LIGHTNING_HEIGHT, LightningScene,
    MAX_BOLT_RECORDS, MAX_LIGHTNING_BOLTS, lightning_bolt_segments, lightning_flash_level,
    push_bolt_records,
};
pub use nametag::{
    MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NAMETAG_BLOCKS_PER_FONT_PIXEL, NametagAtlasRect,
    NametagRecord, NametagScene,
};
pub use panorama::{
    MAX_PANORAMA_FACE_SIDE, PANORAMA_WGSL, PanoramaFaces, PanoramaScene, PanoramaView,
};
pub use panorama_render::PanoramaRenderPlugin;
pub use present_mode::{
    Dx12PresentModePolicy, Dx12PresentModePolicyPlugin, PresentModePreference, PresentModeRemedy,
    resolve_dx12_present_mode_remedy,
};
pub use runtime_profile::{
    RuntimeStage, RuntimeStageProfileSnapshot, RuntimeStageProfiler, RuntimeStageSample,
    RuntimeStageSpans, begin_stage_span, end_stage_span,
};
pub use screen_overlay::{
    MAX_SCREEN_OVERLAY_LAYERS, SCREEN_OVERLAY_TEXTURE_SIDE, ScreenOverlayKind, ScreenOverlayLayer,
    ScreenOverlayScene, ScreenOverlayTextures,
};
pub use screen_overlay_render::ScreenOverlayRenderPlugin;
pub use ui::{
    MAX_UI_BATCHES, MAX_UI_DRAW_BYTES, MAX_UI_INDICES, MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS,
    MAX_UI_TEXTURE_SIDE, MAX_UI_VERTICES, UI_BLEND_ALPHA, UI_BLEND_INVERT, UI_STYLE_ALPHA_TEST,
    UiRenderBatch, UiRenderInput, UiRenderReject, UiRenderRejectReason, UiRenderScene,
    UiRenderStats, UiRenderStatsSnapshot, UiRenderTextureArray, UiRenderVertex, UiScissor,
};
pub use ui_render::{UiGlintSettings, UiRenderPlugin};
pub use visibility_diagnostics::{
    ExtractedCameraIdentity, ExtractedViewGenerations, GraphicsAdapterMetadata,
    MAX_VISIBILITY_DIAGNOSTIC_KEYS, OpaqueDrawMode, VisibilityDiagnosticSnapshot,
    VisibilityDiagnostics, VisibilityDiagnosticsInput, VisibilityKeyDelta, VisibilityKeyDigest,
};
pub use weather::{
    ColumnSample, ColumnSampler, LAYERS_PER_KIND, MAX_PRECIPITATION_LAYERS, OCCLUSION_BLOCKED,
    OCCLUSION_OPEN, OCCLUSION_SIDE, OcclusionGrid, PARTICLE_BOX, PARTICLE_MESH_QUADS,
    PARTICLE_POOL, PRECIPITATION_LEVEL_PER_SECOND, PRECIPITATION_SAMPLE_OFFSETS,
    PRECIPITATION_TICKS_PER_SECOND, Precipitation, PrecipitationLayerRecord, PrecipitationMix,
    PrecipitationParams, PrecipitationScene, PrecipitationSim, RAIN_PARAMS, RainSplashQueue,
    SNOW_PARAMS, WeatherTextureAssets, altitude_adjusted_temperature, approach_level,
    average_precipitation, classify_precipitation, column_heights, particle_mesh,
    particles_per_layer, pick_rain_splashes, precipitation_forward_offset,
};

mod opaque_phase;
pub(crate) use opaque_phase::install_opaque_phase_reset;
mod stars;
