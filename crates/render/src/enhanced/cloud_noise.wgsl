#define_import_path cinnabar::enhanced_cloud_noise

@group(0) @binding(0) var noise_output: texture_storage_3d<rgba8unorm, write>;
@group(0) @binding(1) var noise_input: texture_3d<f32>;

fn noise_hash(cell: vec3<i32>, period: i32) -> u32 {
    let wrapped = ((cell % vec3(period)) + vec3(period)) % vec3(period);
    var value = u32(wrapped.x) * 1597334677u ^ u32(wrapped.y) * 3812015801u
        ^ u32(wrapped.z) * 2798796415u ^ 1013904223u;
    value = (value ^ (value >> 16u)) * 2246822519u;
    value = (value ^ (value >> 13u)) * 3266489917u;
    return value ^ (value >> 16u);
}

fn noise_gradient(cell: vec3<i32>, period: i32) -> vec3<f32> {
    let gradients = array<vec3<f32>, 12>(
        vec3(1.0, 1.0, 0.0), vec3(-1.0, 1.0, 0.0), vec3(1.0, -1.0, 0.0), vec3(-1.0, -1.0, 0.0),
        vec3(1.0, 0.0, 1.0), vec3(-1.0, 0.0, 1.0), vec3(1.0, 0.0, -1.0), vec3(-1.0, 0.0, -1.0),
        vec3(0.0, 1.0, 1.0), vec3(0.0, -1.0, 1.0), vec3(0.0, 1.0, -1.0), vec3(0.0, -1.0, -1.0));
    return gradients[noise_hash(cell, period) % 12u] * 0.70710678119;
}

fn periodic_perlin(coordinate: vec3<f32>, period: i32) -> f32 {
    let point = coordinate * f32(period);
    let cell = vec3<i32>(floor(point));
    let local = fract(point);
    let blend = local * local * local * (local * (local * 6.0 - 15.0) + 10.0);
    var result = 0.0;
    for (var z = 0; z < 2; z += 1) {
        for (var y = 0; y < 2; y += 1) {
            for (var x = 0; x < 2; x += 1) {
                let corner = vec3(x, y, z);
                let weight = mix(vec3(1.0) - blend, blend, vec3<f32>(corner));
                result += dot(noise_gradient(cell + corner, period), local - vec3<f32>(corner))
                    * weight.x * weight.y * weight.z;
            }
        }
    }
    return result;
}

fn periodic_worley(coordinate: vec3<f32>, period: i32) -> f32 {
    let point = coordinate * f32(period);
    let cell = vec3<i32>(floor(point));
    let local = fract(point);
    var nearest = 3.0;
    for (var z = -1; z <= 1; z += 1) {
        for (var y = -1; y <= 1; y += 1) {
            for (var x = -1; x <= 1; x += 1) {
                let offset = vec3(x, y, z);
                let hash = noise_hash(cell + offset, period);
                let feature = vec3<f32>(vec3(hash & 1023u, (hash >> 10u) & 1023u,
                    (hash >> 20u) & 1023u)) / 1024.0;
                let separation = vec3<f32>(offset) + feature - local;
                nearest = min(nearest, dot(separation, separation));
            }
        }
    }
    return clamp(1.0 - sqrt(nearest), 0.0, 1.0);
}

@compute @workgroup_size(CLOUD_NOISE_WORKGROUP, CLOUD_NOISE_WORKGROUP, CLOUD_NOISE_WORKGROUP)
fn generate_noise(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(noise_output);
    if (any(id >= size)) { return; }
    let coordinate = (vec3<f32>(id) + 0.5) / vec3<f32>(size);
    let perlin = clamp(0.5 + (periodic_perlin(coordinate, 4)
        + periodic_perlin(coordinate, 8) * 0.5 + periodic_perlin(coordinate, 16) * 0.25) * 0.75, 0.0, 1.0);
    let coarse = periodic_worley(coordinate, 4);
    let medium = periodic_worley(coordinate, 8);
    let fine = periodic_worley(coordinate, 16);
    let detail = periodic_worley(coordinate, 24);
    let billow = coarse * 0.625 + medium * 0.25 + fine * 0.125;
    let shape = clamp((perlin - (1.0 - billow) * 0.35) / 0.65, 0.0, 1.0);
    let erosion = medium * 0.625 + fine * 0.25 + detail * 0.125;
    // Regional type uses coarse cells; fine cellular detail remains in the erosion channel.
    // Alpha carries shape deviation so filtering retains unresolved cloud edges.
    textureStore(noise_output, vec3<i32>(id), vec4(shape, erosion, coarse, 0.0));
}

@compute @workgroup_size(CLOUD_NOISE_WORKGROUP, CLOUD_NOISE_WORKGROUP, CLOUD_NOISE_WORKGROUP)
fn filter_noise(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(noise_output);
    if (any(id >= size)) { return; }
    let base = vec3<i32>(id) * 2;
    var average = vec4(0.0);
    var second_moment = 0.0;
    for (var z = 0; z < 2; z += 1) {
        for (var y = 0; y < 2; y += 1) {
            for (var x = 0; x < 2; x += 1) {
                let sample = textureLoad(noise_input, base + vec3(x, y, z), 0);
                average += sample;
                second_moment += sample.r * sample.r + sample.a * sample.a;
            }
        }
    }
    average *= 0.125;
    average.a = sqrt(max(second_moment * 0.125 - average.r * average.r, 0.0));
    textureStore(noise_output, vec3<i32>(id), average);
}
