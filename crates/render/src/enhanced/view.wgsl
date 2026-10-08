#define_import_path cinnabar::enhanced_view

#import cinnabar::lighting::lit_colour

#import cinnabar::enhanced_common::{
    EnhancedFrame, CLASS_EMISSION_MASK, CLASS_LEAVES, CLASS_PLANT, FEATURE_SHADOWS,
    FEATURE_WATER, FEATURE_WAVING, interleaved_gradient_noise, wave_offset,
    water_surface_offset,
}

// Main-pass bind group appended as group 2 only on Enhanced pipelines.
@group(2) @binding(0) var<uniform> enhanced_frame: EnhancedFrame;
@group(2) @binding(1) var enhanced_shadow_map: texture_depth_2d_array;
@group(2) @binding(2) var enhanced_shadow_sampler: sampler_comparison;
@group(2) @binding(3) var enhanced_material_classes: texture_2d<u32>;
@group(2) @binding(4) var enhanced_scene_colour: texture_2d<f32>;
@group(2) @binding(5) var enhanced_scene_depth: texture_depth_2d;
@group(2) @binding(6) var enhanced_linear_sampler: sampler;

const EMISSIVE_GAIN: f32 = 3.0;
const SHADOW_TAPS: u32 = 8u;

// Read the palette-derived class, defaulting unknown IDs to ordinary surfaces.
fn material_class(material_id: u32) -> u32 {
    let size = textureDimensions(enhanced_material_classes);
    if (material_id >= size.x * size.y) { return 0u; }
    return textureLoad(enhanced_material_classes,
        vec2<i32>(i32(material_id % size.x), i32(material_id / size.x)), 0).r;
}

// Displace classified foliage only when wind is enabled.
fn waved_position(world: vec3<f32>, surface_class: u32, weight: f32) -> vec3<f32> {
    if ((enhanced_frame.flags.x & FEATURE_WAVING) == 0u) {
        return world;
    }
    return world + wave_offset(
        world,
        surface_class,
        weight,
        enhanced_frame.camera_time.w,
        enhanced_frame.ambient_colour.w,
    );
}

// Displace shared water surface edges with the same analytic wave.
fn waved_water_position(world: vec3<f32>, top_surface: bool) -> vec3<f32> {
    if ((enhanced_frame.flags.x & FEATURE_WAVING) == 0u || !top_surface) {
        return world;
    }
    return world + vec3(0.0, water_surface_offset(world, enhanced_frame.camera_time.w), 0.0);
}

// Rotated Vogel-disk PCF over the hardware 2x2 comparison filter.
fn filtered_shadow(uv: vec2<f32>, depth: f32, cascade: u32, noise: f32) -> f32 {
    let texel = 1.0 / f32(max(enhanced_frame.flags.z, 1u));
    let rotation = noise * 6.2831853;
    var sum = 0.0;
    for (var tap = 0u; tap < SHADOW_TAPS; tap += 1u) {
        let radius = sqrt((f32(tap) + 0.5) / f32(SHADOW_TAPS)) * 1.75 * texel;
        let angle = f32(tap) * 2.3999632 + rotation;
        let offset = vec2(cos(angle), sin(angle)) * radius;
        sum += textureSampleCompareLevel(
            enhanced_shadow_map,
            enhanced_shadow_sampler,
            uv + offset,
            i32(cascade),
            depth,
        );
    }
    return sum / f32(SHADOW_TAPS);
}

// Select a containing cascade and fade its PCF visibility at the far edge.
fn shadow_visibility(world: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>) -> f32 {
    if ((enhanced_frame.flags.x & FEATURE_SHADOWS) == 0u) {
        return 1.0;
    }
    let fade_distance = enhanced_frame.cascade_texel.w;
    let camera_distance = distance(world, enhanced_frame.camera_time.xyz);
    if (camera_distance >= fade_distance) {
        return 1.0;
    }
    let grazing = 1.0 - clamp(dot(normal, enhanced_frame.light_direction.xyz), 0.0, 1.0);
    let margin = 4.0 / f32(max(enhanced_frame.flags.z, 1u));
    let noise = interleaved_gradient_noise(pixel);
    for (var cascade = 0u; cascade < enhanced_frame.flags.y; cascade += 1u) {
        let texel = enhanced_frame.cascade_texel[cascade];
        let offset = normal * texel * (1.5 + 2.5 * grazing);
        let clip = enhanced_frame.cascade_clip_from_world[cascade] * vec4(world + offset, 1.0);
        let ndc = clip.xyz / clip.w;
        let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (all(uv > vec2(margin)) && all(uv < vec2(1.0 - margin)) && ndc.z > 0.0 && ndc.z < 1.0) {
            let bias = texel * 0.5 * enhanced_frame.cascade_depth_scale[cascade];
            let visibility = filtered_shadow(uv, ndc.z - bias, cascade, noise);
            return mix(visibility, 1.0, smoothstep(fade_distance * 0.8, fade_distance, camera_distance));
        }
    }
    return 1.0;
}

