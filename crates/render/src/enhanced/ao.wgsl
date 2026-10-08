#define_import_path cinnabar::enhanced_ao

#import cinnabar::enhanced_common::{EnhancedFrame, interleaved_gradient_noise}

const AO_SLICE_COUNT: u32 = 4u;
const AO_STEPS_PER_SIDE: u32 = 2u;
const AO_RADIUS: f32 = 1.25;
const AO_MAX_PIXEL_RADIUS: f32 = 64.0;
const AO_PI: f32 = 3.141592654;
const CONTACT_STEPS: u32 = 8u;
const CONTACT_REACH: f32 = 1.4;
const CONTACT_STRENGTH: f32 = 0.35;
const CONTACT_REFINE_STEPS: u32 = 4u;

fn ao_depth_at(depth_texture: texture_depth_2d, uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_texture));
    let coord = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2(0), size - vec2(1));
    return textureLoad(depth_texture, coord, 0);
}

fn ao_texel_uv(depth_texture: texture_depth_2d, uv: vec2<f32>) -> vec2<f32> {
    let size = vec2<f32>(textureDimensions(depth_texture));
    return (clamp(floor(uv * size), vec2(0.0), size - vec2(1.0)) + vec2(0.5)) / size;
}

fn ao_world_position(frame: EnhancedFrame, uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec2(2.0 * uv.x - 1.0, 1.0 - 2.0 * uv.y);
    let world = frame.world_from_clip * vec4(ndc, max(depth, 1.0e-8), 1.0);
    return world.xyz / max(world.w, 1.0e-8);
}

fn ao_view_distance(frame: EnhancedFrame, depth: f32) -> f32 {
    return frame.projection.x / max(depth, 1.0e-8);
}

// Reverse-Z rays preserve small surface differences far from the world origin.
fn ao_camera_relative_position(frame: EnhancedFrame, uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec2(2.0 * uv.x - 1.0, 1.0 - 2.0 * uv.y);
    let ray = frame.world_from_clip * vec4(ndc, 0.0, 1.0);
    return ray.xyz * ao_view_distance(frame, depth);
}

// Choosing the closest derivative on each axis keeps foreground silhouettes
// from turning a background plane's normal toward the foreground object.
fn ao_surface_normal(
    frame: EnhancedFrame,
    depth_texture: texture_depth_2d,
    uv: vec2<f32>,
    depth: f32,
) -> vec3<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(depth_texture));
    let centre_uv = ao_texel_uv(depth_texture, uv);
    let centre = ao_camera_relative_position(frame, centre_uv, depth);
    let view = normalize(-centre);
    let distance = ao_view_distance(frame, depth);
    let left_uv = clamp(centre_uv - vec2(texel.x, 0.0), texel * 0.5, vec2(1.0) - texel * 0.5);
    let right_uv = clamp(centre_uv + vec2(texel.x, 0.0), texel * 0.5, vec2(1.0) - texel * 0.5);
    let up_uv = clamp(centre_uv - vec2(0.0, texel.y), texel * 0.5, vec2(1.0) - texel * 0.5);
    let down_uv = clamp(centre_uv + vec2(0.0, texel.y), texel * 0.5, vec2(1.0) - texel * 0.5);
    let left_depth = ao_depth_at(depth_texture, left_uv);
    let right_depth = ao_depth_at(depth_texture, right_uv);
    let up_depth = ao_depth_at(depth_texture, up_uv);
    let down_depth = ao_depth_at(depth_texture, down_uv);
    let left_error = abs(ao_view_distance(frame, left_depth) - distance);
    let right_error = abs(ao_view_distance(frame, right_depth) - distance);
    let up_error = abs(ao_view_distance(frame, up_depth) - distance);
    let down_error = abs(ao_view_distance(frame, down_depth) - distance);
    let horizontal_uv = select(left_uv, right_uv, right_error <= left_error);
    let horizontal_depth = select(left_depth, right_depth, right_error <= left_error);
    let vertical_uv = select(up_uv, down_uv, down_error <= up_error);
    let vertical_depth = select(up_depth, down_depth, down_error <= up_error);
    var dx = ao_camera_relative_position(frame, horizontal_uv, horizontal_depth) - centre;
    var dy = ao_camera_relative_position(frame, vertical_uv, vertical_depth) - centre;
    dx *= select(-1.0, 1.0, right_error <= left_error);
    dy *= select(-1.0, 1.0, down_error <= up_error);
    let unnormalized = cross(dx, dy);
    if (horizontal_depth <= 0.0 || vertical_depth <= 0.0 || dot(unnormalized, unnormalized) < 1.0e-14) {
        return view;
    }
    let normal = normalize(unnormalized);
    return normal * select(-1.0, 1.0, dot(normal, view) >= 0.0);
}

