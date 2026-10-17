import render::lighting::WorldAtmosphere;
// Group 0 of the packed chunk layout, shared by the liquid and model programs so one composed
// shader can draw both without binding the same slot twice.
import bevy_render::view::View;

struct ChunkOrigin { value: vec4<i32>, stream_bases: vec4<u32> }
// ANIMATION_GPU_LAYOUT
// Composable modules avoid member names ending in digits, which naga's writer would rename.
struct AnimationClockGpu { tick: u32, partial_tick: f32, reserved_a: u32, reserved_b: u32 }
// Shaders read a ref's record; the record names its own origin slot, which `metadata_index` mirrors
// for the CPU-side sort.
struct TransparentDrawRef { liquid_record_index: u32, metadata_index: u32 }
alias AtmosphereUniform = WorldAtmosphere;
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
@group(0) @binding(11) var<uniform> clock: AnimationClockGpu;
@group(0) @binding(12) var<storage, read> model_templates: array<u32>;
@group(0) @binding(13) var<storage, read> geometry_streams: array<u32>;
@group(0) @binding(14) var<storage, read> transparent_refs: array<TransparentDrawRef>;
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;
