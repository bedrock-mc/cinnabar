//! Actor geometry on the CPU: skins, the standard biped, rig contracts and entity-catalog meshes.
mod asset_geometry;
mod biped;
pub mod flame;
mod geometry;
mod ids;
mod locator;
mod rig;
pub use locator::{ActorEmitterTransform, sampled_bone_offset, sampled_locator_transform};
mod skin;
mod skin_poly_mesh;
pub mod skin_portrait;
mod surface;
mod texel_fit;
mod texture_mesh;

pub use asset_geometry::{
    entity_geometry, equipment_geometry, find_geometry_index, geometry_bone_binding_expressions,
    geometry_bone_names, geometry_bone_pivots, geometry_from_geometry_index,
    geometry_from_runtime_assets, pack_geometries, resolve_geometry_bones, skin_geometry,
};
pub use biped::{
    ActorVertex, STANDARD_BIPED_BONES, STANDARD_BIPED_VERTEX_COUNT,
    standard_biped_overlay_vertices, standard_biped_vertices,
};
pub use geometry::{ONE_SIDED_BACK_UV, append_entity_cube_vertices};
pub use ids::{
    DIAGNOSTIC_RIG_ID, equipment_rig_id, is_equipment_rig_id, is_layer_geometry_rig_id,
    is_pack_equipment_rig_id, is_pack_rig_id, item_mesh_rig_id, layer_geometries,
    layer_geometry_rig_id, pack_equipment_rig_id, pack_rig_id, skin_rig_id,
};
pub use rig::{
    ACTOR_RIG_VERTEX_WORDS, ActorRigGeometry, ActorRigGeometryError, ActorRigVertex, EntityRigId,
    MAX_ACTOR_CATALOG_VERTEX_BYTES, MAX_ACTOR_CATALOG_VERTICES, MAX_ACTOR_RIG_VERTICES,
    MAX_RENDER_BONES_PER_ACTOR, RenderBoneTransform, UNIT_AXIS_SCALE, actor_catalog_budget_bytes,
    diagnostic_geometry,
};
pub use skin::{
    ActorSkinPixels, DEFAULT_PLAYER_SKIN_PATH, DEFAULT_SKIN_PROVENANCE, MAX_RENDERED_PLAYERS,
    PLAYER_SKIN_BUDGET_BYTES, SKIN_CLASS_SIDES, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE,
    actor_skin_side, default_actor_skin_rgba8, install_default_player_skin, normalize_actor_skin,
    prepare_actor_skin_cached,
};
pub use surface::ActorRigSurface;
pub use texel_fit::fit_rgba_within;
pub use texture_mesh::{attachable_geometry, attachable_raster_frame};

mod slot_recency;
pub use slot_recency::FrameSlotRecency;

mod artwork;
mod cull;
pub mod frame;
mod glint;
pub mod skin_slots;
pub use artwork::{
    ActorArtworkLocation, ActorArtworkPageId, ActorArtworkPages, ActorTexturePage, EquipmentRaster,
    MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
};
pub use cull::{ACTOR_CANDIDATE_RADIUS_BLOCKS, ActorCullView, MAX_ACTOR_RENDER_DISTANCE_BLOCKS};

pub use glint::{
    ACTOR_COLOR_MASK_MATERIAL_FLAG, ActorGlint, GlintTexture, ITEM_GLINT_UV_SCALE,
    LEGACY_COMBAT_MATERIAL_FLAG, legacy_glint_uv,
};

pub use frame::{
    ACTOR_BONE_MATRIX_BYTES, ACTOR_GPU_INSTANCE_WORDS, ACTOR_LAYER_BODY, ACTOR_LAYER_FLAME,
    ACTOR_LAYER_SKIN_BASE, ActorDrawManifestEntry, ActorGpuInstance, ActorMaterial,
    ActorRenderIdentity, ActorRigFrameBuilder, ActorRigGeometrySpan, ActorRigRejects,
    ActorRigRenderFrame, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
    ActorRigVertexSegments, IDENTITY_UV_ANIM, MAX_ACTOR_BONE_ARENA_BYTES, MAX_ACTOR_POSE_BONES,
    MAX_ACTOR_RENDER_INSTANCES, actor_bounds_are_visible, actor_rig_submission_is_visible,
    pack_actor_light, pack_actor_light_without_lightmap, pack_overlay_rgba8,
};

pub use skin_slots::{ActorSkinResidency, ResidentSkin, pack_skin_slot};
