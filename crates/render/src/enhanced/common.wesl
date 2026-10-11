
// Mirrors `EnhancedFrameGpu` in enhanced/frame.rs.
struct EnhancedFrame {
    clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    cascade_clip_from_world: array<mat4x4<f32>, 3>,
    cascade_texel: vec4<f32>,
    cascade_depth_scale: vec4<f32>,
    camera_time: vec4<f32>,
    light_direction: vec4<f32>,
    light_colour: vec4<f32>,
    ambient_colour: vec4<f32>,
    viewport: vec4<f32>,
    grade: vec4<f32>,
    flags: vec4<u32>,
    projection: vec4<f32>,
}

const FEATURE_SHADOWS: u32 = 1u;
const FEATURE_BLOOM: u32 = 2u;
const FEATURE_SHAFTS: u32 = 4u;
const FEATURE_WAVING: u32 = 8u;
const FEATURE_WATER: u32 = 16u;

// Mirrors enhanced/materials.rs.
const CLASS_EMISSION_MASK: u32 = 15u;
const CLASS_LEAVES: u32 = 16u;
const CLASS_PLANT: u32 = 32u;
const CLASS_WATER: u32 = 64u;
const CLASS_LAVA: u32 = 128u;

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

// Gentle downward-only bob for water surfaces.
fn water_surface_offset(world: vec3<f32>, seconds: f32) -> f32 {
    let wave = sin(world.x * 0.9 + seconds * 1.3) * sin(world.z * 0.7 + seconds * 1.1);
    return -0.025 * (0.5 + 0.5 * wave);
}
