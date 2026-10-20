import render::chunk_bindings::{
    view, chunk_origins, block_textures_page_0, block_textures_page_1, block_textures_page_2,
    block_textures_page_3, block_textures_page_4, block_sampler, terrain_gamma_page_0,
    terrain_gamma_page_1, terrain_gamma_page_2, terrain_gamma_page_3, terrain_gamma_page_4,
    model_sampler, animations, animation_frames,
    clock, model_templates, geometry_streams, atmosphere,
};
import render::material::{MaterialGpu, materials, positional_material_id, texture_layer, texture_page, material_uv_flags, texture_model_uv, texture_gradient_scale};
@if(ENHANCED_SHADOW)
import render::enhanced::caster::caster_clip;
import render::world_projection::{section_camera_offset, camera_offset_clip};
import render::biome_tint::{blended_biome_tint, blended_biome_tint_gamma};
import render::lighting::{light_ao_factor, light_colour, material_ambient_occlusion, material_face_shade, tint_to_gamma, tint_to_linear, terrain_light_levels, terrain_light_colour};
@if(ALPHA_TO_COVERAGE)
import render::lighting::{CutoutSample, cutout_alpha_array, cutout_mix};
@if(ENHANCED)
import render::enhanced::view::{sky_illumination, material_class, shade_surface, waved_position};

const MODEL_ALPHA_THRESHOLD: f32 = 0.5;

// BAMBOO_CONSTANTS

struct VertexOutput {
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
    @if(ENHANCED)
    @location(13) @interpolate(flat) surface_class: u32,
    @if(!ENHANCED)
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
    @if(ENHANCED)
    invisible.surface_class = 0u;
    @if(!ENHANCED)
    invisible.native_light_levels = vec2(0.0);
    @if(!ENHANCED)
    invisible.native_ao_face = 0.0;
    @if(!ENHANCED)
    invisible.tint_gamma = vec3(1.0);
    invisible.visibility.y = 0u;
    invisible.world_origin = vec3(0.0);
    invisible.world_position = vec3(0.0);
    return invisible;
}

