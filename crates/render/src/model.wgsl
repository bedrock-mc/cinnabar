#import cinnabar::material::{MaterialGpu, materials, positional_material}
#ifdef ENHANCED_SHADOW
#import cinnabar::enhanced_caster::{caster_clip, caster_previous_clip, caster_history_valid, caster_excludes_emitter}
#endif
#ifdef ENHANCED_MOTION
#import cinnabar::enhanced_actor_motion::submitted_surface_motion
#endif
#import bevy_render::view::View
#import cinnabar::biome_tint::{blended_biome_tint, blended_biome_tint_gamma}
#import cinnabar::lighting::{light_ao_factor, light_colour, face_shade, tint_to_gamma, tint_to_linear, terrain_light_levels, terrain_light_colour}
#ifdef ENHANCED
#import cinnabar::enhanced_view::{sky_illumination, material_class, shade_material, waved_position, enhanced_physical_atmosphere, enhanced_light_direction, enhanced_materials_enabled}
#import cinnabar::enhanced_radiance::{block_illumination, untextured_material_mer}
#endif

struct ChunkOrigin { value: vec4<i32>, cube_bases: vec4<u32> }
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
@group(0) @binding(9) var<storage, read> animations: array<AnimationGpu>;
@group(0) @binding(10) var<storage, read> animation_frames: array<u32>;
@group(0) @binding(11) var<uniform> clock: AnimationClockGpu;
@group(0) @binding(12) var<storage, read> model_templates: array<u32>;
@group(0) @binding(13) var<storage, read> geometry_streams: array<u32>;
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(ENHANCED_COLOR_TEXTURE_BINDING_0) var enhanced_color_page_0: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_COLOR_TEXTURE_BINDING_1) var enhanced_color_page_1: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_NORMAL_TEXTURE_BINDING_0) var enhanced_normal_page_0: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_NORMAL_TEXTURE_BINDING_1) var enhanced_normal_page_1: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_MER_TEXTURE_BINDING_0) var enhanced_mer_page_0: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_MER_TEXTURE_BINDING_1) var enhanced_mer_page_1: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_SAMPLER_BINDING) var enhanced_sampler: sampler;
@group(0) @binding(ENHANCED_TEXTURE_REF_BINDING) var<storage, read> enhanced_texture_refs: array<u32>;

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
#ifdef ENHANCED_MOTION
    @location(14) previous_clip: vec4<f32>,
#endif
#ifdef ENHANCED
    @location(14) @interpolate(flat) surface_class: u32,
#endif
}

struct FrameSample { current: u32, next: u32, blend: f32 }

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
#endif
    invisible.two_sided = 0u;
    invisible.world_origin = vec3(0.0);
    invisible.world_position = vec3(0.0);
#ifdef ENHANCED_MOTION
    invisible.previous_clip = vec4(0.0);
#endif
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
    var rotation = packed_transform >> 12u;
    if (is_lily_pad) {
        rotation = lily_pad_rotation(origin.value.xyz + vec3<i32>(block_position));
    }
    template_position = rotate_cross(template_position, rotation);
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
#ifdef ENHANCED_MOTION
    out.previous_clip = caster_previous_clip(world, material_id, clamp(template_position.y, 0.0, 1.0));
#endif
    out.uv = vec2<f32>(
        f32(packed_u16(template_quad_base + 6u, uv_component)),
        f32(packed_u16(template_quad_base + 6u, uv_component + 1u)),
    ) / 4096.0;
    out.current_texture = frame.current;
    let normals = array(vec3(0.0), vec3(0.0,-1.0,0.0), vec3(0.0,1.0,0.0), vec3(-1.0,0.0,0.0), vec3(1.0,0.0,0.0), vec3(0.0,0.0,-1.0), vec3(0.0,0.0,1.0));
    out.normal = rotate_cross(normals[quad_flags & 7u] + vec3(0.5,0.0,0.5), rotation) - vec3(0.5,0.0,0.5);
    out.material_flags = material.flags;
    out.local_position = block_position;
    out.biome_record = u32(origin.value.w);
    out.next_texture = frame.next;
    out.frame_blend = frame.blend;
    out.visible = is_visible;
    // Vanilla uses white top vertices and RGB 0x0f on the reverse
    // plane. Apply it after sampling, without another 8-bit atlas quantization.
    let pad_shade = select(1.0, 15.0 / 255.0, out.normal.y < 0.0);
    let terrain_shade = select(light_ao_factor((light_sample >> 8u) & 7u) * face_shade(out.normal, (light_sample & 2048u) != 0u), pad_shade, is_lily_pad);
    out.lighting = light_colour(light_sample) * terrain_shade;
