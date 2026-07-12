#import bevy_render::view::View

struct ChunkOrigin { value: vec4<i32> }
struct MaterialGpu { texture: u32, flags: u32, animation: u32 }
struct AnimationGpu { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32 }
struct AnimationClockGpu { tick: u32, partial_tick: f32, padding_0: u32, padding_1: u32 }
struct BiomeTintGpu {
    grass: u32, foliage: u32, birch: u32, evergreen: u32,
    dry_foliage: u32, water: u32, flags: u32, padding: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> cube_quads: array<u32>;
@group(0) @binding(2) var<storage, read> chunk_origins: array<ChunkOrigin>;
@group(0) @binding(3) var<storage, read> materials: array<MaterialGpu>;
@group(0) @binding(4) var block_textures_page_0: texture_2d_array<f32>;
@group(0) @binding(5) var block_textures_page_1: texture_2d_array<f32>;
@group(0) @binding(6) var block_sampler: sampler;
@group(0) @binding(7) var<storage, read> biome_records: array<u32>;
@group(0) @binding(8) var<storage, read> biome_tints: array<BiomeTintGpu>;
@group(0) @binding(9) var<storage, read> animations: array<AnimationGpu>;
@group(0) @binding(10) var<storage, read> animation_frames: array<u32>;
@group(0) @binding(11) var<uniform> clock: AnimationClockGpu;
@group(0) @binding(12) var<storage, read> model_templates: array<u32>;
@group(0) @binding(13) var<storage, read> geometry_streams: array<u32>;

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
    @location(9) light_factor: f32,
    @location(10) @interpolate(flat) two_sided: u32,
}

struct FrameSample { current: u32, next: u32, blend: f32 }

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
    let local_vertex = vertex_index & 127u;
    let metadata_index = vertex_index / 128u;
    let quad_index = local_vertex / 4u;
    let corner = local_vertex & 3u;
    let ref_word = instance_index * 4u;
    let packed_transform = geometry_streams[ref_word];
    let template_id = geometry_streams[ref_word + 1u];
    let lighting_base_index = geometry_streams[ref_word + 2u];
    let visible_quad_mask = geometry_streams[ref_word + 3u];
    let descriptor = 1u + template_id * 3u;
    let quad_start = model_templates[descriptor];
    let quad_count = model_templates[descriptor + 1u];
    if (quad_count == 0u) {
        var invisible: VertexOutput;
        invisible.clip_position = vec4(0.0, 0.0, 0.0, 1.0);
        invisible.uv = vec2(0.0);
        invisible.current_texture = 0u;
        invisible.normal = vec3(0.0);
        invisible.material_flags = 0u;
        invisible.local_position = vec3(0.0);
        invisible.biome_record = 0u;
        invisible.next_texture = 0u;
        invisible.frame_blend = 0.0;
        invisible.visible = 0u;
        invisible.light_factor = 0.0;
        invisible.two_sided = 0u;
        return invisible;
    }
    let last_quad_index = quad_count - 1u;
    let safe_quad_index = min(quad_index, last_quad_index);
    let template_quad_base = 1u + model_templates[0] * 3u + (quad_start + safe_quad_index) * 12u;
    let is_visible = u32(quad_index < quad_count) * ((visible_quad_mask >> quad_index) & 1u);

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

    let light_word = geometry_streams[(lighting_base_index + safe_quad_index) * 2u + corner / 2u];
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
    out.light_factor = max(block_light, sky_light) / 15.0 * (1.0 - ao * 0.12);
    out.two_sided = select(0u, 1u, (quad_flags & 8u) != 0u);
    return out;
}

fn unpack_linear_rgb10(packed: u32) -> vec3<f32> {
    return vec3<f32>(f32(packed & 0x3ffu), f32((packed >> 10u) & 0x3ffu), f32((packed >> 20u) & 0x3ffu)) / 1023.0;
}

fn packed_biome_tint_index(record: u32, coordinate: vec3<u32>) -> u32 {
    let header = biome_records[record];
    let bits = header & 0xffu;
    let palette_len = (header >> 8u) & 0x1fffu;
    if (palette_len == 0u) { return 0u; }
    var word_count = 0u;
    var palette_index = 0u;
    if (bits != 0u) {
        let per_word = 32u / bits;
        word_count = (4096u + per_word - 1u) / per_word;
        let linear = (coordinate.x << 8u) | (coordinate.z << 4u) | coordinate.y;
        let word = biome_records[record + 1u + linear / per_word];
        palette_index = (word >> ((linear % per_word) * bits)) & ((1u << bits) - 1u);
    }
    if (palette_index >= palette_len) { return 0u; }
    return biome_records[record + 1u + word_count + palette_index];
}

fn tinted(sampled: vec4<f32>, flags: u32, record: u32, position: vec3<f32>) -> vec4<f32> {
    let tint_kind = flags & 0x30u;
    if (tint_kind == 0u) { return vec4(sampled.rgb, 1.0); }
    let coordinate = vec3<u32>(clamp(floor(position), vec3(0.0), vec3(15.0)));
    let requested = packed_biome_tint_index(record, coordinate);
    let tint = biome_tints[select(0u, requested, requested < arrayLength(&biome_tints))];
    var colour = tint.foliage;
    if (tint_kind == 0x10u) { colour = tint.grass; }
    if (tint_kind == 0x30u) { colour = tint.water; }
    return vec4(sampled.rgb * unpack_linear_rgb10(colour), 1.0);
}

fn sample_ref(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let layer = i32(texture_ref & 0x7ffu);
    if ((texture_ref >> 31u) == 0u) {
        return textureSampleGrad(block_textures_page_0, block_sampler, uv, layer, dx, dy);
    }
    return textureSampleGrad(block_textures_page_1, block_sampler, uv, layer, dx, dy);
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
    return vec4(colour.rgb * in.light_factor, colour.a);
}
