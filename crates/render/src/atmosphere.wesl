import bevy_render::view::View;
import render::lighting::{tint_to_gamma, tint_to_linear};

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

struct SkyboxUniform { parameters: vec4<f32> }
@group(0) @binding(7) var<uniform> skybox: SkyboxUniform;
@group(0) @binding(8) var skybox_face0: texture_2d<f32>;
@group(0) @binding(9) var skybox_face1: texture_2d<f32>;
@group(0) @binding(10) var skybox_face2: texture_2d<f32>;
@group(0) @binding(11) var skybox_face3: texture_2d<f32>;
@group(0) @binding(12) var skybox_face4: texture_2d<f32>;
@group(0) @binding(13) var skybox_face5: texture_2d<f32>;
@group(0) @binding(14) var skybox_sampler: sampler;

// Intersect a camera-relative ray with the translated cube. Removing the
// translation changes its projection, including the top/bottom face boundaries.
fn pack_skybox_exit(ray: vec3<f32>, offset: f32) -> vec3<f32> {
    return (vec3(1.0) + vec3(0.0, offset, 0.0) * sign(ray)) / max(abs(ray), vec3(0.000001));
}

fn pack_skybox_mapping(ray: vec3<f32>, offset: f32) -> vec3<f32> {
    let centre = vec3(0.0, offset, 0.0);
    let exit = pack_skybox_exit(ray, offset);
    let distance = min(exit.x, min(exit.y, exit.z));
    let point = ray * distance - centre;
    var face: u32;
    var plane: vec2<f32>;
    if (exit.z <= exit.x && exit.z <= exit.y) {
        if (ray.z < 0.0) { face = 0u; plane = vec2(point.x, -point.y); }
        else { face = 2u; plane = vec2(-point.x, -point.y); }
    } else if (exit.x <= exit.y) {
        if (ray.x > 0.0) { face = 1u; plane = vec2(point.z, -point.y); }
        else { face = 3u; plane = vec2(-point.z, -point.y); }
    } else {
        if (ray.y > 0.0) { face = 4u; plane = vec2(point.x, -point.z); }
        else { face = 5u; plane = vec2(point.x, point.z); }
    }
    return vec3(plane * 0.5 + vec2(0.5), f32(face));
}

fn sample_pack_skybox(mapping: vec3<f32>) -> vec4<f32> {
    let face = u32(mapping.z);
    if ((u32(skybox.parameters.y) & (1u << face)) == 0u) { return vec4(0.0); }
    switch face {
        case 0u: { return textureSampleLevel(skybox_face0, skybox_sampler, mapping.xy, 0.0); }
        case 1u: { return textureSampleLevel(skybox_face1, skybox_sampler, mapping.xy, 0.0); }
        case 2u: { return textureSampleLevel(skybox_face2, skybox_sampler, mapping.xy, 0.0); }
        case 3u: { return textureSampleLevel(skybox_face3, skybox_sampler, mapping.xy, 0.0); }
        case 4u: { return textureSampleLevel(skybox_face4, skybox_sampler, mapping.xy, 0.0); }
        default: { return textureSampleLevel(skybox_face5, skybox_sampler, mapping.xy, 0.0); }
    }
}

// Target 1.26.50.26 directional-light builder passes these angular
// diameters to the orbital transform, which scales the ±.5 quad by
// 2*distance*tan(diameter/2).
const SUN_HALF_EXTENT: f32 = tan(28.08 * 0.0174532924 * 0.5);
const MOON_HALF_EXTENT: f32 = tan(18.924 * 0.0174532924 * 0.5);

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) star_alpha: f32,
    @location(1) @interpolate(flat) zenith_gamma: vec3<f32>,
    @location(2) @interpolate(flat) horizon_gamma: vec3<f32>,
    @location(3) @interpolate(flat) fog_gamma: vec3<f32>,
}

// Converts uniform sky colours per vertex and keeps star vertices free of those transfers.
fn sky_vertex(position: vec4<f32>, star_alpha: f32) -> VertexOutput {
    var out: VertexOutput;
    out.position = position;
    out.star_alpha = star_alpha;
    if (star_alpha == 0.0) {
        out.zenith_gamma = tint_to_gamma(vec4(atmosphere.sky_zenith_rain.rgb, 1.0)).rgb;
        out.horizon_gamma = tint_to_gamma(vec4(atmosphere.sky_horizon_thunder.rgb, 1.0)).rgb;
        out.fog_gamma = tint_to_gamma(vec4(atmosphere.fog_color_start.rgb, 1.0)).rgb;
    }
    return out;
}

