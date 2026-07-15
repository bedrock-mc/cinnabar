#import bevy_render::view::View
#import cinnabar::biome_tint::blended_biome_tint
#import cinnabar::lighting::{light_ao_factor, light_brightness, lit_colour}

struct ChunkOrigin { value: vec4<i32>, cube_bases: vec4<u32> }
struct MaterialGpu { texture: u32, flags: u32, animation: u32 }
struct AnimationGpu { frame_start: u32, frame_count: u32, ticks_per_frame: u32, flags: u32 }
struct AnimationClockGpu { tick: u32, partial_tick: f32, padding_0: u32, padding_1: u32 }
struct TransparentDrawRef { liquid_record_index: u32, metadata_index: u32 }
struct FrameSample { current: u32, next: u32, blend: f32 }
struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>, moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>, sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>, fog_end_time: vec4<f32>,
}

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
const LIQUID_DEPTH_WRITE_BIT: u32 = 1u << 31u;

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
@group(0) @binding(14) var<storage, read> transparent_refs: array<TransparentDrawRef>;
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) current_texture: u32,
    @location(2) @interpolate(flat) next_texture: u32,
    @location(3) @interpolate(flat) frame_blend: f32,
    @location(4) @interpolate(flat) water_tint: vec3<f32>,
    @location(5) block_light: f32,
    @location(6) sky_light: f32,
    @location(7) ambient_occlusion: f32,
    @location(8) @interpolate(flat) depth_write_route: u32,
    @location(9) world_position: vec3<f32>,
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
    return vertex_for_ref(transparent_refs[instance_index], vertex_index);
}

@vertex
fn vertex_depth(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let draw_ref = TransparentDrawRef(instance_index, vertex_index / 4u);
    return vertex_for_ref(draw_ref, vertex_index);
}

fn vertex_for_ref(draw_ref: TransparentDrawRef, vertex_index: u32) -> VertexOutput {
    let corner = vertex_index & 3u;
    let liquid_word = draw_ref.liquid_record_index * 4u;
    let geometry = geometry_streams[liquid_word];
    let height_word = geometry_streams[liquid_word + 1u];
    let packed_material = geometry_streams[liquid_word + 2u];
    let material = materials[packed_material & ~LIQUID_DEPTH_WRITE_BIT];
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
    out.water_tint = blended_biome_tint(
        0x30u,
        0u,
        u32(chunk_origin.value.w),
        vec3<f32>(block_coordinate),
    );
    out.block_light = light_brightness(u32(block_light));
    out.sky_light = light_brightness(u32(sky_light));
    out.ambient_occlusion = light_ao_factor(u32(ao));
    out.depth_write_route = packed_material >> 31u;
    out.world_position = world_position;
    return out;
}

fn sample_texture_ref(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
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
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if (in.depth_write_route != 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let current_sample = sample_texture_ref(in.current_texture, in.uv, dx, dy);
    var sampled = current_sample;
    if (in.frame_blend > 0.0) {
        let next_sample = sample_texture_ref(in.next_texture, in.uv, dx, dy);
        sampled = mix(current_sample, next_sample, in.frame_blend);
    }
    let colour = lit_colour(
        sampled.rgb * in.water_tint,
        in.block_light,
        in.sky_light,
        in.ambient_occlusion,
        atmosphere.sun_direction_daylight.w,
    );
    // The background is fogged by the same transfer, so preserving source
    // alpha composes to one fog application instead of double-counting it.
    return vec4(apply_distance_fog(colour, in.world_position), sampled.a);
}

@fragment
fn fragment_depth(in: VertexOutput) -> @location(0) vec4<f32> {
    if (in.depth_write_route == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let current_sample = sample_texture_ref(in.current_texture, in.uv, dx, dy);
    var sampled = current_sample;
    if (in.frame_blend > 0.0) {
        let next_sample = sample_texture_ref(in.next_texture, in.uv, dx, dy);
        sampled = mix(current_sample, next_sample, in.frame_blend);
    }
    let lit = lit_colour(
        sampled.rgb,
        in.block_light,
        in.sky_light,
        in.ambient_occlusion,
        atmosphere.sun_direction_daylight.w,
    );
    return vec4(apply_distance_fog(lit, in.world_position), 1.0);
}
