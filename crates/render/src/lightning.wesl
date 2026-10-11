import bevy_render::view::View;

struct Ribbon {
    start: vec3<f32>,
    half_width: f32,
    end: vec3<f32>,
    intensity: f32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> ribbons: array<Ribbon>;

// Provisional bolt tint; needs native calibration.
const BOLT_COLOUR: vec3<f32> = vec3(0.85, 0.9, 1.0);

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) across: f32,
    @location(1) @interpolate(flat) intensity: f32,
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
fn lightning_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let ribbon = ribbons[vertex_index / 6u];
    let corner = corner_uv(vertex_index % 6u);
    let axis = ribbon.end - ribbon.start;
    let to_camera = view.world_position - (ribbon.start + ribbon.end) * 0.5;
    let side = normalize(cross(axis, to_camera) + vec3(0.000001, 0.0, 0.0));
    let base = mix(ribbon.start, ribbon.end, corner.y);
    let world = base + side * (corner.x * 2.0 - 1.0) * ribbon.half_width;

    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world, 1.0);
    out.across = corner.x * 2.0 - 1.0;
    out.intensity = ribbon.intensity;
    return out;
}

@fragment
fn lightning_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let falloff = 1.0 - in.across * in.across;
    return vec4(BOLT_COLOUR * in.intensity * falloff, 1.0);
}
