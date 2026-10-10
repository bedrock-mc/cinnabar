#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import cinnabar::enhanced_atmosphere::{sky_view_uv, sky_view_ray}
#import cinnabar::enhanced_environment::{environment_coordinate, environment_ray,
    environment_sequence, environment_ggx_half, environment_cosine_ray}

@group(0) @binding(0) var raw_cube: texture_2d_array<f32>;
@group(0) @binding(1) var filter_sampler: sampler;
@group(0) @binding(2) var<uniform> environment_filter: vec4<f32>;
@group(0) @binding(3) var raw_sky: texture_2d<f32>;

fn incoming_radiance(ray: vec3<f32>, sky: bool) -> vec4<f32> {
    if (sky) { return textureSampleLevel(raw_sky, filter_sampler, sky_view_uv(ray), 0.0); }
    let coordinate = environment_coordinate(ray);
    return textureSampleLevel(raw_cube, filter_sampler, coordinate.xy, i32(coordinate.z), 0.0);
}

fn filtered_environment(uv: vec2<f32>) -> vec4<f32> {
    let sky = environment_filter.z >= 2.0;
    let diffuse = environment_filter.z == 1.0 || environment_filter.z == 3.0;
    var normal = environment_ray(uv, u32(environment_filter.x));
    if (sky) { normal = sky_view_ray(uv); }
    let sample_count = u32(environment_filter.w);
    var radiance = vec3(0.0);
    var available = 0.0;
    var total = 0.0;
    for (var index = 0u; index < sample_count; index += 1u) {
        let sequence = environment_sequence(index, sample_count);
        var ray = environment_cosine_ray(normal, sequence);
        var weight = 1.0;
        if (!diffuse) {
            let halfway = environment_ggx_half(normal, environment_filter.y, sequence);
            ray = reflect(-normal, halfway);
            weight = max(dot(normal, ray), 0.0);
        }
        if (weight > 0.0) {
            let incident = incoming_radiance(ray, sky);
            let known = clamp(incident.a, 0.0, 1.0) * weight;
            radiance += max(incident.rgb, vec3(0.0)) * known;
            available += known;
            total += weight;
        }
    }
    // Cosine importance sampling returns irradiance / pi, matching Lambert diffuse.
    return vec4(radiance / max(available, 1.0e-6), available / max(total, 1.0e-6));
}

@fragment fn prefilter_environment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return filtered_environment(in.uv);
}
