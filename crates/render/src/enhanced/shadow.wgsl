#define_import_path cinnabar::enhanced_shadow

#import cinnabar::enhanced_common::{EnhancedFrame, FEATURE_SHADOWS}
#import cinnabar::enhanced_atmosphere::ATM_SUN_RADIUS

const SHADOW_TAPS: u32 = 12u;
const BLOCKER_TAPS: u32 = 8u;

// Receiver-fixed rotation survives camera reprojection without crawling PCF noise.
fn shadow_kernel_rotation(world:vec3<f32>)->f32 {
    let cell=vec3<i32>(floor(world*64.0));
    var hash=bitcast<u32>(cell.x)*73856093u ^ bitcast<u32>(cell.y)*19349663u ^ bitcast<u32>(cell.z)*83492791u;
    hash^=hash>>16u;hash*=2246822519u;hash^=hash>>13u;
    return f32(hash&65535u)/65536.0;
}

// Orthographic rows recover the receiver plane without screen derivatives.
fn shadow_receiver_gradient(matrix: mat4x4<f32>, normal: vec3<f32>) -> vec2<f32> {
    let row_x = vec3(matrix[0].x, matrix[1].x, matrix[2].x);
    let row_y = vec3(matrix[0].y, matrix[1].y, matrix[2].y);
    let row_z = vec3(matrix[0].z, matrix[1].z, matrix[2].z);
    let basis_x = row_x / max(dot(row_x, row_x), 1.0e-12);
    let basis_y = row_y / max(dot(row_y, row_y), 1.0e-12);
    let basis_z = row_z / max(dot(row_z, row_z), 1.0e-12);
    let denominator = dot(normal, basis_z);
    if (!(abs(denominator) > length(basis_z) * 0.01)) { return vec2(0.0); }
    let gradient = vec2(-2.0 * dot(normal, basis_x), 2.0 * dot(normal, basis_y)) / denominator;
    if (!all(abs(gradient) < vec2(1.0e6))) { return vec2(0.0); }
    return gradient;
}

// Directional PCSS converts blocker separation to a physical solar penumbra.
fn shadow_filter_radius(frame: EnhancedFrame, shadow_map: texture_depth_2d_array, uv: vec2<f32>, depth: f32, gradient: vec2<f32>, cascade: u32, noise: f32) -> f32 {
    let size = vec2<i32>(textureDimensions(shadow_map));
    var blockers = 0.0;
    var blocker_separation = 0.0;
    for (var tap = 0u; tap < BLOCKER_TAPS; tap += 1u) {
        let angle = f32(tap) * 2.3999632 + noise * 6.2831853;
        let radius = sqrt((f32(tap) + 0.5) / f32(BLOCKER_TAPS)) * max(frame.cascade_depth_scale.w - 2.0, 1.0);
        let coord = clamp(vec2<i32>(uv * vec2<f32>(size)
            + vec2(cos(angle), sin(angle)) * radius), vec2(0), size - vec2(1));
        let sample = textureLoad(shadow_map, coord, i32(cascade), 0);
        let sample_uv = (vec2<f32>(coord) + vec2(0.5)) / vec2<f32>(size);
        let receiver_depth = depth + dot(gradient, sample_uv - uv);
        if (sample < receiver_depth) {
            blocker_separation += receiver_depth - sample;
            blockers += 1.0;
        }
    }
    if (blockers <= 0.0) { return 1.0; }
    let separation = (blocker_separation / blockers)
        / max(frame.cascade_depth_scale[cascade], 1.0e-6);
    let penumbra = separation * ATM_SUN_RADIUS / max(frame.cascade_texel[cascade], 1.0e-4);
    return clamp(0.85 + penumbra, 0.85, max(frame.cascade_depth_scale.w - 2.0, 1.0));
}

fn filtered_shadow(frame: EnhancedFrame, shadow_map: texture_depth_2d_array, shadow_sampler: sampler_comparison, uv: vec2<f32>, depth: f32, gradient: vec2<f32>, cascade: u32, noise: f32, radius_texels: f32) -> f32 {
    let texel_size = vec2(1.0) / vec2<f32>(textureDimensions(shadow_map));
    // Hardware PCF compares four texel centres against one reference depth.
    let footprint_bias = 0.5 * dot(abs(gradient), texel_size);
    let rotation = noise * 6.2831853;
    var sum = 0.0;
    for (var tap = 0u; tap < SHADOW_TAPS; tap += 1u) {
        let radius = sqrt((f32(tap) + 0.5) / f32(SHADOW_TAPS)) * radius_texels;
        let angle = f32(tap) * 2.3999632 + rotation;
        let offset = vec2(cos(angle), sin(angle)) * radius * texel_size;
        sum += textureSampleCompareLevel(
            shadow_map,
            shadow_sampler,
            uv + offset,
            i32(cascade),
            depth + dot(gradient, offset) - footprint_bias,
        );
    }
    return sum / f32(SHADOW_TAPS);
}

