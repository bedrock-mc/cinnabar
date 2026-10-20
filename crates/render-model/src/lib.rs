//! Render data contracts and CPU geometry shared by presentation and the GPU renderer.
//!
//! Engine-free: no Bevy or wgpu types, so presentation compiles without waiting on the renderer.

pub mod actor;
pub mod cape;
mod chunk_metrics;
pub mod dropped_item;
mod entity_shadow;
pub mod equipment;
mod font_atlas;
pub mod font_shading;
mod frame_latency;
mod frame_pacing;
pub mod hats;
pub use font_atlas::{
    FONT_ATLAS_GUTTER, FONT_ATLAS_SIDE, FontAtlas, FontAtlasFrame, FontAtlasVertex, FontRect,
};
mod item_geometry;
pub mod java_animation;
pub mod legacy_combat;
mod nametag;
pub mod panorama;
mod presentation;
pub mod primitive_shapes;
pub mod terrain_units;
mod ui;
mod ui_textures;
mod visibility;
pub mod visibility_bounds;

pub use actor::{
    ACTOR_RIG_VERTEX_WORDS, ActorEmitterTransform, ActorRigGeometry, ActorRigGeometryError,
    ActorRigSurface, ActorRigVertex, ActorSkinPixels, ActorVertex, DEFAULT_PLAYER_SKIN_PATH,
    DEFAULT_SKIN_PROVENANCE, DIAGNOSTIC_RIG_ID, EntityRigId, FrameSlotRecency,
    MAX_ACTOR_CATALOG_VERTEX_BYTES, MAX_ACTOR_CATALOG_VERTICES, MAX_ACTOR_RIG_VERTICES,
    MAX_RENDER_BONES_PER_ACTOR, MAX_RENDERED_PLAYERS, ONE_SIDED_BACK_UV, PLAYER_SKIN_BUDGET_BYTES,
    RenderBoneTransform, SKIN_CLASS_SIDES, STANDARD_BIPED_BONES, STANDARD_BIPED_VERTEX_COUNT,
    STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE, UNIT_AXIS_SCALE, actor_catalog_budget_bytes,
    actor_skin_side, append_entity_cube_vertices, attachable_geometry, attachable_raster_frame,
    default_actor_skin_rgba8, diagnostic_geometry, entity_geometry, equipment_geometry,
    equipment_rig_id, find_geometry_index, fit_rgba_within, geometry_bone_binding_expressions,
    geometry_bone_names, geometry_bone_pivots, geometry_from_geometry_index,
    geometry_from_runtime_assets, install_default_player_skin, is_equipment_rig_id,
    is_layer_geometry_rig_id, is_pack_equipment_rig_id, is_pack_rig_id, item_mesh_rig_id,
    layer_geometries, layer_geometry_rig_id, normalize_actor_skin, pack_equipment_rig_id,
    pack_geometries, pack_rig_id, prepare_actor_skin_cached, resolve_geometry_bones,
    sampled_locator_transform, skin_geometry, skin_rig_id, standard_biped_overlay_vertices,
    standard_biped_vertices,
};
pub use chunk_metrics::{
    ModelWorkloadCount, ModelWorkloadMetricsSnapshot, TransparentSortMetricsSnapshot,
};
pub use dropped_item::{DroppedItemBlock, DroppedItemCube, DroppedItemSprite, OPAQUE_WHITE};
pub use entity_shadow::{
    EntityShadow, EntityShadowFrame, EntityShadowParams, MAX_ENTITY_SHADOWS,
    SHADOW_VOLUME_BOTTOM_RADIUS, SHADOW_VOLUME_BOTTOM_Y, SHADOW_VOLUME_SIDES,
    SHADOW_VOLUME_TOP_RADIUS, SHADOW_VOLUME_TOP_Y, SHADOW_VOLUME_VERTICES, entity_shadow_colour,
    shadow_screen_rect, shadow_volume_mesh, unit_volume_contains,
};
pub use frame_latency::{FrameLatencyPolicy, FrameWorkSample};
pub use frame_pacing::{
    Cadence, FrameRate, OCCLUDED_FRAME_RATE, UNFOCUSED_FRAME_RATE, WindowActivity,
    effective_frame_rate,
};
pub use item_geometry::{extruded_sprite_vertices, held_sprite_vertices, textured_cube_vertices};
pub use nametag::{
    MAX_NAMETAG_RECORDS, NAMETAG_ALPHA_CUTOFF, NAMETAG_ATLAS_SIDE, NAMETAG_BLOCKS_PER_FONT_PIXEL,
    NAMETAG_DIM_TEXT_ALPHA, NAMETAG_LINE_PITCH_PX, NAMETAG_TEXT_REVERSE_Z_BIAS, NametagAtlasRect,
    NametagRecord, NametagScene, java_nametag_line_backgrounds, nametag_line_backgrounds,
    nametag_line_left,
};
pub use panorama::{MAX_PANORAMA_FACE_SIDE, PanoramaFaces, PanoramaView};
pub use presentation::{
    DisplayTiming, PresentModeKind, PresentationIntent, SurfacePresentModes, VrrStatus,
    configured_present_mode, frame_rate_target, initial_present_mode, select_present_mode,
    vrr_ceiling,
};
pub use ui::{
    MAX_UI_BATCHES, MAX_UI_DRAW_BYTES, MAX_UI_FIXED_TEXTURE_BYTES, MAX_UI_INDICES,
    MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, MAX_UI_TEXTURE_SIDE, MAX_UI_VERTICES,
    UI_BLEND_ALPHA, UI_BLEND_BACKDROP, UI_BLEND_FILTER, UI_BLEND_INVERT, UI_STYLE_ADDITIVE,
    UI_STYLE_ALPHA_TEST, UI_STYLE_COLOR_MASK, UI_STYLE_GLINT, UI_STYLE_PARTICLE,
    UI_STYLE_RADIAL_GRADIENT, UiRenderBatch, UiRenderInput, UiRenderReject, UiRenderRejectReason,
    UiRenderScene, UiRenderStats, UiRenderStatsSnapshot, UiRenderTextureArray, UiRenderVertex,
    UiScissor, ui_particle_uv,
};
pub use ui_textures::{
    MAX_UI_ART_PAGES, MAX_UI_DYNAMIC_PAGES, MAX_UI_MODEL_ATLAS_PAGES, MAX_UI_NATIVE_SHEET_BYTES,
    MAX_UI_NATIVE_SHEET_PAGES, MAX_UI_NATIVE_SHEET_SIDE, MAX_UI_NATIVE_TEXTURE_GROWTH_BYTES,
    MAX_UI_TEXTURE_BUCKETS, UI_ART_PAGE_SIDE, UI_DYNAMIC_PAGE_SIDE, UI_LOCAL_FONT_PAGE_OFFSET,
    UI_LOCAL_FONT_PAGE_SIDE, UI_LOCAL_ICON_FONT_PAGE_SIDE, UI_MODEL_ATLAS_PAGE_OFFSET,
    UI_MODEL_ATLAS_SIDE, UI_NATIVE_SHEET_PAGE_OFFSET, UI_PARTICLE_PAGE_OFFSET,
    UI_PLAYER_SKIN_PAGE_OFFSET, UI_SESSION_GLYPH_PAGE_OFFSET, UI_SESSION_GLYPH_PAGE_SIDE,
    UI_SESSION_GLYPH_PAGES, UI_SESSION_ICON_PAGE_OFFSET, UiTextureBucket, UiTextureCatalog,
    UiTextureFormat, UiTextureLocation, UiTexturePage, UiTexturePlan,
};
pub use visibility::{
    ExtractedCameraIdentity, GraphicsAdapterMetadata, OpaqueDrawMode, VisibilityDiagnosticSnapshot,
    VisibilityKeyDelta, VisibilityKeyDigest,
};

