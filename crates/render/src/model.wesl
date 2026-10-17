import render::chunk_bindings::{
    view, chunk_origins, block_textures_page_0, block_textures_page_1, block_textures_page_2,
    block_textures_page_3, block_textures_page_4, block_sampler, terrain_gamma_page_0,
    terrain_gamma_page_1, terrain_gamma_page_2, terrain_gamma_page_3, terrain_gamma_page_4,
    model_sampler, animations, animation_frames, model_templates, geometry_streams, atmosphere,
};
@if(ENHANCED || ENHANCED_SHADOW)
import render::chunk_bindings::{
    enhanced_color_page_0, enhanced_color_page_1, enhanced_sampler, enhanced_texture_refs,
    enhanced_normal_page_0, enhanced_normal_page_1, enhanced_mer_page_0, enhanced_mer_page_1,
};
import render::material::{MaterialGpu, materials, positional_material_id, texture_layer, texture_page, material_uv_flags, texture_model_uv, texture_gradient_scale};
@if(ENHANCED_SHADOW)
import render::enhanced::caster::{caster_clip, caster_camera_clip, caster_previous_camera_clip, caster_history_valid, caster_excludes_emitter};
@if(ENHANCED_MOTION)
import render::enhanced::actor_motion::submitted_surface_motion;
@if(ENHANCED_MOTION)
import render::enhanced::common::encode_geometric_normal;
import render::world_projection::{section_camera_offset, camera_offset_clip};
import render::biome_tint::{world_animation_clock, blended_biome_tint_gamma};
import render::lighting::{configured_fog_fade, light_ao_factor, light_colour, material_ambient_occlusion, material_face_shade, tint_to_gamma, tint_to_linear, terrain_light_levels, terrain_light_colour};
@if(ALPHA_TO_COVERAGE)
import render::lighting::{CutoutSample, cutout_alpha_array, cutout_mix};
@if(ENHANCED)
import render::enhanced::atmosphere::enhanced_distance_fog;
@if(ENHANCED)
import render::enhanced::view::{sky_illumination, material_class, shade_material, foliage_displacement, enhanced_frame, enhanced_light_direction, enhanced_materials_enabled};
@if(ENHANCED)
import render::enhanced::radiance::block_illumination;
@if(ENHANCED)
import render::enhanced::pbr::{material_basis, material_normal, material_specular_aa, blend_material_sample, material_relief_weight, material_relief_ray, MaterialBasis, PBR_REF_LABPBR};
@if(ENHANCED || ENHANCED_SHADOW)
import render::enhanced::pbr::{authored_texture_slot, authored_layer, AUTHORED_REFERENCE_BASE_WORDS, AUTHORED_REFERENCE_OVERRIDE_WORDS};

const MODEL_ALPHA_THRESHOLD: f32 = 0.5;

// BAMBOO_CONSTANTS

// ENHANCED_PBR_LOOKUP

struct VertexOutput {
    @if(ENHANCED || ENHANCED_SHADOW)
    @location(15) @interpolate(flat) authored_refs: vec2<u32>,
    @builtin(position) @invariant clip_position: vec4<f32>,
    // Partially covered MSAA pixels must sample inside the authored quad.
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
    @location(1) @interpolate(flat) current_texture: u32,
    @location(2) normal: vec3<f32>,
    @location(3) @interpolate(flat) material_flags: u32,
    @location(4) @interpolate(flat) local_position: vec3<f32>,
    @location(5) @interpolate(flat) biome_record: u32,
    @location(6) @interpolate(flat) next_texture: u32,
    @location(7) @interpolate(flat) frame_blend: f32,
    @location(8) @interpolate(flat) visibility: vec2<u32>,
    @location(9) lighting: vec3<f32>,
    @if(ENHANCED)
    @location(11) sky_light: f32,
    @if(ENHANCED)
    @location(14) ambient_occlusion: f32,
    @if(!ENHANCED)
    @location(11) native_light_levels: vec2<f32>,
    @if(!ENHANCED)
    @location(14) native_ao_face: f32,
    @location(10) @interpolate(flat) world_origin: vec3<f32>,
    @location(12) world_position: vec3<f32>,
    @if(ENHANCED_MOTION)
    @location(13) previous_clip: vec4<f32>,
    @if(ENHANCED && !ENHANCED_MOTION)
    @location(13) @interpolate(flat) tint_surface: vec4<f32>,
    @if(!ENHANCED && !ENHANCED_MOTION)
    @location(13) @interpolate(flat) tint_gamma: vec3<f32>,
}

struct FrameSample { current: u32, next: u32, blend: f32 }
// Visibility also carries the admitted tile policy without another interpolator.
const MODEL_VISIBLE: u32 = 1u;
const MODEL_BOUNDED_TILE: u32 = 2u;

