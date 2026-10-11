import bevy_render::view::View;
import render::lighting::{lit_colour, light_colour};

// Port of the vanilla RenderDragon `Weather` material: wrapped particle streaks stretched along
// the layer velocity, hidden below each column's occlusion height.

struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>,
    moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>,
    sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>,
    fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>,
    sky_extra: vec4<f32>,
}

// forward: box offset ahead of the camera, then 1 when the vanilla sheet is bound.
// grid: occlusion grid origin x and z.
struct WeatherParams {
    forward: vec4<f32>,
    grid: vec4<i32>,
}

struct Layer {
    base_offset: vec4<f32>,
    velocity: vec4<f32>,
    uv_rect: vec4<f32>,
    dimensions: vec2<f32>,
    first_particle: u32,
    particle_count: u32,
    kind: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(2) var<storage, read> layers: array<Layer>;
@group(0) @binding(3) var<uniform> weather: WeatherParams;
@group(0) @binding(4) var sheet_texture: texture_2d<f32>;
@group(0) @binding(5) var sheet_sampler: sampler;
@group(0) @binding(6) var<storage, read> particles: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read> occlusion: array<i32>;

const PARTICLE_BOX: f32 = 30.0;
const PARTICLE_POOL: u32 = 925u;
// Projection y-scale the vanilla widths are authored against.
const REFERENCE_PROJECTION_SCALE: f32 = 1.7783410549163818;
const OCCLUSION_SIDE: i32 = 64;
const OCCLUSION_BLOCKED: i32 = 2147483647;
// Sheet colour of the rain streaks, for the diagnostic path without the carrier.
const FALLBACK_STREAK: vec4<f32> = vec4(0.267, 0.396, 0.757, 0.6);

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) cell_u: f32,
    @location(3) @interpolate(flat) kind: u32,
}

fn corner_uv(corner_index: u32) -> vec2<f32> {
    return array<vec2<f32>, 6>(
        vec2(0.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(0.0, 1.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
    )[corner_index];
}

fn wrap_box(value: vec3<f32>) -> vec3<f32> {
    return value - PARTICLE_BOX * trunc(value / PARTICLE_BOX);
}

@vertex
fn weather_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let layer = layers[instance_index];
    let quad = vertex_index / 6u;
    var out: VertexOutput;
    if (quad >= layer.particle_count) {
        out.position = vec4(0.0);
        return out;
    }
    let particle = particles[(layer.first_particle + quad) % PARTICLE_POOL];
    let corner = corner_uv(vertex_index % 6u);
    let relative = wrap_box(particle.xyz + layer.base_offset.xyz)
        - vec3(PARTICLE_BOX * 0.5)
        + weather.forward.xyz;
    let world = view.world_position + relative;
    let bottom = view.clip_from_world * vec4(world, 1.0);
    let top = view.clip_from_world * vec4(world + layer.velocity.xyz * layer.dimensions.y, 1.0);
    let direction = top.xy / top.w - bottom.xy / bottom.w;
    var across = vec2(1.0, 0.0);
    if (dot(direction, direction) > 1.0e-20) {
        across = normalize(vec2(-direction.y, direction.x));
    }
    let width = layer.dimensions.x * view.clip_from_view[1][1] / REFERENCE_PROJECTION_SCALE;
    var position = mix(top, bottom, corner.y);
    position = vec4(position.xy + across * (0.5 - corner.x) * width, position.zw);

    out.position = position;
    out.world_position = world;
    out.uv = layer.uv_rect.xy + corner * layer.uv_rect.zw;
    out.uv.x += particle.w * layer.uv_rect.z;
    out.cell_u = corner.x;
    out.kind = layer.kind;
    return out;
}

fn occluded(world: vec3<f32>, kind: u32) -> bool {
    let cell = vec2<i32>(floor(world.xz)) - weather.grid.xy;
    if (any(cell < vec2(0)) || any(cell >= vec2(OCCLUSION_SIDE))) {
        return false;
    }
    let height = occlusion[i32(kind) * OCCLUSION_SIDE * OCCLUSION_SIDE + cell.y * OCCLUSION_SIDE + cell.x];
    return height == OCCLUSION_BLOCKED || world.y - 0.5 < f32(height);
}

@fragment
fn weather_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var texel = textureSampleLevel(sheet_texture, sheet_sampler, in.uv, 0.0);
    if (weather.forward.w < 0.5) {
        texel = FALLBACK_STREAK * step(abs(in.cell_u - 0.375), 0.125);
    }
    if (texel.a <= 0.0 || occluded(in.world_position, in.kind)) {
        discard;
    }
    let lit = lit_colour(texel.rgb, light_colour(240u));
    let fog = clamp(
        (distance(in.world_position, view.world_position) - atmosphere.fog_color_start.w)
            / max(atmosphere.fog_end_time.x - atmosphere.fog_color_start.w, 0.0001),
        0.0,
        1.0,
    );
    return vec4(mix(lit, atmosphere.fog_color_start.rgb, fog), texel.a);
}