@vertex
fn atmosphere_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    if (vertex_index >= 3u) {
        let star = stars[vertex_index - 3u];
        let angle = atmosphere.sky_extra.y * 6.283185307;
        let cosine = cos(angle);
        let sine = sin(angle);
        // Vanilla rotates the star mesh around +Z.
        let sky = vec3(star.x * cosine - star.y * sine, star.y * cosine + star.x * sine, star.z);
        var clip = view.clip_from_world * vec4(sky + view.world_position, 1.0);
        clip.z = 0.0;
        return sky_vertex(clip, star.w);
    }
    let clip_position = vec2(
        f32(vertex_index & 1u),
        f32((vertex_index >> 1u) & 1u),
    ) * 4.0 - vec2(1.0);
    return sky_vertex(vec4(clip_position, 0.0, 1.0), 0.0);
}

fn view_ray(position: vec2<f32>) -> vec3<f32> {
    let viewport_uv = (position - view.viewport.xy) / view.viewport.zw;
    let ndc = viewport_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    let view_position = view.view_from_clip * vec4(ndc, 1.0, 1.0);
    let view_direction = view_position.xyz / view_position.w;
    return normalize((view.world_from_view * vec4(view_direction, 0.0)).xyz);
}

const SKY_RING = array<vec2<f32>, 10>(
    vec2(1.0, 0.0),
    vec2(0.809017003, 0.587785244),
    vec2(0.309016973, 0.951056540),
    vec2(-0.309017152, 0.951056480),
    vec2(-0.809017062, 0.587785184),
    vec2(-1.0, -0.0000000874227766),
    vec2(-0.809016764, -0.587785542),
    vec2(-0.309017092, -0.951056480),
    vec2(0.309017122, -0.951056480),
    vec2(0.809016943, -0.587785304),
);

