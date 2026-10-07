#define_import_path cinnabar::enhanced_clouds

#import cinnabar::enhanced_common::{
    EnhancedFrame, FEATURE_VOLUMETRIC_CLOUDS, interleaved_gradient_noise,
}
#import cinnabar::enhanced_atmosphere::{
    atmosphere_direct_irradiance, atmosphere_phase_mie, environment_sky,
    ATM_SUN_RADIUS, ATM_CELESTIAL_DISC_SCALE,
}

const CLOUD_LIGHT_STEPS: u32 = 4u;
const CLOUD_SHADOW_STEPS: u32 = 6u;
const CLOUD_EXTINCTION_COEFFICIENT: f32 = 0.055;
const CLOUD_MAX_RANGE: f32 = 8000.0;
const CLOUD_WIND: vec3<f32> = vec3(1.0, 0.0, 0.25);

@group(0) @binding(13) var cloud_noise_volume: texture_3d<f32>;
@group(0) @binding(14) var cloud_noise_sampler: sampler;

fn cloud_extinction(frame: EnhancedFrame) -> f32 {
    return CLOUD_EXTINCTION_COEFFICIENT * mix(0.85, 1.3, clamp(frame.ambient_colour.w, 0.0, 1.0));
}

// Explicit footprint filtering is shared by view, light and shadow marches.
fn cloud_volume_sample(coordinate: vec3<f32>, footprint: f32) -> vec4<f32> {
    let side = f32(textureDimensions(cloud_noise_volume, 0).x);
    let lod = clamp(log2(max(footprint * side, 1.0)), 0.0,
        f32(textureNumLevels(cloud_noise_volume) - 1u));
    return textureSampleLevel(cloud_noise_volume, cloud_noise_sampler, coordinate, lod);
}

fn cloud_coordinates(frame: EnhancedFrame, world: vec3<f32>) -> vec3<f32> {
    return world + CLOUD_WIND * frame.clouds.w;
}

fn cloud_sample_jitter(frame: EnhancedFrame, pixel: vec2<f32>) -> f32 {
    if (frame.temporal.y < 0.5 || frame.temporal.w > 0.5) { return 0.5; }
    return fract(interleaved_gradient_noise(pixel) + fract(frame.temporal.x * 0.61803398875));
}

// Temporal quadrature integrates the axial interval; only the projected pixel filters detail.
fn cloud_pixel_footprint(frame: EnhancedFrame, travel: f32) -> f32 {
    return max(travel * frame.viewport.w * 1.5, 0.5);
}

fn cloud_previous_point(frame: EnhancedFrame, world: vec3<f32>) -> vec3<f32> {
    let wind_rate = frame.clouds.w / max(frame.camera_time.w, 0.0001);
    return world + CLOUD_WIND * wind_rate * frame.temporal.z;
}

fn cloud_height_profile(height: f32, cloud_type: f32, storm: f32) -> f32 {
    let flat = smoothstep(0.02, 0.10, height) * (1.0 - smoothstep(0.20, 0.42, height));
    let cumulus = smoothstep(0.02, 0.16, height) * (1.0 - smoothstep(0.52, 0.96, height));
    let towering = smoothstep(0.01, 0.12, height) * (1.0 - smoothstep(0.72, 1.0, height));
    return mix(mix(flat, cumulus, cloud_type), towering, storm);
}

// Packed shape channels supply resolved erosion without another volume fetch.
fn cloud_structure(frame: EnhancedFrame, world: vec3<f32>, footprint: f32) -> vec2<f32> {
    let thickness = max(frame.clouds.z, 1.0);
    let height = (world.y - frame.clouds.y) / thickness;
    if (height <= 0.0 || height >= 1.0) { return vec2(0.0); }
    let point = cloud_coordinates(frame, world);
    let weather = cloud_volume_sample(vec3(point.x, 1731.0, point.z) * 0.00021,
        footprint * 0.00021);
    let storm = clamp(frame.ambient_colour.w, 0.0, 1.0);
    let coverage = clamp(frame.clouds.x * (0.65 + weather.r * 0.7) + storm * 0.12, 0.0, 0.98);
    if (coverage <= 0.0) { return vec2(0.0); }
    let cloud_type = clamp((weather.b - 0.15) * 2.2, 0.0, 1.0);
    let shear = vec3(height * height * mix(22.0, 85.0, storm), 0.0, height * 11.0);
    let shape = cloud_volume_sample((point + shear) * vec3(0.0017, 0.0030, 0.0017),
        footprint * 0.0030);
    let threshold = 0.55 - coverage * 0.40;
    let core = clamp(shape.r * 1.30 - 0.10, 0.0, 1.0);
    let silhouette = smoothstep(threshold, threshold + mix(0.16, 0.24, storm), core);
    let profile = cloud_height_profile(height, cloud_type, storm);
    let body = silhouette * profile;
    let cellular = dot(shape.gba, vec3(0.625, 0.25, 0.125));
    let erosion = (1.0 - cellular) * mix(0.18, 0.10, storm) * (1.0 - body) * profile;
    let density = max(body * (0.65 + cellular * 0.35) - erosion, 0.0);
    return vec2(body, density);
}