fn ao_temporal_noise(frame: EnhancedFrame, pixel: vec2<f32>) -> vec2<f32> {
    let frame_index = frame.temporal.x - floor(frame.temporal.x / 64.0) * 64.0;
    let noise = interleaved_gradient_noise(pixel);
    return fract(vec2(noise, interleaved_gradient_noise(pixel.yx + vec2(19.0, 43.0)))
        + frame_index * vec2(0.618033989, 0.754877666));
}

// Integral of cos(theta - normal_angle) * abs(sin(theta)) over one visible
// hemisphere arc; this preserves the surface normal's cosine weighting.
fn ao_arc_visibility(horizon: f32, normal_angle: f32) -> f32 {
    return 0.25 * (cos(normal_angle) - cos(2.0 * horizon - normal_angle)
        + 2.0 * horizon * sin(normal_angle));
}

// Returns ambient visibility in [0, 1]. Evaluate at half resolution and
// reconstruct with depth aware spatial and temporal filtering before lighting.
fn horizon_ao(
    frame: EnhancedFrame,
    depth_texture: texture_depth_2d,
    uv: vec2<f32>,
    depth: f32,
    pixel: vec2<f32>,
    geometric_normal: vec3<f32>,
) -> f32 {
    if (depth <= 1.0e-8) {
        return 1.0;
    }
    let centre_uv = ao_texel_uv(depth_texture, uv);
    let centre = ao_world_position(frame, centre_uv, depth);
    let view = normalize(frame.camera_time.xyz - centre);
    let normal = normalize(geometric_normal)
        * select(-1.0, 1.0, dot(geometric_normal, view) >= 0.0);
    let texel = 1.0 / vec2<f32>(textureDimensions(depth_texture));
    let screen_right = ao_world_position(frame, centre_uv + vec2(texel.x, 0.0), depth) - centre;
    let screen_down = ao_world_position(frame, centre_uv + vec2(0.0, texel.y), depth) - centre;
    let pixel_world = max(length(screen_right), length(screen_down));
    let projected_radius = AO_RADIUS / max(pixel_world, 1.0e-5);
    if (projected_radius < 1.5) {
        return 1.0;
    }
    let pixel_radius = min(projected_radius, AO_MAX_PIXEL_RADIUS);
    let origin = centre + normal * min(0.015 + pixel_world * 0.1, 0.06);
    let noise = ao_temporal_noise(frame, pixel);
    var visibility = 0.0;
    var unoccluded_visibility = 0.0;
    for (var slice_index = 0u; slice_index < AO_SLICE_COUNT; slice_index += 1u) {
        let phi = (f32(slice_index) + noise.x) * AO_PI / f32(AO_SLICE_COUNT);
        let direction = vec2(cos(phi), sin(phi));
        let slice_world = screen_right * direction.x + screen_down * direction.y;
        let tangent = normalize(slice_world - view * dot(slice_world, view));
        let plane_normal = cross(view, tangent);
        let projected_normal = normal - plane_normal * dot(normal, plane_normal);
        let projected_length = max(length(projected_normal), 1.0e-5);
        let normal_angle = atan2(dot(projected_normal, tangent), dot(projected_normal, view));
        let left_limit = normal_angle - 0.5 * AO_PI;
        let right_limit = normal_angle + 0.5 * AO_PI;
        unoccluded_visibility += projected_length * (ao_arc_visibility(left_limit, normal_angle)
            + ao_arc_visibility(right_limit, normal_angle));
        var left_horizon = cos(left_limit);
        var right_horizon = cos(right_limit);
        for (var side = 0u; side < 2u; side += 1u) {
            let sign = select(-1.0, 1.0, side == 1u);
            let base_horizon = select(cos(left_limit), cos(right_limit), side == 1u);
            var horizon = base_horizon;
            for (var step_index = 0u; step_index < AO_STEPS_PER_SIDE; step_index += 1u) {
                let fraction = (f32(step_index) + 0.5 + 0.5 * noise.y) / f32(AO_STEPS_PER_SIDE);
                let offset = max(1.0, fraction * fraction * pixel_radius);
                let sample_uv = centre_uv + sign * direction * texel * offset;
                if (any(sample_uv <= vec2(0.0)) || any(sample_uv >= vec2(1.0))) {
                    continue;
                }
                let sample_depth = ao_depth_at(depth_texture, sample_uv);
                if (sample_depth <= 1.0e-8) {
                    continue;
                }
                let sample_world = ao_world_position(frame, ao_texel_uv(depth_texture, sample_uv), sample_depth);
                let separation = sample_world - origin;
                let separation_sq = dot(separation, separation);
                if (separation_sq < 1.0e-8 || separation_sq >= AO_RADIUS * AO_RADIUS) {
                    continue;
                }
                let range_weight = 1.0 - smoothstep(AO_RADIUS * 0.6, AO_RADIUS, sqrt(separation_sq));
                let horizon_cosine = dot(separation, view) * inverseSqrt(separation_sq);
                horizon = max(horizon, mix(base_horizon, horizon_cosine, range_weight));
            }
            if (side == 0u) {
                left_horizon = horizon;
            } else {
                right_horizon = horizon;
            }
        }
        let left_angle = clamp(-acos(clamp(left_horizon, -1.0, 1.0)), left_limit, 0.0);
        let right_angle = clamp(acos(clamp(right_horizon, -1.0, 1.0)), 0.0, right_limit);
        visibility += projected_length * (ao_arc_visibility(left_angle, normal_angle)
            + ao_arc_visibility(right_angle, normal_angle));
    }
    let distance = ao_view_distance(frame, depth);
    let fade = (1.0 - smoothstep(96.0, 192.0, distance)) * smoothstep(1.5, 3.0, projected_radius);
    return mix(1.0, clamp(visibility / max(unoccluded_visibility, 1.0e-5), 0.0, 1.0), fade);
}

