//! Render data contracts and CPU geometry shared by presentation and the GPU renderer.
//!
//! Engine-free: no Bevy or wgpu types, so presentation compiles without waiting on the renderer.

pub mod actor;
mod chunk_metrics;
mod dropped_item;
pub mod equipment;
mod item_geometry;
mod nametag;
mod panorama;
mod ui;
mod ui_textures;
mod visibility;

pub use actor::{
    ActorRigGeometry, ActorRigGeometryError, ActorRigVertex, ActorSkinPixels, ActorVertex,
    DEFAULT_PLAYER_SKIN_PATH, DEFAULT_SKIN_PROVENANCE, DIAGNOSTIC_RIG_ID, EntityRigId,
    MAX_ACTOR_RIG_VERTICES, MAX_RENDER_BONES_PER_ACTOR, MAX_RENDERED_PLAYERS, ONE_SIDED_BACK_UV,
    RenderBoneTransform, STANDARD_BIPED_VERTEX_COUNT, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE,
    UNIT_AXIS_SCALE, append_entity_cube_vertices, attachable_geometry, default_actor_skin_rgba8,
    diagnostic_geometry, entity_geometry, equipment_geometry, equipment_rig_id,
    find_geometry_index, geometry_bone_names, geometry_bone_pivots, geometry_from_geometry_index,
    geometry_from_runtime_assets, install_default_player_skin, is_equipment_rig_id,
    is_layer_geometry_rig_id, is_pack_equipment_rig_id, is_pack_rig_id, item_mesh_rig_id,
    layer_geometries, layer_geometry_rig_id, normalize_actor_skin, normalize_actor_skin_cached,
    pack_equipment_rig_id, pack_geometries, pack_rig_id, skin_geometry, skin_rig_id,
    standard_biped_overlay_vertices, standard_biped_vertices,
};
pub use chunk_metrics::{
    ModelWorkloadCount, ModelWorkloadMetricsSnapshot, TransparentSortMetricsSnapshot,
};
pub use dropped_item::{DroppedItemCube, DroppedItemSprite, OPAQUE_WHITE};
pub use item_geometry::{extruded_sprite_vertices, held_sprite_vertices, textured_cube_vertices};
pub use nametag::{
    MAX_NAMETAG_RECORDS, NAMETAG_ACOS_CUBIC, NAMETAG_ACOS_LINEAR, NAMETAG_ATLAS_SIDE,
    NAMETAG_BLOCKS_PER_FONT_PIXEL, NAMETAG_HORIZONTAL_ZERO, NAMETAG_TEXT_REVERSE_Z_BIAS,
    NametagAtlasRect, NametagRecord, NametagScene,
};
pub use panorama::{MAX_PANORAMA_FACE_SIDE, PanoramaFaces, PanoramaView};
pub use ui::{
    MAX_UI_BATCHES, MAX_UI_DRAW_BYTES, MAX_UI_INDICES, MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS,
    MAX_UI_TEXTURE_SIDE, MAX_UI_VERTICES, UI_BLEND_ALPHA, UI_BLEND_INVERT, UI_STYLE_ALPHA_TEST,
    UiRenderBatch, UiRenderInput, UiRenderReject, UiRenderRejectReason, UiRenderScene,
    UiRenderStats, UiRenderStatsSnapshot, UiRenderTextureArray, UiRenderVertex, UiScissor,
};
pub use ui_textures::{
    MAX_UI_ART_PAGES, MAX_UI_DYNAMIC_PAGES, MAX_UI_MODEL_ATLAS_PAGES, MAX_UI_TEXTURE_BUCKETS,
    UI_ART_PAGE_SIDE, UI_DYNAMIC_PAGE_SIDE, UI_LOCAL_FONT_PAGE_OFFSET, UI_LOCAL_FONT_PAGE_SIDE,
    UI_MODEL_ATLAS_PAGE_OFFSET, UI_MODEL_ATLAS_SIDE, UI_PLAYER_SKIN_PAGE_OFFSET,
    UI_SESSION_ICON_PAGE_OFFSET, UiTextureBucket, UiTextureCatalog, UiTextureLocation,
    UiTexturePage, UiTexturePlan,
};
pub use visibility::{
    ExtractedCameraIdentity, GraphicsAdapterMetadata, OpaqueDrawMode, VisibilityDiagnosticSnapshot,
    VisibilityKeyDelta, VisibilityKeyDigest,
};

/// Enhanced is disabled until the GPU faults and system freezes are resolved.
/// Settings, launch flags and camera components cannot override this switch.
pub const ENHANCED_RENDERING_ENABLED: bool = false;
