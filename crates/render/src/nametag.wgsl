#import bevy_render::view::View
#import cinnabar::lighting::tint_to_gamma

struct Record {
    anchor: vec3<f32>,
    text: u32,
    rect: vec4<f32>,
    uv: vec4<f32>,
    color: vec4<f32>,
    line_lift: f32,
    padding0: u32,
    padding1: u32,
    padding2: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> records: array<Record>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;

// Injected from nametag.rs: no independent copy of the native constants in the shader.
const BLOCKS_PER_FONT_PIXEL: f32 = NAMETAG_SCALE_VALUE;
const ACOS_LINEAR: f32 = NAMETAG_ACOS_LINEAR_VALUE;
const ACOS_CUBIC: f32 = NAMETAG_ACOS_CUBIC_VALUE;
const HORIZONTAL_ZERO: f32 = NAMETAG_HORIZONTAL_ZERO_VALUE;

fn native_acos(value: f32) -> f32 {
    return (ACOS_CUBIC * value * value * value - ACOS_LINEAR * value) + 1.5707963267948966;
}

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
    // Native uses a cubic acos approximation and substitutes each zero horizontal component.
    // Rotation is computed from the original anchor, before the multiline Y translation.
    let direction = view.world_position - record.anchor;
    let x = select(direction.x, HORIZONTAL_ZERO, direction.x == 0.0);
    let z = select(direction.z, HORIZONTAL_ZERO, direction.z == 0.0);
    let horizontal = sqrt(x * x + z * z);
    let yaw = native_acos(-z / horizontal) * select(1.0, -1.0, x > 0.0);
    let pitch_dot = (direction.x * (x / horizontal) + direction.z * (z / horizontal)) / length(direction);
    let pitch = native_acos(pitch_dot) * select(-1.0, 1.0, direction.y > 0.0);
    let right = vec3(-cos(yaw), 0.0, sin(yaw));
    let down = vec3(-sin(yaw) * sin(pitch), -cos(pitch), -cos(yaw) * sin(pitch));
    let local = mix(record.rect.xy, record.rect.zw, at);
    let world = record.anchor + vec3(0.0, record.line_lift, 0.0)
        + (right * local.x + down * local.y) * BLOCKS_PER_FONT_PIXEL;
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world, 1.0);
    out.uv = mix(record.uv.xy, record.uv.zw, at);
    out.color = record.color;
    out.textured = select(0u, 1u, record.uv.z >= 0.0);
    return out;
}

@fragment
fn nametag_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    var color = input.color;
    var texel = textureSample(atlas, atlas_sampler, input.uv);
    if (input.textured != 0u) {
#ifdef NAMETAG_ALPHA_TEST
        // Test the sampled glyph, before multiplying the native 0.125 sneak opacity.
        if (texel.a < 0.5) { discard; }
#endif
#ifdef NAMETAG_GAMMA_BLEND
        texel = tint_to_gamma(texel);
#endif
        color = color * texel;
    }
    if (color.a <= 0.0) {
        discard;
    }
    return color;
}
