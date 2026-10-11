// Enhanced post chain: half-resolution light shafts and
// the HDR composite with grading and a filmic shoulder.
import bevy_core_pipeline::fullscreen_vertex_shader::fullscreen::FullscreenVertexOutput;
import render::enhanced::common::{
    EnhancedFrame, FEATURE_SHAFTS, interleaved_gradient_noise,
};

@group(0) @binding(0) var<uniform> frame: EnhancedFrame;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var bloom_texture: texture_2d<f32>;
@group(0) @binding(4) var shaft_texture: texture_2d<f32>;
@group(0) @binding(5) var depth_texture: texture_depth_2d;
@group(0) @binding(6) var shadow_map: texture_depth_2d_array;
@group(0) @binding(7) var shadow_sampler: sampler_comparison;

const SHAFT_STEPS: u32 = 16u;
const SHAFT_ANISOTROPY: f32 = 0.72;
const SHAFT_DENSITY: f32 = 0.012;

// Linear Rec.709 luminance.
fn luminance(colour: vec3<f32>) -> f32 {
    return dot(colour, vec3(0.2126, 0.7152, 0.0722));
}

// Henyey-Greenstein single-scattering phase function.
fn phase_henyey_greenstein(cos_theta: f32) -> f32 {
    let g = SHAFT_ANISOTROPY;
    let denominator = 1.0 + g * g - 2.0 * g * cos_theta;
    return (1.0 - g * g) / (12.566371 * denominator * sqrt(denominator));
}

// Ray-marches the outermost shadow cascade from the camera to the surface.
@fragment
fn light_shafts(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    if ((frame.flags.x & FEATURE_SHAFTS) == 0u) {
        return vec4(0.0);
    }
    let size = vec2<i32>(textureDimensions(depth_texture));
    let coord = clamp(vec2<i32>(in.uv * vec2<f32>(size)), vec2(0), size - vec2(1));
    let depth = max(textureLoad(depth_texture, coord, 0), 1.0e-6);
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let world_h = frame.world_from_clip * vec4(ndc, depth, 1.0);
    let camera = frame.camera_time.xyz;
    let ray = world_h.xyz / world_h.w - camera;
    let ray_length = length(ray);
    let direction = ray / max(ray_length, 1.0e-4);
    let march = min(ray_length, frame.cascade_texel.w);
    let cascade = frame.flags.y - 1u;
    let bias = frame.cascade_texel[cascade] * frame.cascade_depth_scale[cascade];
    let noise = interleaved_gradient_noise(in.position.xy);
    var lit = 0.0;
    for (var step_index = 0u; step_index < SHAFT_STEPS; step_index += 1u) {
        let travel = (f32(step_index) + noise) / f32(SHAFT_STEPS) * march;
        let clip = frame.cascade_clip_from_world[cascade] * vec4(camera + direction * travel, 1.0);
        let shadow_ndc = clip.xyz / clip.w;
        let uv = vec2(shadow_ndc.x * 0.5 + 0.5, 0.5 - shadow_ndc.y * 0.5);
        if (all(uv > vec2(0.0)) && all(uv < vec2(1.0)) && shadow_ndc.z > 0.0 && shadow_ndc.z < 1.0) {
            lit += textureSampleCompareLevel(
                shadow_map,
                shadow_sampler,
                uv,
                i32(cascade),
                shadow_ndc.z - bias,
            );
        }
    }
    let visibility = lit / f32(SHAFT_STEPS);
    let scatter = visibility * phase_henyey_greenstein(dot(direction, frame.light_direction.xyz))
        * (1.0 - exp(-march * SHAFT_DENSITY));
    let strength = frame.light_direction.w * frame.grade.w * (1.0 - 0.6 * frame.ambient_colour.w);
    return vec4(frame.light_colour.rgb * scatter * strength, 1.0);
}

// Narkowicz's published ACES fit, evaluated in linear RGB (see README.md).
fn filmic_shoulder(colour: vec3<f32>) -> vec3<f32> {
    let positive = max(colour, vec3(0.0));
    let numerator = 2.51 * positive * positive + 0.03 * positive;
    let denominator = 2.43 * positive * positive + 0.59 * positive + vec3(0.14);
    return clamp(numerator / denominator, vec3(0.0), vec3(1.0));
}

// Warm daylight and cool night balance with mild contrast.
fn grade(colour: vec3<f32>, warmth: f32) -> vec3<f32> {
    let warm = vec3(1.06, 1.0, 0.9);
    let cool = vec3(0.9, 0.97, 1.08);
    let balance = mix(vec3(1.0), select(cool, warm, warmth > 0.0), min(abs(warmth), 1.0));
    let balanced = colour * balance;
    let saturated = mix(vec3(luminance(balanced)), balanced, 1.08);
    // Gentle contrast pivoting on mid-grey.
    return 0.18 * pow(max(saturated, vec3(0.0)) / 0.18, vec3(1.04));
}

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var colour = textureSampleLevel(source_texture, linear_sampler, in.uv, 0.0).rgb;
    if ((frame.flags.x & FEATURE_SHAFTS) != 0u) {
        colour += textureSampleLevel(shaft_texture, linear_sampler, in.uv, 0.0).rgb;
    }
    colour = filmic_shoulder(grade(colour * frame.grade.y, frame.grade.x));
    let vignette = mix(1.0, (1.0 - smoothstep(0.3, 0.95, length(in.uv - vec2(0.5)))), 0.22);
    let dither = (interleaved_gradient_noise(in.position.xy) - 0.5) / 255.0;
    return vec4(max(colour * vignette + dither, vec3(0.0)), 1.0);
}
