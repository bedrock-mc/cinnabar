#import bevy_render::view::View
#import cinnabar::biome_tint::blended_biome_tint
#import cinnabar::lighting::{light_ao_factor, light_brightness, lit_colour}

struct ChunkOrigin { value: vec4<i32>, cube_bases: vec4<u32> }
struct MaterialGpu { texture: u32, flags: u32, animation: u32 }
struct AnimationGpu { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32 }
struct AnimationClockGpu { tick: u32, partial_tick: f32, padding_0: u32, padding_1: u32 }
struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>, moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>, sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>, fog_end_time: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> cube_quads: array<u32>;
@group(0) @binding(2) var<storage, read> chunk_origins: array<ChunkOrigin>;
@group(0) @binding(3) var<storage, read> materials: array<MaterialGpu>;
@group(0) @binding(4) var block_textures_page_0: texture_2d_array<f32>;
@group(0) @binding(5) var block_textures_page_1: texture_2d_array<f32>;
@group(0) @binding(6) var block_sampler: sampler;
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
    @location(4) local_position: vec3<f32>,
    @location(5) @interpolate(flat) biome_record: u32,
    @location(6) @interpolate(flat) next_texture: u32,
    @location(7) @interpolate(flat) frame_blend: f32,
    @location(8) @interpolate(flat) visible: u32,
    @location(9) block_light: f32,
    @location(10) sky_light: f32,
    @location(11) ambient_occlusion: f32,
    @location(12) @interpolate(flat) two_sided: u32,
    @location(13) world_position: vec3<f32>,
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
    invisible.block_light = 0.0;
    invisible.sky_light = 0.0;
    invisible.ambient_occlusion = 0.0;
    invisible.two_sided = 0u;
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
    template_position = rotate_cross(template_position, packed_transform >> 12u);
    let block_position = vec3<f32>(
        f32(packed_transform & 15u),
        f32((packed_transform >> 4u) & 15u),
        f32((packed_transform >> 8u) & 15u),
    );
    let local_position = block_position + template_position;
    let origin = chunk_origins[metadata_index];
    let material_id = model_templates[template_quad_base + 10u];
    let quad_flags = model_templates[template_quad_base + 11u];
    let material = materials[material_id];
    let frame = animation_sample(material);
    let uv_component = corner * 2u;

    if (lighting_base_index >= geometry_word_count / 2u || quad_index >= geometry_word_count / 2u - lighting_base_index) {
        return invisible_vertex();
    }
    let light_word = geometry_streams[(lighting_base_index + quad_index) * 2u + corner / 2u];
    let light_sample = select(light_word & 0xffffu, light_word >> 16u, (corner & 1u) != 0u);
    let block_light = f32(light_sample & 15u);
    let sky_light = f32((light_sample >> 4u) & 15u);
    let ao = f32((light_sample >> 8u) & 3u);
    var out: VertexOutput;
    let world = vec3<f32>(origin.value.xyz) + local_position;
    out.clip_position = view.clip_from_world * vec4(world, 1.0);
    out.uv = vec2<f32>(
        f32(packed_u16(template_quad_base + 6u, uv_component)),
        f32(packed_u16(template_quad_base + 6u, uv_component + 1u)),
    ) / 4096.0;
    out.current_texture = frame.current;
    out.normal = vec3(0.0, 1.0, 0.0);
    out.material_flags = material.flags;
    out.local_position = local_position;
    out.biome_record = u32(origin.value.w);
    out.next_texture = frame.next;
    out.frame_blend = frame.blend;
    out.visible = is_visible;
    out.block_light = light_brightness(u32(block_light));
    out.sky_light = light_brightness(u32(sky_light));
    out.ambient_occlusion = light_ao_factor(u32(ao));
    out.two_sided = select(0u, 1u, (quad_flags & 8u) != 0u);
    out.world_position = world;
    return out;
}

fn tinted(sampled: vec4<f32>, flags: u32, record: u32, position: vec3<f32>) -> vec4<f32> {
    let tint_kind = flags & 0x30u;
    if (tint_kind == 0u) { return vec4(sampled.rgb, sampled.a); }
    return vec4(sampled.rgb * blended_biome_tint(tint_kind, flags, record, position), sampled.a);
}

fn sample_ref(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let layer = i32(texture_ref & 0x7ffu);
    if ((texture_ref >> 31u) == 0u) {
        return textureSampleGrad(block_textures_page_0, block_sampler, uv, layer, dx, dy);
    }
    return textureSampleGrad(block_textures_page_1, block_sampler, uv, layer, dx, dy);
}

fn apply_distance_fog(colour: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    let distance_to_camera = distance(world_position, view.world_position);
    let fog = smoothstep(atmosphere.fog_color_start.w, atmosphere.fog_end_time.x, distance_to_camera);
    return mix(colour, atmosphere.fog_color_start.rgb, fog);
}

@fragment
fn fragment(
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
    if (sampled.a < 0.5) { discard; }
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position);
    let lit = lit_colour(
        colour.rgb,
        in.block_light,
        in.sky_light,
        in.ambient_occlusion,
        atmosphere.sun_direction_daylight.w,
    );
    return vec4(apply_distance_fog(lit, in.world_position), colour.a);
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
    let colour = tinted(sampled, in.material_flags, in.biome_record, in.local_position);
    // The background is fogged by the same transfer, so preserving source
    // alpha composes to one fog application instead of double-counting it.
    let lit = lit_colour(
        colour.rgb,
        in.block_light,
        in.sky_light,
        in.ambient_occlusion,
        atmosphere.sun_direction_daylight.w,
    );
    return vec4(apply_distance_fog(lit, in.world_position), colour.a);
}
