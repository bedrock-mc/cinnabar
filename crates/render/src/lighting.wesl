
// Coverage and the nearest covered texel stay separate so opaque RGB is never filtered.
struct CutoutSample { coverage: f32, colour: vec4<f32> }

// Coverage integrates the nearest alpha grid over one pixel; covered RGB stays nearest.
fn cutout_sample(texels: array<vec4<f32>, 4>, fraction: vec2<f32>, footprint: vec2<f32>, threshold: f32) -> CutoutSample {
    let weight = clamp((fraction - vec2(0.5)) / max(footprint, vec2(0.00001)) + vec2(0.5), vec2(0.0), vec2(1.0));
    let mask = vec4<f32>(vec4(texels[0].a, texels[1].a, texels[2].a, texels[3].a) >= vec4(threshold));
    let coverage = mix(mix(mask.x, mask.y, weight.x), mix(mask.z, mask.w, weight.x), weight.y);
    var colour = texels[0];
    var nearest = 1000000.0;
    for (var index = 0u; index < 4u; index++) {
        let offset = fraction - vec2(f32(index & 1u), f32(index >> 1u));
        let distance_squared = dot(offset, offset);
        if (texels[index].a >= threshold && distance_squared < nearest) {
            colour = texels[index];
            nearest = distance_squared;
        }
    }
    return CutoutSample(coverage, colour);
}

// Animated cutout frames mix coverage without introducing transparent RGB into recovered edges.
fn cutout_mix(first: CutoutSample, second: CutoutSample, blend: f32) -> CutoutSample {
    let coverage = mix(first.coverage, second.coverage, blend);
    let colour = mix(first.colour * first.coverage, second.colour * second.coverage, blend) / max(coverage, 0.00001);
    return CutoutSample(coverage, colour);
}

// Minified cutouts cannot cover more samples than their stored mip alpha permits.
fn cutout_mip_coverage(alpha: f32, threshold: f32) -> f32 {
    return min(alpha, clamp(alpha - threshold + 0.5, 0.0, 1.0));
}

// Minification keeps mip coverage; magnification filters only the binary alpha contour.
fn cutout_alpha_array(image: texture_2d_array<f32>, uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>, sampled: vec4<f32>, repeat_uv: bool, threshold: f32) -> CutoutSample {
    let limit = vec2<i32>(textureDimensions(image, 0)) - vec2(1);
    return cutout_alpha_array_region(image, uv, layer, dx, dy, sampled, repeat_uv, threshold, vec4(vec2(0), limit));
}

// An authored face chart repeats its border texels, preserving contours inside that chart.
fn cutout_alpha_array_region(image: texture_2d_array<f32>, uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>, sampled: vec4<f32>, repeat_uv: bool, threshold: f32, region: vec4<i32>) -> CutoutSample {
    let size = vec2<f32>(textureDimensions(image, 0));
    let texel_dx = dx * size;
    let texel_dy = dy * size;
    if (max(length(texel_dx), length(texel_dy)) >= 1.0) {
        return CutoutSample(cutout_mip_coverage(sampled.a, threshold), sampled);
    }
    let position = uv * size - vec2(0.5);
    let base = vec2<i32>(floor(position));
    let texels = array(
        textureLoad(image, cutout_region_texel_address(base, region, vec2<i32>(size), repeat_uv), layer, 0),
        textureLoad(image, cutout_region_texel_address(base + vec2(1, 0), region, vec2<i32>(size), repeat_uv), layer, 0),
        textureLoad(image, cutout_region_texel_address(base + vec2(0, 1), region, vec2<i32>(size), repeat_uv), layer, 0),
        textureLoad(image, cutout_region_texel_address(base + vec2(1), region, vec2<i32>(size), repeat_uv), layer, 0),
    );
    return cutout_sample(texels, fract(position), abs(texel_dx) + abs(texel_dy), threshold);
}


// Clamp sampling stays inside its chart; repeating materials retain whole-image modulo.
fn cutout_region_texel_address(coordinate: vec2<i32>, region: vec4<i32>, size: vec2<i32>, repeat_uv: bool) -> vec2<i32> {
    return select(clamp(coordinate, region.xy, region.zw), cutout_texel_address(coordinate, size, true), repeat_uv);
}