/// Enhanced is disabled until the GPU faults and system freezes are resolved.
/// Ordinary settings, launch flags and camera components cannot override this switch.
pub const ENHANCED_RENDERING_ENABLED: bool = false;

/// The Java shader-pack render mode is opt-in at build time while it is experimental.
/// Default builds keep settings, launch flags and cameras off it.
pub const SHADER_PACKS_ENABLED: bool = cfg!(feature = "shaderpacks");

#[cfg(feature = "enhanced-diagnostics")]
static ENHANCED_DIAGNOSTIC: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Maximum offscreen or hidden viewport admitted for development diagnostics.
#[cfg(feature = "enhanced-diagnostics")]
pub const ENHANCED_DIAGNOSTIC_MAX_VIEWPORT: [u32; 2] = [320, 240];

/// Allows an explicitly configured, bounded diagnostic process to exercise the renderer.
/// The caller must establish an offscreen or hidden target before creating render plugins.
#[cfg(feature = "enhanced-diagnostics")]
pub fn enable_enhanced_diagnostics(viewport: [u32; 2]) -> Result<(), &'static str> {
    if viewport
        .into_iter()
        .zip(ENHANCED_DIAGNOSTIC_MAX_VIEWPORT)
        .any(|(size, maximum)| size == 0 || size > maximum)
    {
        return Err("Enhanced diagnostics require a nonzero viewport within the diagnostic limit");
    }
    ENHANCED_DIAGNOSTIC.store(true, std::sync::atomic::Ordering::Release);
    Ok(())
}

