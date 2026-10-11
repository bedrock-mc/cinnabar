import bevy_render::view::View;
import render::lighting::{actor_light_colour, tint_to_gamma, tint_to_linear};

struct Particle {
    center_light: vec4<f32>,
    axis_x: vec4<f32>,
    axis_y: vec4<f32>,
    uv: vec4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> particles: array<Particle>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) alpha_test: f32,
}

@vertex
fn particle_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
    );
    let particle = particles[instance_index];
    let corner = corners[vertex_index];
    let world_position = particle.center_light.xyz
        + particle.axis_x.xyz * corner.x
        + particle.axis_y.xyz * corner.y;
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world_position, 1.0);
    out.uv = vec2(
        particle.uv.x + (corner.x * 0.5 + 0.5) * particle.uv.z,
        particle.uv.y + (0.5 - corner.y * 0.5) * particle.uv.w,
    );
    var light = vec3(1.0);
    if (particle.axis_y.w > 0.5) {
        light = actor_light_colour(u32(particle.center_light.w));
    }
    out.color = vec4(particle.color.rgb * light, particle.color.a);
    out.alpha_test = particle.axis_x.w;
    return out;
}

@fragment
fn particle_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = tint_to_gamma(textureSample(atlas, atlas_sampler, in.uv));
    var colour = texel * in.color;
    if (in.alpha_test > 0.5) {
        if (texel.a < 0.5) {
            discard;
        }
        colour.a = in.color.a;
    }
    if (colour.a < 0.01) {
        discard;
    }
    return tint_to_linear(colour);
}
