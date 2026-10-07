#import cinnabar::enhanced_common::EnhancedFrame
@group(0) @binding(0) var<uniform> frame: EnhancedFrame;
@group(0) @binding(1) var scene: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var<storage, read_write> histogram: array<atomic<u32>, 64>;
@group(0) @binding(4) var<storage, read_write> exposure: vec4<f32>;

fn exposure_limits(daylight: f32) -> vec2<f32> {
    let day = clamp(daylight, 0.0, 1.0);
    return vec2(mix(0.18, 0.06, day), mix(3.5, 1.4, day));
}

// Night retains a lower display midpoint and a bounded gain even in unlit scenes.
fn exposure_policy(mean_luminance: f32, daylight: f32, rain: f32) -> vec3<f32> {
    let day = clamp(daylight, 0.0, 1.0);
    let midpoint = mix(0.055, 0.16, day) * (1.0 - 0.12 * clamp(rain, 0.0, 1.0));
    let limits = exposure_limits(day);
    return vec3(clamp(midpoint / max(mean_luminance, exp2(-12.0)), limits.x, limits.y),
        midpoint, limits.y);
}

// A metered highlight protects ordinary texture whites without treating rare lamps as gray.
fn highlight_exposure_policy(mean_luminance: f32, highlight: f32, daylight: f32, rain: f32) -> vec3<f32> {
    let day = clamp(daylight, 0.0, 1.0);
    let policy = exposure_policy(mean_luminance, day, rain);
    let white_limit = mix(2.0, 0.8, day);
    let gain = max(min(policy.x, white_limit / max(highlight, exp2(-12.0))), exposure_limits(day).x);
    return vec3(gain, policy.yz);
}

fn adapted_exposure(previous: f32, desired: f32, delta_seconds: f32, valid_history: bool) -> f32 {
    if (!valid_history) { return desired; }
    let safe_previous = max(previous, 0.001);
    let speed = select(1.2, 3.0, desired < safe_previous);
    let weight = 1.0 - exp(-clamp(delta_seconds, 0.0, 0.1) * speed);
    return mix(safe_previous, desired, weight);
}

@compute @workgroup_size(8, 8)
fn build_histogram(@builtin(global_invocation_id) id: vec3<u32>) {
    if (frame.temporal.w > 0.5) { return; }
    let size = textureDimensions(scene);
    let pixel = id.xy * 4u + vec2(2u);
    if (any(pixel >= size)) { return; }
    let d = textureLoad(depth, vec2<i32>(pixel), 0);
    if (d <= 0.00001) { return; }
    let rgb = max(textureLoad(scene, vec2<i32>(pixel), 0).rgb, vec3(0.0));
    let lum = clamp(dot(rgb, vec3(0.2126, 0.7152, 0.0722)), exp2(-12.0), exp2(8.0));
    let bin = min(u32((log2(lum) + 12.0) * (64.0 / 20.0)), 63u);
    let uv = vec2<f32>(pixel) / vec2<f32>(size) - vec2(0.5);
    let weight = 1u + u32(3.0 * exp(-dot(uv, uv) * 8.0));
    atomicAdd(&histogram[bin], weight);
}

@compute @workgroup_size(1)
fn adapt_exposure() {
    if (frame.temporal.w > 0.5) { return; }
    var count = 0u;
    for (var bin = 0u; bin < 64u; bin += 1u) { count += atomicLoad(&histogram[bin]); }
    var weighted_log = 0.0;
    var accepted = 0.0;
    var cumulative = 0.0;
    var highlight_log = 0.0;
    let low = f32(count) * 0.1;
    let high = f32(count) * 0.9;
    for (var bin = 0u; bin < 64u; bin += 1u) {
        let amount = f32(atomicLoad(&histogram[bin]));
        if (cumulative < high && cumulative + amount >= high) {
            highlight_log = -12.0 + (f32(bin) + 1.0) * (20.0 / 64.0);
        }
        let used = max(min(cumulative + amount, high) - max(cumulative, low), 0.0);
        weighted_log += (-12.0 + (f32(bin) + 0.5) * (20.0 / 64.0)) * used;
        accepted += used;
        cumulative += amount;
    }
    let day = select(1.0, smoothstep(-0.12, 0.14, frame.celestial.y), frame.atmosphere.x > 0.5);
    let average = exp2(weighted_log / max(accepted, 1.0));
    let policy = highlight_exposure_policy(average, exp2(highlight_log), day, frame.ambient_colour.w);
    var desired_exposure = clamp(frame.grade.y, exposure_limits(day).x, policy.z);
    if (accepted > 0.0) { desired_exposure = policy.x; }
    exposure.x = adapted_exposure(exposure.x, desired_exposure, frame.temporal.z, frame.temporal.y > 0.5);
    exposure.y = desired_exposure;
    exposure.z = average;
    exposure.w = 1.0;
}
