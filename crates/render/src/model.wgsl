#import cinnabar::material::{MaterialGpu, materials, positional_material, material_uv_flags, texture_uv_scale, texture_gradient_scale}
#ifdef ENHANCED_SHADOW
#import cinnabar::enhanced_caster::caster_clip
#endif
#import bevy_render::view::View
#import cinnabar::biome_tint::{blended_biome_tint, blended_biome_tint_gamma}
#import cinnabar::lighting::{light_ao_factor, light_colour, material_ambient_occlusion, material_face_shade, tint_to_gamma, tint_to_linear, terrain_light_levels, terrain_light_colour}
#ifdef ENHANCED
#import cinnabar::enhanced_view::{sky_illumination, material_class, shade_surface, waved_position}
#endif

struct ChunkOrigin { value: vec4<i32>, cube_bases: vec4<u32> }
// BAMBOO_CONSTANTS
// ANIMATION_GPU_LAYOUT
struct AnimationClockGpu { tick: u32, partial_tick: f32, padding_0: u32, padding_1: u32 }
struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>, moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>, sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>, fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>, sky_extra: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> cube_quads: array<u32>;
@group(0) @binding(2) var<storage, read> chunk_origins: array<ChunkOrigin>;
@group(0) @binding(4) var block_textures_page_0: texture_2d_array<f32>;
@group(0) @binding(5) var block_textures_page_1: texture_2d_array<f32>;
@group(0) @binding(6) var block_sampler: sampler;
@group(0) @binding(NATIVE_LEAF_TEXTURE_BINDING_0) var terrain_gamma_page_0: texture_2d_array<f32>;
@group(0) @binding(NATIVE_LEAF_TEXTURE_BINDING_1) var terrain_gamma_page_1: texture_2d_array<f32>;
@group(0) @binding(NATIVE_LEAF_SAMPLER_BINDING) var model_sampler: sampler;
@group(0) @binding(9) var<storage, read> animations: array<AnimationGpu>;
@group(0) @binding(10) var<storage, read> animation_frames: array<u32>;
@group(0) @binding(11) var<uniform> clock: AnimationClockGpu;
@group(0) @binding(12) var<storage, read> model_templates: array<u32>;
@group(0) @binding(13) var<storage, read> geometry_streams: array<u32>;
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) current_texture: u32,
    @location(2) normal: vec3<f32>,
    @location(3) @interpolate(flat) material_flags: u32,
    @location(4) @interpolate(flat) local_position: vec3<f32>,
    @location(5) @interpolate(flat) biome_record: u32,
    @location(6) @interpolate(flat) next_texture: u32,
    @location(7) @interpolate(flat) frame_blend: f32,
    @location(8) @interpolate(flat) visible: u32,
    @location(9) lighting: vec3<f32>,
#ifdef ENHANCED
    @location(11) sky_light: f32,
    @location(15) ambient_occlusion: f32,
#else
    @location(11) native_light_levels: vec2<f32>,
    @location(15) native_ao_face: f32,
#endif
    @location(10) @interpolate(flat) world_origin: vec3<f32>,
    @location(12) @interpolate(flat) two_sided: u32,
    @location(13) world_position: vec3<f32>,
#ifdef ENHANCED
    @location(14) @interpolate(flat) surface_class: u32,
#else
    @location(14) @interpolate(flat) tint_gamma: vec3<f32>,
#endif
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
    invisible.visible = 0u;
    invisible.lighting = vec3(0.0);
#ifdef ENHANCED
    invisible.sky_light = 0.0;
    invisible.ambient_occlusion = 0.0;
    invisible.surface_class = 0u;
#else
    invisible.native_light_levels = vec2(0.0);
    invisible.native_ao_face = 0.0;
    invisible.tint_gamma = vec3(1.0);
#endif
    invisible.two_sided = 0u;
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

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
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
    let material = positional_material(material_id, origin.value.xyz + vec3<i32>(block_position));
    let frame = animation_sample(material);
    let uv_component = corner * 2u;

    if (lighting_base_index >= geometry_word_count / 2u || quad_index >= geometry_word_count / 2u - lighting_base_index) {
        return invisible_vertex();
    }
    let light_word = geometry_streams[(lighting_base_index + quad_index) * 2u + corner / 2u];
    let light_sample = select(light_word & 0xffffu, light_word >> 16u, (corner & 1u) != 0u);
    var out: VertexOutput;
    let world = vec3<f32>(origin.value.xyz) + local_position;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
