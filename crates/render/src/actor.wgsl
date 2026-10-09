#import bevy_render::view::View
#import cinnabar::lighting::{actor_lighting, actor_distance_fog, actor_light_colour, tint_to_gamma, tint_to_linear}

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
@group(0) @binding(9) var skins_64: texture_2d_array<f32>;
@group(0) @binding(10) var skins_128: texture_2d_array<f32>;
@group(0) @binding(11) var skins_256: texture_2d_array<f32>;

@group(0) @binding(12) var actor_glint: texture_2d<f32>;
@group(0) @binding(13) var glint_sampler: sampler;

struct VertexOutput {
    @builtin(position) @invariant position: vec4<f32>,
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
    @location(12) @interpolate(flat) material: u32,
    @location(13) @interpolate(flat) dissolve_multiplier: f32,
    @location(14) @interpolate(flat) surface: u32,
    @location(15) back_native_lighting: vec3<f32>,
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
    out.back_native_lighting = vec3(1.0);
    out.surface = 0u;
    out.multitexture_layers = vec2(instance_words[instance_base + 25u], instance_words[instance_base + 26u]);
    out.material = instance_words[instance_base + 27u];
    if ((out.material & ACTOR_MATERIAL_DISABLE_OVERLAY_FLAG) != 0u) { out.overlay = vec4(0.0); }
    if ((out.material & ACTOR_MATERIAL_KIND_MASK) == ACTOR_MATERIAL_GLINT) { out.multitexture_layers.x = instance_index; }
    out.dissolve_multiplier = word_f32(instance_base + 28u);
    let light_color_multiplier = word_f32(instance_base + 29u);
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

    // ActorRigVertex uses its Rust stride for both the actor and hand vertex pullers.
    let vertex_base = (span.first_vertex + vertex_index) * ACTOR_RIG_VERTEX_WORDS;
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
    out.surface = vertex_words[vertex_base + 11u];
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
    out.native_lighting = actor_lighting(out.light, out.world_normal, out.overlay.a) * light_color_multiplier;
    out.back_native_lighting = out.native_lighting;
    if (out.surface != 0u) {
        out.back_native_lighting = actor_lighting(out.light, -out.world_normal, out.overlay.a) * light_color_multiplier;
    }
    out.valid = 1u;
    return out;
}

// Player-skin layers carry their resolution class in the top byte; class 0 samples `skins`.
// Every bound texture has one mip, so level 0 matches implicit-derivative sampling.
fn sample_actor_texture(uv: vec2<f32>, layer: u32) -> vec4<f32> {
    let index = i32(layer & 0xffffffu);
    switch (layer >> 24u) {
        case 1u: { return textureSampleLevel(skins_64, skin_sampler, uv, index, 0.0); }
        case 2u: { return textureSampleLevel(skins_128, skin_sampler, uv, index, 0.0); }
        case 3u: { return textureSampleLevel(skins_256, skin_sampler, uv, index, 0.0); }
        default: { return textureSampleLevel(skins, skin_sampler, uv, index, 0.0); }
    }
}

