//! Render data contracts and CPU geometry shared by presentation and the GPU renderer.
//!
//! Engine-free: no Bevy or wgpu types, so presentation compiles without waiting on the renderer.

pub mod actor;
mod chunk_metrics;
mod dropped_item;
mod entity_shadow;
pub mod equipment;
mod frame_pacing;
mod item_geometry;
pub mod java_animation;
mod nametag;
mod panorama;
mod presentation;
pub mod primitive_shapes;
mod ui;
mod ui_textures;
mod visibility;

pub use actor::{
    ACTOR_RIG_VERTEX_WORDS, ActorRigGeometry, ActorRigGeometryError, ActorRigSurface,
    ActorRigVertex, ActorSkinPixels, ActorVertex, DEFAULT_PLAYER_SKIN_PATH,
    DEFAULT_SKIN_PROVENANCE, DIAGNOSTIC_RIG_ID, EntityRigId, FrameSlotRecency,
    MAX_ACTOR_CATALOG_VERTEX_BYTES, MAX_ACTOR_CATALOG_VERTICES, MAX_ACTOR_RIG_VERTICES,
    MAX_RENDER_BONES_PER_ACTOR, MAX_RENDERED_PLAYERS, ONE_SIDED_BACK_UV, PLAYER_SKIN_BUDGET_BYTES,
    RenderBoneTransform, SKIN_CLASS_SIDES, STANDARD_BIPED_VERTEX_COUNT, STANDARD_SKIN_BYTES,
    STANDARD_SKIN_SIDE, UNIT_AXIS_SCALE, actor_skin_side, append_entity_cube_vertices,
    attachable_geometry, attachable_raster_frame, default_actor_skin_rgba8, diagnostic_geometry,
    entity_geometry, equipment_geometry, equipment_rig_id, find_geometry_index,
    geometry_bone_binding_expressions, geometry_bone_names, geometry_bone_pivots,
    geometry_from_geometry_index, geometry_from_runtime_assets, install_default_player_skin,
    is_equipment_rig_id, is_layer_geometry_rig_id, is_pack_equipment_rig_id, is_pack_rig_id,
    item_mesh_rig_id, layer_geometries, layer_geometry_rig_id, normalize_actor_skin,
    pack_equipment_rig_id, pack_geometries, pack_rig_id, prepare_actor_skin_cached,
    resolve_geometry_bones, skin_geometry, skin_rig_id, standard_biped_overlay_vertices,
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
pub use frame_pacing::{
    Cadence, FrameRate, OCCLUDED_FRAME_RATE, UNFOCUSED_FRAME_RATE, WindowActivity,
    effective_frame_rate,
};
pub use item_geometry::{extruded_sprite_vertices, held_sprite_vertices, textured_cube_vertices};
pub use nametag::{
    MAX_NAMETAG_RECORDS, NAMETAG_ACOS_CUBIC, NAMETAG_ACOS_LINEAR, NAMETAG_ATLAS_SIDE,
    NAMETAG_BLOCKS_PER_FONT_PIXEL, NAMETAG_HORIZONTAL_ZERO, NAMETAG_TEXT_REVERSE_Z_BIAS,
    NametagAtlasRect, NametagRecord, NametagScene,
};
pub use panorama::{MAX_PANORAMA_FACE_SIDE, PanoramaFaces, PanoramaView};
pub use presentation::{
    DisplayTiming, PROVISIONAL_REFRESH, PresentModeKind, PresentationIntent, SurfacePresentModes,
    VrrStatus, configured_present_mode, frame_rate_target, initial_present_mode, outpaces_display,
    select_present_mode, vrr_ceiling,
};
pub use ui::{
    MAX_UI_BATCHES, MAX_UI_DRAW_BYTES, MAX_UI_FIXED_TEXTURE_BYTES, MAX_UI_INDICES,
    MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, MAX_UI_TEXTURE_SIDE, MAX_UI_VERTICES,
    UI_BLEND_ALPHA, UI_BLEND_INVERT, UI_STYLE_ALPHA_TEST, UI_STYLE_COLOR_MASK, UI_STYLE_GLINT,
    UiRenderBatch, UiRenderInput, UiRenderReject, UiRenderRejectReason, UiRenderScene,
    UiRenderStats, UiRenderStatsSnapshot, UiRenderTextureArray, UiRenderVertex, UiScissor,
};
pub use ui_textures::{
    MAX_UI_ART_PAGES, MAX_UI_DYNAMIC_PAGES, MAX_UI_FALLBACK_FONT_PAGES, MAX_UI_MODEL_ATLAS_PAGES,
    MAX_UI_NATIVE_TEXTURE_GROWTH_BYTES, MAX_UI_TEXTURE_BUCKETS, UI_ART_PAGE_SIDE,
    UI_DYNAMIC_PAGE_SIDE, UI_FALLBACK_FONT_PAGE_OFFSET, UI_FALLBACK_FONT_PAGE_SIDE,
    UI_LOCAL_FONT_PAGE_OFFSET, UI_LOCAL_FONT_PAGE_SIDE, UI_MODEL_ATLAS_PAGE_OFFSET,
    UI_MODEL_ATLAS_SIDE, UI_PLAYER_SKIN_PAGE_OFFSET, UI_SESSION_ICON_PAGE_OFFSET, UiTextureBucket,
    UiTextureCatalog, UiTextureFormat, UiTextureLocation, UiTexturePage, UiTexturePlan,
};
pub use visibility::{
    ExtractedCameraIdentity, GraphicsAdapterMetadata, OpaqueDrawMode, VisibilityDiagnosticSnapshot,
    VisibilityKeyDelta, VisibilityKeyDigest,
};

/// Enhanced is disabled until the GPU faults and system freezes are resolved.
/// Settings, launch flags and camera components cannot override this switch.
pub const ENHANCED_RENDERING_ENABLED: bool = false;