#ifdef ENHANCED
    out.lighting = block_illumination(light_sample);
    out.sky_light = sky_illumination(light_sample);
    out.ambient_occlusion = light_ao_factor((light_sample >> 8u) & 7u);
#else
    out.native_light_levels = terrain_light_levels(light_sample);
    out.native_ao_face = terrain_shade;
#endif
    out.two_sided = select(0u, 1u, (quad_flags & 8u) != 0u);
    out.world_position = world;
    out.world_origin = vec3<f32>(origin.value.xyz);
#ifdef ENHANCED
    out.surface_class = material_class(material_id);
    out.world_position = waved_position(world, out.surface_class, clamp(template_position.y, 0.0, 1.0));
    out.clip_position = view.clip_from_world * vec4(out.world_position, 1.0);
    out.normal = template_quad_normal(template_quad_base, packed_transform >> 12u);
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
#ifdef ENHANCED
    return sample_enhanced_colour(texture_ref, uv, dx, dy);
#else
#ifdef ENHANCED_SHADOW
    return sample_enhanced_colour(texture_ref, uv, dx, dy);
#else
    return tint_to_gamma(sample_enhanced_fallback(texture_ref, uv, dx, dy));
#endif
#endif
}

fn sample_enhanced_colour(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let lookup_index = (texture_ref >> 31u) * 2048u + (texture_ref & 0x7ffu);
    let authored_ref = enhanced_texture_refs[lookup_index];
    if (authored_ref != 0xffffffffu) {
        let authored_page = authored_ref >> 31u;
        let authored_layer = i32(authored_ref & 0x7ffu);
        if (authored_page == 0u) {
            return textureSampleGrad(enhanced_color_page_0, enhanced_sampler, uv, authored_layer, dx, dy);
        }
        return textureSampleGrad(enhanced_color_page_1, enhanced_sampler, uv, authored_layer, dx, dy);
    }
    return sample_enhanced_fallback(texture_ref, uv, dx, dy);
}

fn sample_enhanced_fallback(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let layer = i32(texture_ref & 0x7ffu);
    var sampled: vec4<f32>;
    if ((texture_ref >> 31u) == 0u) {
        sampled = textureSampleGrad(block_textures_page_0, block_sampler, uv, layer, dx, dy);
    } else {
        sampled = textureSampleGrad(block_textures_page_1, block_sampler, uv, layer, dx, dy);
    }
    return sampled;
}

// ENHANCED_PBR_SAMPLING

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
#ifdef ENHANCED
    if (enhanced_physical_atmosphere()) { return colour; }
    let delta = world_position - view.world_position;
    let direction = delta / max(length(delta), 1.0e-4);
    let horizon = smoothstep(-0.3, 0.75, direction.y);
    let sky = mix(
        atmosphere.sky_horizon_thunder.rgb,
        atmosphere.sky_zenith_rain.rgb,
        smoothstep(0.12, 0.92, horizon),
    );
    let storm = atmosphere.sky_horizon_thunder.a;
    let dusk = atmosphere.sunrise_band.rgb * atmosphere.sunrise_band.a
        * smoothstep(-0.15, 0.75, direction.y) * 0.32;
    let fog_colour = mix(
        atmosphere.fog_color_start.rgb,
        mix(atmosphere.fog_color_start.rgb, sky, 0.28 + 0.24 * horizon),
        1.0 - 0.22 * storm,
    ) + dusk;
    return mix(colour, fog_colour, smoothstep(0.0, 1.0, distance_fog_amount(world_position)));
#else
    return mix(colour, atmosphere.fog_color_start.rgb, distance_fog_amount(world_position));
#endif
}