fn cloud_body(frame: EnhancedFrame, world: vec3<f32>, footprint: f32) -> f32 {
    return cloud_structure(frame, world, footprint).x;
}

fn cloud_density(frame: EnhancedFrame, world: vec3<f32>, footprint: f32) -> f32 {
    return cloud_structure(frame, world, footprint).y;
}

// Handles downward rays above the layer and horizontal rays originating inside it.
fn cloud_interval(frame: EnhancedFrame, origin: vec3<f32>, ray: vec3<f32>, limit: f32) -> vec2<f32> {
    let base = frame.clouds.y;
    let top = base + max(frame.clouds.z, 1.0);
    if (abs(ray.y) < 0.00001) {
        if (origin.y >= base && origin.y <= top) { return vec2(0.0, limit); }
        return vec2(-1.0);
    }
    let a = (base - origin.y) / ray.y;
    let b = (top - origin.y) / ray.y;
    let entry = max(min(a, b), 0.0);
    let end = min(max(a, b), limit);
    if (end <= entry) { return vec2(-1.0); }
    return vec2(entry, end);
}

// Constant-extinction centroids approach the visible front as optical depth increases.
fn cloud_depth_fraction(transmittance: f32) -> f32 {
    let optical_depth = -log(clamp(transmittance, 0.0001, 1.0));
    if (optical_depth < 0.05) { return 0.5 - optical_depth / 12.0; }
    return clamp(1.0 / optical_depth - 1.0 / (exp(optical_depth) - 1.0), 0.0, 0.5);
}

// Thin clouds also advect with the layer; only effectively clear sky reprojects at infinity.
fn cloud_history_position(frame: EnhancedFrame, ray: vec3<f32>, transmittance: f32) -> vec4<f32> {
    let interval = cloud_interval(frame, frame.camera_time.xyz, ray, CLOUD_MAX_RANGE);
    if (transmittance >= 0.98 || transmittance < 0.0 || interval.y <= interval.x || interval.y <= 0.0) {
        return vec4(ray, 0.0);
    }
    let world = frame.camera_time.xyz + ray * mix(interval.x, interval.y, cloud_depth_fraction(transmittance));
    return vec4(cloud_previous_point(frame, world), 1.0);
}

fn cloud_path_optical_depth(
    frame: EnhancedFrame, world: vec3<f32>, light: vec3<f32>, limit: f32, steps: u32,
) -> f32 {
    let interval = cloud_interval(frame, world, light, limit);
    if (interval.y <= 0.0) { return 0.0; }
    var optical_depth = 0.0;
    for (var index = 0u; index < steps; index += 1u) {
        let lower = f32(index) / f32(steps);
        let upper = f32(index + 1u) / f32(steps);
        let a = interval.x + lower * lower * (interval.y - interval.x);
        let b = interval.x + upper * upper * (interval.y - interval.x);
        let point = world + light * (a + b) * 0.5;
        let angular_footprint = (a + b) * 0.5 * ATM_SUN_RADIUS * ATM_CELESTIAL_DISC_SCALE;
        // Grazing segments integrate unresolved horizontal cells rather than aliasing one cell.
        let axial_footprint = (b - a) * length(light.xz) * 0.10;
        let footprint = max(max(angular_footprint, axial_footprint), 1.0);
        optical_depth += cloud_density(frame, point, footprint)
            * (b - a) * cloud_extinction(frame);
    }
    return optical_depth;
}

fn cloud_light_optical_depth(frame: EnhancedFrame, world: vec3<f32>, light: vec3<f32>) -> f32 {
    return cloud_path_optical_depth(frame, world, light, CLOUD_MAX_RANGE, CLOUD_LIGHT_STEPS);
}

