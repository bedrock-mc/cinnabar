#import bevy_render::view::View

struct ActorInstance {
    position_yaw: vec4<f32>,
    look_skin: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> instances: array<ActorInstance>;
@group(0) @binding(2) var skins: texture_2d_array<f32>;
@group(0) @binding(3) var skin_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) part: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) skin_layer: u32,
}

fn rotate_x(value: vec3<f32>, angle: f32) -> vec3<f32> {
    let sine = sin(angle);
    let cosine = cos(angle);
    return vec3(value.x, value.y * cosine - value.z * sine, value.y * sine + value.z * cosine);
}

fn rotate_y(value: vec3<f32>, angle: f32) -> vec3<f32> {
    let sine = sin(angle);
    let cosine = cos(angle);
    return vec3(value.x * cosine + value.z * sine, value.y, -value.x * sine + value.z * cosine);
}

@vertex
fn actor_vertex(input: VertexInput, @builtin(instance_index) instance_index: u32) -> VertexOutput {
    let instance = instances[instance_index];
    var local = input.position;
    if (input.part == 0u) {
        let head_pivot = vec3(0.0, 1.5, 0.0);
        var relative = local - head_pivot;
        relative = rotate_x(relative, instance.look_skin.x);
        relative = rotate_y(relative, instance.look_skin.y - instance.position_yaw.w);
        local = head_pivot + relative;
    }
    let world_position = rotate_y(local, instance.position_yaw.w) + instance.position_yaw.xyz;

    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world_position, 1.0);
    out.uv = input.uv;
    out.skin_layer = u32(instance.look_skin.z);
    return out;
}

@fragment
fn actor_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(skins, skin_sampler, input.uv, i32(input.skin_layer));
    if (color.a < 0.1) {
        discard;
    }
    return color;
}
