#define_import_path cinnabar::enhanced_shadow

#import cinnabar::enhanced_common::{EnhancedFrame, FEATURE_SHADOWS}
#import cinnabar::enhanced_atmosphere::ATM_SUN_RADIUS

const SHADOW_TAPS: u32 = 12u;
const BLOCKER_TAPS: u32 = 8u;
const SHADOW_RECEIVER_OFFSET: f32 = 0.003;
const SHADOW_DEPTH_BIAS: f32 = 0.002;

// A smooth world field keeps neighbouring receivers coherent across reprojection.
fn shadow_kernel_rotation(world:vec3<f32>)->f32 {
    return 0.5 + 0.25*sin(dot(world,vec3(0.37,0.23,0.31)))
        + 0.25*sin(dot(world,vec3(0.13,0.41,0.19)));
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
    if (!(abs(denominator) > length(basis_z) * 0.0001)) { return vec2(0.0); }
    let gradient = vec2(-2.0 * dot(normal, basis_x), 2.0 * dot(normal, basis_y)) / denominator;
    if (!all(abs(gradient) < vec2(1.0e6))) { return vec2(0.0); }
    return gradient;
}

fn shadow_filter_limit(frame: EnhancedFrame) -> f32 {
    let margin = max(frame.cascade_depth_scale.w, 1.0);
    return frame.cascade_receiver_radius.w * max(margin - 2.0, 1.0) / margin;
}

fn shadow_footprint_weights(uv: vec2<f32>, size: vec2<i32>) -> vec4<f32> {
    let fraction = fract(uv * vec2<f32>(size) - vec2(0.5));
    return vec4((1.0 - fraction.x) * (1.0 - fraction.y), fraction.x * (1.0 - fraction.y),
        (1.0 - fraction.x) * fraction.y, fraction.x * fraction.y);
}

// Compare at each stored texel's receiver plane, before interpolating visibility.
fn shadow_depth_footprint(shadow_map: texture_depth_2d_array, uv: vec2<f32>,
    depth: f32, gradient: vec2<f32>, cascade: u32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(shadow_map));
    let base = vec2<i32>(floor(uv * vec2<f32>(size) - vec2(0.5)));
    let offsets = array<vec2<i32>, 4>(vec2(0, 0), vec2(1, 0), vec2(0, 1), vec2(1, 1));
    var separation = vec4(0.0);
    for (var corner = 0u; corner < 4u; corner += 1u) {
        let coord = clamp(base + offsets[corner], vec2(0), size - vec2(1));
        let stored = textureLoad(shadow_map, coord, i32(cascade), 0);
        let sample_uv = (vec2<f32>(coord) + vec2(0.5)) / vec2<f32>(size);
        separation[corner] = select(depth + dot(gradient, sample_uv - uv) - stored,
            -1.0, stored >= 1.0);
    }
    return separation;
}

fn shadow_plane_visibility(shadow_map: texture_depth_2d_array, shadow_sampler: sampler_comparison,
    uv: vec2<f32>, depth: f32, gradient: vec2<f32>, cascade: u32) -> f32 {
    let size = vec2<i32>(textureDimensions(shadow_map));
    if (dot(abs(gradient), 1.0 / vec2<f32>(size)) < 1.0e-8) {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, i32(cascade), depth);
    }
    let difference = shadow_depth_footprint(shadow_map, uv, depth, gradient, cascade);
    return dot(shadow_footprint_weights(uv, size), select(vec4(0.0), vec4(1.0), difference <= vec4(0.0)));
}

// Directional PCSS converts blocker separation to a physical solar penumbra.
fn shadow_filter_radius(frame: EnhancedFrame, shadow_map: texture_depth_2d_array, uv: vec2<f32>, depth: f32, gradient: vec2<f32>, cascade: u32, noise: f32) -> f32 {
    let size = vec2<i32>(textureDimensions(shadow_map));
    let cascade_texel = max(frame.cascade_texel[cascade], 0.0001);
    let search_world = shadow_filter_limit(frame);
    let minimum_world = 0.85 * max(frame.cascade_texel[0], 0.0001);
    var blockers = 0.0;
    var blocker_separation = 0.0;
    let taps=select(BLOCKER_TAPS,u32(clamp(frame.quality.z,1.0,16.0)),frame.quality.z>0.0);
    for (var tap = 0u; tap < taps; tap += 1u) {
        let angle = f32(tap) * 2.3999632 + noise * 6.2831853;
        let radius = sqrt((f32(tap) + 0.5) / f32(taps)) * search_world / cascade_texel;
        let sample_uv = uv + vec2(cos(angle), sin(angle)) * radius / vec2<f32>(size);
        let sample_depth = depth + dot(gradient, sample_uv - uv);
        let separation = max(shadow_depth_footprint(shadow_map, sample_uv, sample_depth, gradient, cascade)
            / max(frame.cascade_depth_scale[cascade], 1.0e-6), vec4(0.0));
        let confidence = smoothstep(vec4(SHADOW_DEPTH_BIAS), vec4(SHADOW_DEPTH_BIAS * 4.0), separation);
        let weight = shadow_footprint_weights(sample_uv, size) * confidence;
        blocker_separation += dot(weight, separation);
        blockers += dot(weight, vec4(1.0));
    }
    if (blockers <= 0.0) { return minimum_world / cascade_texel; }
    // An entering blocker contributes continuously instead of switching kernel width.
    let separation = blocker_separation / max(blockers, 1.0);
    let penumbra = separation * ATM_SUN_RADIUS;
    return clamp(minimum_world + penumbra, minimum_world, search_world) / cascade_texel;
}

