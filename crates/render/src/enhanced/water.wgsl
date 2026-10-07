#define_import_path cinnabar::enhanced_water
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_atmosphere::environment_sky

const SSR_TAPS: u32 = 40u;
const SSR_DISTANCE: f32 = 72.0;
const WATER_ABSORPTION: vec3<f32> = vec3(0.34, 0.105, 0.055);
const WATER_SCATTERING_ALBEDO: vec3<f32> = vec3(0.025, 0.13, 0.18);

fn water_transmittance(path: f32) -> vec3<f32> {
    return exp(-WATER_ABSORPTION * clamp(path, 0.0, 64.0));
}
fn water_medium_radiance(incident: vec3<f32>) -> vec3<f32> {
    return WATER_SCATTERING_ALBEDO * max(incident, vec3(0.0));
}
fn water_ambient_irradiance(frame: EnhancedFrame) -> vec3<f32> {
    return max(frame.ambient_colour.rgb, vec3(0.0)) * max(frame.light_colour.w, 0.0);
}
fn water_transport(source: vec3<f32>, path: f32, incident: vec3<f32>) -> vec3<f32> {
    let transmission = water_transmittance(path);
    return max(source, vec3(0.0)) * transmission
        + water_medium_radiance(incident) * (vec3(1.0) - transmission);
}
// Low reflection energy fades smoothly into the cached environment.
fn water_screen_reflection_weight(fresnel: f32, distance_to_camera: f32) -> f32 {
    return smoothstep(0.025, 0.12, fresnel)
        * (1.0 - smoothstep(48.0, 96.0, distance_to_camera));
}

// SSR hits fade at depth discontinuities instead of flipping between a hit and
// the probe when a neighboring depth texel becomes the nearest surface.
fn water_ssr_hit_confidence(
    edge: f32,
    travel: f32,
    roughness: f32,
    coverage: f32,
    behind: f32,
    thickness: f32,
) -> f32 {
    if (behind < 0.0) { return 0.0; }
    let contact = 1.0 - smoothstep(
        max(thickness * 0.35, 0.001),
        max(thickness, 0.002),
        behind,
    );
    return smoothstep(0.0, 0.08, edge)
        * (1.0 - smoothstep(SSR_DISTANCE * 0.65, SSR_DISTANCE, travel))
        * (1.0 - smoothstep(0.45, 0.6, roughness))
        * smoothstep(0.0, 0.25, coverage)
        * contact;
}

fn water_fresnel(cosine: f32, eta: f32) -> f32 {
    let sine_squared = eta * eta * (1.0 - cosine * cosine);
    if (sine_squared >= 1.0) { return 1.0; }
    let transmitted_cosine = sqrt(max(1.0 - sine_squared, 0.0));
    let perpendicular = (eta * cosine - transmitted_cosine)
        / max(eta * cosine + transmitted_cosine, 1.0e-5);
    let parallel = (eta * transmitted_cosine - cosine)
        / max(eta * transmitted_cosine + cosine, 1.0e-5);
    return clamp(0.5 * (perpendicular * perpendicular + parallel * parallel), 0.0, 1.0);
}

const WATER_HEIGHT_AMPLITUDE: f32 = 0.025;
const WATER_HEIGHT_FREQUENCY: vec2<f32> = vec2(0.9, 0.7);
const WATER_HEIGHT_SPEED: vec2<f32> = vec2(1.3, 1.1);

fn water_surface_phase(xz: vec2<f32>, seconds: f32) -> vec2<f32> {
    return xz * WATER_HEIGHT_FREQUENCY + seconds * WATER_HEIGHT_SPEED;
}

fn water_surface_offset(world: vec3<f32>, seconds: f32) -> f32 {
    let phase = water_surface_phase(world.xz, seconds);
    let wave = sin(phase.x) * sin(phase.y);
    return -WATER_HEIGHT_AMPLITUDE * (0.5 + 0.5 * wave);
}

