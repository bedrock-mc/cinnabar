#define_import_path cinnabar::lighting

// Alpha alone interpolates across a cutout edge; the authored RGB texels stay nearest.
fn cutout_alpha_footprint(alpha: vec4<f32>, fraction: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec2<f32> {
    let interpolated = mix(mix(alpha.x, alpha.y, fraction.x), mix(alpha.z, alpha.w, fraction.x), fraction.y);
    let gradient = vec2(mix(alpha.y - alpha.x, alpha.w - alpha.z, fraction.y), mix(alpha.z - alpha.x, alpha.w - alpha.y, fraction.x));
    return vec2(interpolated, abs(dot(gradient, dx)) + abs(dot(gradient, dy)));
}

// Minification keeps the existing mip alpha; magnification measures the binary texel contour.
fn cutout_alpha_array(image: texture_2d_array<f32>, uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>, sampled_alpha: f32, repeat_uv: bool) -> vec2<f32> {
    let size = vec2<f32>(textureDimensions(image, 0));
    let texel_dx = dx * size;
    let texel_dy = dy * size;
    if (max(length(texel_dx), length(texel_dy)) >= 1.0) { return vec2(sampled_alpha, 1.0); }
    let position = uv * size - vec2(0.5);
    let base = vec2<i32>(floor(position));
    let alpha = vec4(
        textureLoad(image, cutout_texel_address(base, vec2<i32>(size), repeat_uv), layer, 0).a,
        textureLoad(image, cutout_texel_address(base + vec2(1, 0), vec2<i32>(size), repeat_uv), layer, 0).a,
        textureLoad(image, cutout_texel_address(base + vec2(0, 1), vec2<i32>(size), repeat_uv), layer, 0).a,
        textureLoad(image, cutout_texel_address(base + vec2(1), vec2<i32>(size), repeat_uv), layer, 0).a,
    );
    return cutout_alpha_footprint(alpha, fract(position), texel_dx, texel_dy);
}

// Signed modulo keeps bilinear neighbours inside the same array layer at a repeating seam.
fn cutout_texel_address(coordinate: vec2<i32>, size: vec2<i32>, repeat_uv: bool) -> vec2<i32> {
    return select(clamp(coordinate, vec2(0), size - vec2(1)), (coordinate % size + size) % size, repeat_uv);
}

// Atlas cutouts use the same alpha footprint without adding a filtering sampler.
fn cutout_alpha_2d(image: texture_2d<f32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, sampled_alpha: f32) -> vec2<f32> {
    let size = vec2<f32>(textureDimensions(image, 0));
    let texel_dx = dx * size;
    let texel_dy = dy * size;
    if (max(length(texel_dx), length(texel_dy)) >= 1.0) { return vec2(sampled_alpha, 1.0); }
    let position = uv * size - vec2(0.5);
    let base = vec2<i32>(floor(position));
    let limit = vec2<i32>(size) - vec2(1);
    let alpha = vec4(
        textureLoad(image, clamp(base, vec2(0), limit), 0).a,
        textureLoad(image, clamp(base + vec2(1, 0), vec2(0), limit), 0).a,
        textureLoad(image, clamp(base + vec2(0, 1), vec2(0), limit), 0).a,
        textureLoad(image, clamp(base + vec2(1), vec2(0), limit), 0).a,
    );
    return cutout_alpha_footprint(alpha, fract(position), texel_dx, texel_dy);
}

// The original alpha threshold stays at the contour's center, with a one-pixel coverage ramp.
fn cutout_coverage(footprint: vec2<f32>, threshold: f32) -> f32 {
    return clamp((footprint.x - threshold) / max(footprint.y, 0.00001) + 0.5, 0.0, 1.0);
}
// ACTOR_SHADE_CONSTANTS

// Native ordinary materials compose normalized UNORM RGB. Bevy's sRGB textures
// and targets need these transfers at the boundaries, never between products.
fn tint_to_gamma(rgba: vec4<f32>) -> vec4<f32> {
    let linear = rgba.rgb;
    return vec4(select(12.92 * linear, 1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055, linear > vec3(0.0031308)), rgba.a);
}

fn tint_to_linear(rgba: vec4<f32>) -> vec4<f32> {
    let gamma = rgba.rgb;
    return vec4(select(gamma / 12.92, pow((gamma + 0.055) / 1.055, vec3(2.4)), gamma > vec3(0.04045)), rgba.a);
}

@group(1) @binding(0) var<uniform> world_lightmap: array<vec4<f32>, 256>;

// Both nibbles address the same environment table in every ordinary world pass.
fn light_colour(sample: u32) -> vec3<f32> {
    return world_lightmap[sample & 255u].rgb;
}

// The vanilla classic light texture stores RGB truncated to bytes before
// lookups read it.
fn native_light_texel(sample: u32) -> vec3<f32> {
    return floor(clamp(light_colour(sample), vec3(0.0), vec3(1.0)) * 255.0) / 255.0;
}

// Current terrain emitters pack two four-bit levels. Installed near-version
// 1.26.51.01 RenderChunk Metal divides each by15 in the vertex shader, then
// samples the clamp-linear16x16 byte texture in the fragment shader. Unlike
// actors' /16 lookup, interpolate levels before this nonlinear table lookup.
const TERRAIN_LIGHTMAP_SIDE: u32 = 1u << 4u;

fn terrain_light_levels(sample: u32) -> vec2<f32> {
    return vec2(f32(sample & 15u), f32((sample >> 4u) & 15u));
}

fn terrain_light_colour(levels: vec2<f32>) -> vec3<f32> {
    let last = TERRAIN_LIGHTMAP_SIDE - 1u;
    let coordinate = clamp(levels * (f32(TERRAIN_LIGHTMAP_SIDE) / f32(last)) - vec2(0.5), vec2(0.0), vec2(f32(last)));
    let lower = vec2<u32>(floor(coordinate));
    let upper = min(lower + vec2(1u), vec2(last));
    let fraction = coordinate - vec2<f32>(lower);
    return mix(
        mix(native_light_texel(lower.x | (lower.y << 4u)), native_light_texel(upper.x | (lower.y << 4u)), fraction.x),
        mix(native_light_texel(lower.x | (upper.y << 4u)), native_light_texel(upper.x | (upper.y << 4u)), fraction.x),
        fraction.y,
    );
}

// Actor constants use (sky, block)/16, without a half-texel bias.
// Vanilla clamps 16*uv-.5 then interpolates the byte table in gamma.
// Our shared table is transposed: block in the low nibble, sky in the high.
fn actor_light_colour(sample: u32) -> vec3<f32> {
    let block = sample & 15u;
    let sky = (sample >> 4u) & 15u;
    let previous_block = max(block, 1u) - 1u;
    let previous_sky = max(sky, 1u) - 1u;
    return mix(
        mix(native_light_texel(previous_block | (previous_sky << 4u)), native_light_texel(block | (previous_sky << 4u)), 0.5),
        mix(native_light_texel(previous_block | (sky << 4u)), native_light_texel(block | (sky << 4u)), 0.5),
        0.5,
    );
}

// Ordinary Fancy Actor/Entity materials shade posed world normals in the
// vertex stage, not with terrain's face coefficients. Overworld TileLightColor
// W is +1; other dimension signs remain a separate gate.
fn actor_lighting(sample: u32, normal: vec3<f32>, overlay_alpha: f32) -> vec3<f32> {
    if ((sample & 0x80000000u) == 0u) { return vec3(1.0); }
    let shade = (((1.0 + normal.y) * ACTOR_SHADE[0] + normal.x * normal.x * ACTOR_SHADE[1]) + normal.z * normal.z * ACTOR_SHADE[2]) + ACTOR_SHADE[3] + overlay_alpha * ACTOR_SHADE[4];
    return actor_light_colour(sample) * shade;
}

// Kept separate from the lightmap: AO shades geometry, not light coordinates.
fn light_ao_factor(level: u32) -> f32 {
    return 1.0 - f32(min(level, 4u)) * 0.2;
}

// Vertex colors interpolate the lightmap result rather than nonlinear light levels.
fn lit_colour(colour: vec3<f32>, lighting: vec3<f32>) -> vec3<f32> {
    return colour * lighting;
}

// Vanilla ordinary and emitting face coefficients.
fn face_shade(normal: vec3<f32>, emitting: bool) -> f32 {
    if normal.y < -0.5 { return select(0.5, 0.875, emitting); }
    if abs(normal.x) > 0.5 { return select(0.6, 0.9, emitting); }
    if abs(normal.z) > 0.5 { return select(0.8, 0.95, emitting); }
    return 1.0;
}

fn material_ambient_occlusion(ao: f32, flags: u32) -> f32 {
    return select(ao, 1.0, (flags & MATERIAL_DISABLE_AO_FLAG) != 0u);
}

fn material_face_shade(normal: vec3<f32>, emitting: bool, flags: u32) -> f32 {
    return select(face_shade(normal, emitting), 1.0, (flags & MATERIAL_DISABLE_FACE_DIMMING_FLAG) != 0u);
}

struct WorldAtmosphere {
    sun_direction_daylight: vec4<f32>, moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>, sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>, fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>, sky_extra: vec4<f32>,
}
@group(1) @binding(1) var<uniform> world_atmosphere: WorldAtmosphere;

// Uses the same distance fade as the terrain passes after lighting and overlays.
fn world_fog_amount(position: vec3<f32>, camera: vec3<f32>) -> f32 {
    let start = world_atmosphere.fog_color_start.w;
    let end = world_atmosphere.fog_end_time.x;
    return clamp((distance(position, camera) - start) / max(end - start, 0.0001), 0.0, 1.0);
}

fn world_distance_fog(colour: vec3<f32>, position: vec3<f32>, camera: vec3<f32>) -> vec3<f32> {
    return mix(colour, world_atmosphere.fog_color_start.rgb, world_fog_amount(position, camera));
}

fn actor_distance_fog(colour_gamma: vec3<f32>, position: vec3<f32>, camera: vec3<f32>) -> vec3<f32> {
    let fog_gamma = tint_to_gamma(vec4(world_atmosphere.fog_color_start.rgb, 1.0)).rgb;
    return mix(colour_gamma, fog_gamma, world_fog_amount(position, camera));
}