// Boost bright texels of materials whose block states all emit light.
fn emissive_light(albedo: vec3<f32>, surface_class: u32) -> vec3<f32> {
    let level = f32(surface_class & CLASS_EMISSION_MASK) / 15.0;
    if (level <= 0.0) {
        return vec3(0.0);
    }
    // Bright texels glow; dark detail (torch sticks, lamp frames) stays matte.
    let mask = smoothstep(0.3, 0.85, max(albedo.r, max(albedo.g, albedo.b)));
    return albedo * level * mask * EMISSIVE_GAIN;
}

// Add HDR illumination to the shared RGB lightmap, including its AO and face shade.
fn shade_surface(
    albedo: vec3<f32>,
    normal: vec3<f32>,
    world: vec3<f32>,
    pixel: vec2<f32>,
    lighting: vec3<f32>,
    sky_light: f32,
    ambient_occlusion: f32,
    surface_class: u32,
) -> vec3<f32> {
    let light = enhanced_frame.light_direction.xyz;
    let foliage = (surface_class & (CLASS_LEAVES | CLASS_PLANT)) != 0u;
    let facing = dot(normal, light);
    // Foliage transmits light, so it is lit from both sides.
    let diffuse = select(max(facing, 0.0), 0.4 + 0.6 * abs(facing), foliage);
    // Direct sun needs an open sky; the shadow map alone cannot see every cave.
    let sky_gate = smoothstep(0.25, 0.8, sky_light);
    var shadow = 0.0;
    if (diffuse > 0.0 && sky_gate > 0.0) {
        shadow = shadow_visibility(world, normal, pixel);
    }
    let direct = enhanced_frame.light_colour.rgb * enhanced_frame.light_direction.w
        * diffuse * shadow * sky_gate;
    let ambient = enhanced_frame.ambient_colour.rgb * enhanced_frame.light_colour.w
        * sky_light * (0.8 + 0.2 * normal.y);
    let ao = clamp(ambient_occlusion, 0.0, 1.0);
    let extra = direct * mix(1.0, ao, 0.5) + ambient * ao;
    return lit_colour(albedo, lighting) + albedo * extra + emissive_light(albedo, surface_class);
}

// Directional ripples from four analytic sine waves.
fn ripple_normal(xz: vec2<f32>, seconds: f32) -> vec3<f32> {
    let d0 = vec2(0.8, 0.6);
    let d1 = vec2(-0.6, 0.8);
    let d2 = vec2(0.28, -0.96);
    let d3 = vec2(-0.92, -0.39);
    let s0 = 0.08 * cos(dot(d0, xz) * 1.3 + seconds * 1.1);
    let s1 = 0.08 * cos(dot(d1, xz) * 2.1 + seconds * 1.6);
    let s2 = 0.09 * cos(dot(d2, xz) * 3.7 + seconds * 2.3);
    let s3 = 0.08 * cos(dot(d3, xz) * 5.3 + seconds * 3.1);
    let slope = d0 * s0 + d1 * s1 + d2 * s2 + d3 * s3;
    return normalize(vec3(-slope.x, 1.0, -slope.y));
}

// Project a ray sample into the opaque scene snapshot.
fn scene_uv(world: vec3<f32>) -> vec3<f32> {
    let clip = enhanced_frame.clip_from_world * vec4(world, 1.0);
    if (clip.w <= 0.0) {
        return vec3(-1.0);
    }
    let ndc = clip.xyz / clip.w;
    return vec3(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5, ndc.z);
}

// Read reverse-Z opaque depth with clamped integer coordinates.
fn scene_depth_at(uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(enhanced_scene_depth));
    let coord = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2(0), size - vec2(1));
    return textureLoad(enhanced_scene_depth, coord, 0);
}

