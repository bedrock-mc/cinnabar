#define_import_path cinnabar::enhanced_atmosphere

#import cinnabar::enhanced_common::EnhancedFrame

const ATM_PI: f32 = 3.14159265359;
const ATM_GROUND_RADIUS: f32 = 6360.0;
const ATM_TOP_RADIUS: f32 = 6420.0;
const ATM_RAYLEIGH: vec3<f32> = vec3(0.005802, 0.013558, 0.033100);
const ATM_MIE_SCATTER: f32 = 0.003996;
const ATM_MIE_EXTINCT: f32 = 0.004440;
const ATM_OZONE: vec3<f32> = vec3(0.000650, 0.001881, 0.000085);
const ATM_RAYLEIGH_HEIGHT: f32 = 8.0;
const ATM_MIE_HEIGHT: f32 = 1.2;
const ATM_OZONE_PEAK: f32 = 25.0;
const ATM_OZONE_HALF_WIDTH: f32 = 15.0;
const ATM_SUN_RADIUS: f32 = 0.00465;
const ATM_MOON_RADIUS: f32 = 0.00452;
// Enhanced enlarges visible discs without changing the sources' irradiance.
const ATM_CELESTIAL_DISC_SCALE: f32 = 1.5;
const ATM_STORM_AEROSOL: f32 = 6.0;
const ATM_VIEW_STEPS: u32 = 8u;
const ATM_LIGHT_STEPS: u32 = 6u;

fn sky_view_uv(ray: vec3<f32>) -> vec2<f32> {
    return vec2(atan2(ray.z, ray.x) / (2.0 * ATM_PI) + 0.5,
        acos(clamp(ray.y, -1.0, 1.0)) / ATM_PI);
}

fn sky_view_ray(uv: vec2<f32>) -> vec3<f32> {
    let azimuth = (uv.x - 0.5) * 2.0 * ATM_PI;
    let elevation = uv.y * ATM_PI;
    return vec3(cos(azimuth) * sin(elevation), cos(elevation),
        sin(azimuth) * sin(elevation));
}

struct AtmosphereIntegral {
    radiance: vec3<f32>,
    transmittance: vec3<f32>,
}

// Atmospheric lengths and coefficients use kilometres; world blocks use metres.
fn atmosphere_position(world: vec3<f32>) -> vec3<f32> {
    return vec3(0.0, ATM_GROUND_RADIUS + clamp(world.y * 0.001, 0.002, 250.0), 0.0);
}

fn atmosphere_sphere(origin: vec3<f32>, ray: vec3<f32>, radius: f32) -> vec2<f32> {
    let distance = length(origin);
    let b = dot(origin, ray);
    let discriminant = b * b - (distance - radius) * (distance + radius);
    if (discriminant < 0.0) { return vec2(-1.0); }
    let root = sqrt(max(discriminant, 0.0));
    return vec2(-b - root, -b + root);
}

fn atmosphere_density(height: f32) -> vec3<f32> {
    let altitude = max(height, 0.0);
    let ozone = max(0.0, 1.0 - abs(altitude - ATM_OZONE_PEAK) / ATM_OZONE_HALF_WIDTH);
    return vec3(exp(-altitude / ATM_RAYLEIGH_HEIGHT), exp(-altitude / ATM_MIE_HEIGHT), ozone);
}

fn atmosphere_extinction(density: vec3<f32>, rain: f32) -> vec3<f32> {
    let aerosol = mix(1.0, ATM_STORM_AEROSOL, rain);
    return ATM_RAYLEIGH * density.x
        + vec3(ATM_MIE_EXTINCT * density.y * aerosol)
        + ATM_OZONE * density.z;
}

fn atmosphere_phase_rayleigh(cosine: f32) -> f32 {
    return 3.0 * (1.0 + cosine * cosine) / (16.0 * ATM_PI);
}

fn atmosphere_phase_mie(cosine: f32, anisotropy: f32) -> f32 {
    let g = anisotropy;
    let denominator = max(1.0 + g * g - 2.0 * g * cosine, 0.001);
    return (1.0 - g * g) / (4.0 * ATM_PI * denominator * sqrt(denominator));
}