fn cloud_shadow(frame: EnhancedFrame, world: vec3<f32>) -> f32 {
    if (frame.atmosphere.x < 0.5 || frame.light_direction.w <= 0.0
        || (frame.flags.x & FEATURE_VOLUMETRIC_CLOUDS) == 0u) {
        return 1.0;
    }
    let light = frame.light_direction.xyz;
    if (light.y <= 0.01) { return 1.0; }
    return exp(-cloud_path_optical_depth(frame, world, light, CLOUD_MAX_RANGE, CLOUD_SHADOW_STEPS));
}

// Higher bounces lose directional contrast and extinction without filling every core equally.
fn cloud_scattering(phase: vec3<f32>, optical_depth: f32) -> f32 {
    return dot(phase * vec3(1.0, 0.22, 0.07),
        exp(-max(optical_depth, 0.0) * vec3(1.0, 0.5, 0.25)));
}

// Returns premultiplied HDR radiance and opacity; composite with rgb + (1-a)*sky.
fn integrate_clouds(
    frame: EnhancedFrame, ray: vec3<f32>, max_distance: f32, pixel: vec2<f32>,
) -> vec4<f32> {
    if (frame.atmosphere.x < 0.5 || (frame.flags.x & FEATURE_VOLUMETRIC_CLOUDS) == 0u) {
        return vec4(0.0);
    }
    let camera = frame.camera_time.xyz;
    let interval = cloud_interval(frame, camera, ray, min(max_distance, CLOUD_MAX_RANGE));
    if (interval.y <= 0.0) { return vec4(0.0); }
    let light = frame.light_direction.xyz;
    let layer_point = vec3(camera.x, frame.clouds.y + frame.clouds.z * 0.5, camera.z);
    let irradiance = atmosphere_direct_irradiance(frame, layer_point);
    let cosine = dot(ray, light);
    let phase = vec3(atmosphere_phase_mie(cosine, 0.72) * 0.82
        + atmosphere_phase_mie(cosine, -0.25) * 0.18,
        atmosphere_phase_mie(cosine, 0.36), atmosphere_phase_mie(cosine, 0.0));
    let skylight = environment_sky(frame, vec3(0.0, 1.0, 0.0)) * 0.75
        + environment_sky(frame, normalize(vec3(ray.x, 0.08, ray.z))) * 0.25;
    let noise = cloud_sample_jitter(frame, pixel);
    var transmittance = 1.0;
    var radiance = vec3(0.0);
    // A uniform bound keeps optimized DX12 compilers from expanding the nested march.
    let view_steps = max(frame.flags.w, 1u);
    for (var index = 0u; index < view_steps; index += 1u) {
        let lower = f32(index) / f32(view_steps);
        let upper = f32(index + 1u) / f32(view_steps);
        let a = interval.x + lower * lower * (interval.y - interval.x);
        let b = interval.x + upper * upper * (interval.y - interval.x);
        let step = b - a;
        let travel = mix(a, b, noise);
        let point = camera + ray * travel;
        let footprint = cloud_pixel_footprint(frame, travel);
        let density = cloud_density(frame, point, footprint);
        if (density <= 0.0001) { continue; }
        var optical_depth = 0.0;
        if (any(irradiance > vec3(0.0))) {
            optical_depth = cloud_light_optical_depth(frame, point, light);
        }
        let light_source = irradiance * cloud_scattering(phase, optical_depth);
        let height = clamp((point.y - frame.clouds.y) / max(frame.clouds.z, 1.0), 0.0, 1.0);
        let local_depth = density * max(frame.clouds.z, 1.0) * 0.2 * cloud_extinction(frame);
        let powder = 1.0 - exp(-local_depth * 2.0);
        let interior = mix(1.0, 0.55 + powder * 0.55, clamp(-cosine, 0.0, 1.0));
        let ambient = skylight * mix(0.12, 0.95, smoothstep(0.0, 1.0, height)) * exp(-local_depth * 0.8);
        let opacity = 1.0 - exp(-density * step * cloud_extinction(frame));
        radiance += transmittance * opacity * (light_source * interior + ambient) * 0.98;
        transmittance *= 1.0 - opacity;
        if (transmittance < 0.01) { break; }
    }
    return vec4(max(radiance, vec3(0.0)), 1.0 - transmittance);
}