fn invisible_vertex() -> VertexOutput {
    var invisible: VertexOutput;
    invisible.clip_position = vec4(2.0, 2.0, 2.0, 1.0);
    invisible.uv = vec2(0.0);
    invisible.current_texture = 0u;
    @if(ENHANCED || ENHANCED_SHADOW)
    invisible.authored_refs = vec2(0xffffffffu);
    invisible.normal = vec3(0.0);
    invisible.material_flags = 0u;
    invisible.local_position = vec3(0.0);
    invisible.biome_record = 0u;
    invisible.next_texture = 0u;
    invisible.frame_blend = 0.0;
    invisible.visibility.x = 0u;
    invisible.lighting = vec3(0.0);
    @if(ENHANCED)
    invisible.sky_light = 0.0;
    @if(ENHANCED)
    invisible.ambient_occlusion = 0.0;
    @if(ENHANCED && !ENHANCED_MOTION)
    invisible.tint_surface = vec4(1.0, 1.0, 1.0, 0.0);
    @if(!ENHANCED)
    invisible.native_light_levels = vec2(0.0);
    @if(!ENHANCED)
    invisible.native_ao_face = 0.0;
    @if(!ENHANCED && !ENHANCED_MOTION)
    invisible.tint_gamma = vec3(1.0);
    invisible.visibility.y = 0u;
    invisible.world_origin = vec3(0.0);
    invisible.world_position = vec3(0.0);
    @if(ENHANCED_MOTION)
    invisible.previous_clip = vec4(0.0);
    return invisible;
}

fn animation_sample(material: MaterialGpu) -> FrameSample {
    if (material.animation == 0xffffffffu) {
        return FrameSample(material.texture, material.texture, 0.0);
    }
    let clock = world_animation_clock();
    let animation = animations[material.animation];
    let current_index = (clock.tick / animation.ticks_per_frame) % animation.frame_count;
    let current = animation_frames[animation.frame_start + current_index];
    if ((animation.flags & 1u) == 0u || animation.frame_count == 1u) {
        return FrameSample(current, current, 0.0);
    }
    let next_index = (current_index + 1u) % animation.frame_count;
    let partial = (f32(clock.tick % animation.ticks_per_frame) + clamp(clock.partial_tick, 0.0, 0.99999994)) /
        f32(animation.ticks_per_frame);
    return FrameSample(current, animation_frames[animation.frame_start + next_index], partial);
}

fn signed_i16(word: u32, high: bool) -> i32 {
    if (high) { return bitcast<i32>(word) >> 16u; }
    return bitcast<i32>(word << 16u) >> 16u;
}

fn packed_i16(words: u32, component: u32) -> i32 {
    let word = model_templates[words + component / 2u];
    return signed_i16(word, (component & 1u) != 0u);
}

fn packed_u16(words: u32, component: u32) -> u32 {
    let word = model_templates[words + component / 2u];
    return select(word & 0xffffu, word >> 16u, (component & 1u) != 0u);
}

fn rotate_cross(position: vec3<f32>, transform: u32) -> vec3<f32> {
    let centered = position - vec3(0.5, 0.0, 0.5);
    var rotated = centered;
    switch transform & 3u {
        case 1u: { rotated = vec3(-centered.z, centered.y, centered.x); }
        case 2u: { rotated = vec3(-centered.x, centered.y, -centered.z); }
        case 3u: { rotated = vec3(centered.z, centered.y, -centered.x); }
        default: {}
    }
    return rotated + vec3(0.5, 0.0, 0.5);
}

// Current lily-pad tessellator: unsigned wrapping position hash.
fn lily_pad_rotation(position: vec3<i32>) -> u32 {
    let h = (bitcast<u32>(position.z) * 0x6ebfff5u) ^
        (bitcast<u32>(position.x) * 0x2fc20fu) ^ bitcast<u32>(position.y);
    return (((h * 0x285b825u + 11u) * h) >> 16u) & 3u;
}

// Consecutive units share an instance; reject padding before fetching a descriptor or geometry.
@if(TERRAIN_UNITS)
@group(2) @binding(0) var<storage, read> terrain_units: array<vec4<u32>>;

@if(TERRAIN_UNITS)
@vertex
fn vertex_unit(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    // Header locates packed visibility references beside stable descriptor slots.
    let unit_index = instance_index * TERRAIN_UNITS_PER_INSTANCE
        + vertex_index / (TERRAIN_UNIT_QUADS * 4u);
    if (unit_index >= terrain_units[0].y) {
        var invisible: VertexOutput;
        invisible.clip_position = vec4(2.0, 2.0, 2.0, 1.0);
        return invisible;
    }
    let references = terrain_units[0].x;
    let slot = terrain_units[references + unit_index / 4u][unit_index % 4u];
    let unit = terrain_units[1u + slot];
    let quad = (vertex_index / 4u) % TERRAIN_UNIT_QUADS;
    if (quad >= unit.y || unit.w != 0u) {
        return invisible_vertex();
    }
    return model_vertex(unit.z * 4u + vertex_index % 4u, unit.x + quad);
}