fn atmosphere_moon_fraction(frame: EnhancedFrame) -> f32 {
    return 0.5 + 0.5 * cos(frame.celestial.w * ATM_PI / 4.0);
}

fn atmosphere_solar_source(frame: EnhancedFrame) -> vec3<f32> {
    return vec3(max(frame.projection.y, 0.0));
}

fn atmosphere_lunar_source(frame: EnhancedFrame) -> vec3<f32> {
    return vec3(max(frame.projection.z, 0.0));
}

fn atmosphere_airglow(frame: EnhancedFrame, ray: vec3<f32>) -> vec3<f32> {
    let night = 1.0 - smoothstep(-0.18, 0.05, frame.celestial.y);
    let altitude = mix(0.6, 1.0, clamp(ray.y, 0.0, 1.0));
    return vec3(0.38, 0.57, 1.0) * max(frame.light_colour.w, 0.0) * 0.12 * night * altitude;
}

// The planet occludes the sun below its geometric horizon, including at altitude.
fn atmosphere_light_transport(
    origin: vec3<f32>, light: vec3<f32>, rain: f32,
) -> vec3<f32> {
    let ground = atmosphere_sphere(origin, light, ATM_GROUND_RADIUS);
    if (ground.x > 0.0) { return vec3(0.0); }
    let outer = atmosphere_sphere(origin, light, ATM_TOP_RADIUS);
    if (outer.y <= 0.0) { return vec3(1.0); }
    let entry = max(outer.x, 0.0);
    let segment = outer.y - entry;
    var optical_depth = vec3(0.0);
    for (var index = 0u; index < ATM_LIGHT_STEPS; index += 1u) {
        let lower = f32(index) / f32(ATM_LIGHT_STEPS);
        let upper = f32(index + 1u) / f32(ATM_LIGHT_STEPS);
        let a = entry + lower * lower * segment;
        let b = entry + upper * upper * segment;
        let sample_point = origin + light * (a + b) * 0.5;
        let density = atmosphere_density(length(sample_point) - ATM_GROUND_RADIUS);
        optical_depth += atmosphere_extinction(density, rain) * (b - a);
    }
    return exp(-optical_depth);
}

fn atmosphere_transmittance(
    frame: EnhancedFrame, world: vec3<f32>, to_light: vec3<f32>,
) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5) { return vec3(1.0); }
    return atmosphere_light_transport(atmosphere_position(world), to_light,
        clamp(frame.ambient_colour.w, 0.0, 1.0));
}

fn atmosphere_sun_irradiance(frame: EnhancedFrame, world: vec3<f32>) -> vec3<f32> {
    let sun = frame.celestial.xyz;
    return atmosphere_solar_source(frame) * atmosphere_transmittance(frame, world, sun);
}

// The rational column matches vertical and grazing Gaussian limits without a fragment march.
fn atmosphere_curved_column(height: f32, cosine: f32, scale: f32) -> f32 {
    let mu = clamp(cosine, 0.0, 1.0);
    let tangent_slope = 2.0 / ATM_PI;
    let curvature = 2.0 * scale / (ATM_PI * (ATM_GROUND_RADIUS + height));
    let denominator = tangent_slope * mu + sqrt((1.0 - tangent_slope) * (1.0 - tangent_slope)
        * mu * mu + curvature * (1.0 - mu * mu));
    return exp(-max(height, 0.0) / scale) * scale / max(denominator, 0.00001);
}