// Signed modulo keeps neighbours inside the same array layer at a repeating seam.
fn cutout_texel_address(coordinate: vec2<i32>, size: vec2<i32>, repeat_uv: bool) -> vec2<i32> {
    return select(clamp(coordinate, vec2(0), size - vec2(1)), (coordinate % size + size) % size, repeat_uv);
}

// Atlas cutouts use the same one-pixel contour and recover covered RGB at transparent texels.
fn cutout_alpha_2d(image: texture_2d<f32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, sampled: vec4<f32>, threshold: f32) -> CutoutSample {
    let size = vec2<f32>(textureDimensions(image, 0));
    let texel_dx = dx * size;
    let texel_dy = dy * size;
    if (max(length(texel_dx), length(texel_dy)) >= 1.0) {
        return CutoutSample(cutout_mip_coverage(sampled.a, threshold), sampled);
    }
    let position = uv * size - vec2(0.5);
    let base = vec2<i32>(floor(position));
    let limit = vec2<i32>(size) - vec2(1);
    let texels = array(
        textureLoad(image, clamp(base, vec2(0), limit), 0),
        textureLoad(image, clamp(base + vec2(1, 0), vec2(0), limit), 0),
        textureLoad(image, clamp(base + vec2(0, 1), vec2(0), limit), 0),
        textureLoad(image, clamp(base + vec2(1), vec2(0), limit), 0),
    );
    return cutout_sample(texels, fract(position), abs(texel_dx) + abs(texel_dy), threshold);
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

struct LightmapUniform {
    raw: array<vec4<f32>, 256>,
    native: array<vec4<f32>, 256>,
}
@group(1) @binding(0) var<uniform> world_lightmap: LightmapUniform;

// Both nibbles address the same environment table in every ordinary world pass.
fn light_colour(sample: u32) -> vec3<f32> {
    if ((sample & JAVA_PACKED_LIGHT_FLAG) != 0u) {
        let levels = vec2(f32(sample & 63u), f32((sample >> 6u) & 63u)) * 0.25;
        let lower = vec2<u32>(floor(levels));
        let upper = min(lower + vec2(1u), vec2(15u));
        let blend = fract(levels);
        return mix(
            mix(world_lightmap.raw[lower.x | (lower.y << 4u)].rgb, world_lightmap.raw[upper.x | (lower.y << 4u)].rgb, blend.x),
            mix(world_lightmap.raw[lower.x | (upper.y << 4u)].rgb, world_lightmap.raw[upper.x | (upper.y << 4u)].rgb, blend.x), blend.y);
    }
    return world_lightmap.raw[sample & 255u].rgb;
}

// Old and replacement meshes carry their own sample format during a style transition.
fn sample_ao(sample: u32) -> u32 {
    return select((sample >> 8u) & 7u, (sample >> 12u) & 7u, (sample & JAVA_PACKED_LIGHT_FLAG) != 0u);
}

// The Java record uses this bit for its sky coordinate rather than an emitter flag.
fn sample_emits(sample: u32) -> bool {
    return (sample & JAVA_PACKED_LIGHT_FLAG) == 0u && (sample & 2048u) != 0u;
}

// Native consumers read the byte-truncated table prepared once at upload.
fn native_light_texel(sample: u32) -> vec3<f32> {
    return world_lightmap.native[sample & 255u].rgb;
}

// Current terrain emitters pack two four-bit levels. Installed near-version
// 1.26.51.01 RenderChunk Metal divides each by15 in the vertex shader, then
// samples the clamp-linear16x16 byte texture in the fragment shader. Unlike
// actors' /16 lookup, interpolate levels before this nonlinear table lookup.
const TERRAIN_LIGHTMAP_SIDE: u32 = 1u << 4u;

fn terrain_light_levels(sample: u32) -> vec2<f32> {
    if ((sample & JAVA_PACKED_LIGHT_FLAG) != 0u) { return vec2(f32(sample & 63u), f32((sample >> 6u) & 63u)) * 0.25; }
    return vec2(f32(sample & 15u), f32((sample >> 4u) & 15u));
}

fn terrain_light_colour(levels: vec2<f32>) -> vec3<f32> {
    let last = TERRAIN_LIGHTMAP_SIDE - 1u;
    var coordinate = clamp(levels * (f32(TERRAIN_LIGHTMAP_SIDE) / f32(last)) - vec2(0.5), vec2(0.0), vec2(f32(last)));
    if (world_atmosphere.java_fog.x > 0.0) { coordinate = clamp(levels, vec2(0.0), vec2(f32(last))); }
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
    if ((sample & (ACTOR_LIGHT_WORLD_FLAG | ACTOR_LIGHT_DIRECTIONAL_FLAG)) == 0u) { return vec3(1.0); }
    var illumination = vec3(1.0);
    if ((sample & ACTOR_LIGHT_WORLD_FLAG) != 0u) {
        if (world_atmosphere.java_fog.x > 0.0) { illumination = light_colour(sample); }
        else { illumination = actor_light_colour(sample); }
    }
    if (world_atmosphere.java_fog.x > 0.0) {
        let first = normalize(vec3(0.2, 1.0, -0.7));
        let second = normalize(vec3(-0.2, select(1.0, -1.0, u32(world_atmosphere.sky_extra.w) % 4u == 1u), 0.7));
        return illumination * min(1.0, 0.4 + 0.6 * (max(0.0, dot(normal, first)) + max(0.0, dot(normal, second))));
    }
    let shade = (((1.0 + normal.y) * ACTOR_SHADE[0] + normal.x * normal.x * ACTOR_SHADE[1]) + normal.z * normal.z * ACTOR_SHADE[2]) + ACTOR_SHADE[3] + overlay_alpha * ACTOR_SHADE[4];
    return illumination * shade;
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
    if (world_atmosphere.java_fog.x > 0.0) {
        if (abs(normal.y) > 0.5 && u32(world_atmosphere.sky_extra.w) % 4u == 1u) { return 0.9; }
        if (normal.y < -0.5) { return 0.5; }
        if (abs(normal.x) > 0.5) { return 0.6; }
        if (abs(normal.z) > 0.5) { return 0.8; }
        return 1.0;
    }
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
    liquid_distance: vec4<f32>,
    java_fog: vec4<f32>,
    java_sky: vec4<f32>,
    java_cloud: vec4<f32>,
}
@group(1) @binding(1) var<uniform> world_atmosphere: WorldAtmosphere;

// The optional preset combines spherical atmosphere fog and cylindrical view-distance fog.
fn distance_fog_weight(delta: vec3<f32>, start: f32, end: f32, java: vec4<f32>) -> f32 {
    let spherical = length(delta);
    var render_distance = spherical;
    if (java.x > 0.0) { render_distance = max(length(delta.xz), abs(delta.y)); }
    let render_fog = clamp((render_distance - start) / max(end - start, 0.0001), 0.0, 1.0);
    if (java.x > 0.0) {
        return max(render_fog, clamp((spherical - java.y) / max(java.z - java.y, 0.0001), 0.0, 1.0));
    }
    return render_fog;
}

// Uses the same distance fade as the terrain passes after lighting and overlays.
fn world_fog_amount(position: vec3<f32>, camera: vec3<f32>) -> f32 {
    let start = world_atmosphere.fog_color_start.w;
    let end = world_atmosphere.fog_end_time.x;
    return distance_fog_weight(position - camera, start, end, world_atmosphere.java_fog);
}

fn world_distance_fog(colour: vec3<f32>, position: vec3<f32>, camera: vec3<f32>) -> vec3<f32> {
    return mix(colour, world_atmosphere.fog_color_start.rgb, world_fog_amount(position, camera));
}

fn actor_distance_fog(colour_gamma: vec3<f32>, position: vec3<f32>, camera: vec3<f32>) -> vec3<f32> {
    let fog_gamma = tint_to_gamma(vec4(world_atmosphere.fog_color_start.rgb, 1.0)).rgb;
    return mix(colour_gamma, fog_gamma, world_fog_amount(position, camera));
}


struct JavaWaterInfo { frames: array<vec4<u32>, JAVA_WATER_KINDS> }
@group(1) @binding(2) var java_water_texture: texture_2d_array<f32>;
@group(1) @binding(3) var java_water_sampler: sampler;
@group(1) @binding(4) var<uniform> java_water_info: JavaWaterInfo;

// Samples one regular flipbook without filtering between neighbouring animation frames.
fn java_water_sample(kind: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, tick: u32) -> vec4<f32> {
    let info = java_water_info.frames[kind - 1u];
    let frame = (tick / max(info.z, 1u)) % max(info.y, 1u);
    let scale = f32(info.w) / f32(textureDimensions(java_water_texture, 0).x);
    return textureSampleGrad(java_water_texture, java_water_sampler, fract(uv) * scale, i32(info.x + frame), dx * scale, dy * scale);
}
