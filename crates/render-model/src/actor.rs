//! Actor geometry on the CPU: skins, the standard biped, rig contracts and entity-catalog meshes.
mod asset_geometry;
mod biped;
mod geometry;
mod ids;
mod rig;
mod skin;
mod skin_poly_mesh;
mod surface;
mod texture_mesh;

pub use asset_geometry::{
    entity_geometry, equipment_geometry, find_geometry_index, geometry_bone_binding_expressions,
    geometry_bone_names, geometry_bone_pivots, geometry_from_geometry_index,
    geometry_from_runtime_assets, pack_geometries, resolve_geometry_bones, skin_geometry,
};
pub use biped::{
    ActorVertex, STANDARD_BIPED_VERTEX_COUNT, standard_biped_overlay_vertices,
    standard_biped_vertices,
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
    MAX_RENDER_BONES_PER_ACTOR, RenderBoneTransform, UNIT_AXIS_SCALE, diagnostic_geometry,
};
pub use skin::{
    ActorSkinPixels, DEFAULT_PLAYER_SKIN_PATH, DEFAULT_SKIN_PROVENANCE, MAX_RENDERED_PLAYERS,
    STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE, default_actor_skin_rgba8, install_default_player_skin,
    normalize_actor_skin, normalize_actor_skin_cached,
};
pub use surface::ActorRigSurface;
pub use texture_mesh::{attachable_geometry, attachable_raster_frame};