fn animation_sample(material: MaterialGpu) -> FrameSample {
    if (material.animation == 0xffffffffu) {
        return FrameSample(material.texture, material.texture, 0.0);
    }
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
        var invisible: VertexOutput;
        invisible.clip_position = vec4(2.0, 2.0, 2.0, 1.0);
        return invisible;
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

/// One quad of a placed model, decoded as every model program reads it.
struct ModelQuad {
    /// False when the draw ref, model ref, template or lighting record is out of range, or
    /// the quad is hidden by its ref's visibility mask; no other field is meaningful then.
    valid: bool,
    template_quad_base: u32,
    quad_index: u32,
    packed_transform: u32,
    lighting_base_index: u32,
    /// Placed block corner, relative to the section origin.
    block_position: vec3<f32>,
    /// Quarter turns about the block's vertical centre line, after bamboo and lily-pad rules.
    rotation: u32,
    is_lily_pad: bool,
    is_bamboo: bool,
    /// Bamboo's packed placement offset, added to template corners before `rotation`.
    template_offset: vec3<f32>,
    /// Bamboo leaf plane inset, added after `template_offset` and before `rotation`.
    leaf_inset: vec3<f32>,
    /// Random placement offset, added after `rotation`.
    placement_offset: vec3<f32>,
    material_id: u32,
    quad_flags: u32,
    /// `material_id` after positional variation at the placed block.
    selected_material_id: u32,
    /// Section origin in blocks, and the section's biome record in `w`.
    chunk_origin: vec4<i32>,
}

fn invalid_model_quad() -> ModelQuad {
    var quad: ModelQuad;
    quad.valid = false;
    return quad;
}

/// Decodes the model quad that draw-ref record `instance_index` names, in origin slot
/// `metadata_index`, validating every address it reads.
fn model_quad(instance_index: u32, metadata_index: u32) -> ModelQuad {
    let geometry_word_count = arrayLength(&geometry_streams);
    if (instance_index > 0x7fffffffu) {
        return invalid_model_quad();
    }
    let draw_ref_word = instance_index * 2u;
    if (draw_ref_word + 1u >= geometry_word_count) {
        return invalid_model_quad();
    }
    let model_ref_index = geometry_streams[draw_ref_word];
    let quad_index = geometry_streams[draw_ref_word + 1u];
    if (quad_index >= 32u || model_ref_index > 0x3fffffffu) {
        return invalid_model_quad();
    }
    let ref_word = model_ref_index * 4u;
    if (ref_word + 3u >= geometry_word_count) {
        return invalid_model_quad();
    }
    let packed_transform = geometry_streams[ref_word];
    let template_id = geometry_streams[ref_word + 1u];
    let lighting_base_index = geometry_streams[ref_word + 2u];
    let visible_quad_mask = geometry_streams[ref_word + 3u];
    let template_word_count = arrayLength(&model_templates);
    if (template_word_count < 4u) {
        return invalid_model_quad();
    }
    let template_count = model_templates[0];
    if (template_count > (template_word_count - 1u) / 3u || template_id >= template_count) {
        return invalid_model_quad();
    }
    let descriptor = 1u + template_id * 3u;
    let quad_start = model_templates[descriptor];
    let quad_count = model_templates[descriptor + 1u];
    if (quad_count == 0u || quad_index >= quad_count) {
        return invalid_model_quad();
    }
    if (((visible_quad_mask >> quad_index) & 1u) == 0u) {
        return invalid_model_quad();
    }
    let template_quad_words = 1u + template_count * 3u;
    let stored_quad_count = (template_word_count - template_quad_words) / 12u;
    if (quad_start >= stored_quad_count || quad_index >= stored_quad_count - quad_start) {
        return invalid_model_quad();
    }
    var quad: ModelQuad;
    quad.valid = true;
    quad.template_quad_base = template_quad_words + (quad_start + quad_index) * 12u;
    quad.quad_index = quad_index;
    quad.packed_transform = packed_transform;
    quad.lighting_base_index = lighting_base_index;
    quad.block_position = vec3<f32>(
        f32(packed_transform & 15u),
        f32((packed_transform >> 4u) & 15u),
        f32((packed_transform >> 8u) & 15u),
    );
    quad.chunk_origin = chunk_origins[metadata_index].value;
    quad.is_lily_pad = (model_templates[descriptor + 2u] & MODEL_LILY_PAD_FLAG) != 0u;
    quad.is_bamboo = (model_templates[descriptor + 2u] & MODEL_BAMBOO_FLAG) != 0u;
    let has_offset = (packed_transform & MODEL_RANDOM_OFFSET_FLAG) != 0u;
    quad.rotation = packed_transform >> 12u;
    if (quad.is_bamboo) {
        if (!has_offset) {
            quad.template_offset.x = BAMBOO_OFFSET_MIN + f32(quad.rotation & 15u) * BAMBOO_OFFSET_STEP;
            quad.template_offset.z = BAMBOO_OFFSET_MIN + f32((quad.rotation >> 4u) & 15u) * BAMBOO_OFFSET_STEP;
        }
        if (quad_index == BAMBOO_POSITIVE_X_LEAF_QUAD) { quad.leaf_inset.z = BAMBOO_LEAF_PLANE_INSET; }
        if (quad_index == BAMBOO_POSITIVE_Z_LEAF_QUAD) { quad.leaf_inset.x = BAMBOO_LEAF_PLANE_INSET; }
        quad.rotation = 0u;
    }
    if (quad.is_lily_pad) {
        quad.rotation = lily_pad_rotation(quad.chunk_origin.xyz + vec3<i32>(quad.block_position));
    }
    if (has_offset) {
        if (lighting_base_index < 2u || lighting_base_index > geometry_word_count / 2u) { return invalid_model_quad(); }
        let offset_word = lighting_base_index * 2u - 4u;
        quad.placement_offset = vec3<f32>(bitcast<f32>(geometry_streams[offset_word]),
            bitcast<f32>(geometry_streams[offset_word + 1u]), bitcast<f32>(geometry_streams[offset_word + 2u]));
    }
    quad.material_id = model_templates[quad.template_quad_base + 10u];
    quad.quad_flags = model_templates[quad.template_quad_base + 11u];
    quad.selected_material_id = positional_material_id(quad.material_id, quad.chunk_origin.xyz + vec3<i32>(quad.block_position));
    if (lighting_base_index >= geometry_word_count / 2u || quad_index >= geometry_word_count / 2u - lighting_base_index) {
        return invalid_model_quad();
    }
    return quad;
}

/// Template-space position of `corner` (block units from the placed block corner), after the
/// quad's offsets and rotation.
fn model_corner_position(quad: ModelQuad, corner: u32) -> vec3<f32> {
    let component = corner * 3u;
    let template_position = vec3<f32>(
        f32(packed_i16(quad.template_quad_base, component)),
        f32(packed_i16(quad.template_quad_base, component + 1u)),
        f32(packed_i16(quad.template_quad_base, component + 2u)),
    ) / 256.0;
    return rotate_cross(template_position + quad.template_offset + quad.leaf_inset, quad.rotation) + quad.placement_offset;
}

/// Texture coordinates of `corner` in the material's texture, before its page scale.
fn model_corner_uv(quad: ModelQuad, material: MaterialGpu, corner: u32) -> vec2<f32> {
    let uv_component = corner * 2u;
    var uv = vec2<f32>(
        f32(packed_u16(quad.template_quad_base + 6u, uv_component)),
        f32(packed_u16(quad.template_quad_base + 6u, uv_component + 1u)),
    ) / 4096.0;
    // World rotation acts on the authored UV rectangle, independently of model geometry.
    if ((material.flags & MATERIAL_ISOTROPIC_FLAG) != 0u) {
        let uv_flags = material_uv_flags(material.flags, quad.chunk_origin.xyz + vec3<i32>(quad.block_position));
        switch uv_flags & 3u {
            case 1u: { uv = vec2(uv.y, 1.0 - uv.x); }
            case 2u: { uv = vec2(1.0) - uv; }
            case 3u: { uv = vec2(1.0 - uv.y, uv.x); }
            default: {}
        }
    }
    if (quad.is_bamboo && ((BAMBOO_STEM_SIDE_QUAD_MASK >> quad.quad_index) & 1u) != 0u) {
        uv.x += f32((quad.packed_transform >> 20u) & 3u) * BAMBOO_STEM_UV_STRIDE;
    }
    return uv;
}

/// Packed light sample of `corner`, laid out as `cube_light_sample` describes in `chunk.wesl`.
fn model_light_sample(quad: ModelQuad, corner: u32) -> u32 {
    let light_word = geometry_streams[(quad.lighting_base_index + quad.quad_index) * 2u + corner / 2u];
    return select(light_word & 0xffffu, light_word >> 16u, (corner & 1u) != 0u);
}

/// The quad's authored shading normal, turned with the model.
fn model_quad_normal(quad: ModelQuad) -> vec3<f32> {
    let normals = array(vec3(0.0), vec3(0.0,-1.0,0.0), vec3(0.0,1.0,0.0), vec3(-1.0,0.0,0.0), vec3(1.0,0.0,0.0), vec3(0.0,0.0,-1.0), vec3(0.0,0.0,1.0));
    return rotate_cross(normals[quad.quad_flags & 7u] + vec3(0.5,0.0,0.5), quad.rotation) - vec3(0.5,0.0,0.5);
}

fn model_vertex(vertex_index: u32, instance_index: u32) -> VertexOutput {
    let corner = vertex_index & 3u;
    let quad = model_quad(instance_index, vertex_index / 4u);
    if (!quad.valid) {
        return invisible_vertex();
    }
    let template_position = model_corner_position(quad, corner);
    let block_position = quad.block_position;
    let local_position = block_position + template_position;
    let material_id = quad.material_id;
    let quad_flags = quad.quad_flags;
    let rotation = quad.rotation;
    let is_lily_pad = quad.is_lily_pad;
    let is_bamboo = quad.is_bamboo;
    let material = materials[quad.selected_material_id];
    let frame = animation_sample(material);
    let light_sample = model_light_sample(quad, corner);
    var out: VertexOutput;
    let world = vec3<f32>(quad.chunk_origin.xyz) + local_position;
    let camera_offset = section_camera_offset(quad.chunk_origin.xyz, local_position, view.world_position);
    out.clip_position = camera_offset_clip(view.clip_from_world, view.world_position, camera_offset);
    @if(ENHANCED_SHADOW)
    out.clip_position = caster_clip(world, material_id, clamp(template_position.y, 0.0, 1.0));
    out.uv = model_corner_uv(quad, material, corner);
    out.current_texture = frame.current;
    out.normal = model_quad_normal(quad);
    out.material_flags = material.flags;
    out.local_position = block_position;
    out.biome_record = u32(quad.chunk_origin.w);
    out.next_texture = frame.next;
    out.frame_blend = frame.blend;
    out.visibility.x = 1u | select(0u, MODEL_BOUNDED_TILE, is_bamboo);
    // Vanilla uses white top vertices and RGB 0x0f on the reverse
    // plane. Apply it after sampling, without another 8-bit atlas quantization.
    let pad_shade = select(1.0, 15.0 / 255.0, out.normal.y < 0.0);
    let ao = material_ambient_occlusion(light_ao_factor((light_sample >> 8u) & 7u), material.flags);
    let dimming = material_face_shade(out.normal, (light_sample & 2048u) != 0u, material.flags);
    let terrain_shade = select(ao * dimming, pad_shade, is_lily_pad);
    out.lighting = light_colour(light_sample) * terrain_shade;
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
    out.world_origin = vec3<f32>(quad.chunk_origin.xyz);
    @if(!ENHANCED)
    out.tint_gamma = stem_colour(material.flags);
    // Every corner belongs to the same block; keep its exact tint flat across the model.
    @if(!ENHANCED && !ENHANCED_SHADOW && !OPAQUE_OVERDRAW)
    let tint_kind = material.flags & 0x30u;
    @if(!ENHANCED && !ENHANCED_SHADOW && !OPAQUE_OVERDRAW)
    if (tint_kind != 0u) {
        out.tint_gamma = blended_biome_tint_gamma(tint_kind, material.flags, out.biome_record, block_position, out.world_origin).rgb;
    }
    @if(ENHANCED)
    out.surface_class = material_class(material_id);
    @if(ENHANCED)
    out.world_position = waved_position(world, out.surface_class, clamp(template_position.y, 0.0, 1.0));
    @if(ENHANCED)
    out.clip_position = camera_offset_clip(
        view.clip_from_world,
        view.world_position,
        camera_offset + (out.world_position - world),
    );
    @if(ENHANCED)
    out.normal = template_quad_normal(quad.template_quad_base, rotation);
    return out;
}

// Decode a rotated model corner for its geometric normal.
@if(ENHANCED)
fn template_corner(@if(ENHANCED)
template_quad_base: u32, @if(ENHANCED)
corner: u32, @if(ENHANCED)
transform: u32) -> vec3<f32> {
    let component = corner * 3u;
    return rotate_cross(vec3<f32>(
        f32(packed_i16(template_quad_base, component)),
        f32(packed_i16(template_quad_base, component + 1u)),
        f32(packed_i16(template_quad_base, component + 2u)),
    ) / 256.0, transform);
}

// Counter-clockwise front faces make this cross product point outward.
@if(ENHANCED)
fn template_quad_normal(@if(ENHANCED)
template_quad_base: u32, @if(ENHANCED)
transform: u32) -> vec3<f32> {
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

fn tinted(sampled: vec4<f32>, flags: u32, record: u32, position: vec3<f32>, world_origin: vec3<f32>) -> vec4<f32> {
    if ((flags & MATERIAL_STEM_FLAG) != 0u) {
        return vec4(sampled.rgb * tint_to_linear(vec4(stem_colour(flags), 1.0)).rgb, sampled.a);
    }
    let tint_kind = flags & 0x30u;
    if (tint_kind == 0u) { return vec4(sampled.rgb, sampled.a); }
    return vec4(sampled.rgb * blended_biome_tint(tint_kind, flags, record, position, world_origin).rgb, sampled.a);
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
        return sample_ref(texture_ref, in.uv, dx, dy);
    }
    let layer = texture_layer(texture_ref);
    let page = texture_page(texture_ref);
    @if(ENHANCED)
    if (page == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_0, 0));
        return textureSampleGrad(block_textures_page_0, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    if (page == 1u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_1, 0));
        return textureSampleGrad(block_textures_page_1, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    if (page == 2u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_2, 0));
        return textureSampleGrad(block_textures_page_2, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    if (page == 3u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_3, 0));
        return textureSampleGrad(block_textures_page_3, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
    }
    @if(ENHANCED)
    let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_4, 0));
    @if(ENHANCED)
    return textureSampleGrad(block_textures_page_4, block_sampler, texture_model_uv(texture_ref, in.uv), layer, dx * scale, dy * scale);
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