@fragment
fn actor_fragment(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let material = input.material & ACTOR_MATERIAL_KIND_MASK;
    let authored = (input.material & ACTOR_MATERIAL_AUTHORED_FLAG) != 0u;
    let emissive = material == ACTOR_MATERIAL_DRAGON || (input.material & ACTOR_MATERIAL_EMISSIVE_FLAG) != 0u;
    if (input.valid == 0u) {
        discard;
    }
    if (!front && input.back_uv.x < -1.0e8) {
        discard;
    }
    if (!front && authored && (input.material & ACTOR_MATERIAL_CULL_FLAG) != 0u) { discard; }
    let one_sided_material = material == ACTOR_MATERIAL_DRAGON || material == ACTOR_MATERIAL_DISSOLVE_DEPTH || material == ACTOR_MATERIAL_DISSOLVE_COLOR;
    if (!front && one_sided_material && input.surface == 0u) {
        discard;
    }
    var uv = select(input.back_uv, input.uv, front);
    // Every uv_anim material vanilla and packs ship samples with repeat wrap (scrolling armor).
    if (input.uv_wrap != 0u) {
        uv = fract(uv);
    }
    // Ordinary native actor materials compose gamma RGB. Undo Bevy's texture
    // decode before dye/overlay products, then transfer once at the output.
    var color = tint_to_gamma(sample_actor_texture(uv, input.skin_layer));
    if (material == ACTOR_MATERIAL_DISSOLVE_DEPTH) {
        if (color.a * input.dissolve_multiplier < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
        return vec4(0.0);
    }
    if (material == ACTOR_MATERIAL_DISSOLVE_COLOR && color.a < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
    if (material == ACTOR_MATERIAL_DRAGON && all(color == vec4(0.0))) { discard; }
    if (material == ACTOR_MATERIAL_GLINT && color.a < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
    let color_mask_material = material_class.y != 0u;
    let multitexture_material = material_class.z != 0u && all(input.multitexture_layers != vec2(0xffffffffu));
    if (authored && (input.material & ACTOR_MATERIAL_ALPHA_TEST_FLAG) != 0u) {
        if (emissive && all(color == vec4(0.0))) { discard; }
        if (!emissive && color.a < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
    }
    if (!authored && !color_mask_material && !multitexture_material && material == ACTOR_MATERIAL_DEFAULT && ((material_class.x == 0u && color.a < 0.1) || (material_class.x == 1u && color.a == 0.0))) {
        discard;
    }
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
    // Actor/Entity overlays blend BEFORE the shaded lightmap product. Vertex
    // shading preserves its interpolation and the explicit zero/unlit override.
    let overlay = select(input.overlay, vec4(0.0), material == ACTOR_MATERIAL_DISSOLVE_COLOR);
    let native_lighting = select(input.native_lighting, input.back_native_lighting, !front && input.surface != 0u);
    let lighting = select(native_lighting, mix(vec3(1.0), native_lighting, color.a), emissive);
    var lit_gamma = mix(color.rgb, overlay.rgb, overlay.a) * lighting;
    if (material == ACTOR_MATERIAL_GLINT) {
        let base = input.multitexture_layers.x * ACTOR_GPU_INSTANCE_WORDS;
        let offsets = vec2(word_f32(base + 30u), word_f32(base + 31u));
        let strength = word_f32(base + 32u);
        let centered_uv = uv - vec2(0.5);
        let first_uv = vec2(0.9396926 * centered_uv.x + 0.34202015 * centered_uv.y, -0.34202015 * centered_uv.x + 0.9396926 * centered_uv.y) + vec2(offsets.x + 0.5, 0.5);
        let second_uv = vec2(0.17364818 * centered_uv.x - 0.9848077 * centered_uv.y, 0.9848077 * centered_uv.x + 0.17364818 * centered_uv.y) + vec2(offsets.y + 0.5, 0.5);
        let first = tint_to_gamma(textureSampleLevel(actor_glint, glint_sampler, first_uv, 0.0)).rgb;
        let second = tint_to_gamma(textureSampleLevel(actor_glint, glint_sampler, second_uv, 0.0)).rgb;
        let foil_light = select(vec3(1.0), actor_light_colour(input.light), (input.light & ACTOR_LIGHT_WORLD_FLAG) != 0u);
        let foil = (first + second) * vec3(0.38, 0.19, 0.608) * strength * foil_light;
        lit_gamma += foil * foil;
    }
    let fogged_gamma = vec4(actor_distance_fog(lit_gamma, input.world_position, view.world_position), color.a);
#ifdef ACTOR_GAMMA_BLEND
    return fogged_gamma;
#else
    return tint_to_linear(fogged_gamma);
#endif
}
