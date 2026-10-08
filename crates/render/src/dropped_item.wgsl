#import bevy_render::view::View
#import cinnabar::lighting::{actor_light_colour, actor_distance_fog, tint_to_gamma, tint_to_linear}
#ifdef ALPHA_TO_COVERAGE
#import cinnabar::lighting::{CutoutSample, cutout_alpha_array}
#endif

const ITEM_ALPHA_THRESHOLD: f32 = 0.1;

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var sprites: texture_2d_array<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;
// x = daylight scale for the sky channel.
@group(0) @binding(3) var<uniform> environment: vec4<f32>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) row_0: vec4<f32>,
    @location(4) row_1: vec4<f32>,
    @location(5) row_2: vec4<f32>,
    // x = model index, y = block light level, z = sky light level, w = packed RGBA8 overlay.
    @location(6) packed: vec4<u32>,
    @location(7) layer: u32,
    @location(8) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) shade: f32,
    @location(3) @interpolate(flat) levels: vec2<u32>,
    @location(4) color: vec4<f32>,
    @location(5) @interpolate(flat) overlay: vec4<f32>,
    @location(6) world_position: vec3<f32>,
}

// Provisional directional shade: full on top faces, half on bottom faces.
const SHADE_BASE: f32 = 0.75;
const SHADE_SLOPE: f32 = 0.25;

@vertex
fn item_vertex(input: VertexInput) -> VertexOutput {
    let local = vec4(input.position, 1.0);
    let world = vec4(
        dot(input.row_0, local),
        dot(input.row_1, local),
        dot(input.row_2, local),
        1.0,
    );
    let world_normal = normalize(vec3(
        dot(input.row_0.xyz, input.normal),
        dot(input.row_1.xyz, input.normal),
        dot(input.row_2.xyz, input.normal),
    ));
    var out: VertexOutput;
    out.position = view.clip_from_world * world;
    out.world_position = world.xyz;
    out.uv = input.uv;
    out.layer = input.layer;
    out.color = input.color;
    out.overlay = unpack4x8unorm(input.packed.w);
    out.shade = SHADE_BASE + SHADE_SLOPE * world_normal.y;
    out.levels = vec2(input.packed.y, input.packed.z);
    return out;
}

@fragment
fn item_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(sprites, sprite_sampler, input.uv, i32(input.layer));
    var color = tint_to_gamma(texel) * input.color;
#ifndef ALPHA_TO_COVERAGE
    if (color.a < ITEM_ALPHA_THRESHOLD) {
        discard;
    }
#else
    let threshold = ITEM_ALPHA_THRESHOLD / max(input.color.a, 0.00001);
    let cutout = cutout_alpha_array(sprites, input.uv, i32(input.layer), dpdx(input.uv), dpdy(input.uv), texel, false, threshold);
    if (color.a < ITEM_ALPHA_THRESHOLD && cutout.coverage > 0.0) { color = tint_to_gamma(cutout.colour) * input.color; }
    color.a = cutout.coverage;
#endif
    // Native item materials compose gamma RGB with the actor /16 lightmap
    // lookup. Transfer the completed product once for Bevy's sRGB target.
    let lit = mix(color.rgb, input.overlay.rgb, input.overlay.a) * input.shade * actor_light_colour(input.levels.x | (input.levels.y << 4u));
    return tint_to_linear(vec4(actor_distance_fog(lit, input.world_position, view.world_position), color.a));
}