fn filtered_shadow(frame: EnhancedFrame, shadow_map: texture_depth_2d_array, shadow_sampler: sampler_comparison, uv: vec2<f32>, depth: f32, gradient: vec2<f32>, cascade: u32, noise: f32, radius_texels: f32) -> f32 {
    let texel_size = vec2(1.0) / vec2<f32>(textureDimensions(shadow_map));
    let rotation = noise * 6.2831853;
    var sum = 0.0;
    let taps=select(SHADOW_TAPS,u32(clamp(frame.quality.y,1.0,32.0)),frame.quality.y>0.0);
    for (var tap = 0u; tap < taps; tap += 1u) {
        let radius = sqrt((f32(tap) + 0.5) / f32(taps)) * radius_texels;
        let angle = f32(tap) * 2.3999632 + rotation;
        let offset = vec2(cos(angle), sin(angle)) * radius * texel_size;
        sum += shadow_plane_visibility(shadow_map, shadow_sampler, uv + offset,
            depth + dot(gradient, offset), gradient, cascade);
    }
    return sum / f32(taps);
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
    let receiver_normal = select(-normal, normal, dot(normal, frame.light_direction.xyz) >= 0.0);
    let grazing = 1.0 - clamp(dot(receiver_normal, frame.light_direction.xyz), 0.0, 1.0);
    // Camera movement and cascade handovers cannot move the geometric receiver.
    let offset = receiver_normal * SHADOW_RECEIVER_OFFSET * (0.5 + 0.5 * grazing);
    let clip = frame.cascade_clip_from_world[cascade] * vec4(world + offset, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let margin = (shadow_filter_limit(frame) + 0.5 * texel)
        / max(texel * f32(frame.flags.z), 0.0001);
    if (!all(uv > vec2(margin)) || !all(uv < vec2(1.0 - margin)) || ndc.z <= 0.0 || ndc.z >= 1.0) {
        return vec3(-1.0, 0.0, 0.0);
    }
    let bias = SHADOW_DEPTH_BIAS * frame.cascade_depth_scale[cascade];
    let depth = ndc.z - bias;
    let gradient = shadow_receiver_gradient(frame.cascade_clip_from_world[cascade], normal);
    let radius = shadow_filter_radius(frame, shadow_map, uv, depth, gradient, cascade, noise);
    let visibility = filtered_shadow(frame, shadow_map, shadow_sampler, uv, depth, gradient, cascade, noise, radius);
    return vec3(visibility, uv.x, uv.y);
}

// Receiver selection excludes the independent caster/filter guard band.
fn shadow_receiver_radius(frame: EnhancedFrame, cascade: u32) -> f32 {
    return frame.cascade_receiver_radius[cascade];
}

fn shadow_cascade_blend(frame: EnhancedFrame, cascade: u32, receiver_distance: f32) -> f32 {
    let radius = max(shadow_receiver_radius(frame, cascade), 0.001);
    return smoothstep(radius * 0.85, radius, receiver_distance);
}

fn shadow_cascade_coverage(frame: EnhancedFrame, sample: vec3<f32>, cascade: u32) -> f32 {
    let span = max(frame.cascade_texel[cascade] * f32(frame.flags.z), 0.0001);
    let margin = (shadow_filter_limit(frame) + 0.5 * frame.cascade_texel[cascade]) / span;
    let border = frame.cascade_receiver_radius.w / span;
    let edge = min(min(sample.y, sample.z), min(1.0 - sample.y, 1.0 - sample.z));
    return smoothstep(margin, max(border, margin + 0.000001), edge);
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
                let blend = max(shadow_cascade_blend(frame, cascade, camera_distance),
                    1.0 - shadow_cascade_coverage(frame, sample, cascade));
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