#ifdef ENHANCED_SHADOW
    out.clip_position = caster_clip(world, material_id, clamp(template_position.y, 0.0, 1.0));
#endif
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
    out.frame_blend = frame.blend;
    out.visible = is_visible | select(0u, MODEL_BOUNDED_TILE, is_bamboo && is_visible != 0u);
    // Vanilla uses white top vertices and RGB 0x0f on the reverse
    // plane. Apply it after sampling, without another 8-bit atlas quantization.
    let pad_shade = select(1.0, 15.0 / 255.0, out.normal.y < 0.0);
    let ao = material_ambient_occlusion(light_ao_factor((light_sample >> 8u) & 7u), material.flags);
    let dimming = material_face_shade(out.normal, (light_sample & 2048u) != 0u, material.flags);
    let terrain_shade = select(ao * dimming, pad_shade, is_lily_pad);
    out.lighting = light_colour(light_sample) * terrain_shade;
#ifdef ENHANCED
    out.sky_light = sky_illumination(light_sample);
    out.ambient_occlusion = ao;
#else
    out.native_light_levels = terrain_light_levels(light_sample);
    out.native_ao_face = terrain_shade;
#endif
    out.two_sided = select(0u, 1u, (quad_flags & 8u) != 0u);
    out.world_position = world;
    out.world_origin = vec3<f32>(origin.value.xyz);
#ifndef ENHANCED
    out.tint_gamma = vec3(1.0);
#ifndef ENHANCED_SHADOW
#ifndef OPAQUE_OVERDRAW
    // Every corner belongs to the same block; keep its exact tint flat across the model.
    let tint_kind = material.flags & 0x30u;
    if (tint_kind != 0u) {
        out.tint_gamma = blended_biome_tint_gamma(tint_kind, material.flags, out.biome_record, block_position, out.world_origin).rgb;
    }
#endif
#endif
#endif
#ifdef ENHANCED
    out.surface_class = material_class(material_id);
    out.world_position = waved_position(world, out.surface_class, clamp(template_position.y, 0.0, 1.0));
    out.clip_position = view.clip_from_world * vec4(out.world_position, 1.0);
    out.normal = template_quad_normal(template_quad_base, rotation);
#endif
    return out;
}

#ifdef ENHANCED
// Decode a rotated model corner for its geometric normal.
fn template_corner(template_quad_base: u32, corner: u32, transform: u32) -> vec3<f32> {
    let component = corner * 3u;
    return rotate_cross(vec3<f32>(
        f32(packed_i16(template_quad_base, component)),
        f32(packed_i16(template_quad_base, component + 1u)),
        f32(packed_i16(template_quad_base, component + 2u)),
    ) / 256.0, transform);
}

// Counter-clockwise front faces make this cross product point outward.
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

#endif
fn tinted(sampled: vec4<f32>, flags: u32, record: u32, position: vec3<f32>, world_origin: vec3<f32>) -> vec4<f32> {
    let tint_kind = flags & 0x30u;
    if (tint_kind == 0u) { return vec4(sampled.rgb, sampled.a); }
    return vec4(sampled.rgb * blended_biome_tint(tint_kind, flags, record, position, world_origin).rgb, sampled.a);
}

fn sample_ref(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let layer = i32(texture_ref & 0x7ffu);
#ifdef ENHANCED
    if ((texture_ref >> 31u) == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_0, 0));
        return textureSampleGrad(block_textures_page_0, model_sampler, uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
    }
    let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_1, 0));
    return textureSampleGrad(block_textures_page_1, model_sampler, uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
#else
    if ((texture_ref >> 31u) == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_0, 0));
        return textureSampleGrad(terrain_gamma_page_0, model_sampler, uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
    }
    let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_1, 0));
    return textureSampleGrad(terrain_gamma_page_1, model_sampler, uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
#endif
}

