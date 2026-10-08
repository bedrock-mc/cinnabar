#define_import_path cinnabar::enhanced_common

// Mirrors `EnhancedFrameGpu` in enhanced/frame.rs.
struct EnhancedFrame {
    clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    cascade_clip_from_world: array<mat4x4<f32>, 3>,
    cascade_texel: vec4<f32>,
    cascade_depth_scale: vec4<f32>,
    cascade_receiver_radius: vec4<f32>,
    camera_time: vec4<f32>,
    light_direction: vec4<f32>,
    light_colour: vec4<f32>,
    ambient_colour: vec4<f32>,
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    viewport: vec4<f32>,
    grade: vec4<f32>,
    flags: vec4<u32>, // Features, cascades, shadow resolution, cloud view steps.
    projection: vec4<f32>, // Reverse-Z near, weathered solar/full-moon irradiance, visibility ready.
    clouds: vec4<f32>,
    previous_clip_from_world: mat4x4<f32>,
    celestial: vec4<f32>,
    atmosphere: vec4<f32>,
    temporal: vec4<f32>,
    probe: vec4<f32>,
    cloud_shadow: vec4<f32>,
    quality: vec4<f32>, // SSR, directional PCF, blocker and point filter budgets.
}

const FEATURE_SHADOWS: u32 = 1u;
const FEATURE_BLOOM: u32 = 2u;
const FEATURE_SHAFTS: u32 = 4u;
const FEATURE_WAVING: u32 = 8u;
const FEATURE_WATER: u32 = 16u;
const FEATURE_PBR: u32 = 32u;
const FEATURE_SSAO: u32 = 64u;
const FEATURE_VOLUMETRIC_CLOUDS: u32 = 128u;

// Mirrors enhanced/materials.rs.
const CLASS_EMISSION_MASK: u32 = 15u;
const CLASS_LEAVES: u32 = 16u;
const CLASS_PLANT: u32 = 32u;
const CLASS_WATER: u32 = 64u;
const CLASS_LAVA: u32 = 128u;

fn encode_geometric_normal(normal: vec3<f32>) -> vec2<f32> {
    let projected = normal / max(dot(abs(normal), vec3(1.0)), 0.000001);
    let signs = select(vec2(-1.0), vec2(1.0), projected.xy >= vec2(0.0));
    return select(projected.xy, (vec2(1.0) - abs(projected.yx)) * signs, projected.z < 0.0);
}

fn decode_geometric_normal(encoded: vec2<f32>) -> vec3<f32> {
    var normal = vec3(encoded, 1.0 - abs(encoded.x) - abs(encoded.y));
    let signs = select(vec2(-1.0), vec2(1.0), normal.xy >= vec2(0.0));
    normal = vec3(select(normal.xy, (vec2(1.0) - abs(normal.yx)) * signs, normal.z < 0.0), normal.z);
    return normalize(normal);
}

// Per-pixel blue-ish noise in [0, 1) for rotating sample kernels.
fn interleaved_gradient_noise(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2(0.06711056, 0.00583715))));
}

// Wind displacement as a pure function of world position, so vertices shared
// by neighbouring quads move together and never open cracks.
fn wave_offset(world: vec3<f32>, surface_class: u32, weight: f32, seconds: f32, rain: f32) -> vec3<f32> {
    if ((surface_class & (CLASS_LEAVES | CLASS_PLANT)) == 0u || weight <= 0.0) {
        return vec3(0.0);
    }
    let phase = dot(world, vec3(0.61, 0.23, 0.37));
    let gust = 0.65 + 0.35 * sin(seconds * 0.45 + world.x * 0.031 + world.z * 0.027);
    let base = select(0.03, 0.065, (surface_class & CLASS_PLANT) != 0u);
    let amplitude = base * gust * (1.0 + rain) * weight;
    return vec3(
        sin(seconds * 1.9 + phase * 2.3),
        0.35 * sin(seconds * 2.6 + phase * 1.7),
        cos(seconds * 1.4 + phase * 1.9),
    ) * amplitude;
}
