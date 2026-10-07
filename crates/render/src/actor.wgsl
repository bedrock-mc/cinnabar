#import bevy_render::view::View
#import cinnabar::lighting::{actor_lighting, actor_distance_fog, tint_to_gamma, tint_to_linear}
#ifdef ENHANCED
#import cinnabar::enhanced_view::{shade_actor_surface}
#endif
#ifdef ENHANCED_SHADOW
#import cinnabar::enhanced_caster::{caster}
#endif
#ifdef ENHANCED_MOTION
#import cinnabar::enhanced_actor_motion::{submitted_actor_position, submitted_surface_motion}
#endif

struct GeometrySpan {
    first_vertex: u32,
    vertex_count: u32,
}

struct BoneMatrix {
    row_0: vec4<f32>,
    row_1: vec4<f32>,
    row_2: vec4<f32>,
}

// ActorGpuInstance is read as packed words; the loader substitutes its Rust layout stride.
// A WGSL struct containing vec4 rows would pad the array differently.
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
    @location(10) @interpolate(flat) multitexture_layers: vec2<u32>,
    @location(11) native_lighting: vec3<f32>,
#ifdef ENHANCED_MOTION
    @location(12) current_clip: vec4<f32>,
    @location(13) previous_clip: vec4<f32>,
    @location(14) @interpolate(flat) motion_valid: u32,
#endif
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
    let instance_base = instance_index * ACTOR_GPU_INSTANCE_WORDS;
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
    out.native_lighting = vec3(1.0);
#ifdef ENHANCED_MOTION
    out.current_clip = vec4(0.0);
    out.previous_clip = vec4(0.0);
    out.motion_valid = 0u;
#endif
    out.multitexture_layers = vec2(instance_words[instance_base + 25u], instance_words[instance_base + 26u]);
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
#ifdef ENHANCED_SHADOW
    out.position = caster.clip_from_world * world;
#else
    out.position = view.clip_from_world * world;
#endif
#ifdef ENHANCED_MOTION
    let submitted_world = submitted_actor_position(instance_index, bone_index, local);
    out.current_clip = out.position;
    out.previous_clip = caster.previous_clip_from_world * vec4(submitted_world.xyz, 1.0);
    out.motion_valid = select(0u, 1u, submitted_world.w > 0.5 && caster.previous_params.w > 0.5);
#endif
    out.world_position = world.xyz;
    out.world_normal = normalize(vec3(
        dot(instance_row(instance_base, 0u).xyz, posed_normal),
        dot(instance_row(instance_base, 1u).xyz, posed_normal),
        dot(instance_row(instance_base, 2u).xyz, posed_normal),
    ));
#ifndef ENHANCED_SHADOW
#ifndef ENHANCED
    out.native_lighting = actor_lighting(out.light, out.world_normal, out.overlay.a);
#endif
#endif
    out.valid = 1u;
    return out;
}

fn actor_surface_color(input: VertexOutput, front: bool) -> vec4<f32> {
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
    // Ordinary native actor materials compose gamma RGB. Undo Bevy's texture
    // decode before dye/overlay products, then transfer once at the output.
    var color = tint_to_gamma(textureSample(skins, skin_sampler, uv, i32(input.skin_layer)));
    let color_mask_material = material_class.y != 0u;
    let multitexture_material = material_class.z != 0u && all(input.multitexture_layers != vec2(0xffffffffu));
    if (!color_mask_material && !multitexture_material && ((material_class.x == 0u && color.a < 0.1) || (material_class.x == 1u && color.a == 0.0))) {
        discard;
    }
#ifndef ENHANCED_SHADOW
    if (input.tint != 0u) {
        let change_color = unpack4x8unorm(input.tint);
        let dye = change_color.rgb;
        if (color_mask_material) {
            // Native entity_change_color: alpha weights dye, never coverage/opacity. The
            // material has no alpha test and the pipeline has no blending, with depth writes.
            color = vec4(mix(color.rgb, color.rgb * dye, color.a), color.a * change_color.a);
        } else if (!multitexture_material && color.a > 0.99) {
            color = vec4(color.rgb * dye, color.a);
        }
    }
    if (multitexture_material) {
        // Native llama:entity_multitexture has three samplers, no ALPHA_TEST and no blend.
        // Coverage never comes from the base alpha; overlay alpha weights RGB instead.
        let tex1 = tint_to_gamma(textureSample(skins, skin_sampler, uv, i32(input.multitexture_layers.x)));
        let tex2 = tint_to_gamma(textureSample(skins, skin_sampler, uv, i32(input.multitexture_layers.y)));
        color = vec4(mix(mix(color.rgb, tex1.rgb, tex1.a), tex2.rgb, tex2.a), color.a);
    }
#endif
    return color;
}

#ifdef ENHANCED_SHADOW
#ifdef ENHANCED_MOTION
@fragment
fn actor_fragment_motion(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let color = actor_surface_color(input, front);
    return submitted_surface_motion(input.current_clip, input.previous_clip, input.motion_valid != 0u);
}
#endif
@fragment
fn actor_fragment_shadow(input: VertexOutput, @builtin(front_facing) front: bool) {
    let color = actor_surface_color(input, front);
}
#else
@fragment
fn actor_fragment(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let color = actor_surface_color(input, front);
#ifdef ENHANCED
    let albedo = tint_to_linear(vec4(mix(color.rgb, input.overlay.rgb, input.overlay.a), color.a));
    var lit = albedo.rgb;
    if ((input.light & 0x80000000u) != 0u) {
        let normal = select(-input.world_normal, input.world_normal, front);
        lit = shade_actor_surface(albedo.rgb, normalize(normal), input.world_position, input.position.xy, input.light);
    }
    return vec4(lit, albedo.a);
#else
    // Actor/Entity overlays blend BEFORE the shaded lightmap product. Vertex
    // shading preserves its interpolation and the explicit zero/unlit override.
    let lit_gamma = mix(color.rgb, input.overlay.rgb, input.overlay.a) * input.native_lighting;
    return tint_to_linear(vec4(actor_distance_fog(lit_gamma, input.world_position, view.world_position), color.a));
#endif
}
#endif