// Fetches the vertex index and first instance from vertex buffer 0, as `chunk.wesl` describes.
@vertex
fn vertex(
    @location(0) offsets: vec2<u32>,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    return model_vertex(offsets.x, offsets.y + instance_index);
}

fn model_vertex(vertex_index: u32, instance_index: u32) -> VertexOutput {
    let local_vertex = vertex_index & 3u;
    let metadata_index = vertex_index / 4u;
    let corner = local_vertex;
    let geometry_word_count = arrayLength(&geometry_streams);
    if (instance_index > 0x7fffffffu) {
        return invisible_vertex();
    }
    let draw_ref_word = instance_index * 2u;
    if (draw_ref_word + 1u >= geometry_word_count) {
        return invisible_vertex();
    }
    let model_ref_index = geometry_streams[draw_ref_word];
    let quad_index = geometry_streams[draw_ref_word + 1u];
    if (quad_index >= 32u || model_ref_index > 0x3fffffffu) {
        return invisible_vertex();
    }
    let ref_word = model_ref_index * 4u;
    if (ref_word + 3u >= geometry_word_count) {
        return invisible_vertex();
    }
    let packed_transform = geometry_streams[ref_word];
    let template_id = geometry_streams[ref_word + 1u];
    let lighting_base_index = geometry_streams[ref_word + 2u];
    let visible_quad_mask = geometry_streams[ref_word + 3u];
    let template_word_count = arrayLength(&model_templates);
    if (template_word_count < 4u) {
        return invisible_vertex();
    }
    let template_count = model_templates[0];
    if (template_count > (template_word_count - 1u) / 3u || template_id >= template_count) {
        return invisible_vertex();
    }
    let descriptor = 1u + template_id * 3u;
    let quad_start = model_templates[descriptor];
    let quad_count = model_templates[descriptor + 1u];
    if (quad_count == 0u || quad_index >= quad_count) {
        return invisible_vertex();
    }
    let is_visible = (visible_quad_mask >> quad_index) & 1u;
    if (is_visible == 0u) {
        return invisible_vertex();
    }
    let template_quad_words = 1u + template_count * 3u;
    let stored_quad_count = (template_word_count - template_quad_words) / 12u;
    if (quad_start >= stored_quad_count || quad_index >= stored_quad_count - quad_start) {
        return invisible_vertex();
    }
    let template_quad_base = template_quad_words + (quad_start + quad_index) * 12u;

    let component = corner * 3u;
    var template_position = vec3<f32>(
        f32(packed_i16(template_quad_base, component)),
        f32(packed_i16(template_quad_base, component + 1u)),
        f32(packed_i16(template_quad_base, component + 2u)),
    ) / 256.0;
    let block_position = vec3<f32>(
        f32(packed_transform & 15u),
        f32((packed_transform >> 4u) & 15u),
        f32((packed_transform >> 8u) & 15u),
    );
    let origin = chunk_origins[metadata_index];
    let is_lily_pad = (model_templates[descriptor + 2u] & MODEL_LILY_PAD_FLAG) != 0u;
    let is_bamboo = (model_templates[descriptor + 2u] & MODEL_BAMBOO_FLAG) != 0u;
    let has_offset = (packed_transform & MODEL_RANDOM_OFFSET_FLAG) != 0u;
    var rotation = packed_transform >> 12u;
    if (is_bamboo) {
        if (!has_offset) {
            template_position.x += BAMBOO_OFFSET_MIN + f32(rotation & 15u) * BAMBOO_OFFSET_STEP;
            template_position.z += BAMBOO_OFFSET_MIN + f32((rotation >> 4u) & 15u) * BAMBOO_OFFSET_STEP;
        }
        if (quad_index == BAMBOO_POSITIVE_X_LEAF_QUAD) { template_position.z += BAMBOO_LEAF_PLANE_INSET; }
        if (quad_index == BAMBOO_POSITIVE_Z_LEAF_QUAD) { template_position.x += BAMBOO_LEAF_PLANE_INSET; }
        rotation = 0u;
    }
    if (is_lily_pad) {
        rotation = lily_pad_rotation(origin.value.xyz + vec3<i32>(block_position));
    }
    template_position = rotate_cross(template_position, rotation);
    if (has_offset) {
        if (lighting_base_index < 2u || lighting_base_index > geometry_word_count / 2u) { return invisible_vertex(); }
        let offset_word = lighting_base_index * 2u - 4u;
        template_position += vec3<f32>(bitcast<f32>(geometry_streams[offset_word]),
            bitcast<f32>(geometry_streams[offset_word + 1u]), bitcast<f32>(geometry_streams[offset_word + 2u]));
    }
    let local_position = block_position + template_position;
    let material_id = model_templates[template_quad_base + 10u];
    let quad_flags = model_templates[template_quad_base + 11u];
    let selected_material_id = positional_material_id(material_id, origin.value.xyz + vec3<i32>(block_position));
    let material = materials[selected_material_id];
    let frame = animation_sample(material);
    let uv_component = corner * 2u;

    if (lighting_base_index >= geometry_word_count / 2u || quad_index >= geometry_word_count / 2u - lighting_base_index) {
        return invisible_vertex();
    }
    let light_word = geometry_streams[(lighting_base_index + quad_index) * 2u + corner / 2u];
    let light_sample = select(light_word & 0xffffu, light_word >> 16u, (corner & 1u) != 0u);
    var out: VertexOutput;
    let world = vec3<f32>(origin.value.xyz) + local_position;
    let camera_offset = section_camera_offset(origin.value.xyz, local_position, view.world_position);
    out.clip_position = camera_offset_clip(view.clip_from_world, view.world_position, camera_offset);
    @if(ENHANCED_SHADOW && !ENHANCED_MOTION)
    out.clip_position = caster_clip(world, material_id, clamp(template_position.y, 0.0, 1.0));
    // The camera prepass keeps the main pass's camera-relative precision for its coverage.
    @if(ENHANCED_MOTION)
    out.clip_position = caster_camera_clip(world, view.world_position, camera_offset, material_id, clamp(template_position.y, 0.0, 1.0));
    @if(ENHANCED_MOTION)
    out.previous_clip = caster_previous_camera_clip(world, view.world_position, camera_offset, material_id, clamp(template_position.y, 0.0, 1.0));
    out.uv = vec2<f32>(
        f32(packed_u16(template_quad_base + 6u, uv_component)),
        f32(packed_u16(template_quad_base + 6u, uv_component + 1u)),
    ) / 4096.0;
    // World rotation acts on the authored UV rectangle, independently of model geometry.
    if ((material.flags & MATERIAL_ISOTROPIC_FLAG) != 0u) {
        let uv_flags = material_uv_flags(material.flags, origin.value.xyz + vec3<i32>(block_position));
        switch uv_flags & 3u {
            case 1u: { out.uv = vec2(out.uv.y, 1.0 - out.uv.x); }
            case 2u: { out.uv = vec2(1.0) - out.uv; }
            case 3u: { out.uv = vec2(1.0 - out.uv.y, out.uv.x); }
            default: {}
        }
    }
    if (is_bamboo && ((BAMBOO_STEM_SIDE_QUAD_MASK >> quad_index) & 1u) != 0u) {
        out.uv.x += f32((packed_transform >> 20u) & 3u) * BAMBOO_STEM_UV_STRIDE;
    }
    out.current_texture = frame.current;
    let normals = array(vec3(0.0), vec3(0.0,-1.0,0.0), vec3(0.0,1.0,0.0), vec3(-1.0,0.0,0.0), vec3(1.0,0.0,0.0), vec3(0.0,0.0,-1.0), vec3(0.0,0.0,1.0));
    out.normal = rotate_cross(normals[quad_flags & 7u] + vec3(0.5,0.0,0.5), rotation) - vec3(0.5,0.0,0.5);
    out.material_flags = material.flags;
    out.local_position = block_position;
    out.biome_record = u32(origin.value.w);
    out.next_texture = frame.next;
    @if(ENHANCED || ENHANCED_SHADOW)
    out.authored_refs = vec2(authored_material_ref(selected_material_id, out.current_texture), authored_material_ref(selected_material_id, out.next_texture));
    out.frame_blend = frame.blend;
    out.visibility.x = is_visible | select(0u, MODEL_BOUNDED_TILE, is_bamboo && is_visible != 0u);
    // Vanilla uses white top vertices and RGB 0x0f on the reverse
    // plane. Apply it after sampling, without another 8-bit atlas quantization.
    let pad_shade = select(1.0, 15.0 / 255.0, out.normal.y < 0.0);
    let ao = material_ambient_occlusion(light_ao_factor((light_sample >> 8u) & 7u), material.flags);
    let dimming = material_face_shade(out.normal, (light_sample & 2048u) != 0u, material.flags);
    let terrain_shade = select(ao * dimming, pad_shade, is_lily_pad);
    out.lighting = light_colour(light_sample) * terrain_shade;
    @if(ENHANCED)
    out.lighting = block_illumination(light_sample);
    @if(ENHANCED)
    out.sky_light = sky_illumination(light_sample);
    @if(ENHANCED)
    out.ambient_occlusion = ao;
    @if(!ENHANCED)
    out.native_light_levels = terrain_light_levels(light_sample);
    @if(!ENHANCED)
    out.native_ao_face = terrain_shade;
    out.visibility.y = select(0u, 1u, (quad_flags & 8u) != 0u);
    out.world_position = world;
    out.world_origin = vec3<f32>(origin.value.xyz);
    @if(!ENHANCED_MOTION)
    var tint_gamma = stem_colour(material.flags);
    // Every corner belongs to the same block; keep its exact tint flat across the model.
    @if(!ENHANCED_MOTION && !ENHANCED_SHADOW && !OPAQUE_OVERDRAW)
    let tint_kind = material.flags & 0x30u;
    @if(!ENHANCED_MOTION && !ENHANCED_SHADOW && !OPAQUE_OVERDRAW)
    if (tint_kind != 0u) {
        tint_gamma = blended_biome_tint_gamma(tint_kind, material.flags, out.biome_record, block_position, out.world_origin).rgb;
    }
    @if(ENHANCED && !ENHANCED_MOTION)
    out.tint_surface = vec4(tint_gamma, f32(material_class(material_id)));
    @if(!ENHANCED && !ENHANCED_MOTION)
    out.tint_gamma = tint_gamma;
    @if(ENHANCED && !ENHANCED_MOTION)
    let displacement = foliage_displacement(world, u32(out.tint_surface.w), clamp(template_position.y, 0.0, 1.0));
    @if(ENHANCED && !ENHANCED_MOTION)
    out.world_position = world + displacement;
    @if(ENHANCED && !ENHANCED_MOTION)
    out.clip_position = camera_offset_clip(
        view.clip_from_world,
        view.world_position,
        camera_offset + displacement,
    );
    @if(ENHANCED || ENHANCED_MOTION)
    out.normal = template_quad_normal(template_quad_base, rotation);
    return out;
}