fn atmosphere_surface_transport(frame: EnhancedFrame, world: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5) { return vec3(1.0); }
    let origin = atmosphere_position(world);
    if (atmosphere_sphere(origin, light, ATM_GROUND_RADIUS).x > 0.0) { return vec3(0.0); }
    let height = max(length(origin) - ATM_GROUND_RADIUS, 0.0);
    let rayleigh = atmosphere_curved_column(height, light.y, ATM_RAYLEIGH_HEIGHT);
    let mie = atmosphere_curved_column(height, light.y, ATM_MIE_HEIGHT);
    var ozone_area = ATM_OZONE_HALF_WIDTH;
    if (height > ATM_OZONE_PEAK) {
        let remaining = max(ATM_OZONE_PEAK + ATM_OZONE_HALF_WIDTH - height, 0.0);
        ozone_area = remaining * remaining / (2.0 * ATM_OZONE_HALF_WIDTH);
    } else if (height > ATM_OZONE_PEAK - ATM_OZONE_HALF_WIDTH) {
        let removed = height - (ATM_OZONE_PEAK - ATM_OZONE_HALF_WIDTH);
        ozone_area -= removed * removed / (2.0 * ATM_OZONE_HALF_WIDTH);
    }
    let mu = clamp(light.y, 0.0, 1.0);
    let ozone_column = ozone_area / max(sqrt(mu * mu + 2.0 * max(ATM_OZONE_PEAK - height, 0.0)
        / (ATM_GROUND_RADIUS + height) * (1.0 - mu * mu)), 0.00001);
    let aerosol = mix(1.0, ATM_STORM_AEROSOL, clamp(frame.ambient_colour.w, 0.0, 1.0));
    return exp(-(ATM_RAYLEIGH * rayleigh + vec3(ATM_MIE_EXTINCT * mie * aerosol)
        + ATM_OZONE * ozone_column));
}

fn atmosphere_direct_irradiance(frame: EnhancedFrame, world: vec3<f32>) -> vec3<f32> {
    return max(frame.light_colour.rgb * frame.light_direction.w, vec3(0.0))
        * atmosphere_surface_transport(frame, world, frame.light_direction.xyz);
}

fn atmosphere_integral(
    frame: EnhancedFrame, origin: vec3<f32>, ray: vec3<f32>, entry: f32, end: f32,
) -> AtmosphereIntegral {
    var result: AtmosphereIntegral;
    result.radiance = vec3(0.0);
    result.transmittance = vec3(1.0);
    let sun = frame.celestial.xyz;
    let moon = -sun;
    let rain = clamp(frame.ambient_colour.w, 0.0, 1.0);
    let aerosol = mix(1.0, ATM_STORM_AEROSOL, rain);
    let solar = atmosphere_solar_source(frame);
    let lunar = atmosphere_lunar_source(frame) * atmosphere_moon_fraction(frame);
    let sun_rayleigh = atmosphere_phase_rayleigh(dot(ray, sun));
    let sun_mie = atmosphere_phase_mie(dot(ray, sun), 0.76);
    let moon_rayleigh = atmosphere_phase_rayleigh(dot(ray, moon));
    let moon_mie = atmosphere_phase_mie(dot(ray, moon), 0.76);
    let day = smoothstep(-0.14, 0.06, sun.y);
    for (var index = 0u; index < ATM_VIEW_STEPS; index += 1u) {
        let lower = f32(index) / f32(ATM_VIEW_STEPS);
        let upper = f32(index + 1u) / f32(ATM_VIEW_STEPS);
        let a = entry + lower * lower * (end - entry);
        let b = entry + upper * upper * (end - entry);
        let step = b - a;
        let point = origin + ray * (a + b) * 0.5;
        let density = atmosphere_density(length(point) - ATM_GROUND_RADIUS);
        let extinction = atmosphere_extinction(density, rain);
        let rayleigh = ATM_RAYLEIGH * density.x;
        let mie = vec3(ATM_MIE_SCATTER * density.y * aerosol);
        let solar_transport = atmosphere_light_transport(point, sun, rain);
        let lunar_transport = atmosphere_light_transport(point, moon, rain);
        var source = solar * solar_transport * (rayleigh * sun_rayleigh + mie * sun_mie)
            + lunar * lunar_transport * (rayleigh * moon_rayleigh + mie * moon_mie);
        // An isotropic second-bounce estimate fills the shadowed sky without a tint ramp.
        let albedo = (rayleigh + mie) / max(extinction, vec3(1.0e-6));
        let isotropic = solar * (day * 0.12) + lunar * 0.12;
        source += isotropic * (rayleigh + mie) / (4.0 * ATM_PI)
            / max(vec3(1.0) - albedo * 0.25, vec3(0.5));
        let step_transmittance = exp(-extinction * step);
        result.radiance += result.transmittance * source
            * (vec3(1.0) - step_transmittance) / max(extinction, vec3(1.0e-6));
        result.transmittance *= step_transmittance;
    }
    return result;
}

