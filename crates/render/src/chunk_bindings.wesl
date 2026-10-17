// Group 0 of the packed chunk layout, shared by the liquid and model programs so one composed
// shader can draw both without binding the same slot twice.
import bevy_render::view::View;

struct ChunkOrigin { value: vec4<i32>, stream_bases: vec4<u32> }
// ANIMATION_GPU_LAYOUT
// Shaders read a ref's record; the record names its own origin slot, which `metadata_index` mirrors
// for the CPU-side sort.
struct TransparentDrawRef { liquid_record_index: u32, metadata_index: u32 }
struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>, moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>, sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>, fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>, sky_extra: vec4<f32>,
    liquid_distance: vec4<f32>,
}
// LIQUID_GEOMETRY_CONSTANTS

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> cube_quads: array<u32>;
@group(0) @binding(2) var<storage, read> chunk_origins: array<ChunkOrigin>;
@group(0) @binding(4) var block_textures_page_0: texture_2d_array<f32>;
@group(0) @binding(5) var block_textures_page_1: texture_2d_array<f32>;
@group(0) @binding(19) var block_textures_page_2: texture_2d_array<f32>;
@group(0) @binding(20) var block_textures_page_3: texture_2d_array<f32>;
@group(0) @binding(21) var block_textures_page_4: texture_2d_array<f32>;
@group(0) @binding(6) var block_sampler: sampler;
@group(0) @binding(NATIVE_LEAF_TEXTURE_BINDING_0) var terrain_gamma_page_0: texture_2d_array<f32>;
@group(0) @binding(NATIVE_LEAF_TEXTURE_BINDING_1) var terrain_gamma_page_1: texture_2d_array<f32>;
@group(0) @binding(22) var terrain_gamma_page_2: texture_2d_array<f32>;
@group(0) @binding(23) var terrain_gamma_page_3: texture_2d_array<f32>;
@group(0) @binding(24) var terrain_gamma_page_4: texture_2d_array<f32>;
@group(0) @binding(NATIVE_LEAF_SAMPLER_BINDING) var model_sampler: sampler;
@group(0) @binding(9) var<storage, read> animations: array<AnimationGpu>;
@group(0) @binding(10) var<storage, read> animation_frames: array<u32>;
@group(0) @binding(12) var<storage, read> model_templates: array<u32>;
@group(0) @binding(13) var<storage, read> geometry_streams: array<u32>;
@group(0) @binding(14) var<storage, read> transparent_refs: array<TransparentDrawRef>;
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;

@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_COLOR_TEXTURE_BINDING_0) var enhanced_color_page_0: texture_2d_array<f32>;
@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_COLOR_TEXTURE_BINDING_1) var enhanced_color_page_1: texture_2d_array<f32>;
@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_NORMAL_TEXTURE_BINDING_0) var enhanced_normal_page_0: texture_2d_array<f32>;
@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_NORMAL_TEXTURE_BINDING_1) var enhanced_normal_page_1: texture_2d_array<f32>;
@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_MER_TEXTURE_BINDING_0) var enhanced_mer_page_0: texture_2d_array<f32>;
@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_MER_TEXTURE_BINDING_1) var enhanced_mer_page_1: texture_2d_array<f32>;
@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_SAMPLER_BINDING) var enhanced_sampler: sampler;
@if(ENHANCED || ENHANCED_SHADOW)
@group(0) @binding(ENHANCED_TEXTURE_REF_BINDING) var<storage, read> enhanced_texture_refs: array<u32>;
