#import bevy_render::view::View

struct Highlight {
    center: vec3<f32>,
    _pad0: f32,
    right: vec3<f32>,
    _pad1: f32,
    up: vec3<f32>,
    _pad2: f32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> highlight: Highlight;
@group(0) @binding(2) var highlight_texture: texture_2d<f32>;
@group(0) @binding(3) var highlight_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn highlight_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let uv = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    )[vertex_index];
    let world = highlight.center + highlight.right * (0.5 - uv.x)
        + highlight.up * (0.5 - uv.y);
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn highlight_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(highlight_texture, highlight_sampler, in.uv);
}

@fragment
fn highlight_occluded_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = textureSample(highlight_texture, highlight_sampler, in.uv);
    return vec4(pixel.rgb, pixel.a * 0.5);
}