// Unloaded terrain reveals horizon haze rather than an invented planetary ground surface.
fn atmosphere_background_ray(frame: EnhancedFrame, ray: vec3<f32>) -> vec3<f32> {
    let radius = length(atmosphere_position(frame.camera_time.xyz));
    let horizon = -sqrt(max(1.0 - (ATM_GROUND_RADIUS / radius) * (ATM_GROUND_RADIUS / radius), 0.0));
    let floor = horizon + 0.0002;
    let delta = ray.y - floor;
    let elevation = clamp(floor + 0.5 * (delta + sqrt(delta * delta + 0.000144)), -0.9999, 1.0);
    let horizontal_length = length(ray.xz);
    let horizontal = select(vec2(1.0, 0.0), ray.xz / max(horizontal_length, 0.0001), horizontal_length > 0.0001);
    return vec3(horizontal.x * sqrt(max(1.0 - elevation * elevation, 0.0)), elevation,
        horizontal.y * sqrt(max(1.0 - elevation * elevation, 0.0)));
}

// Disc-free sky radiance is suitable for a low-resolution sky-view lookup table.
fn atmospheric_sky_background(frame: EnhancedFrame, ray: vec3<f32>) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5) {
        return mix(frame.sky_horizon.rgb, frame.sky_zenith.rgb, max(ray.y, 0.0));
    }
    let background_ray = atmosphere_background_ray(frame, ray);
    let origin = atmosphere_position(frame.camera_time.xyz);
    let outer = atmosphere_sphere(origin, background_ray, ATM_TOP_RADIUS);
    if (outer.y <= 0.0) { return vec3(0.0); }
    let entry = max(outer.x, 0.0);
    let integral = atmosphere_integral(frame, origin, background_ray, entry, outer.y);
    let airglow = atmosphere_airglow(frame, background_ray);
    return max(integral.radiance + airglow * integral.transmittance, vec3(0.0));
}

fn atmosphere_discs(frame: EnhancedFrame, ray: vec3<f32>) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5) { return vec3(0.0); }
    let origin = atmosphere_position(frame.camera_time.xyz);
    let ground = atmosphere_sphere(origin, ray, ATM_GROUND_RADIUS);
    if (ground.x > 0.0) { return vec3(0.0); }
    let sun = frame.celestial.xyz;
    let sun_angle = acos(clamp(dot(ray, sun), -1.0, 1.0));
    let moon_angle = acos(clamp(dot(ray, -sun), -1.0, 1.0));
    let antialias = max(0.00012, 0.5 * frame.viewport.w);
    let sun_radius = ATM_SUN_RADIUS * ATM_CELESTIAL_DISC_SCALE;
    let moon_radius = ATM_MOON_RADIUS * ATM_CELESTIAL_DISC_SCALE;
    let sun_disc = 1.0 - smoothstep(sun_radius - antialias,
        sun_radius + antialias, sun_angle);
    let moon_disc = 1.0 - smoothstep(moon_radius - antialias,
        moon_radius + antialias, moon_angle);
    if (sun_disc + moon_disc <= 0.0) { return vec3(0.0); }
    let transport = atmosphere_light_transport(origin, ray, frame.ambient_colour.w);
    let sun_radiance = atmosphere_solar_source(frame) * sun_disc / (ATM_PI * sun_radius * sun_radius);
    var moon_radiance = vec3(0.0);
    if (moon_disc > 0.0) {
        let moon = -sun;
        let helper = select(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), abs(moon.y) > 0.95);
        let right = normalize(cross(moon, helper));
        let up = cross(right, moon);
        let local = vec2(dot(ray, right), dot(ray, up)) / moon_radius;
        let normal = vec3(local, sqrt(max(1.0 - dot(local, local), 0.0)));
        let phase = frame.celestial.w * ATM_PI / 4.0;
        let lit = max(dot(normal, vec3(sin(phase), 0.0, cos(phase))), 0.0);
        moon_radiance = atmosphere_lunar_source(frame) * moon_disc * lit
            / (ATM_PI * moon_radius * moon_radius);
    }
    // Rgba16Float cannot represent the unscaled solar disc radiance.
    return min((sun_radiance + moon_radiance) * transport, vec3(60000.0));
}