// Decode a rotated model corner for its geometric normal.
@if(ENHANCED || ENHANCED_MOTION)
fn template_corner(template_quad_base: u32, corner: u32, transform: u32) -> vec3<f32> {
    let component = corner * 3u;
    return rotate_cross(vec3<f32>(
        f32(packed_i16(template_quad_base, component)),
        f32(packed_i16(template_quad_base, component + 1u)),
        f32(packed_i16(template_quad_base, component + 2u)),
    ) / 256.0, transform);
}

// Counter-clockwise front faces make this cross product point outward.
@if(ENHANCED || ENHANCED_MOTION)
fn template_quad_normal(template_quad_base: u32, transform: u32) -> vec3<f32> {
    let origin = template_corner(template_quad_base, 0u, transform);
    let face = cross(
        template_corner(template_quad_base, 1u, transform) - origin,
        template_corner(template_quad_base, 3u, transform) - origin,
    );
    if (dot(face, face) < 1.0e-10) {
        return vec3(0.0, 1.0, 0.0);
    }
    return normalize(face);
}

// STEM_COLOURS

fn tinted(sampled: vec4<f32>, tint_gamma: vec3<f32>) -> vec4<f32> {
    return vec4(sampled.rgb * tint_to_linear(vec4(tint_gamma, 1.0)).rgb, sampled.a);
}