fn water_surface_gradient(xz: vec2<f32>, seconds: f32) -> vec2<f32> {
    let phase = water_surface_phase(xz, seconds);
    return -WATER_HEIGHT_AMPLITUDE * 0.5 * WATER_HEIGHT_FREQUENCY
        * vec2(cos(phase.x) * sin(phase.y), sin(phase.x) * cos(phase.y));
}

fn ripple_normal(xz: vec2<f32>, seconds: f32, footprint: f32, displaced: bool) -> vec3<f32> {
    let d0 = vec2(0.8, 0.6);
    let d1 = vec2(-0.6, 0.8);
    let d2 = vec2(0.28, -0.96);
    let d3 = vec2(-0.92, -0.39);
    // Filter the analytic height derivatives over the projected pixel footprint.
    let width_squared = max(footprint, 0.0) * max(footprint, 0.0);
    let frequency = vec4(1.3, 2.1, 3.7, 5.3);
    let phase = vec4(dot(d0, xz), dot(d1, xz), dot(d2, xz), dot(d3, xz))
        * frequency + seconds * vec4(1.1, 1.6, 2.3, 3.1);
    let slopes = vec4(0.006, 0.011, 0.016, 0.014) * cos(phase)
        * exp(-0.5 * frequency * frequency * width_squared);
    let height_slope = select(vec2(0.0), water_surface_gradient(xz, seconds), displaced)
        * exp(-0.5 * dot(WATER_HEIGHT_FREQUENCY, WATER_HEIGHT_FREQUENCY) * width_squared);
    let slope = height_slope + d0 * slopes.x + d1 * slopes.y + d2 * slopes.z + d3 * slopes.w;
    return normalize(vec3(-slope.x, 1.0, -slope.y));
}

fn scene_uv(frame: EnhancedFrame, world: vec3<f32>) -> vec3<f32> {
    let clip = frame.clip_from_world * vec4(world, 1.0);
    if (!(clip.w > 1.0e-5 && clip.w < 1.0e20)) {
        return vec3(-1.0);
    }
    let ndc = clip.xyz / clip.w;
    return vec3(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}

fn valid_scene_uv(projected: vec3<f32>) -> bool {
    return all(projected.xy >= vec2(0.0)) && all(projected.xy < vec2(1.0))
        && projected.z > 0.0 && projected.z <= 1.0;
}

fn scene_world(frame: EnhancedFrame, uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let clip = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let world = frame.world_from_clip * clip;
    return world.xyz / max(world.w, 1.0e-6);
}

fn scene_depth_at(depth_map: texture_depth_2d, uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(depth_map));
    let coord = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2(0), size - vec2(1));
    return textureLoad(depth_map, coord, 0);
}

fn water_depth_weight(near: f32, reference_depth: f32, sample_depth: f32) -> f32 {
    if (sample_depth <= 0.0 || reference_depth <= 0.0) { return 0.0; }
    let reference_distance = near / reference_depth;
    let separation = abs(near / sample_depth - reference_distance);
    return 1.0 - smoothstep(max(0.06, reference_distance * 0.008),
        max(0.12, reference_distance * 0.016), separation);
}

// A sky texel beside a thin submerged object must never enter its refraction footprint.
fn filtered_scene_colour(frame: EnhancedFrame, colour: texture_2d<f32>, depth_map: texture_depth_2d,
    uv: vec2<f32>, reference_depth: f32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_map));
    let coordinate = uv * vec2<f32>(size) - vec2(0.5);
    let base = vec2<i32>(floor(coordinate));
    let fraction = fract(coordinate);
    var sum = vec3(0.0);
    var total = 0.0;
    for (var y = 0; y < 2; y += 1) {
        for (var x = 0; x < 2; x += 1) {
            let p = clamp(base + vec2(x, y), vec2(0), size - vec2(1));
            let axes = select(vec2(1.0) - fraction, fraction, vec2<bool>(x == 1, y == 1));
            let d = textureLoad(depth_map, p, 0);
            let weight = axes.x * axes.y * water_depth_weight(frame.projection.x, reference_depth, d);
            sum += max(textureLoad(colour, p, 0).rgb, vec3(0.0)) * weight;
            total += weight;
        }
    }
    if (total < 0.0001) { return vec4(0.0); }
    return vec4(sum / total, clamp(total, 0.0, 1.0));
}

