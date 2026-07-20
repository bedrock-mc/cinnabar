#import bevy_render::view::View

struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>,
    moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>,
    sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>,
    fog_end_time: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(2) var sun_texture: texture_2d<f32>;
@group(0) @binding(3) var moon_phases_texture: texture_2d<f32>;
@group(0) @binding(4) var atmosphere_sampler: sampler;

const CELESTIAL_HALF_ANGLE: f32 = 0.075;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn atmosphere_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let clip_position = vec2(
        f32(vertex_index & 1u),
        f32((vertex_index >> 1u) & 1u),
    ) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(clip_position, 0.0, 1.0));
}

fn view_ray(position: vec2<f32>) -> vec3<f32> {
    let viewport_uv = (position - view.viewport.xy) / view.viewport.zw;
    let ndc = viewport_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    let view_position = view.view_from_clip * vec4(ndc, 1.0, 1.0);
    let view_direction = view_position.xyz / view_position.w;
    return normalize((view.world_from_view * vec4(view_direction, 0.0)).xyz);
}

// Returns top-left-origin image UV and a hard quad coverage mask. The basis is
// stable at zenith so the pinned square textures never roll with the camera.
fn celestial_uv(ray: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    var right = cross(direction, vec3(0.0, 1.0, 0.0));
    if (dot(right, right) < 0.0001) {
        right = vec3(0.0, 0.0, 1.0);
    }
    right = normalize(right);
    let local_up = normalize(cross(right, direction));
    let extent = sin(CELESTIAL_HALF_ANGLE);
    let local = vec2(dot(ray, right), dot(ray, local_up)) / extent;
    let coverage = select(0.0, 1.0, max(abs(local.x), abs(local.y)) <= 1.0);
    return vec3(local * vec2(0.5, -0.5) + vec2(0.5), coverage);
}

fn celestial_visibility(direction_y: f32) -> f32 {
    return smoothstep(-0.04, 0.02, direction_y);
}

fn composite_celestial(
    destination: vec3<f32>,
    sampled_rgb: vec3<f32>,
    coverage: f32,
) -> vec3<f32> {
    return destination + sampled_rgb * coverage;
}

fn sample_sun(ray: vec3<f32>, direction: vec3<f32>) -> vec4<f32> {
    let mapping = celestial_uv(ray, direction);
    let texel_uv = (clamp(mapping.xy, vec2(0.0), vec2(1.0)) * 31.0 + 0.5) / 32.0;
    let sampled = textureSampleLevel(sun_texture, atmosphere_sampler, texel_uv, 0.0);
    let visible = celestial_visibility(direction.y);
    return vec4(sampled.rgb, mapping.z * visible);
}

fn sample_moon(ray: vec3<f32>, direction: vec3<f32>) -> vec4<f32> {
    let mapping = celestial_uv(ray, direction);
    let phase = u32(atmosphere.moon_direction_phase.w) % 8u;
    let phase_column = phase % 4u;
    let phase_row = phase / 4u;
    let local_texel = clamp(mapping.xy, vec2(0.0), vec2(1.0)) * 31.0 + 0.5;
    let atlas_texel = vec2(f32(phase_column * 32u), f32(phase_row * 32u)) + local_texel;
    let atlas_uv = atlas_texel / vec2(128.0, 64.0);
    let sampled = textureSampleLevel(moon_phases_texture, atmosphere_sampler, atlas_uv, 0.0);
    let visible = celestial_visibility(direction.y);
    return vec4(sampled.rgb, mapping.z * visible);
}

@fragment
fn atmosphere_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if (atmosphere.fog_end_time.x <= 32.0) {
        return vec4(atmosphere.fog_color_start.rgb, 1.0);
    }
    let ray = view_ray(in.position.xy);
    let horizon_to_zenith = smoothstep(-0.08, 0.72, ray.y);
    var colour = mix(
        atmosphere.sky_horizon_thunder.rgb,
        atmosphere.sky_zenith_rain.rgb,
        horizon_to_zenith,
    );
    if (ray.y < -0.08) {
        colour *= 0.72;
    }

    let sun_direction = normalize(atmosphere.sun_direction_daylight.xyz);
    let sun = sample_sun(ray, sun_direction);
    colour = composite_celestial(colour, sun.rgb, sun.a);

    let moon_direction = normalize(atmosphere.moon_direction_phase.xyz);
    let moon = sample_moon(ray, moon_direction);
    colour = composite_celestial(colour, moon.rgb, moon.a);

    return vec4(colour, 1.0);
}