// Samples alpha before colour conversion or lighting work.
fn sample_raw_ref(in: VertexOutput, texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    @if(ENHANCED || ENHANCED_SHADOW)
    return sample_enhanced_colour(in, texture_ref, uv, dx, dy);
    @if(!ENHANCED && !ENHANCED_SHADOW)
    return sample_ref(texture_ref, uv, dx, dy);
}

@if(ENHANCED || ENHANCED_SHADOW)
fn sample_enhanced_colour(in: VertexOutput, texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let authored_ref = select(in.authored_refs.x, in.authored_refs.y, texture_ref == in.next_texture);
    if (authored_ref != 0xffffffffu) {
        let authored_page = authored_ref >> 31u;
        let authored_layer = authored_layer(authored_ref);
        if (authored_page == 0u) {
            return textureSampleGrad(enhanced_color_page_0, enhanced_sampler, uv, authored_layer, dx, dy);
        }
        return textureSampleGrad(enhanced_color_page_1, enhanced_sampler, uv, authored_layer, dx, dy);
    }
    return sample_ref(texture_ref, uv, dx, dy);
}

fn sample_ref(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let layer = texture_layer(texture_ref);
    let page = texture_page(texture_ref);
    @if(ENHANCED)
    if (page == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_0, 0));
        return textureSampleGrad(block_textures_page_0, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    if (page == 1u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_1, 0));
        return textureSampleGrad(block_textures_page_1, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    if (page == 2u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_2, 0));
        return textureSampleGrad(block_textures_page_2, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    if (page == 3u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_3, 0));
        return textureSampleGrad(block_textures_page_3, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_4, 0));
    @if(ENHANCED)
    return textureSampleGrad(block_textures_page_4, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    @if(!ENHANCED)
    if (page == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_0, 0));
        return textureSampleGrad(terrain_gamma_page_0, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    if (page == 1u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_1, 0));
        return textureSampleGrad(terrain_gamma_page_1, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    if (page == 2u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_2, 0));
        return textureSampleGrad(terrain_gamma_page_2, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    if (page == 3u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_3, 0));
        return textureSampleGrad(terrain_gamma_page_3, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_4, 0));
    @if(!ENHANCED)
    return textureSampleGrad(terrain_gamma_page_4, model_sampler, texture_model_uv(texture_ref, uv), layer, dx * scale, dy * scale);
}

