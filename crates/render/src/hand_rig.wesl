import render::actor_rig::{BoneMatrix, posed_point, placed_point};
import render::lighting::{lit_colour, light_colour, native_light_texel, tint_to_gamma, tint_to_linear};
import render::glint::glint_layer_uvs;

// Near-camera first-person rig pass. It reuses the actor rig's packed storage layout
// (Rust-strided instances and vertices, bones as 3x vec4 rows) so the
// same CPU buffers feed both paths; only the view is hand-local and the fragment is lit.

struct HandView {
    clip_from_hand: mat4x4<f32>,
}

struct GeometrySpan {
    first_vertex: u32,
    vertex_count: u32,
}

// block/sky are raw 0..=15 light levels sampled at the player; daylight scales the sky channel.
struct HandLight {
    block_level: u32,
    sky_level: u32,
    daylight: f32,
    pad: u32,
    java_lights: array<vec4<f32>, 2>,
    java_normal_axes: array<vec4<f32>, 3>,
}

struct HandMaterial {
    texture_flags: vec4<u32>,
    layer_mask: vec4<u32>,
}

@group(0) @binding(0) var<uniform> view: HandView;
@group(0) @binding(1) var<storage, read> instance_words: array<u32>;
@group(0) @binding(2) var<storage, read> vertex_words: array<u32>;
@group(0) @binding(3) var<storage, read> geometry_spans: array<GeometrySpan>;
@group(0) @binding(4) var<storage, read> previous_bones: array<BoneMatrix>;
@group(0) @binding(5) var<storage, read> current_bones: array<BoneMatrix>;
@group(0) @binding(6) var skins: texture_2d_array<f32>;
@group(0) @binding(7) var skin_sampler: sampler;
@group(0) @binding(8) var<uniform> hand_material: HandMaterial;
@group(0) @binding(9) var<uniform> hand_light: HandLight;
// Instances whose texture layer has its top bit set sample this equipment atlas page instead.
@group(0) @binding(10) var item_atlas: texture_2d_array<f32>;
@group(0) @binding(14) var map_background: texture_2d_array<f32>;
@group(0) @binding(11) var offhand_atlas: texture_2d_array<f32>;
// Held-item enchantment glint, sampled with repeat addressing.
@group(0) @binding(12) var item_glint: texture_2d<f32>;
@group(0) @binding(13) var glint_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    // Keep edge samples inside the item's atlas region under MSAA.
    @location(0) @interpolate(perspective, centroid) uv: vec4<f32>,
    @location(1) @interpolate(flat) skin_layer: u32,
    @location(2) @interpolate(flat) valid: u32,
    @location(3) @interpolate(perspective, centroid) back_uv: vec2<f32>,
    @location(4) @interpolate(flat) shade: f32,
    // Instance index whose glint words apply, or MAX without glint.
    @location(5) @interpolate(flat) glint_instance: u32,
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

// Rescale inverse-transpose normals by the native model's inverse Z-row length.
fn java_normal(matrix: mat3x3<f32>, normal: vec3<f32>, reference: vec3<f32>) -> vec3<f32> {
    let cofactors = mat3x3(cross(matrix[1], matrix[2]), cross(matrix[2], matrix[0]), cross(matrix[0], matrix[1]));
    let orientation = sign(dot(matrix[0], cofactors[0]));
    return (cofactors * normal) * orientation / max(length(cofactors * reference), 1.0e-20);
}

// Java's two directional lights share diffuse 0.6 and ambient 0.4, without specular.
fn java_shade(normal: vec3<f32>) -> f32 {
    let first = max(dot(normal, hand_light.java_lights[0].xyz), 0.0);
    let second = max(dot(normal, hand_light.java_lights[1].xyz), 0.0);
    return min(0.4 + 0.6 * (first + second), 1.0);
}

// Native item glint: two rotated, scrolled layers of the item raster at the instance's UV
// scale, tinted and squared over the lit colour in gamma space.
fn item_foil(uv: vec2<f32>, origin: vec2<f32>, instance: u32, sample: u32) -> vec3<f32> {
    let base = instance * ACTOR_GPU_INSTANCE_WORDS;
    let offsets = vec2(word_f32(base + 30u), word_f32(base + 31u));
    let strength = word_f32(base + 32u);
    let scale = vec2(word_f32(base + 33u), word_f32(base + 34u));
    let coordinates = glint_layer_uvs(uv, origin, scale, offsets * scale.x, word_f32(base + 35u), false);
    let layers = tint_to_gamma(textureSampleLevel(item_glint, glint_sampler, coordinates[0], 0.0)).rgb + tint_to_gamma(textureSampleLevel(item_glint, glint_sampler, coordinates[1], 0.0)).rgb;
    let foil = layers * GLINT_TINT_RGB * strength * native_light_texel(sample);
    return foil * foil;
}

