#import bevy_render::view::View

struct GeometrySpan {
    first_vertex: u32,
    vertex_count: u32,
}

struct BoneMatrix {
    row_0: vec4<f32>,
    row_1: vec4<f32>,
    row_2: vec4<f32>,
}

// ActorGpuInstance is deliberately read as 22 packed words. Its Rust contract
// is 88 bytes; packed words avoid WGSL storage-struct padding ambiguity.
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> instance_words: array<u32>;
@group(0) @binding(2) var<storage, read> vertex_words: array<u32>;
@group(0) @binding(3) var<storage, read> geometry_spans: array<GeometrySpan>;
@group(0) @binding(4) var<storage, read> previous_bones: array<BoneMatrix>;
@group(0) @binding(5) var<storage, read> current_bones: array<BoneMatrix>;
@group(0) @binding(6) var skins: texture_2d<f32>;
@group(0) @binding(7) var skin_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) valid: u32,
    @location(2) world_normal: vec3<f32>,
}

fn word_f32(index: u32) -> f32 {
    return bitcast<f32>(instance_words[index]);
}

fn instance_row(base: u32, row: u32) -> vec4<f32> {
    let offset = base + row * 4u;
    return vec4(
        word_f32(offset),
        word_f32(offset + 1u),
        word_f32(offset + 2u),
        word_f32(offset + 3u),
    );
}

fn transform_point(matrix: BoneMatrix, point: vec3<f32>) -> vec3<f32> {
    let homogeneous = vec4(point, 1.0);
    return vec3(
        dot(matrix.row_0, homogeneous),
        dot(matrix.row_1, homogeneous),
        dot(matrix.row_2, homogeneous),
    );
}

fn transform_direction(matrix: BoneMatrix, direction: vec3<f32>) -> vec3<f32> {
    return vec3(
        dot(matrix.row_0.xyz, direction),
        dot(matrix.row_1.xyz, direction),
        dot(matrix.row_2.xyz, direction),
    );
}

@vertex
fn actor_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let instance_base = instance_index * 22u;
    let previous_bone_base = instance_words[instance_base + 12u];
    let current_bone_base = instance_words[instance_base + 13u];
    let geometry_id = instance_words[instance_base + 14u];
    let partial_tick = clamp(word_f32(instance_base + 16u), 0.0, 1.0);
    let texture_offset = vec2(word_f32(instance_base + 18u), word_f32(instance_base + 19u));
    let texture_scale = vec2(word_f32(instance_base + 20u), word_f32(instance_base + 21u));
    let span = geometry_spans[geometry_id];

    var out: VertexOutput;
    if (vertex_index >= span.vertex_count) {
        out.position = vec4(2.0, 2.0, 2.0, 1.0);
        out.uv = vec2(0.0);
        out.valid = 0u;
        out.world_normal = vec3(0.0, 1.0, 0.0);
        return out;
    }

    // ActorRigVertex is nine packed words (position, normal, UV, bone).
    let vertex_base = (span.first_vertex + vertex_index) * 9u;
    let local = vec3(
        bitcast<f32>(vertex_words[vertex_base]),
        bitcast<f32>(vertex_words[vertex_base + 1u]),
        bitcast<f32>(vertex_words[vertex_base + 2u]),
    );
    let local_normal = vec3(
        bitcast<f32>(vertex_words[vertex_base + 3u]),
        bitcast<f32>(vertex_words[vertex_base + 4u]),
        bitcast<f32>(vertex_words[vertex_base + 5u]),
    );
    let local_uv = vec2(
        bitcast<f32>(vertex_words[vertex_base + 6u]),
        bitcast<f32>(vertex_words[vertex_base + 7u]),
    );
    out.uv = texture_offset + clamp(local_uv, vec2(0.0), vec2(1.0)) * texture_scale;
    let bone_index = vertex_words[vertex_base + 8u];
    let previous = transform_point(previous_bones[previous_bone_base + bone_index], local);
    let current = transform_point(current_bones[current_bone_base + bone_index], local);
    let posed = mix(previous, current, partial_tick);
    let previous_normal = transform_direction(
        previous_bones[previous_bone_base + bone_index],
        local_normal,
    );
    let current_normal = transform_direction(
        current_bones[current_bone_base + bone_index],
        local_normal,
    );
    let posed_normal = normalize(mix(previous_normal, current_normal, partial_tick));
    let world = vec4(
        dot(instance_row(instance_base, 0u), vec4(posed, 1.0)),
        dot(instance_row(instance_base, 1u), vec4(posed, 1.0)),
        dot(instance_row(instance_base, 2u), vec4(posed, 1.0)),
        1.0,
    );
    out.position = view.clip_from_world * world;
    out.world_normal = normalize(vec3(
        dot(instance_row(instance_base, 0u).xyz, posed_normal),
        dot(instance_row(instance_base, 1u).xyz, posed_normal),
        dot(instance_row(instance_base, 2u).xyz, posed_normal),
    ));
    out.valid = 1u;
    return out;
}

@fragment
fn actor_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.valid == 0u) {
        discard;
    }
    let color = textureSample(skins, skin_sampler, input.uv);
    if (color.a < 0.1) {
        discard;
    }
    return color;
}