fn distance_fog_amount(world_position: vec3<f32>) -> f32 {
    let distance_to_camera = distance(world_position, view.world_position);
    return clamp(
        (distance_to_camera - atmosphere.fog_color_start.w)
            / max(atmosphere.fog_end_time.x - atmosphere.fog_color_start.w, 0.0001),
        0.0,
        1.0,
    );
}

fn apply_distance_fog(colour: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    return mix(colour, atmosphere.fog_color_start.rgb, distance_fog_amount(world_position));
}

// Current TopSnow tessellator -> ordinary terrain ->
// AO/flat lighting. A bounded world model is not an entity material:
// RenderChunk multiplies atlas/palette, vertex AO and lightmap in gamma RGB.
// Encode only at our existing Bevy sRGB framebuffer boundary.
@if(!ENHANCED)
fn ordinary_world_model_gamma_colour(@if(!ENHANCED)
in: VertexOutput, @if(!ENHANCED)
sampled_gamma: vec4<f32>) -> vec4<f32> {
    let lit_gamma = ((sampled_gamma.rgb * in.tint_gamma) * in.native_ao_face) * terrain_light_colour(in.native_light_levels);
    let fog_gamma = tint_to_gamma(vec4(atmosphere.fog_color_start.rgb, 1.0)).rgb;
    return vec4(mix(lit_gamma, fog_gamma, distance_fog_amount(in.world_position)), sampled_gamma.a);
}

