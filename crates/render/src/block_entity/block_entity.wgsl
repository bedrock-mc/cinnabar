#import bevy_render::view::View
#import cinnabar::lighting::{actor_lighting, actor_distance_fog, tint_to_gamma, tint_to_linear}

// Packed Rust BlockEntityVertex: position, atlas UV, RGBA, world normal, actor light.
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> vertex_words: array<u32>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) native_lighting: vec3<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) @interpolate(flat) actor_light: u32,
}

fn vertex_f32(index: u32) -> f32 { return bitcast<f32>(vertex_words[index]); }

@vertex
fn block_entity_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let base = vertex_index * BLOCK_ENTITY_VERTEX_WORDS;
    let position = vec3(vertex_f32(base), vertex_f32(base + 1u), vertex_f32(base + 2u));
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(position, 1.0);
    out.uv = vec2(vertex_f32(base + 3u), vertex_f32(base + 4u));
    out.color = vec4(
        vertex_f32(base + 5u),
        vertex_f32(base + 6u),
        vertex_f32(base + 7u),
        vertex_f32(base + 8u),
    );
    out.world_position = position;
    out.actor_light = vertex_words[base + 12u];
    let normal = vec3(vertex_f32(base + 9u), vertex_f32(base + 10u), vertex_f32(base + 11u));
    out.native_lighting = actor_lighting(out.actor_light, normal, 0.0);
    return out;
}

@fragment
fn block_entity_solid(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    if (texel.a < 0.5) {
        discard;
    }
    if (input.actor_light != 0u) {
        // Native mob_head:entity_alphatest uses Fancy world-normal shading and
        // the shared lightmap, composed in gamma before the output transfer.
        let gamma = tint_to_gamma(texel).rgb * input.color.rgb * input.native_lighting;
        return tint_to_linear(vec4(actor_distance_fog(gamma, input.world_position, view.world_position), 1.0));
    }
    return vec4(texel.rgb * input.color.rgb, 1.0);
}

@fragment
fn block_entity_overlay(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.uv.x < 0.0) { return input.color; }
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    let alpha = texel.a * input.color.a;
    if (alpha < 0.02) {
        discard;
    }
    return vec4(texel.rgb * input.color.rgb, alpha);
}

@fragment
fn block_entity_crack(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.uv.x < 0.0) { return input.color; }
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    if (texel.a < 0.02) {
        discard;
    }
    return vec4(texel.rgb, 1.0);
}

@fragment
fn block_entity_additive(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    return vec4(texel.rgb * input.color.rgb, 1.0);
}
