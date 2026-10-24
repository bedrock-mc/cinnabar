import render::chunk_records::{chunk_origins, animations, animation_frames, clock};

@if(PACKED_TERRAIN)
import render::terrain_tables::{terrain_word, terrain_length};

// Group 0 of the packed chunk layout, shared by the liquid and model programs so one composed
// shader can draw both without binding the same slot twice.
import bevy_render::view::View;

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
@if(!PACKED_TERRAIN)
@group(0) @binding(12) var<storage, read> model_templates_data: array<u32>;

// Reads the same typed record from either terrain layout.
fn model_templates(index: u32) -> u32 {
    @if(!PACKED_TERRAIN) return model_templates_data[index];
    @if(PACKED_TERRAIN) {
        if (index >= model_templates_length()) { var empty: u32; return empty; }
        let base = index * 1u;
        return terrain_word(12u, base + 0u);
    }
}

// Returns the record count without including adjacent packed tables.
fn model_templates_length() -> u32 {
    @if(!PACKED_TERRAIN) return arrayLength(&model_templates_data);
    @if(PACKED_TERRAIN) return terrain_length(12u) / 1u;
}
@group(0) @binding(13) var<storage, read> geometry_streams: array<u32>;
@if(!PACKED_TERRAIN)
@group(0) @binding(14) var<storage, read> transparent_refs_data: array<TransparentDrawRef>;

// Reads the same typed record from either terrain layout.
fn transparent_refs(index: u32) -> TransparentDrawRef {
    @if(!PACKED_TERRAIN) return transparent_refs_data[index];
    @if(PACKED_TERRAIN) {
        if (index >= transparent_refs_length()) { var empty: TransparentDrawRef; return empty; }
        let base = index * 2u;
        return TransparentDrawRef(terrain_word(14u, base + 0u), terrain_word(14u, base + 1u));
    }
}

// Returns the record count without including adjacent packed tables.
fn transparent_refs_length() -> u32 {
    @if(!PACKED_TERRAIN) return arrayLength(&transparent_refs_data);
    @if(PACKED_TERRAIN) return terrain_length(14u) / 2u;
}
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;