fn trace_reflection(frame: EnhancedFrame, colour: texture_2d<f32>, depth_map: texture_depth_2d, linear_sampler: sampler, origin: vec3<f32>, direction: vec3<f32>, jitter: f32, roughness: f32) -> vec4<f32> {
    if (frame.probe.w < 0.0) { return vec4(0.0); }
    let near = frame.projection.x;
    let start_world = origin + direction * 0.12;
    let start_clip = frame.clip_from_world * vec4(start_world, 1.0);
    if (!(start_clip.w > near && start_clip.w < 1.0e20)) { return vec4(0.0); }
    let clip_direction = frame.clip_from_world * vec4(direction, 0.0);
    var ray_length = SSR_DISTANCE;
    if (clip_direction.w < 0.0) {
        ray_length = min(ray_length, (near * 1.05 - start_clip.w) / clip_direction.w);
    }
    if (ray_length < 0.2) { return vec4(0.0); }
    let end_clip = start_clip + clip_direction * ray_length;
    let start = scene_uv(frame, start_world);
    let end_ndc = end_clip.xyz / max(end_clip.w, 1.0e-5);
    let end = vec3(end_ndc.x * 0.5 + 0.5, 0.5 - end_ndc.y * 0.5, end_ndc.z);
    if (!valid_scene_uv(start)) { return vec4(0.0); }
    let delta = end - start;
    var end_fraction = 1.0;
    for (var axis = 0u; axis < 2u; axis += 1u) {
        if (delta[axis] > 1.0e-5) {
            end_fraction = min(end_fraction, (0.999 - start[axis]) / delta[axis]);
        } else if (delta[axis] < -1.0e-5) {
            end_fraction = min(end_fraction, (0.001 - start[axis]) / delta[axis]);
        }
    }
    let pixel_delta = abs(delta.xy) * frame.viewport.xy;
    let pixel_length = max(pixel_delta.x, pixel_delta.y);
    if (!(pixel_length > 1.0 && pixel_length < 1.0e10)) { return vec4(0.0); }
    // Screen-space steps interpolate NDC depth, avoiding exponential jumps in world space.
    let stride = max(1.0 / pixel_length, end_fraction / f32(SSR_TAPS));
    var previous_fraction = 0.0;
    var previous_delta = -1.0;
    var fraction = stride * (0.5 + jitter * 0.5);
    for (var march = 0u; march < SSR_TAPS; march += 1u) {
        if (fraction > end_fraction) { break; }
        let probe = start + delta * fraction;
        if (!valid_scene_uv(probe)) { break; }
        let scene_depth = scene_depth_at(depth_map, probe.xy);
        if (scene_depth > 0.0) {
            let separation = near / probe.z - near / scene_depth;
            if (separation >= 0.0 && previous_delta < 0.0) {
                var low = previous_fraction;
                var high = fraction;
                for (var refine = 0u; refine < 6u; refine += 1u) {
                    let middle = (low + high) * 0.5;
                    let sample = start + delta * middle;
                    if (sample.z < scene_depth_at(depth_map, sample.xy)) {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                let hit = start + delta * high;
                let hit_depth = scene_depth_at(depth_map, hit.xy);
                let hit_distance = near / max(hit_depth, 1.0e-6);
                let behind = near / hit.z - hit_distance;
                let thickness = clamp(0.08 + hit_distance * 0.002, 0.08, 0.6);
                var retry_hit = hit_depth <= 0.0 || behind < 0.0;
                if (hit_depth > 0.0 && behind >= 0.0) {
                    let hit_world = scene_world(frame, hit.xy, hit_depth);
                    let travel = distance(hit_world, origin);
                    let mip = roughness * roughness
                        * f32(textureNumLevels(colour) - 1u);
                    let sample = filtered_scene_colour(frame, colour, depth_map, hit.xy, hit_depth);
                    var radiance = sample.rgb;
                    if (mip > 0.5 && sample.a > 0.999) {
                        radiance = textureSampleLevel(colour, linear_sampler, hit.xy, mip).rgb;
                    }
                    let edge = min(min(hit.x, 1.0 - hit.x), min(hit.y, 1.0 - hit.y));
                    let confidence = water_ssr_hit_confidence(
                        edge,
                        travel,
                        roughness,
                        sample.a,
                        behind,
                        thickness,
                    );
                    if (travel > 0.2 && confidence > 0.001
                        && all(radiance >= vec3(0.0)) && all(radiance < vec3(1.0e6))) {
                        return vec4(radiance, confidence);
                    }
                    retry_hit = true;
                }
                // A rejected foreground edge must not suppress a later stable
                // hit behind it on the same ray.
                previous_delta = select(separation, -1.0, retry_hit);
            } else {
                previous_delta = separation;
            }
        } else {
            previous_delta = -1.0;
        }
        previous_fraction = fraction;
        fraction += stride;
    }
    return vec4(0.0);
}

fn water_background_valid(frame: EnhancedFrame, depth_map: texture_depth_2d, world: vec3<f32>, surface_normal: vec3<f32>, uv: vec2<f32>, above: bool) -> bool {
    if (!all(uv >= vec2(0.001)) || !all(uv < vec2(0.999))) { return false; }
    let depth = scene_depth_at(depth_map, uv);
    if (depth <= 0.0) { return !above; }
    let behind = scene_world(frame, uv, depth);
    let surface_depth = scene_uv(frame, world).z;
    let plane_distance = dot(behind - world, surface_normal);
    return depth < surface_depth + 0.000001
        && select(plane_distance >= -0.025, plane_distance <= 0.025, above);
}

fn water_refraction(frame: EnhancedFrame, colour: texture_2d<f32>, depth_map: texture_depth_2d, linear_sampler: sampler, world: vec3<f32>, surface_normal: vec3<f32>, ray: vec3<f32>, uv: vec2<f32>, above: bool) -> vec4<f32> {
    let base_depth = scene_depth_at(depth_map, uv);
    var optical_path = 48.0;
    if (base_depth > 0.0) {
        optical_path = clamp(distance(scene_world(frame, uv, base_depth), world), 0.0, 48.0);
    }
    let refracted_projection = scene_uv(frame, world + ray * max(min(optical_path, 24.0), 0.05));
    var displacement = vec2(0.0);
    if (valid_scene_uv(refracted_projection)) {
        displacement = clamp(refracted_projection.xy - uv, vec2(-0.04), vec2(0.04));
    }
    var refraction_uv = uv;
    if (water_background_valid(frame, depth_map, world, surface_normal, uv + displacement, above)) {
        refraction_uv += displacement;
    } else if (water_background_valid(frame, depth_map, world, surface_normal, uv + displacement * 0.5, above)) {
        refraction_uv += displacement * 0.5;
    }
    let depth = scene_depth_at(depth_map, refraction_uv);
    if (!water_background_valid(frame, depth_map, world, surface_normal, refraction_uv, above)) {
        return vec4(0.0, 0.0, 0.0, 48.0);
    }
    if (depth <= 0.0) {
        return vec4(environment_sky(frame, ray), 0.0);
    }
    let background_world = scene_world(frame, refraction_uv, depth);
    optical_path = clamp(distance(background_world, world), 0.0, 48.0);
    let sample = filtered_scene_colour(frame, colour, depth_map, refraction_uv, depth);
    if (sample.a <= 0.0001) { return vec4(0.0, 0.0, 0.0, 48.0); }
    return vec4(sample.rgb, optical_path);
}

fn water_caustic(frame: EnhancedFrame, world: vec3<f32>, optical_path: f32, direct_light: f32) -> f32 {
    let seconds = frame.camera_time.w;
    let warp = vec2(sin(world.z * 0.91 + seconds * 0.41), cos(world.x * 1.13 - seconds * 0.36));
    let phase = world.xz * 3.8 + warp * 1.5;
    let ridges = pow(1.0 - abs(sin(phase.x + seconds * 0.8)
        * sin(phase.y - seconds * 0.65)), 10.0);
    return ridges * 0.22 * direct_light * (1.0 - smoothstep(1.0, 8.0, optical_path));
}
