#import bevy_render::view::View

struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>,
    moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>,
    sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>,
    fog_end_time: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> atmosphere: AtmosphereUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn atmosphere_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let clip_position = vec2(
        f32(vertex_index & 1u),
        f32((vertex_index >> 1u) & 1u),
    ) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(clip_position, 0.0, 1.0));
}

fn view_ray(position: vec2<f32>) -> vec3<f32> {
    let viewport_uv = (position - view.viewport.xy) / view.viewport.zw;
    let ndc = viewport_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    let view_position = view.view_from_clip * vec4(ndc, 1.0, 1.0);
    let view_direction = view_position.xyz / view_position.w;
    return normalize((view.world_from_view * vec4(view_direction, 0.0)).xyz);
}

fn moon_phase(ray: vec3<f32>, moon_direction: vec3<f32>, phase: f32) -> f32 {
    let radius = 0.018;
    let disk = smoothstep(cos(radius), cos(radius * 0.78), dot(ray, moon_direction));
    var tangent = cross(moon_direction, vec3(0.0, 1.0, 0.0));
    if (dot(tangent, tangent) < 0.001) {
        tangent = vec3(1.0, 0.0, 0.0);
    }
    tangent = normalize(tangent);
    let local_x = dot(ray, tangent) / sin(radius);
    let phase_angle = phase / 8.0 * 6.283185307179586;
    let terminator = cos(phase_angle);
    let waxing_sign = select(-1.0, 1.0, phase < 4.0);
    let illuminated = smoothstep(-0.08, 0.08, local_x * waxing_sign - terminator);
    return disk * (0.08 + illuminated * 0.92);
}

@fragment
fn atmosphere_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray = view_ray(in.position.xy);
    let horizon_to_zenith = smoothstep(-0.08, 0.72, ray.y);
    var colour = mix(
        atmosphere.sky_horizon_thunder.rgb,
        atmosphere.sky_zenith_rain.rgb,
        horizon_to_zenith,
    );
    if (ray.y < -0.08) {
        colour *= 0.72;
    }

    let sun_direction = normalize(atmosphere.sun_direction_daylight.xyz);
    let sun_disk = smoothstep(
        cos(0.013),
        cos(0.010),
        dot(ray, sun_direction),
    ) * smoothstep(-0.04, 0.02, sun_direction.y);
    let sun_colour = mix(vec3(1.0, 0.38, 0.12), vec3(1.0, 0.93, 0.72), atmosphere.sun_direction_daylight.w);
    colour = mix(colour, sun_colour, sun_disk);

    let moon_direction = normalize(atmosphere.moon_direction_phase.xyz);
    let moon_disk = moon_phase(ray, moon_direction, atmosphere.moon_direction_phase.w)
        * smoothstep(-0.04, 0.02, moon_direction.y);
    colour = mix(colour, vec3(0.68, 0.74, 0.88), moon_disk);

    return vec4(colour, 1.0);
}