fn atmosphere_star_hash(point: vec2<f32>) -> f32 {
    var value = fract(vec3(point.x, point.y, point.x) * vec3(0.1031, 0.1030, 0.0973));
    value += dot(value, value.yzx + vec3(33.33));
    return fract((value.x + value.y) * value.z);
}

// Direction-space stars remain fixed when the camera translates or the resolution changes.
fn atmosphere_stars(frame: EnhancedFrame, ray: vec3<f32>) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5 || ray.y <= 0.0) { return vec3(0.0); }
    let night = 1.0 - smoothstep(-0.2, 0.02, frame.celestial.y);
    if (night <= 0.0) { return vec3(0.0); }
    let uv = vec2(atan2(ray.z, ray.x) / (2.0 * ATM_PI) + 0.5,
        acos(clamp(ray.y, -1.0, 1.0)) / ATM_PI);
    let grid = vec2(1024.0, 512.0);
    let cell = floor(uv * grid);
    let seed = atmosphere_star_hash(cell);
    if (seed < 0.9975) { return vec3(0.0); }
    let centre = vec2(atmosphere_star_hash(cell + vec2(17.0, 31.0)),
        atmosphere_star_hash(cell + vec2(59.0, 73.0))) * 0.6 + 0.2;
    let separation = length(fract(uv * grid) - centre);
    let footprint = clamp(frame.viewport.w * 512.0, 0.04, 0.6);
    let radius = 0.075;
    let star = 1.0 - smoothstep(max(radius - footprint * 0.5, 0.0),
        radius + footprint * 0.5, separation);
    let energy = min(1.0, radius * radius / max(footprint * footprint, 0.0001));
    let tint = mix(vec3(0.72, 0.82, 1.0), vec3(1.0, 0.86, 0.65),
        atmosphere_star_hash(cell + vec2(101.0, 13.0)));
    return tint * star * energy * (0.006 + 0.02 * seed * seed) * night
        * (1.0 - frame.ambient_colour.w) * smoothstep(0.0, 0.15, ray.y);
}

fn atmospheric_sky(frame: EnhancedFrame, ray: vec3<f32>) -> vec3<f32> {
    return atmospheric_sky_background(frame, ray) + atmosphere_discs(frame, ray)
        + atmosphere_stars(frame, ray);
}

// A curved-path column approximation provides an inexpensive reflection fallback.
fn environment_sky(frame: EnhancedFrame, ray: vec3<f32>) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5) {
        return mix(frame.sky_horizon.rgb, frame.sky_zenith.rgb, max(ray.y, 0.0));
    }
    let background_ray = atmosphere_background_ray(frame, ray);
    let height = max(frame.camera_time.y * 0.001, 0.002);
    let mu = max(background_ray.y, 0.0);
    let rayleigh_column = atmosphere_curved_column(height, mu, ATM_RAYLEIGH_HEIGHT);
    let mie_column = atmosphere_curved_column(height, mu, ATM_MIE_HEIGHT);
    let rain = clamp(frame.ambient_colour.w, 0.0, 1.0);
    let aerosol = mix(1.0, ATM_STORM_AEROSOL, rain);
    let optical_depth = ATM_RAYLEIGH * rayleigh_column
        + vec3(ATM_MIE_EXTINCT * mie_column * aerosol);
    let sun = frame.celestial.xyz;
    let light = frame.light_direction.xyz;
    let transport = atmosphere_surface_transport(frame, frame.camera_time.xyz, light);
    let day = smoothstep(-0.14, 0.06, sun.y);
    let scattering = ATM_RAYLEIGH * rayleigh_column
        * atmosphere_phase_rayleigh(dot(background_ray, light))
        + vec3(ATM_MIE_SCATTER * mie_column * aerosol)
        * atmosphere_phase_mie(dot(background_ray, light), 0.6);
    let solar = atmosphere_solar_source(frame) * day;
    let direct = scattering * max(frame.light_colour.rgb * frame.light_direction.w, vec3(0.0)) * transport;
    let diffuse = optical_depth * (solar * 0.02
        + atmosphere_lunar_source(frame) * atmosphere_moon_fraction(frame) * 0.08);
    let radiance = (direct + diffuse) * (vec3(1.0) - exp(-optical_depth))
        / max(optical_depth, vec3(1.0e-5));
    return max(radiance + atmosphere_airglow(frame, background_ray), vec3(0.0));
}