fn contact_depth_thickness(travel: f32) -> f32 {
    return 0.08 + travel * 0.04;
}

// Keep hit confidence and the refinement bracket on the same continuous depth footprint.
fn contact_depth_probe(
    frame: EnhancedFrame,
    depth_texture: texture_depth_2d,
    sample_uv: vec2<f32>,
    ray_depth: f32,
    origin: vec3<f32>,
    normal: vec3<f32>,
    travel: f32,
) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_texture));
    let coordinate = sample_uv * vec2<f32>(size) - vec2(0.5);
    let base = vec2<i32>(floor(coordinate));
    let fraction = fract(coordinate);
    let ray_distance = ao_view_distance(frame, ray_depth);
    let thickness = contact_depth_thickness(travel);
    var confidence = 0.0;
    var error = 0.0;
    var coverage = 0.0;
    var caster_confidence = 0.0;
    for (var y = 0; y < 2; y += 1) {
        for (var x = 0; x < 2; x += 1) {
            let coord = clamp(base + vec2(x, y), vec2(0), size - vec2(1));
            let sampled_depth = textureLoad(depth_texture, coord, 0);
            if (sampled_depth <= 1.0e-8) { continue; }
            let axis = vec2(select(1.0 - fraction.x, fraction.x, x == 1),
                select(1.0 - fraction.y, fraction.y, y == 1));
            let weight = axis.x * axis.y;
            let delta = ray_distance - ao_view_distance(frame, sampled_depth);
            coverage += weight;
            let depth_error = clamp(delta - thickness * 0.5, -CONTACT_REACH, CONTACT_REACH);
            if (delta <= 0.01) {
                error += weight * depth_error;
                continue;
            }
            let centre_uv = (vec2<f32>(coord) + vec2(0.5)) / vec2<f32>(size);
            let candidate = ao_world_position(frame, centre_uv, sampled_depth);
            let separation = candidate - origin;
            let plane_weight = smoothstep(0.005, 0.035, dot(separation, normal));
            let fade_radius = CONTACT_REACH * 0.85;
            let range_weight = 1.0 - smoothstep(fade_radius * fade_radius,
                CONTACT_REACH * CONTACT_REACH, dot(separation, separation));
            let candidate_weight = plane_weight * range_weight;
            error += weight * mix(-CONTACT_REACH, depth_error, candidate_weight);
            caster_confidence += weight * candidate_weight;
            if (delta >= thickness) { continue; }
            confidence += weight * plane_weight * range_weight * smoothstep(0.01, 0.035, delta)
                * (1.0 - smoothstep(thickness * 0.7, thickness, delta));
        }
    }
    return vec4(confidence, select(-CONTACT_REACH, error / max(coverage, 1.0e-5), coverage > 1.0e-5),
        coverage, caster_confidence);
}

