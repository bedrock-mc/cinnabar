import bevy_render::view::View;
import render::lighting::{tint_to_gamma, tint_to_linear};

struct Record {
    anchor: vec3<f32>,
    text: u32,
    rect: vec4<f32>,
    uv: vec4<f32>,
    color: vec4<f32>,
    line_lift: f32,
    depth_tested: u32,
    padding1: u32,
    padding2: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> records: array<Record>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;

// Injected from nametag.rs: font-pixel scale has one shared value.
const BLOCKS_PER_FONT_PIXEL: f32 = NAMETAG_SCALE_VALUE;
const ALPHA_CUTOFF: f32 = NAMETAG_ALPHA_CUTOFF_VALUE;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) textured: u32,
}

fn corner(index: u32) -> vec2<f32> {
    return array<vec2<f32>, 6>(
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 1.0),
    )[index];
}

@vertex
fn nametag_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let record = records[vertex_index / 6u];
    let at = corner(vertex_index % 6u);
    // Every tag shares the camera rotation, including at the edges of the view.
    let right = normalize(view.world_from_view[0].xyz);
    let up = normalize(view.world_from_view[1].xyz);
    let local = mix(record.rect.xy, record.rect.zw, at);
    let anchor = record.anchor + vec3(0.0, record.line_lift, 0.0);
    let world = anchor + (right * local.x - up * local.y) * BLOCKS_PER_FONT_PIXEL;
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world, 1.0);
    // A facing quad has one depth. Corner rounding must not pit shadows against glyphs.
    let plane = view.clip_from_world * vec4(anchor, 1.0);
    out.position.z = plane.z;
    out.position.w = plane.w;
    out.uv = mix(record.uv.xy, record.uv.zw, at);
    out.color = record.color;
    out.textured = select(0u, 1u, record.uv.z >= 0.0);
    return out;
}

@fragment
fn nametag_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    var color = input.color;
    @if(!NAMETAG_GAMMA_BLEND)
    color = tint_to_linear(color);
    var texel = textureSample(atlas, atlas_sampler, input.uv);
    if (input.textured != 0u) {
        @if(NAMETAG_GAMMA_BLEND)
        texel = tint_to_gamma(texel);
        // Test atlas coverage before text opacity and cosmetic fading.
        if (texel.a <= ALPHA_CUTOFF) {
            discard;
        }
        color = color * texel;
    }
    // Invisible glyphs must not write depth over later labels.
    if (color.a <= 0.0) {
        discard;
    }
    return color;
}
