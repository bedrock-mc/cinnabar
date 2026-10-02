#import bevy_render::view::View

// One vertex is nine packed f32 words: position, atlas uv, rgba multiplier.
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> vertex_words: array<f32>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn block_entity_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let base = vertex_index * 9u;
    let position = vec3(vertex_words[base], vertex_words[base + 1u], vertex_words[base + 2u]);
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(position, 1.0);
    out.uv = vec2(vertex_words[base + 3u], vertex_words[base + 4u]);
    out.color = vec4(
        vertex_words[base + 5u],
        vertex_words[base + 6u],
        vertex_words[base + 7u],
        vertex_words[base + 8u],
    );
    return out;
}

@fragment
fn block_entity_solid(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    if (texel.a < 0.5) {
        discard;
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
