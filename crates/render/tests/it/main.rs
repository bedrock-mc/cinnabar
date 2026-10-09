#[path = "../../src/chunk/constants.rs"]
#[allow(dead_code, reason = "reuse the production quad index order")]
mod chunk_constants;
#[path = "support/gpu_snapshot.rs"]
mod gpu_snapshot;
#[path = "../../src/material_shader.rs"]
#[allow(dead_code, reason = "shared production shader substitutions")]
mod material_shader;
#[path = "../../src/nametag_render/shader.rs"]
mod nametag_shader;
#[path = "../../src/shader_safety.rs"]
#[allow(dead_code, reason = "shared checked shader constructors")]
mod shader_safety;
#[path = "support/shader_source.rs"]
mod shader_source;
#[path = "../../src/ui_render/shader.rs"]
mod ui_shader;

mod actor_colour;
mod actor_rig;
mod actor_sidedness;
mod actor_skin_classes;
mod atmosphere;
#[cfg(feature = "enhanced-diagnostics")]
mod bamboo_enhanced_scene;
mod bamboo_overlay_depth;
mod bamboo_shader;
mod bamboo_texture_filtering;
mod biome_shader;
mod biome_tint_bounds;
mod block_selection;
mod block_selection_native;
mod block_selection_snapshot;
mod camera_fire;
mod cloud_config;
mod cloud_render;
mod cube_tint_raster;
mod dragon_death_rays;
mod end_sky;
mod entity_shadow;
mod gpu_culling;
mod hand_lighting;
mod item_particle_lighting;
mod leaf_colour;
mod leaf_metadata;
mod leaf_shader;
mod leaf_texture_filtering;
mod leaf_uv;
mod lightmap;
mod lily_pad;
mod liquid_geometry;
mod liquid_raster;
mod liquid_shader;
mod material_variations;
mod mod_render;
mod model_alpha;
mod model_tint_raster;
mod model_uv_raster;
mod native_sky;
mod oreui_font;
mod plugin;
mod portal_overlay;
mod present_mode_policy;
mod shaders;
mod skull_lighting;
mod solid_terrain_raster;
mod star_rotation;
mod terrain_lightmap;
mod terrain_seams;
mod ui_textures;
mod water_material;
mod world_model_colour;