@vertex
fn hand_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let instance_base = instance_index * ACTOR_GPU_INSTANCE_WORDS;
    let previous_bone_base = instance_words[instance_base + 12u];
    let current_bone_base = instance_words[instance_base + 13u];
    let geometry_id = instance_words[instance_base + 14u];
    let texture_layer = instance_words[instance_base + 15u];
    let partial_tick = clamp(word_f32(instance_base + 16u), 0.0, 1.0);
    let span = geometry_spans[geometry_id];

    var out: VertexOutput;
    out.skin_layer = texture_layer;
    out.shade = 1.0;
    let material = instance_words[instance_base + 27u];
    let has_glint = (material & ACTOR_MATERIAL_KIND_MASK) == ACTOR_MATERIAL_GLINT || word_f32(instance_base + 35u) != 0.0;
    out.glint_instance = select(0xffffffffu, instance_index, has_glint);
    if (vertex_index >= span.vertex_count) {
        out.position = vec4(2.0, 2.0, 2.0, 1.0);
        out.uv = vec4(0.0);
        out.back_uv = vec2(0.0);
        out.valid = 0u;
        return out;
    }

    let vertex_base = (span.first_vertex + vertex_index) * ACTOR_RIG_VERTEX_WORDS;
    let local = vec3(
        bitcast<f32>(vertex_words[vertex_base]),
        bitcast<f32>(vertex_words[vertex_base + 1u]),
        bitcast<f32>(vertex_words[vertex_base + 2u]),
    );
    let artwork_uv = vec2(
        bitcast<f32>(vertex_words[vertex_base + 6u]),
        bitcast<f32>(vertex_words[vertex_base + 7u]),
    );
    let glint_origin = vec2(
        bitcast<f32>(vertex_words[vertex_base + ACTOR_RIG_VERTEX_WORDS - 2u]),
        bitcast<f32>(vertex_words[vertex_base + ACTOR_RIG_VERTEX_WORDS - 1u]),
    );
    out.uv = vec4(artwork_uv, glint_origin);
    out.back_uv = vec2(
        bitcast<f32>(vertex_words[vertex_base + 8u]),
        bitcast<f32>(vertex_words[vertex_base + 9u]),
    );
    let bone_index = vertex_words[vertex_base + 10u];
    let posed = posed_point(previous_bones[previous_bone_base + bone_index], current_bones[current_bone_base + bone_index], local, partial_tick);
    let placement = BoneMatrix(instance_row(instance_base, 0u), instance_row(instance_base, 1u), instance_row(instance_base, 2u));
    let world = vec4(placed_point(placement, posed), 1.0);
    out.position = view.clip_from_hand * world;
    if (hand_light.java_lights[0].w != 0.0) {
        let previous_matrix = previous_bones[previous_bone_base + bone_index];
        let current_matrix = current_bones[current_bone_base + bone_index];
        let bone = transpose(mat3x3(
            mix(previous_matrix.row_0.xyz, current_matrix.row_0.xyz, partial_tick),
            mix(previous_matrix.row_1.xyz, current_matrix.row_1.xyz, partial_tick),
            mix(previous_matrix.row_2.xyz, current_matrix.row_2.xyz, partial_tick),
        ));
        let placement = transpose(mat3x3(instance_row(instance_base, 0u).xyz, instance_row(instance_base, 1u).xyz, instance_row(instance_base, 2u).xyz));
        let normal = vec3(bitcast<f32>(vertex_words[vertex_base + 3u]), bitcast<f32>(vertex_words[vertex_base + 4u]), bitcast<f32>(vertex_words[vertex_base + 5u]));
        let item = (texture_layer & hand_material.texture_flags.x) != 0u;
        let offhand = (texture_layer & hand_material.texture_flags.y) != 0u;
        let light_index = select(0u, select(1u, 2u, offhand), item);
        out.shade = java_shade(java_normal(placement * bone, normal, hand_light.java_normal_axes[light_index].xyz));
    }
    out.valid = 1u;
    return out;
}

@fragment
fn hand_fragment(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let cutout = (input.skin_layer & hand_material.texture_flags.w) != 0u;
    let reverse_cube = select(false, input.back_uv.x < -1.0e8, !front);
    let uv = select(input.back_uv, input.uv.xy, select(reverse_cube, true, front));
    // Sample footprints are computed before any lane is discarded.
    let uv_dx = dpdx(uv);
    let uv_dy = dpdy(uv);
    if (input.valid == 0u) {
        discard;
    }
    if (reverse_cube && !cutout) {
        discard;
    }
    let layer = i32(input.skin_layer & hand_material.layer_mask.x);
    let skin_color = textureSampleGrad(skins, skin_sampler, uv, layer, uv_dx, uv_dy);
    let main_color = textureSampleGrad(item_atlas, skin_sampler, uv, layer, uv_dx, uv_dy);
    let off_color = textureSampleGrad(offhand_atlas, skin_sampler, uv, layer, uv_dx, uv_dy);
    let hand_color = select(main_color, off_color, (input.skin_layer & hand_material.texture_flags.y) != 0u);
    let paper_color = textureSampleGrad(map_background, skin_sampler, uv, 0, uv_dx, uv_dy);
    let item_color = select(hand_color, paper_color, (input.skin_layer & hand_material.layer_mask.y) != 0u);
    let is_item = (input.skin_layer & hand_material.texture_flags.x) != 0u;
    let color = select(skin_color, item_color, is_item);
    if ((!is_item || cutout) && color.a < 0.5) {
        discard;
    }
    let sample = hand_light.block_level | (hand_light.sky_level << 4u);
    var lit = lit_colour(color.rgb, light_colour(sample));
    if (hand_light.java_lights[0].w != 0.0) {
        lit = tint_to_linear(vec4(tint_to_gamma(color).rgb * input.shade * native_light_texel(sample), 1.0)).rgb;
    }
    if (is_item && input.glint_instance != 0xffffffffu) {
        let foiled = tint_to_gamma(vec4(lit, 1.0)).rgb + item_foil(uv, input.uv.zw, input.glint_instance, sample);
        lit = tint_to_linear(vec4(foiled, 1.0)).rgb;
    }
    let blend = is_item && (input.skin_layer & hand_material.texture_flags.z) != 0u;
    return vec4(lit, select(1.0, color.a, blend));
}