fn contact_ray_probe(
    frame: EnhancedFrame,
    depth_texture: texture_depth_2d,
    origin: vec3<f32>,
    normal: vec3<f32>,
    light: vec3<f32>,
    travel: f32,
) -> vec4<f32> {
    let clip = frame.clip_from_world * vec4(origin + light * travel, 1.0);
    if (clip.w <= 1.0e-6) { return vec4(0.0, -CONTACT_REACH, 0.0, 0.0); }
    let ndc = clip.xyz / clip.w;
    let sample_uv = ndc.xy * vec2(0.5, -0.5) + vec2(0.5);
    if (any(sample_uv <= vec2(0.0)) || any(sample_uv >= vec2(1.0)) || ndc.z <= 0.0 || ndc.z >= 1.0) {
        return vec4(0.0, -CONTACT_REACH, 0.0, 0.0);
    }
    let probe = contact_depth_probe(frame, depth_texture, sample_uv, ndc.z, origin, normal, travel);
    let size = vec2<f32>(textureDimensions(depth_texture));
    let edge_pixels = min(sample_uv, vec2(1.0) - sample_uv) * size;
    let edge_fade = smoothstep(0.0, 16.0, min(edge_pixels.x, edge_pixels.y));
    let near_fade = 1.0 - smoothstep(0.9, 1.0, ndc.z);
    let reach_fade = 1.0 - smoothstep(CONTACT_REACH * 0.65, CONTACT_REACH, travel);
    return vec4(probe.x * edge_fade * near_fade * reach_fade, probe.yzw);
}

// Cascade shadows own cast visibility; screen depth adds a bounded local correction.
fn screen_contact_shadow(
    frame: EnhancedFrame,
    depth_texture: texture_depth_2d,
    uv: vec2<f32>,
    depth: f32,
    pixel: vec2<f32>,
    geometric_normal: vec3<f32>,
) -> f32 {
    if (depth <= 1.0e-8 || frame.light_direction.w <= 0.0) {
        return 1.0;
    }
    let centre_uv = ao_texel_uv(depth_texture, uv);
    let centre = ao_world_position(frame, centre_uv, depth);
    let light = normalize(frame.light_direction.xyz);
    let normal = normalize(geometric_normal)
        * select(-1.0, 1.0, dot(geometric_normal, light) >= 0.0);
    let facing = smoothstep(0.0, 0.15, dot(normal, light));
    if (facing <= 0.0) {
        return 1.0;
    }
    let origin = centre + normal * 0.03;
    let distance = ao_view_distance(frame, depth);
    let distance_fade = 1.0 - smoothstep(64.0, 128.0, distance);
    if (distance_fade <= 0.0) {
        return 1.0;
    }
    var occlusion = 0.0;
    var previous_travel = 0.0;
    var previous = vec4(0.0, -CONTACT_REACH, 0.0, 0.0);
    var refined = false;
    for (var step_index = 0u; step_index < CONTACT_STEPS; step_index += 1u) {
        let travel = (f32(step_index) + 0.5) * CONTACT_REACH / f32(CONTACT_STEPS);
        let probe = contact_ray_probe(frame, depth_texture, origin, normal, light, travel);
        occlusion = max(occlusion, probe.x);
        // Missing depth reduces confidence instead of inventing a fully covered bracket.
        if (!refined && previous.y <= 0.0 && probe.y > 0.0
            && min(previous.z, probe.z) > 1.0e-5 && probe.w > 1.0e-5) {
            refined = true;
            var lower = previous_travel;
            var upper = travel;
            // Admission changes at either endpoint; refinement converges to the coarse samples.
            let crossing = clamp(-previous.y / max(probe.y - previous.y, 1.0e-5), 0.0, 1.0);
            let crossing_weight = 4.0 * crossing * (1.0 - crossing);
            let bracket_confidence = min(previous.z, probe.z)
                * smoothstep(0.0, 0.25, probe.w) * crossing_weight;
            for (var refinement = 0u; refinement < CONTACT_REFINE_STEPS; refinement += 1u) {
                let middle = (lower + upper) * 0.5;
                let middle_probe = contact_ray_probe(frame, depth_texture, origin, normal, light, middle);
                occlusion = max(occlusion, middle_probe.x * bracket_confidence);
                if (middle_probe.y > 0.0) { upper = middle; }
                else { lower = middle; }
            }
        }
        previous_travel = travel;
        previous = probe;
        if (occlusion > 0.995) { break; }
    }
    return 1.0 - CONTACT_STRENGTH * distance_fade * facing * occlusion;
}
