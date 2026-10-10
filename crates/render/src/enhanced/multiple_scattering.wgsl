#import cinnabar::enhanced_atmosphere::{ATM_PI, ATM_GROUND_RADIUS, ATM_TOP_RADIUS, ATM_MIN_HEIGHT,
    ATM_RAYLEIGH, ATM_MIE_SCATTER, ATM_STORM_AEROSOL, atmosphere_sphere,
    atmosphere_density, atmosphere_extinction, atmosphere_light_transport}

@group(0) @binding(0) var destination: texture_storage_3d<rgba16float, write>;
@group(0) @binding(1) var<uniform> batch: vec4<u32>;

struct ScatteringTransfer {
    radiance: vec3<f32>,
    feedback: vec3<f32>,
}

fn scattering_transfer(origin: vec3<f32>, ray: vec3<f32>, sun: vec3<f32>, rain: f32) -> ScatteringTransfer {
    let outer = atmosphere_sphere(origin, ray, ATM_TOP_RADIUS);
    let ground = atmosphere_sphere(origin, ray, ATM_GROUND_RADIUS);
    let end = select(outer.y, min(ground.x, outer.y), ground.x > 0.0);
    var result: ScatteringTransfer;
    result.radiance = vec3(0.0);
    result.feedback = vec3(0.0);
    var transmittance = vec3(1.0);
    for (var index = 0u; index < MULTIPLE_SCATTER_STEPS; index += 1u) {
        let lower = f32(index) / f32(MULTIPLE_SCATTER_STEPS);
        let upper = f32(index + 1u) / f32(MULTIPLE_SCATTER_STEPS);
        let a = lower * lower * end;
        let b = upper * upper * end;
        let point = origin + ray * (a + b) * 0.5;
        let density = atmosphere_density(length(point) - ATM_GROUND_RADIUS);
        let extinction = atmosphere_extinction(density, rain);
        let scattering = ATM_RAYLEIGH * density.x
            + vec3(ATM_MIE_SCATTER * density.y * mix(1.0, ATM_STORM_AEROSOL, rain));
        let step_transmittance = exp(-extinction * (b - a));
        let weight = transmittance * (vec3(1.0) - step_transmittance)
            / max(extinction, vec3(1.0e-6));
        result.feedback += weight * scattering;
        result.radiance += weight * scattering * atmosphere_light_transport(point, sun, rain)
            / (4.0 * ATM_PI);
        transmittance *= step_transmittance;
    }
    // The unknown planetary surface absorbs incident light; resident geometry owns its bounce.
    return result;
}

@compute @workgroup_size(MULTIPLE_SCATTER_WORKGROUP, MULTIPLE_SCATTER_WORKGROUP, 1)
fn generate_multiple_scattering(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(destination);
    let coordinate = vec3(id.x, id.y + batch.x, id.z + batch.y);
    if (coordinate.x >= size.x || coordinate.y >= min(batch.x + batch.z, size.y)
        || coordinate.z >= size.z) { return; }
    let unit = vec3<f32>(coordinate) / vec3<f32>(size - vec3(1u));
    let sun_parameter = unit.x * 2.0 - 1.0;
    let sun_cosine = sign(sun_parameter) * sun_parameter * sun_parameter;
    let sun = vec3(sqrt(max(1.0 - sun_cosine * sun_cosine, 0.0)), sun_cosine, 0.0);
    let height = mix(ATM_MIN_HEIGHT, ATM_TOP_RADIUS - ATM_GROUND_RADIUS, unit.y * unit.y);
    let origin = vec3(0.0, ATM_GROUND_RADIUS + height, 0.0);
    var radiance = vec3(0.0);
    var feedback = vec3(0.0);
    for (var index = 0u; index < MULTIPLE_SCATTER_DIRECTIONS; index += 1u) {
        let cosine = 1.0 - 2.0 * (f32(index) + 0.5) / f32(MULTIPLE_SCATTER_DIRECTIONS);
        let azimuth = 2.0 * ATM_PI * fract(f32(index) * 0.61803398875);
        let sine = sqrt(max(1.0 - cosine * cosine, 0.0));
        let ray = vec3(cos(azimuth) * sine, cosine, sin(azimuth) * sine);
        let transfer = scattering_transfer(origin, ray, sun, unit.z);
        radiance += transfer.radiance / f32(MULTIPLE_SCATTER_DIRECTIONS);
        feedback += transfer.feedback / f32(MULTIPLE_SCATTER_DIRECTIONS);
    }
    let multiple = radiance / max(vec3(1.0) - feedback, vec3(1.0e-4));
    textureStore(destination, vec3<i32>(coordinate), vec4(max(multiple, vec3(0.0)), 1.0));
}