fn shadow_cascade_sample(
    frame: EnhancedFrame,
    shadow_map: texture_depth_2d_array,
    shadow_sampler: sampler_comparison,
    world: vec3<f32>,
    normal: vec3<f32>,
    cascade: u32,
    noise: f32,
) -> vec3<f32> {
    let texel = frame.cascade_texel[cascade];
    let grazing = 1.0 - clamp(dot(normal, frame.light_direction.xyz), 0.0, 1.0);
    // Bound world displacement so thin ledges retain their contact shadow.
    let offset = normal * min(texel * (0.15 + 0.45 * grazing), 0.075);
    let clip = frame.cascade_clip_from_world[cascade] * vec4(world + offset, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let margin = max(frame.cascade_depth_scale.w, 1.0) / f32(max(frame.flags.z, 1u));
    if (!all(uv > vec2(margin)) || !all(uv < vec2(1.0 - margin)) || ndc.z <= 0.0 || ndc.z >= 1.0) {
        return vec3(-1.0, 0.0, 0.0);
    }
    let bias = min(texel * (0.12 + 0.3 * grazing), 0.04) * frame.cascade_depth_scale[cascade];
    let depth = ndc.z - bias;
    let gradient = shadow_receiver_gradient(frame.cascade_clip_from_world[cascade], normal);
    let radius = shadow_filter_radius(frame, shadow_map, uv, depth, gradient, cascade, noise);
    let visibility = filtered_shadow(frame, shadow_map, shadow_sampler, uv, depth, gradient, cascade, noise, radius);
    return vec3(visibility, uv.x, uv.y);
}

// Recover radial receiver coverage from the padded, snapped projection grid.
fn shadow_receiver_radius(frame: EnhancedFrame, cascade: u32) -> f32 {
    let usable = max(f32(frame.flags.z) - 2.0 * frame.cascade_depth_scale.w - 1.0, 1.0);
    return frame.cascade_texel[cascade] * usable * 0.5;
}

fn shadow_cascade_blend(frame: EnhancedFrame, cascade: u32, receiver_distance: f32) -> f32 {
    let radius = max(shadow_receiver_radius(frame, cascade), 0.001);
    return smoothstep(radius * 0.85, radius, receiver_distance);
}

// Nested radial coverage keeps selection independent of camera yaw and pitch.
fn sun_shadow_sample(frame: EnhancedFrame, shadow_map: texture_depth_2d_array,
    shadow_sampler: sampler_comparison, world: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>) -> vec2<f32> {
    if ((frame.flags.x & FEATURE_SHADOWS) == 0u) {
        return vec2(1.0, -1.0);
    }
    let fade_distance = frame.cascade_texel.w;
    let camera_distance = distance(world, frame.camera_time.xyz);
    if (camera_distance >= fade_distance) {
        return vec2(1.0, -1.0);
    }
    let noise = shadow_kernel_rotation(world);
    for (var cascade = 0u; cascade < frame.flags.y; cascade += 1u) {
        if (cascade + 1u < frame.flags.y && camera_distance >= shadow_receiver_radius(frame, cascade)) {
            continue;
        }
        let sample = shadow_cascade_sample(frame, shadow_map, shadow_sampler, world, normal, cascade, noise);
        if (sample.x >= 0.0) {
            var visibility = sample.x;
            // Both maps fully cover the sphere throughout this overlap.
            if (cascade + 1u < frame.flags.y) {
                let blend = shadow_cascade_blend(frame, cascade, camera_distance);
                if (blend > 0.0) {
                    let next = shadow_cascade_sample(frame, shadow_map, shadow_sampler, world, normal, cascade + 1u, noise);
                    if (next.x >= 0.0) {
                        visibility = mix(visibility, next.x, blend);
                    }
                }
            }
            return vec2(mix(visibility, 1.0, smoothstep(fade_distance * 0.82, fade_distance, camera_distance)), f32(cascade));
        }
    }
    return vec2(1.0, -1.0);
}

