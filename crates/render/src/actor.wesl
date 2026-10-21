import bevy_render::view::View;
import render::actor_rig::{BoneMatrix, transform_point, posed_point, posed_normal, placed_point, placed_normal};
import render::glint::glint_layer_uvs;
import render::lighting::{actor_lighting, actor_distance_fog, actor_light_colour, tint_to_gamma, tint_to_linear};
@if(ALPHA_TO_COVERAGE)
import render::lighting::{CutoutSample, cutout_alpha_array_region};

struct GeometrySpan {
    first_vertex: u32,
    vertex_count: u32,
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
@group(0) @binding(14) var item_glint: texture_2d<f32>;

@if(ACTOR_MOTION)
struct MotionInstance {
    current_clip_from_actor: mat4x4<f32>,
    previous_clip_from_actor: mat4x4<f32>,
    pose: vec4<u32>,
}
@if(ACTOR_MOTION)
@group(2) @binding(0) var<storage, read> actor_motion: array<MotionInstance>;
@if(ACTOR_MOTION)
@group(2) @binding(1) var<storage, read> rendered_previous_bones: array<BoneMatrix>;

struct VertexOutput {
    @builtin(position) @invariant position: vec4<f32>,
    // xy samples the artwork; zw carries the item sprite or face origin.
    @location(0) @interpolate(perspective, centroid) uv: vec4<f32>,
    // Texture layer, geometry validity, optional glint instance index and UV wrapping.
    @location(1) @interpolate(flat) skin_info: vec4<u32>,
    @if(!ACTOR_MOTION)
    @location(2) world_normal: vec3<f32>,
    @if(ACTOR_MOTION)
    @location(2) current_clip: vec4<f32>,
    @location(3) @interpolate(perspective, centroid) back_uv: vec2<f32>,
    @location(4) @interpolate(flat) tint: u32,
    @location(5) @interpolate(flat) overlay: vec4<f32>,
    @location(7) @interpolate(flat) light: u32,
    @location(8) world_position: vec3<f32>,
    @location(9) @interpolate(flat) multitexture_layers: vec2<u32>,
    @location(10) native_lighting: vec3<f32>,
    @location(11) @interpolate(flat) material: u32,
    // Stored float bits and surface kind share a slot within default motion interface limits.
    @location(12) @interpolate(flat) material_parameters: vec2<u32>,
    @location(14) back_native_lighting: vec3<f32>,
    @if(ALPHA_TO_COVERAGE)
    @location(13) @interpolate(flat) chart_bounds: vec4<u32>,
    @if(ACTOR_MOTION)
    @location(6) previous_clip: vec4<f32>,
}

// Geometry texture dimensions use u16 coordinates; both charts fit one flat varying.
@if(ALPHA_TO_COVERAGE)
const ACTOR_CHART_BITS: u32 = ACTOR_CHART_COORDINATE_BITS;

// Selects the physical raster used by this skin resolution class.
@if(ALPHA_TO_COVERAGE)
fn actor_texture_dimensions(layer: u32) -> vec2<f32> {
    switch (layer >> 24u) {
        case 1u: { return vec2<f32>(textureDimensions(skins_64, 0)); }
        case 2u: { return vec2<f32>(textureDimensions(skins_128, 0)); }
        case 3u: { return vec2<f32>(textureDimensions(skins_256, 0)); }
        default: { return vec2<f32>(textureDimensions(skins, 0)); }
    }
}

// Reads both UV maps from the existing geometry buffer.
@if(ALPHA_TO_COVERAGE)
fn actor_triangle_uv(vertex: u32) -> vec4<f32> {
    let base = vertex * ACTOR_RIG_VERTEX_WORDS;
    return vec4(
        bitcast<f32>(vertex_words[base + 6u]), bitcast<f32>(vertex_words[base + 7u]),
        bitcast<f32>(vertex_words[base + 8u]), bitcast<f32>(vertex_words[base + 9u]),
    );
}

// Bounds texture taps to texels touched by the chart, including fractional border texels.
@if(ALPHA_TO_COVERAGE)
fn actor_chart_region(low: vec2<f32>, high: vec2<f32>, size: vec2<f32>) -> vec4<u32> {
    let limit = vec2<i32>(size) - vec2(1);
    let first = clamp(vec2<i32>(floor(clamp(low, vec2(0.0), vec2(1.0)) * size)), vec2(0), limit);
    let last = max(first, clamp(vec2<i32>(ceil(clamp(high, vec2(0.0), vec2(1.0)) * size)) - vec2(1), vec2(0), limit));
    return vec4<u32>(vec4(first, last));
}

// Packs independent front/back UV extents without growing the stored rig vertex.
@if(ALPHA_TO_COVERAGE)
fn actor_triangle_chart_bounds(first: u32, last: u32, layer: u32) -> vec4<u32> {
    let a = actor_triangle_uv(first);
    let b = actor_triangle_uv(min(first + 1u, last));
    let c = actor_triangle_uv(min(first + 2u, last));
    let low = min(min(a, b), c);
    let high = max(max(a, b), c);
    let size = actor_texture_dimensions(layer);
    let front = actor_chart_region(low.xy, high.xy, size);
    let back = actor_chart_region(low.zw, high.zw, size);
    return front | (back << vec4(ACTOR_CHART_BITS));
}

// Selects the chart belonging to the rasterized side of the triangle.
@if(ALPHA_TO_COVERAGE)
fn actor_fragment_chart_bounds(packed: vec4<u32>, front: bool) -> vec4<i32> {
    let mask = (1u << ACTOR_CHART_BITS) - 1u;
    return vec4<i32>(select(packed >> vec4(ACTOR_CHART_BITS), packed & vec4(mask), front));
}

fn word_f32(index: u32) -> f32 {
    return bitcast<f32>(instance_words[index]);
}

// The two glint layers share sprite-normalized coordinates and the selected raster.
fn glint_layers(uv: vec2<f32>, origin: vec2<f32>, offsets: vec2<f32>, scale: vec2<f32>, item_extent: f32, legacy: bool) -> vec3<f32> {
    let layers = glint_layer_uvs(uv, origin, scale, offsets * scale.x, item_extent, legacy);
    if (item_extent > 0.0) {
        return tint_to_gamma(textureSampleLevel(item_glint, glint_sampler, layers[0], 0.0)).rgb + tint_to_gamma(textureSampleLevel(item_glint, glint_sampler, layers[1], 0.0)).rgb;
    }
    return tint_to_gamma(textureSampleLevel(actor_glint, glint_sampler, layers[0], 0.0)).rgb + tint_to_gamma(textureSampleLevel(actor_glint, glint_sampler, layers[1], 0.0)).rgb;
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

@if(ACTOR_MASK)
@group(2) @binding(0) var<uniform> mask_displacement: vec4<f32>;

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
    out.skin_info.x = texture_layer;
    out.tint = instance_words[instance_base + 18u];
    out.overlay = unpack4x8unorm(overlay_rgba8);
    out.light = instance_words[instance_base + 24u];
    out.native_lighting = vec3(1.0);
    out.back_native_lighting = vec3(1.0);
    out.material_parameters.y = 0u;
    out.multitexture_layers = vec2(instance_words[instance_base + 25u], instance_words[instance_base + 26u]);
    out.material = instance_words[instance_base + 27u];
    if ((out.material & ACTOR_MATERIAL_DISABLE_OVERLAY_FLAG) != 0u) { out.overlay = vec4(0.0); }
    let has_glint = (out.material & ACTOR_MATERIAL_KIND_MASK) == ACTOR_MATERIAL_GLINT || word_f32(instance_base + 35u) != 0.0;
    out.skin_info.z = select(0xffffffffu, instance_index, has_glint);
    out.material_parameters.x = instance_words[instance_base + 28u];
    let light_color_multiplier = word_f32(instance_base + 29u);
    // Render-controller uv_anim, applied as vanilla's entity shader does: offset + uv * scale.
    let uv_offset = vec2(word_f32(instance_base + 20u), word_f32(instance_base + 21u));
    let uv_scale = vec2(word_f32(instance_base + 22u), word_f32(instance_base + 23u));
    out.skin_info.w = select(0u, 1u, any(uv_offset != vec2(0.0)) || any(uv_scale != vec2(1.0)));
    if (vertex_index >= span.vertex_count) {
        out.position = vec4(2.0, 2.0, 2.0, 1.0);
        @if(ACTOR_MOTION)
        out.current_clip = out.position;
        @if(ACTOR_MOTION)
        out.previous_clip = out.position;
        out.uv = vec4(0.0);
        out.back_uv = vec2(0.0);
        out.skin_info.y = 0u;
        @if(!ACTOR_MOTION)
        out.world_normal = vec3(0.0, 1.0, 0.0);
        return out;
    }

    @if(ALPHA_TO_COVERAGE)
    out.chart_bounds = actor_triangle_chart_bounds(span.first_vertex + (vertex_index / 3u) * 3u, span.first_vertex + span.vertex_count - 1u, texture_layer);

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
    let artwork_uv = uv_offset + vec2(
        bitcast<f32>(vertex_words[vertex_base + 6u]),
        bitcast<f32>(vertex_words[vertex_base + 7u]),
    ) * uv_scale;
    let glint_origin = uv_offset + vec2(
        bitcast<f32>(vertex_words[vertex_base + ACTOR_RIG_VERTEX_WORDS - 2u]),
        bitcast<f32>(vertex_words[vertex_base + ACTOR_RIG_VERTEX_WORDS - 1u]),
    ) * uv_scale;
    out.uv = vec4(artwork_uv, glint_origin);
    let raw_back_uv = vec2(
        bitcast<f32>(vertex_words[vertex_base + 8u]),
        bitcast<f32>(vertex_words[vertex_base + 9u]),
    );
    // A one-sided plane's back keeps its sentinel so the fragment stage can discard it.
    out.back_uv = select(uv_offset + raw_back_uv * uv_scale, raw_back_uv, raw_back_uv.x < -1.0e8);
    let bone_index = vertex_words[vertex_base + 10u];
    out.material_parameters.y = vertex_words[vertex_base + 11u];
    let previous_bone = previous_bones[previous_bone_base + bone_index];
    let current_bone = current_bones[current_bone_base + bone_index];
    let posed = posed_point(previous_bone, current_bone, local, partial_tick);
    let bone_normal = posed_normal(previous_bone, current_bone, local_normal, partial_tick);
    let placement = BoneMatrix(
        instance_row(instance_base, 0u),
        instance_row(instance_base, 1u),
        instance_row(instance_base, 2u),
    );
    var world = vec4(placed_point(placement, posed), 1.0);
    @if(ACTOR_MASK)
    { world = vec4(world.xyz + mask_displacement.xyz, 1.0); }
    out.position = view.clip_from_world * world;
    @if(ACTOR_MOTION)
    {
        let motion = actor_motion[instance_index];
        out.current_clip = motion.current_clip_from_actor * vec4(posed, 1.0);
        out.previous_clip = out.current_clip;
        if (motion.pose.y != 0u) {
            let previous_rendered = transform_point(rendered_previous_bones[motion.pose.x + bone_index], local);
            out.previous_clip = motion.previous_clip_from_actor * vec4(previous_rendered, 1.0);
        }
    }
    out.world_position = world.xyz;
    let world_normal = placed_normal(placement, bone_normal);
    @if(!ACTOR_MOTION)
    out.world_normal = world_normal;
    out.native_lighting = actor_lighting(out.light, world_normal, out.overlay.a) * light_color_multiplier;
    out.back_native_lighting = out.native_lighting;
    if (out.material_parameters.y != 0u) {
        out.back_native_lighting = actor_lighting(out.light, -world_normal, out.overlay.a) * light_color_multiplier;
    }
    out.skin_info.y = 1u;
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

// Player skin resolution classes keep their own texel footprint when measuring cutout coverage.
@if(ALPHA_TO_COVERAGE)
fn actor_alpha_footprint(@if(ALPHA_TO_COVERAGE)
uv: vec2<f32>, @if(ALPHA_TO_COVERAGE)
layer: u32, @if(ALPHA_TO_COVERAGE)
dx: vec2<f32>, @if(ALPHA_TO_COVERAGE)
dy: vec2<f32>, @if(ALPHA_TO_COVERAGE)
sampled: vec4<f32>, @if(ALPHA_TO_COVERAGE)
repeat_uv: bool, @if(ALPHA_TO_COVERAGE)
region: vec4<i32>) -> CutoutSample {
    let index = i32(layer & 0xffffffu);
    switch (layer >> 24u) {
        case 1u: { return cutout_alpha_array_region(skins_64, uv, index, dx, dy, sampled, repeat_uv, ACTOR_ALPHA_TEST_THRESHOLD, region); }
        case 2u: { return cutout_alpha_array_region(skins_128, uv, index, dx, dy, sampled, repeat_uv, ACTOR_ALPHA_TEST_THRESHOLD, region); }
        case 3u: { return cutout_alpha_array_region(skins_256, uv, index, dx, dy, sampled, repeat_uv, ACTOR_ALPHA_TEST_THRESHOLD, region); }
        default: { return cutout_alpha_array_region(skins, uv, index, dx, dy, sampled, repeat_uv, ACTOR_ALPHA_TEST_THRESHOLD, region); }
    }
}

fn actor_colour(input: VertexOutput, front: bool) -> vec4<f32> {
    var uv = select(input.back_uv, input.uv.xy, front);
    // Gradients precede per-pixel rejection and remain valid inside material branches.
    let uv_dx = dpdx(uv);
    let uv_dy = dpdy(uv);
    let material = input.material & ACTOR_MATERIAL_KIND_MASK;
    let legacy = (input.material & ACTOR_MATERIAL_LEGACY112_FLAG) != 0u;
    let color_mask_material = material_class.y != 0u || (input.material & ACTOR_COLOR_MASK_MATERIAL_FLAG) != 0u;
    let legacy_mask = legacy && (input.material & ACTOR_COLOR_MASK_MATERIAL_FLAG) != 0u;
    let authored = (input.material & ACTOR_MATERIAL_AUTHORED_FLAG) != 0u;
    let emissive = material == ACTOR_MATERIAL_DRAGON || (input.material & ACTOR_MATERIAL_EMISSIVE_FLAG) != 0u;
    if (input.skin_info.y == 0u) {
        discard;
    }
    if (!front && input.back_uv.x < -1.0e8) {
        discard;
    }
    if (!front && authored && (input.material & ACTOR_MATERIAL_CULL_FLAG) != 0u) { discard; }
    let one_sided_material = material == ACTOR_MATERIAL_DRAGON || material == ACTOR_MATERIAL_DISSOLVE_DEPTH || material == ACTOR_MATERIAL_DISSOLVE_COLOR;
    if (!front && one_sided_material && input.material_parameters.y == 0u) {
        discard;
    }
    // Every uv_anim material vanilla and packs ship samples with repeat wrap (scrolling armor).
    if (input.skin_info.w != 0u) {
        uv = fract(uv);
    }
    // Ordinary native actor materials compose gamma RGB. Undo Bevy's texture
    // decode before dye/overlay products, then transfer once at the output.
    let texel = sample_actor_texture(uv, input.skin_info.x);
    var color = tint_to_gamma(texel);
    @if(ALPHA_TO_COVERAGE)
    let cutout = actor_alpha_footprint(uv, input.skin_info.x, uv_dx, uv_dy, texel, input.skin_info.w != 0u, actor_fragment_chart_bounds(input.chart_bounds, front));
    @if(ALPHA_TO_COVERAGE)
    let coverage = cutout.coverage;
    @if(ALPHA_TO_COVERAGE)
    if (color.a < ACTOR_ALPHA_TEST_THRESHOLD && coverage > 0.0) { color = tint_to_gamma(cutout.colour); }
    if (material == ACTOR_MATERIAL_DISSOLVE_DEPTH) {
        if (color.a * bitcast<f32>(input.material_parameters.x) < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
        return vec4(0.0);
    }
    if (material == ACTOR_MATERIAL_DISSOLVE_COLOR && color.a < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
    if (material == ACTOR_MATERIAL_DRAGON && all(color == vec4(0.0))) { discard; }
    @if(!ALPHA_TO_COVERAGE)
    if (material == ACTOR_MATERIAL_GLINT && !legacy_mask && color.a < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
    if (legacy_mask && color.a == 0.0) { discard; }
    let multitexture_material = material_class.z != 0u && all(input.multitexture_layers != vec2(0xffffffffu));
    if (authored && (input.material & ACTOR_MATERIAL_ALPHA_TEST_FLAG) != 0u) {
        if (emissive && all(color == vec4(0.0))) { discard; }
        @if(!ALPHA_TO_COVERAGE)
        if (!emissive && !legacy_mask && color.a < ACTOR_ALPHA_TEST_THRESHOLD) { discard; }
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
        } else if (!multitexture_material && authored && (input.material & ACTOR_MATERIAL_BLEND_FLAG) != 0u) {
            color *= change_color;
        } else if (!multitexture_material && color.a > 0.99) {
            color = vec4(color.rgb * dye, color.a);
        }
    }
    if (multitexture_material) {
        // Native llama:entity_multitexture has three samplers, no ALPHA_TEST and no blend.
        // Coverage never comes from the base alpha; overlay alpha weights RGB instead.
        let tex1 = tint_to_gamma(textureSampleGrad(skins, skin_sampler, uv, i32(input.multitexture_layers.x), uv_dx, uv_dy));
        let tex2 = tint_to_gamma(textureSampleGrad(skins, skin_sampler, uv, i32(input.multitexture_layers.y), uv_dx, uv_dy));
        color = vec4(mix(mix(color.rgb, tex1.rgb, tex1.a), tex2.rgb, tex2.a), color.a);
    }
    // Actor/Entity overlays blend BEFORE the shaded lightmap product. Vertex
    // shading preserves its interpolation and the explicit zero/unlit override.
    let overlay = select(input.overlay, vec4(0.0), material == ACTOR_MATERIAL_DISSOLVE_COLOR);
    let native_lighting = select(input.native_lighting, input.back_native_lighting, !front && input.material_parameters.y != 0u);
    let lighting = select(native_lighting, mix(vec3(1.0), native_lighting, color.a), emissive);
    var lit_gamma = mix(color.rgb, overlay.rgb, overlay.a) * lighting;
    var legacy_foil = vec3(0.0);
    var legacy_alpha = color.a;
    if (input.skin_info.z != 0xffffffffu) {
        let base = input.skin_info.z * ACTOR_GPU_INSTANCE_WORDS;
        let offsets = vec2(word_f32(base + 30u), word_f32(base + 31u));
        let strength = word_f32(base + 32u);
        let glint_scale = vec2(word_f32(base + 33u), word_f32(base + 34u));
        let item_extent = word_f32(base + 35u);
        let worn_legacy = legacy && material == ACTOR_MATERIAL_GLINT && item_extent == 0.0;
        var foil_tex = vec3(0.0);
        if (worn_legacy) {
            foil_tex = glint_layers(uv, vec2(0.0), offsets, vec2(1.0), 1.0, true);
        } else {
            foil_tex = glint_layers(uv, input.uv.zw, offsets, glint_scale, item_extent, legacy);
        }
        let foil_light = select(vec3(1.0), actor_light_colour(input.light), (input.light & ACTOR_LIGHT_WORLD_FLAG) != 0u);
        let foil = foil_tex * GLINT_TINT_RGB * strength * foil_light;
        if (worn_legacy) {
            legacy_foil = foil * foil;
            legacy_alpha = foil_tex.r * strength;
        } else {
            lit_gamma += foil * foil;
        }
    }
    @if(ALPHA_TO_COVERAGE)
    let fogged_gamma = vec4(actor_distance_fog(lit_gamma, input.world_position, view.world_position) + legacy_foil, coverage);
    @if(!ALPHA_TO_COVERAGE)
    let fogged_gamma = vec4(actor_distance_fog(lit_gamma, input.world_position, view.world_position) + legacy_foil, legacy_alpha);
    @if(ACTOR_GAMMA_BLEND)
    return fogged_gamma;
    @if(!ACTOR_GAMMA_BLEND)
    return tint_to_linear(fogged_gamma);
}

@fragment
fn actor_fragment(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return actor_colour(input, front);
}

@if(MASK_DISSOLVE && !MASK_DISSOLVE_MSAA)
@group(2) @binding(1) var mask_dissolve_depth: texture_depth_2d;
@if(MASK_DISSOLVE_MSAA)
@group(2) @binding(1) var mask_dissolve_depth: texture_depth_multisampled_2d;

@fragment
fn actor_mask_fragment(input: VertexOutput, @builtin(front_facing) front: bool,
    @if(MASK_DISSOLVE_MSAA) @builtin(sample_index) sample_index: u32,
) -> @location(0) vec4<f32> {
    // The scene's function owns every material rejection and multisample cutout footprint.
    let colour = actor_colour(input, front);
    @if(MASK_DISSOLVE && !MASK_DISSOLVE_MSAA)
    let companion_depth = textureLoad(mask_dissolve_depth, vec2<i32>(input.position.xy), 0);
    @if(MASK_DISSOLVE_MSAA)
    let companion_depth = textureLoad(mask_dissolve_depth, vec2<i32>(input.position.xy), i32(sample_index));
    @if(MASK_DISSOLVE)
    if (companion_depth == 0.0 || input.position.z != companion_depth) { discard; }
    @if(MASK_BLEND)
    return vec4(vec3(clamp(colour.a, 0.0, 1.0)), 1.0);
    @if(!MASK_BLEND)
    return vec4(vec3(1.0), colour.a);
}

@if(ACTOR_MOTION)
// Both frames retain local detail by projecting their actor roots relative to their cameras.
fn actor_motion_velocity(input: VertexOutput) -> vec2<f32> {
    var velocity = vec2(0.0);
    if (input.previous_clip.w > 0.00001 && input.current_clip.w > 0.00001) {
        velocity = (input.current_clip.xy / input.current_clip.w - input.previous_clip.xy / input.previous_clip.w) * vec2(0.5, -0.5);
    }
    return velocity;
}

@if(ACTOR_MOTION)
@fragment
fn actor_motion_fragment(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // The same artwork and discard rules retain cutouts and multisample alpha coverage.
    let colour = actor_colour(input, front);
    return vec4(actor_motion_velocity(input), 1.0, colour.a);
}
