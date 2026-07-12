#import bevy_render::view::View

struct ChunkOrigin { value: vec4<i32> }
struct MaterialGpu { texture: u32, flags: u32, animation: u32 }
struct AnimationGpu { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32 }
struct AnimationClockGpu { tick: u32, partial_tick: f32, padding_0: u32, padding_1: u32 }
struct BiomeTintGpu {
    grass: u32, foliage: u32, birch: u32, evergreen: u32,
    dry_foliage: u32, water: u32, flags: u32, padding: u32,
}
struct TransparentDrawRef { liquid_record_index: u32, metadata_index: u32 }
struct FrameSample { current: u32, next: u32, blend: f32 }

// These literal tables are the GPU half of the packed-liquid stream contract.
// Four entries per face in Face's numeric order, then packed corner order.
const LIQUID_FACE_XZ: array<vec2<f32>, 24> = array(
    vec2(0.0, 0.0), vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0),
    vec2(1.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 0.0),
    vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 0.0),
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    vec2(1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 0.0), vec2(0.0, 0.0),
    vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 1.0),
);
const LIQUID_BASE_UV: array<vec2<f32>, 24> = array(
    vec2(0.0, 0.0), vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 0.0),
    vec2(0.0, 0.0), vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 0.0),
    vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 0.0),
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    vec2(0.0, 0.0), vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 0.0),
    vec2(0.0, 0.0), vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 0.0),
);
const FLOW_ANGLE_DIRECTION: f32 = 1.0;
const FLOW_ANGLE_OFFSET: f32 = -1.5707963267948966;
const SIDE_HEIGHT_BIAS: f32 = 1.0;
const SIDE_HEIGHT_SCALE: f32 = -1.0;
const FALLING_SCROLL_DIRECTION: f32 = 1.0;
const FALLING_SCROLL_TICKS: f32 = 32.0;
const FLOW_FACE: u32 = 3u;
const SIDE_FACE_MASK: u32 = 51u;
const FALLING_FACE_MASK: u32 = 51u;

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
@group(0) @binding(14) var<storage, read> transparent_refs: array<TransparentDrawRef>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) current_texture: u32,
    @location(2) @interpolate(flat) next_texture: u32,
    @location(3) @interpolate(flat) frame_blend: f32,
    @location(4) @interpolate(flat) water_tint: vec3<f32>,
    @location(5) light_factor: f32,
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
    let next = animation_frames[animation.frame_start + next_index];
    let blend = (
        f32(clock.tick % animation.ticks_per_frame) +
        clamp(clock.partial_tick, 0.0, 0.99999994)
    ) / f32(animation.ticks_per_frame);
    return FrameSample(current, next, blend);
}

fn signed_i8(word: u32, shift: u32) -> i32 {
    return bitcast<i32>((word >> shift) << 24u) >> 24u;
}

// Task 12 winding: top NW/NE/SE/SW; bottom NW/SW/SE/NE;
// -X bottom-N/top-N/top-S/bottom-S; +X bottom-S/top-S/top-N/bottom-N;
// -Z bottom-E/top-E/top-W/bottom-W; +Z bottom-W/top-W/top-E/bottom-E.
fn liquid_corner(geometry: u32, height_word: u32, corner: u32) -> vec3<f32> {
    let face = (geometry >> 12u) & 7u;
    let origin = vec3<f32>(
        f32(geometry & 15u),
        f32((geometry >> 4u) & 15u),
        f32((geometry >> 8u) & 15u),
    );
    let height = f32((height_word >> (corner * 8u)) & 255u) / 255.0;
    let xz = LIQUID_FACE_XZ[face * 4u + corner];
    return origin + vec3(xz.x, height, xz.y);
}

fn face_normal(face: u32) -> vec3<f32> {
    switch face {
        case 0u: { return vec3(-1.0, 0.0, 0.0); }
        case 1u: { return vec3(1.0, 0.0, 0.0); }
        case 2u: { return vec3(0.0, -1.0, 0.0); }
        case 3u: { return vec3(0.0, 1.0, 0.0); }
        case 4u: { return vec3(0.0, 0.0, -1.0); }
        default: { return vec3(0.0, 0.0, 1.0); }
    }
}