// Bamboo's atlas rectangle includes replicated border texels. Other model UVs retain their addressing.
fn sample_model_ref(in: VertexOutput, texture_ref: u32, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if ((in.visible & MODEL_BOUNDED_TILE) != 0u) {
        return sample_ref(texture_ref, in.uv, dx, dy);
    }
    let layer = i32(texture_ref & 0x7ffu);
#ifdef ENHANCED
    if ((texture_ref >> 31u) == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_0, 0));
        return textureSampleGrad(block_textures_page_0, block_sampler, in.uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
    }
    let scale = texture_gradient_scale(texture_ref, textureDimensions(block_textures_page_1, 0));
    return textureSampleGrad(block_textures_page_1, block_sampler, in.uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
#else
    if ((texture_ref >> 31u) == 0u) {
        let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_0, 0));
        return textureSampleGrad(terrain_gamma_page_0, block_sampler, in.uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
    }
    let scale = texture_gradient_scale(texture_ref, textureDimensions(terrain_gamma_page_1, 0));
    return textureSampleGrad(terrain_gamma_page_1, block_sampler, in.uv * texture_uv_scale(texture_ref), layer, dx * scale, dy * scale);
#endif
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

#ifndef ENHANCED
// Current TopSnow tessellator -> ordinary terrain ->
// AO/flat lighting. A bounded world model is not an entity material:
// RenderChunk multiplies atlas/palette, vertex AO and lightmap in gamma RGB.
// Encode only at our existing Bevy sRGB framebuffer boundary.
fn ordinary_world_model_gamma_colour(in: VertexOutput, sampled_gamma: vec4<f32>) -> vec4<f32> {
    let lit_gamma = ((sampled_gamma.rgb * in.tint_gamma) * in.native_ao_face) * terrain_light_colour(in.native_light_levels);
    let fog_gamma = tint_to_gamma(vec4(atmosphere.fog_color_start.rgb, 1.0)).rgb;
    return vec4(mix(lit_gamma, fog_gamma, distance_fog_amount(in.world_position)), sampled_gamma.a);
}

fn ordinary_world_model_colour(in: VertexOutput, sampled_gamma: vec4<f32>) -> vec4<f32> {
    return tint_to_linear(ordinary_world_model_gamma_colour(in, sampled_gamma));
}
#endif

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) front_facing: bool,
) -> @location(0) vec4<f32> {
    if (in.visible == 0u) { discard; }
    if (!front_facing && in.two_sided == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    // Both views already hold the working colour space, so frames blend before the alpha test.
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_model_ref(in, in.next_texture, dx, dy), in.frame_blend);
    }
    if ((in.material_flags & MATERIAL_ALPHA_CUTOUT_FLAG) != 0u && sampled.a < 0.5) { discard; }
    let output_alpha = select(1.0, sampled.a, (in.material_flags & MATERIAL_ALPHA_CUTOUT_FLAG) != 0u);
#ifdef OPAQUE_OVERDRAW
    return vec4(1.0);
#else
#ifdef ENHANCED
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position, in.world_origin);
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
    return vec4(apply_distance_fog(shaded, in.world_position), output_alpha);
#else
    let colour = ordinary_world_model_colour(in, sampled);
    return vec4(colour.rgb, output_alpha);
#endif
#endif
}

@fragment
fn fragment_blend(
    in: VertexOutput,
    @builtin(front_facing) front_facing: bool,
) -> @location(0) vec4<f32> {
    if (in.visible == 0u) { discard; }
    if (!front_facing && in.two_sided == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_model_ref(in, in.next_texture, dx, dy), in.frame_blend);
    }
    // The background is fogged by the same transfer, so preserving source
    // alpha composes to one fog application instead of double-counting it.
#ifdef ENHANCED
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position, in.world_origin);
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
    return vec4(apply_distance_fog(shaded, in.world_position), colour.a);
#else
#ifdef NATIVE_GAMMA_BLEND
    return ordinary_world_model_gamma_colour(in, sampled);
#else
    return ordinary_world_model_colour(in, sampled);
#endif
#endif
}
#ifdef ENHANCED_SHADOW

// Alpha-tested terrain depth; opaque texels cast independently of baked light.
@fragment
fn fragment_shadow(in: VertexOutput) {
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    var sampled = sample_model_ref(in, in.current_texture, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_model_ref(in, in.next_texture, dx, dy), in.frame_blend);
    }
    if (in.visible == 0u || ((in.material_flags & MATERIAL_ALPHA_CUTOUT_FLAG) != 0u && sampled.a < 0.5)) { discard; }
}
#endif