// Bamboo's atlas rectangle includes replicated border texels. Other model UVs retain their addressing.
fn sample_model_ref(in: VertexOutput, texture_ref: u32, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if ((in.visibility.x & MODEL_BOUNDED_TILE) != 0u) {
        return sample_raw_ref(in, texture_ref, in.uv, dx, dy);
    }
    let layer = texture_layer(texture_ref);
    let page = texture_page(texture_ref);
    @if(ENHANCED)
    return sample_enhanced_colour(in, texture_ref, in.uv, dx, dy);
    @if(!ENHANCED)
    if (page == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_0, 0));
        return textureSampleGrad(terrain_gamma_page_0, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    if (page == 1u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_1, 0));
        return textureSampleGrad(terrain_gamma_page_1, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    if (page == 2u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_2, 0));
        return textureSampleGrad(terrain_gamma_page_2, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    if (page == 3u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_3, 0));
        return textureSampleGrad(terrain_gamma_page_3, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(!ENHANCED)
    let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_4, 0));
    @if(!ENHANCED)
    return textureSampleGrad(terrain_gamma_page_4, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
}

// Coverage shares the authored UV rectangle and gradients for the source image with color.
@if(ALPHA_TO_COVERAGE)
fn model_alpha_footprint(@if(ALPHA_TO_COVERAGE)
texture_ref: u32, @if(ALPHA_TO_COVERAGE)
uv: vec2<f32>, @if(ALPHA_TO_COVERAGE)
dx: vec2<f32>, @if(ALPHA_TO_COVERAGE)
dy: vec2<f32>, @if(ALPHA_TO_COVERAGE)
sampled: vec4<f32>) -> CutoutSample {
    let layer = texture_layer(texture_ref);
    let page = texture_page(texture_ref);
    let coordinate = texture_model_uv(texture_ref, uv);
    @if(ENHANCED)
    if (page == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_0, 0));
        return cutout_alpha_array(block_textures_page_0, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(ENHANCED)
    if (page == 1u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_1, 0));
        return cutout_alpha_array(block_textures_page_1, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(ENHANCED)
    if (page == 2u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_2, 0));
        return cutout_alpha_array(block_textures_page_2, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(ENHANCED)
    if (page == 3u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_3, 0));
        return cutout_alpha_array(block_textures_page_3, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(ENHANCED)
    let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_4, 0));
    @if(ENHANCED)
    return cutout_alpha_array(block_textures_page_4, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    @if(!ENHANCED)
    if (page == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_0, 0));
        return cutout_alpha_array(terrain_gamma_page_0, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(!ENHANCED)
    if (page == 1u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_1, 0));
        return cutout_alpha_array(terrain_gamma_page_1, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(!ENHANCED)
    if (page == 2u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_2, 0));
        return cutout_alpha_array(terrain_gamma_page_2, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(!ENHANCED)
    if (page == 3u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_3, 0));
        return cutout_alpha_array(terrain_gamma_page_3, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
    }
    @if(!ENHANCED)
    let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_4, 0));
    @if(!ENHANCED)
    return cutout_alpha_array(terrain_gamma_page_4, coordinate, layer, dx * scale, dy * scale, sampled, true, MODEL_ALPHA_THRESHOLD);
}

// ENHANCED_PBR_SAMPLING

// Reads the shared configured fade for native gamma-space shading.
fn distance_fog_amount(world_position: vec3<f32>) -> f32 {
    let distance_to_camera = distance(world_position, view.world_position);
    return configured_fog_fade(distance_to_camera, atmosphere.fog_color_start.w, atmosphere.fog_end_time.x);
}

// Applies fog before blending so depthless surfaces retain their own distance.
fn apply_distance_fog(colour: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    @if(ENHANCED)
    return enhanced_distance_fog(enhanced_frame, colour, world_position);
    @if(!ENHANCED)
    return mix(colour, atmosphere.fog_color_start.rgb, distance_fog_amount(world_position));
}