/// Reports whether this process explicitly admitted the bounded diagnostic path.
#[cfg(feature = "enhanced-diagnostics")]
#[must_use]
pub fn enhanced_diagnostics_enabled() -> bool {
    ENHANCED_DIAGNOSTIC.load(std::sync::atomic::Ordering::Acquire)
}

/// Resolves the production switch and the separately compiled diagnostic admission.
pub fn enhanced_rendering_enabled() -> bool {
    #[cfg(feature = "enhanced-diagnostics")]
    if enhanced_diagnostics_enabled() {
        return true;
    }
    ENHANCED_RENDERING_ENABLED
}

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;
pub mod block_entity;
pub mod celestial;
pub mod hand;
pub mod lightmap;

pub mod native_trig;
pub mod spawner;
pub mod viewmodel;
pub mod weather;
pub use actor::ActorRigVertexSegments;
pub use actor::{
    ACTOR_BONE_MATRIX_BYTES, ACTOR_GPU_INSTANCE_WORDS, ACTOR_LAYER_BODY, ACTOR_LAYER_FLAME,
    ACTOR_LAYER_SKIN_BASE, ActorDrawManifestEntry, ActorGpuInstance, ActorMaterial,
    ActorRenderIdentity, ActorRigFrameBuilder, ActorRigGeometrySpan, ActorRigRejects,
    ActorRigRenderFrame, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, IDENTITY_UV_ANIM,
    MAX_ACTOR_BONE_ARENA_BYTES, MAX_ACTOR_RENDER_INSTANCES, actor_bounds_are_visible,
    actor_rig_submission_is_visible, pack_actor_light, pack_actor_light_without_lightmap,
    pack_overlay_rgba8,
};
pub use actor::{ACTOR_CANDIDATE_RADIUS_BLOCKS, ActorCullView, MAX_ACTOR_RENDER_DISTANCE_BLOCKS};
pub use actor::{
    ACTOR_COLOR_MASK_MATERIAL_FLAG, ActorGlint, LEGACY_COMBAT_MATERIAL_FLAG, legacy_glint_uv,
};
pub use actor::{
    ActorArtworkLocation, ActorArtworkPageId, ActorArtworkPages, ActorTexturePage, EquipmentRaster,
    MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
};
pub use actor::{ActorSkinResidency, ResidentSkin, pack_skin_slot};
pub use actor::{GlintTexture, ITEM_GLINT_UV_SCALE};
pub use block_entity::BeaconModel;
pub use block_entity::ConduitModel;
pub use block_entity::CrystalBeamModel;
pub use block_entity::DragonDeathModel;
pub use block_entity::ShulkerModel;
pub use block_entity::{
    AtlasRect, BlockEntityAtlas, MAP_CELL, MIN_DYNAMIC_ATLAS_WIDTH, TEXT_CELL, TEXT_SLOT_COUNT,
    TextureRef,
};
pub use block_entity::{
    BLOCK_ENTITY_REACH_BLOCKS, BlockEntityAtlasImage, BlockEntityFrame, BlockEntityKind,
    BlockEntityLight, BlockEntityScene, BlockEntitySubmission, CrackInstance, SceneClock,
};
pub use block_entity::{
    BLOCK_ENTITY_VERTEX_WORDS, BlockEntityVertex, Facing, MAX_BLOCK_ENTITY_VERTICES,
};
pub use block_entity::{
    BLOCK_SELECTION_VERTICES_PER_EDGE, BlockSelectionFrame, BlockSelectionTarget,
};
pub use block_entity::{BannerLayer, BannerModel, BannerMount};
pub use block_entity::{BedModel, bed_color};
pub use block_entity::{BellAttachment, BellModel, BookPose, EnchantingBook, swing_degrees};
pub use block_entity::{BlockEntityArtworkPreparer, PreparedBlockEntityArtwork};
pub use block_entity::{ChestModel, ChestPair, ChestVariant, CopperAge, lid_angle_radians};
pub use block_entity::{
    CrackQuad, CrackShape, CrackShapeCache, crack_shape_for_block, crack_shape_from_template,
    crack_texture_name,
};
pub use block_entity::{DecoratedPotModel, sherd_pattern};
pub use block_entity::{
    ITEM_FRAME_PANEL_FRONT_PIXELS, ITEM_FRAME_ROTATION_STEP_DEGREES, ItemFrameModel,
    item_frame_item_transform,
};
pub use block_entity::{Oxidation, StatueModel, StatuePose};
pub use block_entity::{SignFace, SignModel, SignMount};
pub use block_entity::{SkullKind, SkullModel, SkullMount, floor_yaw_degrees, skull_geometry};
pub use block_entity::{StaticItemPlacement, StaticItemPlacements, matrix_rows};
pub use celestial::{
    NIGHT_SKY_TRANSFER, celestial_angle, day_plateau, daylight, lightmap_sky_darken,
    star_brightness, sun_direction, sunrise_band,
};
pub use dropped_item::ItemMeshVertex;
pub use dropped_item::{
    DroppedItemInstance, DroppedItemModel, MAX_DROPPED_ITEM_INSTANCES, MAX_DYNAMIC_ITEM_VERTICES,
    MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE, MIN_ITEM_ATLAS_SIDE, TerrainItemInstance,
    TerrainItemTransition, WHITE_LAYER, dropped_item_transform,
};
pub use dropped_item::{DroppedItemShape, DroppedItemSpawnPose, native_dropped_item_transform};
pub use dropped_item::{rope_color, rope_point, rope_ribbon};
pub use hand::{
    HAND_ITEM_LAYER_FLAG, HAND_OFFHAND_LAYER_FLAG, HandItemAlphaMode, HandItemAtlas, HandRigLight,
};
pub use lightmap::{LightmapInputs, LightmapUniform, darkness_pulse};

pub use viewmodel::{ViewmodelGeometry, ViewmodelSkin};
pub use weather::{
    ColumnSample, LAYERS_PER_KIND, MAX_PRECIPITATION_LAYERS, OCCLUSION_BLOCKED, OCCLUSION_OPEN,
    OCCLUSION_SIDE, OcclusionGrid, PARTICLE_BOX, PARTICLE_MESH_QUADS, PARTICLE_POOL,
    PRECIPITATION_LEVEL_PER_SECOND, PRECIPITATION_LEVEL_PER_TICK, PRECIPITATION_SAMPLE_OFFSETS,
    PRECIPITATION_TICKS_PER_SECOND, PrecipitationLayerRecord, PrecipitationMix,
    PrecipitationParams, PrecipitationSim, RAIN_PARAMS, SNOW_PARAMS, approach_level,
    average_precipitation, column_heights, particle_mesh, particles_per_layer, pick_rain_splashes,
    precipitation_forward_offset,
};

pub use block_entity::block_matrix;
pub use weather::ColumnSampler;

mod actor_bindings;
pub use actor_bindings::{ActorAttachmentBindings, CapeBoneBinding};