// Returns the oriented area used for angular ordering and fan barycentrics.
fn sky_cross(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

// Finds the same decagon wedge in at most three bisections, including its asymmetric seam.
fn sky_sector(point: vec2<f32>) -> u32 {
    var lower = 0u;
    var upper = 5u;
    if (point.y < 0.0) {
        // The fifth stored rim point is slightly below the negative X axis.
        if (point.x < 0.0 && sky_cross(SKY_RING[5], point) < 0.0) { return 4u; }
        lower = 5u;
        upper = 10u;
    }
    while (upper - lower > 1u) {
        let middle = (lower + upper) / 2u;
        if (sky_cross(SKY_RING[middle], point) >= 0.0) { lower = middle; }
        else { upper = middle; }
    }
    return lower;
}

// Intersects the Y256 fan scaled by2000 and retains its barycentric colour interpolation.
fn native_sky_fog_weight(ray: vec3<f32>) -> f32 {
    if (ray.y <= 0.0) { return 1.0; }
    let point = ray.xz * (256.0 / 2000.0) / ray.y;
    let index = sky_sector(point);
    let a = SKY_RING[index];
    let b = SKY_RING[(index + 1u) % 10u];
    let determinant = sky_cross(a, b);
    let alpha = sky_cross(point, b) / determinant;
    let beta = sky_cross(a, point) / determinant;
    if (alpha >= 0.0 && beta >= 0.0) { return clamp(alpha + beta, 0.0, 1.0); }
    // Match the exhaustive fan's rounding gaps on stored rim edges.
    return 1.0;
}

// The stock orbital transform keeps local-X on−Z through the whole
// orbit. The celestial quad maps−X→u1 and−Z→v0, hence fixed+Z
// image-right and this rotating image-down basis. A world-up cross product
// instead flips both texture axes as the celestial body crosses the zenith.
fn celestial_uv(ray: vec3<f32>, direction: vec3<f32>, half_extent: f32) -> vec3<f32> {
    let right = vec3(0.0, 0.0, 1.0);
    let image_down = normalize(vec3(direction.y, -direction.x, 0.0));
    let facing = dot(ray, direction);
    let local = vec2(dot(ray, right), dot(ray, image_down)) / max(facing, 0.0001) / half_extent;
    let inside = facing > 0.0 && max(abs(local.x), abs(local.y)) <= 1.0;
    let coverage = select(0.0, 1.0, inside);
    return vec3(local * 0.5 + vec2(0.5), coverage);
}

// Current orbital calculation stores the eased day angle in
// degrees (moon offset 180). Ordinary vanilla admits the
// sprite through 105/255, without any horizon-height alpha interpolation.
fn celestial_visibility(phase_offset: f32) -> f32 {
    let half_angle = atmosphere.sky_extra.y * 3.141592741;
    let degrees = ((half_angle + half_angle) * 57.2957763671875 + phase_offset) % 360.0;
    return select(0.0, 1.0, degrees <= 105.0 || degrees >= 255.0);
}

// The target version scales the stock celestial alpha by
// clamp(1−2*interpolatedRain,0,1). SunMoon's fragment shader multiplies
// colour by the sampled RGBA, and its material blends SourceAlpha→One.
fn celestial_weather_alpha() -> f32 {
    return clamp(1.0 - 2.0 * atmosphere.sky_zenith_rain.w, 0.0, 1.0);
}

fn composite_celestial(
    destination: vec3<f32>,
    sampled_rgb: vec3<f32>,
    coverage: f32,
) -> vec3<f32> {
    return destination + sampled_rgb * coverage;
}

// Native Sky/SunMoon render into the classic UNORM framebuffer in gamma
// space. Our view target is sRGB: recover input gamma, compose there, then
// encode once at the output boundary instead of interpolating linear colours.
fn native_sky_colour(weight: f32, zenith_gamma: vec3<f32>, horizon_gamma: vec3<f32>) -> vec3<f32> {
    return mix(zenith_gamma, horizon_gamma, weight);
}

fn sky_output(colour: vec3<f32>) -> vec4<f32> {
    return tint_to_linear(vec4(clamp(colour, vec3(0.0), vec3(1.0)), 1.0));
}

fn sample_sun(ray: vec3<f32>, direction: vec3<f32>) -> vec4<f32> {
    let mapping = celestial_uv(ray, direction, SUN_HALF_EXTENT);
    let sampled = textureSampleLevel(sun_texture, atmosphere_sampler, mapping.xy, 0.0);
    let visible = celestial_visibility(0.0);
    return vec4(tint_to_gamma(sampled).rgb, sampled.a * mapping.z * visible * celestial_weather_alpha());
}

fn sample_moon(ray: vec3<f32>, direction: vec3<f32>) -> vec4<f32> {
    let mapping = celestial_uv(ray, direction, MOON_HALF_EXTENT);
    let phase = u32(atmosphere.moon_direction_phase.w) % 8u;
    let phase_column = phase % 4u;
    let phase_row = phase / 4u;
    let atlas_uv = (vec2(f32(phase_column), f32(phase_row)) + mapping.xy) / vec2(4.0, 2.0);
    let sampled = textureSampleLevel(moon_phases_texture, atmosphere_sampler, atlas_uv, 0.0);
    let visible = celestial_visibility(180.0);
    return vec4(tint_to_gamma(sampled).rgb, sampled.a * mapping.z * visible * celestial_weather_alpha());
}

// Native Stars fragment outputs vertexRGB*StarsColorRGB*vertexAlpha.
// The stock mesh vertexRGB is white; vertexAlpha is not framebuffer alpha.
fn native_star_colour(vertex_alpha: f32) -> vec4<f32> {
    return vec4(vec3(vertex_alpha * atmosphere.sky_extra.x), vertex_alpha);
}

// The world-aligned cube repeats its texture sixteen times on each face.
fn end_sky_uv(ray: vec3<f32>) -> vec2<f32> {
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
    return (plane / major * 0.5 + vec2(0.5)) * 16.0;
}

fn end_sky(ray: vec3<f32>, fog_gamma: vec3<f32>) -> vec3<f32> {
    let sampled = textureSampleLevel(end_sky_texture, atmosphere_sampler, end_sky_uv(ray), 0.0);
    return tint_to_gamma(sampled).rgb * (2.0 * fog_gamma);
}

@fragment
fn atmosphere_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let code = u32(atmosphere.sky_extra.w + 0.5);
    if (in.star_alpha > 0.0) {
        if (code != 0u || atmosphere.sky_extra.x <= 0.0) { discard; }
        return tint_to_linear(native_star_colour(in.star_alpha));
    }
    if (code / 4u != 0u) {
        return vec4(atmosphere.fog_color_start.rgb, 1.0);
    }
    let kind = code % 4u;
    if (kind == 1u) {
        return vec4(atmosphere.sky_horizon_thunder.rgb, 1.0);
    }
    let ray = view_ray(in.position.xy);
    if (kind == 2u) {
        return sky_output(end_sky(ray, in.fog_gamma));
    }
    var colour = native_sky_colour(native_sky_fog_weight(ray), in.zenith_gamma, in.horizon_gamma);

    let sun_direction = normalize(atmosphere.sun_direction_daylight.xyz);
    let sun = sample_sun(ray, sun_direction);
    colour = composite_celestial(colour, sun.rgb, sun.a);

    let moon_direction = normalize(atmosphere.moon_direction_phase.xyz);
    let moon = sample_moon(ray, moon_direction);
    colour = composite_celestial(colour, moon.rgb, moon.a);

    return sky_output(colour);
}

// The pack cube follows the stock sky, celestial sprites and stars.
struct PackSkyboxOutput {
    @location(0) colour: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn pack_skybox_fragment(in: VertexOutput) -> PackSkyboxOutput {
    let code = u32(atmosphere.sky_extra.w + 0.5);
    if (skybox.parameters.y == 0.0 || skybox.parameters.z == 0.0 || code % 4u == 1u) { discard; }
    let ray = view_ray(in.position.xy);
    let mapping = pack_skybox_mapping(ray, skybox.parameters.x);
    let sampled = sample_pack_skybox(mapping);
    let exit = pack_skybox_exit(ray, skybox.parameters.x);
    let distance = min(exit.x, min(exit.y, exit.z)) * skybox.parameters.w;
    let clip = view.clip_from_world * vec4(ray * distance + view.world_position, 1.0);
    return PackSkyboxOutput(tint_to_linear(sampled), clamp(clip.z / clip.w, 0.0, 1.0));
}