// Current TopSnow tessellator -> ordinary terrain ->
// AO/flat lighting. A bounded world model is not an entity material:
// RenderChunk multiplies atlas/palette, vertex AO and lightmap in gamma RGB.
// Encode only at our existing Bevy sRGB framebuffer boundary.
@if(!ENHANCED && !ENHANCED_MOTION)
fn ordinary_world_model_gamma_colour(in: VertexOutput, sampled_gamma: vec4<f32>) -> vec4<f32> {
    let lit_gamma = ((sampled_gamma.rgb * in.tint_gamma) * in.native_ao_face) * terrain_light_colour(in.native_light_levels);
    let fog_gamma = tint_to_gamma(vec4(atmosphere.fog_color_start.rgb, 1.0)).rgb;
    return vec4(mix(lit_gamma, fog_gamma, distance_fog_amount(in.world_position)), sampled_gamma.a);
}

@if(!ENHANCED && !ENHANCED_MOTION)
fn ordinary_world_model_colour(in: VertexOutput, sampled_gamma: vec4<f32>) -> vec4<f32> {
    return tint_to_linear(ordinary_world_model_gamma_colour(in, sampled_gamma));
}

@if(!ENHANCED_MOTION)
@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) front_facing: bool,
) -> @location(0) vec4<f32> {
    if (in.visibility.x == 0u) { discard; }
    if (!front_facing && in.visibility.y == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    @if(ENHANCED)
    let basis = material_basis(in.normal, dpdx(in.world_position), dpdy(in.world_position), dx, dy);
    @if(ENHANCED)
    let material_uv = parallax_material_uv(in.authored_refs.x, in.uv, dx, dy, normalize(view.world_position - in.world_position), basis,
        distance(view.world_position, in.world_position), enhanced_materials_enabled() && (in.material_flags & (1u << 8u)) == 0u && in.visibility.y == 0u && (u32(in.tint_surface.w) & 48u) == 0u && in.frame_blend == 0.0);
    @if(ENHANCED)
    let current = sample_raw_ref(in, in.current_texture, material_uv, dx, dy);
    @if(ENHANCED)
    var next = current;
    @if(ENHANCED)
    if (in.frame_blend > 0.0) {
        next = sample_raw_ref(in, in.next_texture, material_uv, dx, dy);
    }
    @if(ENHANCED)
    var sampled = mix(current, next, in.frame_blend);
    // Both views already hold the working colour space, so frames blend before the alpha test.
    @if(!ENHANCED)
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
    @if(!ENHANCED)
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_model_ref(in, in.next_texture, dx, dy), in.frame_blend);
    }
    let alpha_cutout = (in.material_flags & MATERIAL_ALPHA_CUTOUT_FLAG) != 0u;
    @if(!ALPHA_TO_COVERAGE)
    if (alpha_cutout && sampled.a < MODEL_ALPHA_THRESHOLD) { discard; }
    @if(ALPHA_TO_COVERAGE)
    if (alpha_cutout) {
        var cutout = model_alpha_footprint(in.current_texture, in.uv, dx, dy, sampled);
        if (in.frame_blend > 0.0) {
            cutout = cutout_mix(cutout, model_alpha_footprint(in.next_texture, in.uv, dx, dy, sampled), in.frame_blend);
        }
        if (sampled.a < MODEL_ALPHA_THRESHOLD && cutout.coverage > 0.0) { sampled = cutout.colour; }
        sampled.a = cutout.coverage;
    }
    let output_alpha = select(1.0, sampled.a, alpha_cutout);
    @if(OPAQUE_OVERDRAW)
    return vec4(1.0);
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    let colour = tinted(sampled, in.tint_surface.xyz);
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    var material_normal_sample = sample_pbr_texture(true, in.authored_refs.x, material_uv, dx, dy);
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    var material_mer_sample = sample_pbr_texture(false, in.authored_refs.x, material_uv, dx, dy);
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    if (in.frame_blend > 0.0) {
        material_normal_sample = mix(material_normal_sample, sample_pbr_texture(true, in.authored_refs.y, material_uv, dx, dy), in.frame_blend);
        material_mer_sample = blend_material_sample(material_mer_sample, sample_pbr_texture(false, in.authored_refs.y, material_uv, dx, dy), in.frame_blend,
            select(0u, in.authored_refs.x, in.authored_refs.x != 0xffffffffu));
    }
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    let shading_normal = material_normal(material_normal_sample, basis);
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    material_mer_sample = material_specular_aa(material_mer_sample, shading_normal);
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    let shaded = shade_material(
        colour.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion * material_normal_sample.b,
        u32(in.tint_surface.w),
        shading_normal,
        material_mer_sample,
        select(0u, in.authored_refs.x, in.authored_refs.x != 0xffffffffu),
        parallax_direct_visibility(in.authored_refs.x, material_uv, dx, dy, enhanced_light_direction(), basis, distance(view.world_position, in.world_position),
            enhanced_materials_enabled() && (in.material_flags & (1u << 8u)) == 0u && (u32(in.tint_surface.w) & 48u) == 0u && in.frame_blend == 0.0),
    );
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    return vec4(apply_distance_fog(shaded, in.world_position), output_alpha);
    @if(!OPAQUE_OVERDRAW && !ENHANCED)
    let colour = ordinary_world_model_colour(in, sampled);
    @if(!OPAQUE_OVERDRAW && !ENHANCED)
    return vec4(colour.rgb, output_alpha);
}