@if(!ENHANCED)
fn ordinary_world_model_colour(@if(!ENHANCED)
in: VertexOutput, @if(!ENHANCED)
sampled_gamma: vec4<f32>) -> vec4<f32> {
    return tint_to_linear(ordinary_world_model_gamma_colour(in, sampled_gamma));
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) front_facing: bool,
) -> @location(0) vec4<f32> {
    if (in.visibility.x == 0u) { discard; }
    if (!front_facing && in.visibility.y == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    // Both views already hold the working colour space, so frames blend before the alpha test.
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
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
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position, in.world_origin);
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    let shaded = shade_surface(
        colour.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion,
        in.surface_class,
    );
    @if(!OPAQUE_OVERDRAW && ENHANCED)
    return vec4(apply_distance_fog(shaded, in.world_position), output_alpha);
    @if(!OPAQUE_OVERDRAW && !ENHANCED)
    let colour = ordinary_world_model_colour(in, sampled);
    @if(!OPAQUE_OVERDRAW && !ENHANCED)
    return vec4(colour.rgb, output_alpha);
}

@fragment
fn fragment_blend(
    in: VertexOutput,
    @builtin(front_facing) front_facing: bool,
) -> @location(0) vec4<f32> {
    return shade_blend(in, front_facing, dpdx(in.uv), dpdy(in.uv));
}

// Blended transparent model colour; `dx`/`dy` are the UV derivatives, taken by the caller in
// uniform control flow.
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
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position, in.world_origin);
    @if(ENHANCED)
    let shaded = shade_surface(
        colour.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion,
        in.surface_class,
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
@fragment
fn fragment_shadow(@if(ENHANCED_SHADOW)
in: VertexOutput) {
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_model_ref(in, in.next_texture, dx, dy), in.frame_blend);
    }
    if (in.visibility.x == 0u || ((in.material_flags & MATERIAL_ALPHA_CUTOUT_FLAG) != 0u && sampled.a < MODEL_ALPHA_THRESHOLD)) { discard; }
}
