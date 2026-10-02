#import bevy_render::view::View
#import cinnabar::lighting::{lit_colour, light_colour, world_distance_fog}

struct GeometrySpan {
    first_vertex: u32,
    vertex_count: u32,
}

struct BoneMatrix {
    row_0: vec4<f32>,
    row_1: vec4<f32>,
    row_2: vec4<f32>,
}

// ActorGpuInstance is deliberately read as 25 packed words. Its Rust contract
// is 100 bytes; a WGSL struct containing vec4 rows would round the array stride
// to 112 bytes under storage-buffer layout rules.
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> instance_words: array<u32>;
@group(0) @binding(2) var<storage, read> vertex_words: array<u32>;
@group(0) @binding(3) var<storage, read> geometry_spans: array<GeometrySpan>;
@group(0) @binding(4) var<storage, read> previous_bones: array<BoneMatrix>;
@group(0) @binding(5) var<storage, read> current_bones: array<BoneMatrix>;
@group(0) @binding(6) var skins: texture_2d_array<f32>;
@group(0) @binding(7) var skin_sampler: sampler;
@group(0) @binding(8) var<uniform> material_class: vec4<u32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) skin_layer: u32,
    @location(2) @interpolate(flat) valid: u32,
    @location(3) world_normal: vec3<f32>,
    @location(4) back_uv: vec2<f32>,
    @location(5) @interpolate(flat) tint: u32,
    @location(6) @interpolate(flat) overlay: vec4<f32>,
    @location(7) @interpolate(flat) uv_wrap: u32,
    @location(8) @interpolate(flat) light: u32,
    @location(9) world_position: vec3<f32>,
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
    let instance_base = instance_index * 25u;
    let previous_bone_base = instance_words[instance_base + 12u];
    let current_bone_base = instance_words[instance_base + 13u];
    let geometry_id = instance_words[instance_base + 14u];
    let texture_layer = instance_words[instance_base + 15u];
    let partial_tick = clamp(word_f32(instance_base + 16u), 0.0, 1.0);
    let overlay_rgba8 = instance_words[instance_base + 19u];
    let span = geometry_spans[geometry_id];

    var out: VertexOutput;
    out.skin_layer = texture_layer;
    out.tint = instance_words[instance_base + 18u];
    out.overlay = unpack4x8unorm(overlay_rgba8);
    out.light = instance_words[instance_base + 24u];
    // Render-controller uv_anim, applied as vanilla's entity shader does: offset + uv * scale.
    let uv_offset = vec2(word_f32(instance_base + 20u), word_f32(instance_base + 21u));
    let uv_scale = vec2(word_f32(instance_base + 22u), word_f32(instance_base + 23u));
    out.uv_wrap = select(0u, 1u, any(uv_offset != vec2(0.0)) || any(uv_scale != vec2(1.0)));
    if (vertex_index >= span.vertex_count) {
        out.position = vec4(2.0, 2.0, 2.0, 1.0);
        out.uv = vec2(0.0);
        out.back_uv = vec2(0.0);
        out.valid = 0u;
        out.world_normal = vec3(0.0, 1.0, 0.0);
        return out;
    }

    // ActorRigVertex is eleven packed words (position, normal, front/back UV, bone).
    let vertex_base = (span.first_vertex + vertex_index) * 11u;
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
    out.uv = uv_offset + vec2(
        bitcast<f32>(vertex_words[vertex_base + 6u]),
        bitcast<f32>(vertex_words[vertex_base + 7u]),
    ) * uv_scale;
    let raw_back_uv = vec2(
        bitcast<f32>(vertex_words[vertex_base + 8u]),
        bitcast<f32>(vertex_words[vertex_base + 9u]),
    );
    // A one-sided plane's back keeps its sentinel so the fragment stage can discard it.
    out.back_uv = select(uv_offset + raw_back_uv * uv_scale, raw_back_uv, raw_back_uv.x < -1.0e8);
    let bone_index = vertex_words[vertex_base + 10u];
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
    out.world_position = world.xyz;
    out.world_normal = normalize(vec3(
        dot(instance_row(instance_base, 0u).xyz, posed_normal),
        dot(instance_row(instance_base, 1u).xyz, posed_normal),
        dot(instance_row(instance_base, 2u).xyz, posed_normal),
    ));
    out.valid = 1u;
    return out;
}

@fragment
fn actor_fragment(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (input.valid == 0u) {
        discard;
    }
    if (!front && input.back_uv.x < -1.0e8) {
        discard;
    }
    var uv = select(input.back_uv, input.uv, front);
    // Every uv_anim material vanilla and packs ship samples with repeat wrap (scrolling armor).
    if (input.uv_wrap != 0u) {
        uv = fract(uv);
    }
    var color = textureSample(skins, skin_sampler, uv, i32(input.skin_layer));
    if ((material_class.x == 0u && color.a < 0.1) || (material_class.x == 1u && color.a == 0.0)) {
        discard;
    }
    // Dye multiplies only fully opaque texels; partially transparent texels are untinted overlay.
    if (input.tint != 0u && color.a > 0.99) {
        color = vec4(color.rgb * pow(unpack4x8unorm(input.tint).rgb, vec3(2.2)), color.a);
    }
    // Zero retains the explicit unlit material override.
    if ((input.light & 0x80000000u) != 0u) {
        color = vec4(
            lit_colour(
                color.rgb,
                light_colour(input.light),
            ),
            color.a,
        );
    }
    // The hurt/death overlay blends after the dye and light.
    return vec4(world_distance_fog(mix(color.rgb, input.overlay.rgb, input.overlay.a), input.world_position, view.world_position), color.a);
}