@if(!ENHANCED_MOTION)
@fragment
fn fragment_blend(
    in: VertexOutput,
    @builtin(front_facing) front_facing: bool,
) -> @location(0) vec4<f32> {
    return shade_blend(in, front_facing, dpdx(in.uv), dpdy(in.uv));
}

// Blended transparent model colour; `dx`/`dy` are the UV derivatives, taken by the caller in
// uniform control flow.
@if(!ENHANCED_MOTION)
fn shade_blend(in: VertexOutput, front_facing: bool, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if (in.visibility.x == 0u) { discard; }
    if (!front_facing && in.visibility.y == 0u) { discard; }
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_model_ref(in, in.next_texture, dx, dy), in.frame_blend);
    }
    if ((in.material_flags & MATERIAL_CAULDRON_WATER_FLAG) != 0u) {
        @if(ENHANCED)
        sampled *= tint_to_linear(CAULDRON_WATER_COLOUR);
        @if(!ENHANCED)
        sampled *= CAULDRON_WATER_COLOUR;
    }
    // The background is fogged by the same transfer, so preserving source
    // alpha composes to one fog application instead of double-counting it.
    @if(ENHANCED)
    let colour = tinted(sampled, in.tint_surface.xyz);
    @if(ENHANCED)
    let basis = material_basis(in.normal, dpdx(in.world_position), dpdy(in.world_position), dx, dy);
    @if(ENHANCED)
    var material_normal_sample = sample_pbr_texture(true, in.authored_refs.x, in.uv, dx, dy);
    @if(ENHANCED)
    var material_mer_sample = sample_pbr_texture(false, in.authored_refs.x, in.uv, dx, dy);
    @if(ENHANCED)
    if (in.frame_blend > 0.0) {
        material_normal_sample = mix(material_normal_sample, sample_pbr_texture(true, in.authored_refs.y, in.uv, dx, dy), in.frame_blend);
        material_mer_sample = blend_material_sample(material_mer_sample, sample_pbr_texture(false, in.authored_refs.y, in.uv, dx, dy), in.frame_blend,
            select(0u, in.authored_refs.x, in.authored_refs.x != 0xffffffffu));
    }
    @if(ENHANCED)
    let shading_normal = material_normal(material_normal_sample, basis);
    @if(ENHANCED)
    material_mer_sample = material_specular_aa(material_mer_sample, shading_normal);
    @if(ENHANCED)
    let shaded = shade_material(
        colour.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion * material_normal_sample.b,
        u32(in.tint_surface.w),
        shading_normal,
        material_mer_sample,
        select(0u, in.authored_refs.x, in.authored_refs.x != 0xffffffffu),
        1.0,
    );
    @if(ENHANCED)
    return vec4(apply_distance_fog(shaded, in.world_position), colour.a);
    @if(!ENHANCED && NATIVE_GAMMA_BLEND)
    return ordinary_world_model_gamma_colour(in, sampled);
    @if(!ENHANCED && !NATIVE_GAMMA_BLEND)
    return ordinary_world_model_colour(in, sampled);
}

// Alpha-tested terrain depth; opaque texels cast independently of baked light.
@if(ENHANCED_SHADOW)
fn shadow_coverage(in: VertexOutput, front: bool) {
    if (!front && in.visibility.y == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    var sampled = sample_raw_ref(in, in.current_texture, in.uv, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_raw_ref(in, in.next_texture, in.uv, dx, dy), in.frame_blend);
    }
    if (in.visibility.x == 0u || ((in.material_flags & MATERIAL_ALPHA_CUTOUT_FLAG) != 0u && sampled.a < MODEL_ALPHA_THRESHOLD)) { discard; }
}

@if(ENHANCED_SHADOW)
@fragment
fn fragment_shadow(in: VertexOutput, @builtin(front_facing) front: bool) {
    shadow_coverage(in, front);
    if (caster_excludes_emitter(in.world_position)) { discard; }
}

@if(ENHANCED_SHADOW && ENHANCED_MOTION)
struct CameraSurface {
    @location(0) motion: vec4<f32>,
    @location(1) normal: vec2<f32>,
}

@if(ENHANCED_SHADOW && ENHANCED_MOTION)
@fragment
fn fragment_motion(in: VertexOutput, @builtin(front_facing) front: bool) -> CameraSurface {
    shadow_coverage(in, front);
    let current_uv = (in.clip_position.xy - view.viewport.xy) / view.viewport.zw;
    let clip = vec4(current_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    return CameraSurface(submitted_surface_motion(clip, in.previous_clip, caster_history_valid()), encode_geometric_normal(in.normal));
}