fn liquid_uv(
    face: u32,
    corner: u32,
    height: f32,
    flow_x: i32,
    flow_z: i32,
    falling: bool,
) -> vec2<f32> {
    var uv = LIQUID_BASE_UV[face * 4u + corner];
    if (((SIDE_FACE_MASK >> face) & 1u) != 0u) {
        uv.y = SIDE_HEIGHT_BIAS + SIDE_HEIGHT_SCALE * height;
    }
    if (face == FLOW_FACE) {
        if (flow_x != 0 || flow_z != 0) {
            let radians = FLOW_ANGLE_DIRECTION * atan2(f32(flow_z), f32(flow_x)) + FLOW_ANGLE_OFFSET;
            let centered = uv - vec2(0.5);
            let cosine = cos(radians);
            let sine = sin(radians);
            uv = vec2(
                centered.x * cosine - centered.y * sine,
                centered.x * sine + centered.y * cosine,
            ) + vec2(0.5);
        }
    }
    if (falling && ((FALLING_FACE_MASK >> face) & 1u) != 0u) {
        let falling_phase = fract(
            (f32(clock.tick) + clamp(clock.partial_tick, 0.0, 0.99999994)) /
            FALLING_SCROLL_TICKS,
        );
        uv.y += FALLING_SCROLL_DIRECTION * falling_phase;
    }
    return uv;
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let corner = vertex_index & 3u;
    let draw_ref = transparent_refs[instance_index];
    let liquid_word = draw_ref.liquid_record_index * 4u;
    let geometry = geometry_streams[liquid_word];
    let height_word = geometry_streams[liquid_word + 1u];
    let material = materials[geometry_streams[liquid_word + 2u]];
    let lighting_record_index = geometry_streams[liquid_word + 3u];
    let lighting_word = geometry_streams[lighting_record_index * 2u + corner / 2u];
    let light_sample = select(
        lighting_word & 0xffffu,
        lighting_word >> 16u,
        (corner & 1u) != 0u,
    );
    let block_light = f32(light_sample & 15u);
    let sky_light = f32((light_sample >> 4u) & 15u);
    let ao = f32((light_sample >> 8u) & 3u);
    let face = (geometry >> 12u) & 7u;
    let local_position = liquid_corner(geometry, height_word, corner);
    let chunk_origin = chunk_origins[draw_ref.metadata_index];
    let world_position = vec3<f32>(chunk_origin.value.xyz) + local_position;
    let frame = animation_sample(material);
    let block_coordinate = vec3<u32>(
        geometry & 15u,
        (geometry >> 4u) & 15u,
        (geometry >> 8u) & 15u,
    );
    let requested_tint = packed_biome_tint_index(u32(chunk_origin.value.w), block_coordinate);
    let tint_index = select(0u, requested_tint, requested_tint < arrayLength(&biome_tints));
    let tint = biome_tints[tint_index];

    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4(world_position, 1.0);
    out.uv = liquid_uv(
        face,
        corner,
        f32((height_word >> (corner * 8u)) & 255u) / 255.0,
        signed_i8(geometry, 16u),
        signed_i8(geometry, 24u),
        (geometry & (1u << 15u)) != 0u,
    );
    out.current_texture = frame.current;
    out.next_texture = frame.next;
    out.frame_blend = frame.blend;
    out.water_tint = unpack_linear_rgb10(tint.water);
    out.light_factor = max(block_light, sky_light) / 15.0 * (1.0 - ao * 0.12);
    return out;
}

fn unpack_linear_rgb10(packed: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(packed & 0x3ffu),
        f32((packed >> 10u) & 0x3ffu),
        f32((packed >> 20u) & 0x3ffu),
    ) / 1023.0;
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

fn sample_texture_ref(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let layer = i32(texture_ref & 0x7ffu);
    if ((texture_ref >> 31u) == 0u) {
        return textureSampleGrad(block_textures_page_0, block_sampler, uv, layer, dx, dy);
    }
    return textureSampleGrad(block_textures_page_1, block_sampler, uv, layer, dx, dy);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let current_sample = sample_texture_ref(in.current_texture, in.uv, dx, dy);
    var sampled = current_sample;
    if (in.frame_blend > 0.0) {
        let next_sample = sample_texture_ref(in.next_texture, in.uv, dx, dy);
        sampled = mix(current_sample, next_sample, in.frame_blend);
    }
    return vec4(sampled.rgb * in.water_tint * in.light_factor, sampled.a);
}