#ifndef ENHANCED
// Current TopSnow tessellator -> ordinary terrain ->
// AO/flat lighting. A bounded world model is not an entity material:
// RenderChunk multiplies atlas/palette, vertex AO and lightmap in gamma RGB.
// Encode only at our existing Bevy sRGB framebuffer boundary.
fn ordinary_world_model_gamma_colour(in: VertexOutput, sampled_gamma: vec4<f32>) -> vec4<f32> {
    var tint_gamma = vec3(1.0);
    let tint_kind = in.material_flags & 0x30u;
    if (tint_kind != 0u) {
        tint_gamma = blended_biome_tint_gamma(tint_kind, in.material_flags, in.biome_record, in.local_position, in.world_origin).rgb;
    }
    let lit_gamma = ((sampled_gamma.rgb * tint_gamma) * in.native_ao_face) * terrain_light_colour(in.native_light_levels);
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
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    var material_uv=in.uv;
#ifdef ENHANCED
    let basis=material_basis(in.normal,dpdx(in.world_position),dpdy(in.world_position),dx,dy);
    material_uv=parallax_material_uv(in.current_texture,in.uv,dx,dy,normalize(view.world_position-in.world_position),basis,
        distance(view.world_position,in.world_position),enhanced_materials_enabled() && (in.material_flags&(1u<<8u))==0u && in.two_sided==0u && (in.surface_class&48u)==0u && in.frame_blend==0.0);
#endif
    if (in.visible == 0u) { discard; }
    if (!front_facing && in.two_sided == 0u) { discard; }
    var sampled = sample_ref(in.current_texture, material_uv, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_ref(in.next_texture, material_uv, dx, dy), in.frame_blend);
    }
    if (sampled.a < 0.5) { discard; }
#ifdef ENHANCED
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position, in.world_origin);
    var material_normal_sample=sample_pbr_texture(true,in.current_texture,material_uv,dx,dy);
    var material_mer_sample=sample_pbr_texture(false,in.current_texture,material_uv,dx,dy);
    if (in.frame_blend>0.0) {
        material_normal_sample=mix(material_normal_sample,sample_pbr_texture(true,in.next_texture,material_uv,dx,dy),in.frame_blend);
        material_mer_sample=mix(material_mer_sample,sample_pbr_texture(false,in.next_texture,material_uv,dx,dy),in.frame_blend);
    }
    let shaded = shade_material(
        colour.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion * material_normal_sample.b,
        in.surface_class,
        material_normal(material_normal_sample,basis),
        material_mer_sample,
        select(0u,authored_material_ref(in.current_texture),authored_material_ref(in.current_texture)!=0xffffffffu),
        parallax_direct_visibility(in.current_texture,material_uv,dx,dy,enhanced_light_direction(),basis,distance(view.world_position,in.world_position),
            enhanced_materials_enabled() && (in.material_flags&(1u<<8u))==0u && (in.surface_class&48u)==0u && in.frame_blend==0.0),
    );
    return vec4(apply_distance_fog(shaded, in.world_position), colour.a);
#else
    return ordinary_world_model_colour(in, sampled);
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
    var sampled = sample_ref(in.current_texture, in.uv, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_ref(in.next_texture, in.uv, dx, dy), in.frame_blend);
    }
    // The background is fogged by the same transfer, so preserving source
    // alpha composes to one fog application instead of double-counting it.
#ifdef ENHANCED
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position, in.world_origin);
    let basis=material_basis(in.normal,dpdx(in.world_position),dpdy(in.world_position),dx,dy);
    var material_normal_sample=sample_pbr_texture(true,in.current_texture,in.uv,dx,dy);
    var material_mer_sample=sample_pbr_texture(false,in.current_texture,in.uv,dx,dy);
    if (in.frame_blend>0.0) {
        material_normal_sample=mix(material_normal_sample,sample_pbr_texture(true,in.next_texture,in.uv,dx,dy),in.frame_blend);
        material_mer_sample=mix(material_mer_sample,sample_pbr_texture(false,in.next_texture,in.uv,dx,dy),in.frame_blend);
    }
    let shaded = shade_material(
        colour.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion * material_normal_sample.b,
        in.surface_class,
        material_normal(material_normal_sample,basis),
        material_mer_sample,
        select(0u,authored_material_ref(in.current_texture),authored_material_ref(in.current_texture)!=0xffffffffu),
        1.0,
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
fn shadow_coverage(in: VertexOutput, front: bool) {
    if (!front && in.two_sided == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    var sampled = sample_ref(in.current_texture, in.uv, dx, dy);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_ref(in.next_texture, in.uv, dx, dy), in.frame_blend);
    }
    if (sampled.a < 0.5 || in.visible == 0u) { discard; }
}
@fragment
fn fragment_shadow(in: VertexOutput, @builtin(front_facing) front: bool) {
    shadow_coverage(in, front);
    if (caster_excludes_emitter(in.world_position)) { discard; }
}
#ifdef ENHANCED_MOTION
@fragment
fn fragment_motion(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    shadow_coverage(in, front);
    let current_uv = (in.clip_position.xy - view.viewport.xy) / view.viewport.zw;
    let clip = vec4(current_uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    return submitted_surface_motion(clip, in.previous_clip, caster_history_valid());
}
#endif
#endif
