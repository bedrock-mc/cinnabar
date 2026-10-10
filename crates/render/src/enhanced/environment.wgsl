#define_import_path cinnabar::enhanced_environment

const ENVIRONMENT_PI: f32 = 3.14159265359;

// Coordinates share the six capture cameras' +X,-X,+Y,-Y,+Z,-Z ordering.
fn environment_coordinate(direction: vec3<f32>) -> vec3<f32> {
    let magnitude = abs(direction);
    var face = 0.0;
    var uv = vec2(0.0);
    if (magnitude.x >= magnitude.y && magnitude.x >= magnitude.z) {
        if (direction.x >= 0.0) { uv = vec2(-direction.z, -direction.y) / max(magnitude.x, 1.0e-6); }
        else { face = 1.0; uv = vec2(direction.z, -direction.y) / max(magnitude.x, 1.0e-6); }
    } else if (magnitude.y >= magnitude.z) {
        if (direction.y >= 0.0) { face = 2.0; uv = vec2(direction.x, direction.z) / max(magnitude.y, 1.0e-6); }
        else { face = 3.0; uv = vec2(direction.x, -direction.z) / max(magnitude.y, 1.0e-6); }
    } else if (direction.z >= 0.0) { face = 4.0; uv = vec2(direction.x, -direction.y) / max(magnitude.z, 1.0e-6); }
    else { face = 5.0; uv = vec2(-direction.x, -direction.y) / max(magnitude.z, 1.0e-6); }
    return vec3(uv * vec2(0.5, -0.5) + vec2(0.5), face);
}

fn environment_ray(uv: vec2<f32>, face: u32) -> vec3<f32> {
    let p = uv * 2.0 - vec2(1.0);
    switch face {
        case 0u: { return normalize(vec3(1.0, p.y, -p.x)); }
        case 1u: { return normalize(vec3(-1.0, p.y, p.x)); }
        case 2u: { return normalize(vec3(p.x, 1.0, -p.y)); }
        case 3u: { return normalize(vec3(p.x, -1.0, p.y)); }
        case 4u: { return normalize(vec3(p.x, p.y, 1.0)); }
        default: { return normalize(vec3(-p.x, p.y, -1.0)); }
    }
}

fn environment_sequence(index: u32, count: u32) -> vec2<f32> {
    return vec2((f32(index) + 0.5) / f32(count), f32(reverseBits(index)) * 2.3283064365386963e-10);
}

fn environment_basis(normal: vec3<f32>, local: vec3<f32>) -> vec3<f32> {
    let axis = select(vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0), abs(normal.z) > 0.999);
    let tangent = normalize(cross(axis, normal));
    return tangent * local.x + cross(normal, tangent) * local.y + normal * local.z;
}

fn environment_ggx_half(normal: vec3<f32>, roughness: f32, sample: vec2<f32>) -> vec3<f32> {
    let alpha = max(roughness * roughness, 0.001);
    let cosine = sqrt((1.0 - sample.y) / (1.0 + (alpha * alpha - 1.0) * sample.y));
    let sine = sqrt(max(1.0 - cosine * cosine, 0.0));
    let azimuth = 2.0 * ENVIRONMENT_PI * sample.x;
    return environment_basis(normal, vec3(cos(azimuth) * sine, sin(azimuth) * sine, cosine));
}

fn environment_cosine_ray(normal: vec3<f32>, sample: vec2<f32>) -> vec3<f32> {
    let radius = sqrt(sample.y);
    let azimuth = 2.0 * ENVIRONMENT_PI * sample.x;
    return environment_basis(normal, vec3(radius * cos(azimuth), radius * sin(azimuth), sqrt(1.0 - sample.y)));
}

// Analytic split-sum fit integrates Fresnel and visibility over the GGX lobe.
fn environment_brdf(roughness: f32, n_dot_v: f32) -> vec2<f32> {
    let fit = clamp(roughness, 0.0, 1.0) * vec4(-1.0, -0.0275, -0.572, 0.022)
        + vec4(1.0, 0.0425, 1.04, -0.04);
    let visibility = min(fit.x * fit.x, exp2(-9.28 * clamp(n_dot_v, 0.0, 1.0))) * fit.x + fit.y;
    return max(vec2(-1.04, 1.04) * visibility + fit.zw, vec2(0.0));
}

fn environment_specular_weight(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let integrated = environment_brdf(roughness, n_dot_v);
    return clamp(f0 * integrated.x + vec3(integrated.y), vec3(0.0), vec3(1.0));
}

fn environment_diffuse_weight(f0: vec3<f32>, roughness: f32, n_dot_v: f32, metallic: f32) -> vec3<f32> {
    return (vec3(1.0) - environment_specular_weight(f0, roughness, n_dot_v)) * (1.0 - metallic);
}
