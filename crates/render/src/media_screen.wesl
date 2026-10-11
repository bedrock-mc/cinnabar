import bevy_render::view::View;

struct Screen {
    center: vec3<f32>,
    textured: f32,
    half_right: vec3<f32>,
    _pad0: f32,
    half_up: vec3<f32>,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> screen: Screen;
@group(0) @binding(2) var frame_texture: texture_2d<f32>;
@group(0) @binding(3) var frame_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) lit: f32,
}

fn corner_uv(corner_index: u32) -> vec2<f32> {
    return array<vec2<f32>, 6>(
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 1.0),
    )[corner_index];
}

@vertex
fn media_screen_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let uv = corner_uv(vertex_index % 6u);
    let world = screen.center
        + screen.half_right * (uv.x * 2.0 - 1.0)
        + screen.half_up * (1.0 - uv.y * 2.0);
    // Winding-independent: only the side the local +Z normal faces shows the picture.
    let front = dot(view.world_position - screen.center, cross(screen.half_right, screen.half_up)) > 0.0;

    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world, 1.0);
    out.uv = uv;
    out.lit = select(0.0, screen.textured, front);
    return out;
}

@fragment
fn media_screen_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(frame_texture, frame_sampler, in.uv).rgb;
    return vec4(colour * in.lit, 1.0);
}