// Screen-space reflection against the opaque snapshot; alpha is hit confidence.
fn trace_reflection(origin: vec3<f32>, direction: vec3<f32>, jitter: f32) -> vec4<f32> {
    let near = enhanced_frame.projection.x;
    var previous = 0.0;
    var travel = 0.3 + 0.4 * jitter;
    for (var march = 0u; march < 20u; march += 1u) {
        let probe_uv = scene_uv(origin + direction * travel);
        if (any(probe_uv.xy < vec2(0.0)) || any(probe_uv.xy > vec2(1.0)) || probe_uv.z <= 0.0) {
            break;
        }
        let scene = scene_depth_at(probe_uv.xy);
        if (scene > 0.0 && probe_uv.z < scene) {
            let behind = near / probe_uv.z - near / scene;
            if (behind < max(travel * 0.3, 0.6)) {
                var low = previous;
                var high = travel;
                for (var refine = 0u; refine < 5u; refine += 1u) {
                    let middle = 0.5 * (low + high);
                    let probe = scene_uv(origin + direction * middle);
                    if (probe.z < scene_depth_at(probe.xy)) {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                let hit = scene_uv(origin + direction * high);
                let colour = textureSampleLevel(
                    enhanced_scene_colour,
                    enhanced_linear_sampler,
                    hit.xy,
                    0.0,
                ).rgb;
                let edge = min(min(hit.x, 1.0 - hit.x), min(hit.y, 1.0 - hit.y));
                let confidence = smoothstep(0.0, 0.08, edge) * (1.0 - f32(march) / 20.0);
                return vec4(colour, confidence);
            }
        }
        previous = travel;
        travel = travel * 1.3 + 0.25;
    }
    return vec4(0.0);
}

// Water top surface with Fresnel reflection, sun glint and depth absorption.
// Returns straight alpha colour for ALPHA_BLENDING.
fn shade_water(
    base: vec3<f32>,
    alpha: f32,
    face_normal: vec3<f32>,
    world: vec3<f32>,
    frag: vec4<f32>,
    lighting: vec3<f32>,
    sky_light: f32,
    ambient_occlusion: f32,
    sky_zenith: vec3<f32>,
    sky_horizon: vec3<f32>,
) -> vec4<f32> {
    let to_camera = enhanced_frame.camera_time.xyz - world;
    let above = dot(to_camera, face_normal) > 0.0;
    if ((enhanced_frame.flags.x & FEATURE_WATER) == 0u || face_normal.y < 0.5 || !above) {
        let lit = shade_surface(base, face_normal, world, frag.xy, lighting, sky_light,
            ambient_occlusion, 0u);
        return vec4(lit, alpha);
    }
    let distance_to_camera = length(to_camera);
    let eye = to_camera / max(distance_to_camera, 1.0e-4);
    let ripple = ripple_normal(world.xz, enhanced_frame.camera_time.w);
    let normal = normalize(mix(ripple, vec3(0.0, 1.0, 0.0), smoothstep(24.0, 72.0, distance_to_camera)));
    let n_dot_v = max(dot(normal, eye), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - n_dot_v, 5.0);
    let reflected = reflect(-eye, normal);
    let sky = mix(sky_horizon, sky_zenith, clamp(reflected.y, 0.0, 1.0)) * sky_light;
    let noise = interleaved_gradient_noise(frag.xy);
    let traced = trace_reflection(world, reflected, noise);
    let reflection = mix(sky, traced.rgb, traced.a);
    let sky_gate = smoothstep(0.25, 0.8, sky_light);
    let half_vector = normalize(enhanced_frame.light_direction.xyz + eye);
    let glint = pow(max(dot(normal, half_vector), 0.0), 220.0) * 6.0
        * enhanced_frame.light_direction.w * sky_gate
        * shadow_visibility(world, face_normal, frag.xy);
    let near = enhanced_frame.projection.x;
    let scene = scene_depth_at(frag.xy * enhanced_frame.viewport.zw);
    var thickness = 64.0;
    if (scene > 0.0) {
        thickness = clamp(near / scene - near / frag.z, 0.0, 64.0);
    }
    let transmittance = exp(-thickness * 0.28);
    let body = shade_surface(base, face_normal, world, frag.xy, lighting, sky_light,
        ambient_occlusion, 0u) * 0.55;
    let coverage = 1.0 - (1.0 - fresnel) * transmittance;
    let radiance = (1.0 - fresnel) * (1.0 - transmittance) * body
        + fresnel * reflection
        + enhanced_frame.light_colour.rgb * glint;
    return vec4(radiance / max(coverage, 1.0e-3), coverage);
}

// Raw sky exposure gates direct sun/moon light; the lightmap already applies time and brightness.
fn sky_illumination(sample: u32) -> f32 {
    let value = f32((sample >> 4u) & 15u) / 15.0;
    return value / (4.0 - 3.0 * value);
}