fn atmospheric_environment(frame: EnhancedFrame, ray: vec3<f32>, roughness: f32) -> vec3<f32> {
    let directional = environment_sky(frame, ray);
    let hemisphere = environment_sky(frame, normalize(vec3(ray.x * 0.2, 0.75, ray.z * 0.2)));
    return mix(directional, hemisphere, roughness * roughness * 0.7);
}

// Integrate exponential height fog analytically; small height changes use its limit.
fn atmosphere_height_column(start_y: f32, end_y: f32, distance: f32, scale: f32) -> f32 {
    let lower = max(start_y, 0.0);
    let upper = max(end_y, 0.0);
    let delta = (upper - lower) / scale;
    var average = exp(-lower / scale);
    if (abs(delta) > 0.001) {
        average = (exp(-lower / scale) - exp(-upper / scale)) / delta;
    }
    return max(average * distance, 0.0);
}

// Streaming bounds are horizontal; altitude alone must not erase the terrain below.
fn finite_world_haze(frame: EnhancedFrame, world: vec3<f32>) -> f32 {
    if (frame.atmosphere.x < 0.5 || frame.atmosphere.w > 0.5) { return 0.0; }
    let range = max(frame.atmosphere.z, 1.0);
    return smoothstep(range * 0.86, range, length((world - frame.camera_time.xyz).xz));
}

fn cloud_shadow_receiver_height(frame: EnhancedFrame) -> f32 {
    return min(frame.camera_time.y, frame.clouds.y);
}

// The receiver plane remains beneath the layer when the camera flies above it.
fn cloud_shadow_uv(frame: EnhancedFrame, world: vec3<f32>) -> vec2<f32> {
    let light = frame.light_direction.xyz;
    let projected = world.xz + light.xz / max(light.y, 0.01)
        * (cloud_shadow_receiver_height(frame) - world.y);
    return (projected - frame.cloud_shadow.xy) / max(frame.cloud_shadow.z, 1.0) + vec2(0.5);
}

fn atmosphere_aerial_transmittance(frame: EnhancedFrame, world: vec3<f32>) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5) { return vec3(1.0); }
    let camera = frame.camera_time.xyz;
    let travel = world - camera;
    let distance = length(travel);
    if (distance < 0.01) { return vec3(1.0); }
    let rain = clamp(frame.ambient_colour.w, 0.0, 1.0);
    let rayleigh = atmosphere_height_column(camera.y, world.y, distance * 0.001, ATM_RAYLEIGH_HEIGHT * 1000.0);
    let mie = atmosphere_height_column(camera.y, world.y, distance * 0.001, ATM_MIE_HEIGHT * 1000.0);
    let low_fog = atmosphere_height_column(camera.y, world.y, distance, 240.0);
    let optical_depth = ATM_RAYLEIGH * rayleigh
        + vec3(ATM_MIE_EXTINCT * mie * mix(1.0, ATM_STORM_AEROSOL, rain))
        + vec3(low_fog * rain * 0.00065);
    return exp(-optical_depth);
}

fn physical_aerial(
    frame: EnhancedFrame, colour: vec3<f32>, world: vec3<f32>, horizon: vec3<f32>,
) -> vec3<f32> {
    if (frame.atmosphere.x < 0.5) { return colour; }
    let travel = world - frame.camera_time.xyz;
    let distance = length(travel);
    if (distance < 0.01) { return colour; }
    let ray = travel / distance;
    let transmittance = atmosphere_aerial_transmittance(frame, world);
    let source = environment_sky(frame, ray) + environment_sky(frame, vec3(0.0, 1.0, 0.0)) * 0.15;
    let aerial = colour * transmittance + source * (vec3(1.0) - transmittance);
    return mix(aerial, horizon, finite_world_haze(frame, world));
}
