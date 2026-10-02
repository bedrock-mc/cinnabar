#import bevy_render::view::View

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

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(2) var sun_texture: texture_2d<f32>;
@group(0) @binding(3) var moon_phases_texture: texture_2d<f32>;
@group(0) @binding(4) var atmosphere_sampler: sampler;
@group(0) @binding(5) var end_sky_texture: texture_2d<f32>;
@group(0) @binding(6) var<storage, read> stars: array<vec4<f32>>;

// Half-extents of the flat sun and moon quads as a tangent at unit distance; both need
// native measurement.
const SUN_HALF_EXTENT: f32 = 0.15;
const MOON_HALF_EXTENT: f32 = 0.10;
// Vanilla dims the End sky texture to roughly this fraction of its stored brightness.
const END_SKY_BRIGHTNESS: f32 = 0.157;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) star_alpha: f32,
}

@vertex
fn atmosphere_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    if (vertex_index >= 3u) {
        let star = stars[vertex_index - 3u];
        let angle = atmosphere.sky_extra.y * 6.283185307;
        let cosine = cos(angle);
        let sine = sin(angle);
        // R:l/LevelRendererCamera.cpp:6765 rotates the star mesh around +Z.
        let sky = vec3(star.x * cosine - star.y * sine, star.y * cosine + star.x * sine, star.z);
        var clip = view.clip_from_world * vec4(sky + view.world_position, 1.0);
        clip.z = 0.0;
        return VertexOutput(clip, star.w);
    }
    let clip_position = vec2(
        f32(vertex_index & 1u),
        f32((vertex_index >> 1u) & 1u),
    ) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(clip_position, 0.0, 1.0), 0.0);
}

fn view_ray(position: vec2<f32>) -> vec3<f32> {
    let viewport_uv = (position - view.viewport.xy) / view.viewport.zw;
    let ndc = viewport_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    let view_position = view.view_from_clip * vec4(ndc, 1.0, 1.0);
    let view_direction = view_position.xyz / view_position.w;
    return normalize((view.world_from_view * vec4(view_direction, 0.0)).xyz);
}

// Returns top-left-origin image UV and a hard quad coverage mask from a flat quad tangent
// to the sky sphere. The basis is stable at zenith so the pinned textures never roll.
fn celestial_uv(ray: vec3<f32>, direction: vec3<f32>, half_extent: f32) -> vec3<f32> {
    var right = cross(direction, vec3(0.0, 1.0, 0.0));
    if (dot(right, right) < 0.0001) {
        right = vec3(0.0, 0.0, 1.0);
    }
    right = normalize(right);
    let local_up = normalize(cross(right, direction));
    let facing = dot(ray, direction);
    let local = vec2(dot(ray, right), dot(ray, local_up)) / max(facing, 0.0001) / half_extent;
    let inside = facing > 0.0 && max(abs(local.x), abs(local.y)) <= 1.0;
    let coverage = select(0.0, 1.0, inside);
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
    let mapping = celestial_uv(ray, direction, SUN_HALF_EXTENT);
    let texel_uv = (clamp(mapping.xy, vec2(0.0), vec2(1.0)) * 31.0 + 0.5) / 32.0;
    let sampled = textureSampleLevel(sun_texture, atmosphere_sampler, texel_uv, 0.0);
    let visible = celestial_visibility(direction.y);
    return vec4(sampled.rgb, mapping.z * visible);
}

fn sample_moon(ray: vec3<f32>, direction: vec3<f32>) -> vec4<f32> {
    let mapping = celestial_uv(ray, direction, MOON_HALF_EXTENT);
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

fn end_sky(ray: vec3<f32>) -> vec3<f32> {
    let magnitude = abs(ray);
    var plane: vec2<f32>;
    var major: f32;
    if (magnitude.x >= magnitude.y && magnitude.x >= magnitude.z) {
        plane = ray.yz;
        major = magnitude.x;
    } else if (magnitude.y >= magnitude.z) {
        plane = ray.xz;
        major = magnitude.y;
    } else {
        plane = ray.xy;
        major = magnitude.z;
    }
    let uv = plane / major * 0.5 + vec2(0.5);
    return textureSampleLevel(end_sky_texture, atmosphere_sampler, uv, 0.0).rgb * END_SKY_BRIGHTNESS;
}

// Sunrise/sunset glow hugging the horizon on the side the sun crosses.
fn sunrise_glow(ray: vec3<f32>) -> f32 {
    let alpha = atmosphere.sunrise_band.a;
    if (alpha <= 0.0) {
        return 0.0;
    }
    let side = select(-1.0, 1.0, atmosphere.sun_direction_daylight.x >= 0.0);
    let flat_ray = normalize(vec2(ray.x, ray.z) + vec2(0.00001));
    let azimuth = max(flat_ray.x * side, 0.0);
    let height = 1.0 - smoothstep(-0.05, 0.5, ray.y);
    return alpha * azimuth * azimuth * height;
}

@fragment
fn atmosphere_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray = view_ray(in.position.xy);
    let code = u32(atmosphere.sky_extra.w + 0.5);
    if (code / 4u != 0u) {
        return vec4(atmosphere.fog_color_start.rgb, 1.0);
    }
    if (in.star_alpha > 0.0) {
        if (code != 0u || atmosphere.sky_extra.x <= 0.0) { discard; }
        let sun = sample_sun(ray, normalize(atmosphere.sun_direction_daylight.xyz));
        let moon = sample_moon(ray, normalize(atmosphere.moon_direction_phase.xyz));
        return vec4(vec3(1.0), in.star_alpha * atmosphere.sky_extra.x * (1.0 - sun.a) * (1.0 - moon.a));
    }
    let kind = code % 4u;
    if (kind == 1u) {
        return vec4(atmosphere.sky_horizon_thunder.rgb, 1.0);
    }
    if (kind == 2u) {
        return vec4(atmosphere.sky_zenith_rain.rgb + end_sky(ray), 1.0);
    }
    let horizon_to_zenith = smoothstep(-0.08, 0.72, ray.y);
    var colour = mix(
        atmosphere.sky_horizon_thunder.rgb,
        atmosphere.sky_zenith_rain.rgb,
        horizon_to_zenith,
    );
    if (ray.y < -0.08) {
        colour *= 0.72;
    }
    colour = mix(colour, atmosphere.sunrise_band.rgb, sunrise_glow(ray));

    let sun_direction = normalize(atmosphere.sun_direction_daylight.xyz);
    let sun = sample_sun(ray, sun_direction);
    colour = composite_celestial(colour, sun.rgb, sun.a);

    let moon_direction = normalize(atmosphere.moon_direction_phase.xyz);
    let moon = sample_moon(ray, moon_direction);
    colour = composite_celestial(colour, moon.rgb, moon.a);

    return vec4(colour, 1.0);
}
