#import cinnabar::material::{MaterialGpu, materials, positional_material}
#import bevy_render::view::View
#import cinnabar::biome_tint::blended_biome_tint
#import cinnabar::lighting::{light_ao_factor, light_colour, face_shade, tint_to_gamma, tint_to_linear, terrain_light_levels, terrain_light_colour}
#ifdef ENHANCED
#import cinnabar::enhanced_common::CLASS_WATER
#import cinnabar::enhanced_radiance::block_illumination
#import cinnabar::enhanced_view::{sky_illumination, material_class, shade_surface, shade_water, waved_water_position, enhanced_physical_atmosphere}
#endif

struct ChunkOrigin { value: vec4<i32>, cube_bases: vec4<u32> }
// ANIMATION_GPU_LAYOUT
struct AnimationClockGpu { tick: u32, partial_tick: f32, padding_0: u32, padding_1: u32 }
struct TransparentDrawRef { liquid_record_index: u32, metadata_index: u32 }
struct FrameSample { current: u32, next: u32, blend: f32, uv_scale: f32 }
struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>, moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>, sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>, fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>, sky_extra: vec4<f32>,
    liquid_distance: vec4<f32>,
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
// Native angle and UVs, expressed in our NW/NE/SE/SW order.
const FLOW_ANGLE_DIRECTION: f32 = -1.0;
const FLOW_ANGLE_OFFSET: f32 = 1.5707963267948966;
const SIDE_HEIGHT_BIAS: f32 = 1.0;
const SIDE_HEIGHT_SCALE: f32 = -1.0;
const FLOW_FACE: u32 = 3u;
const SIDE_FACE_MASK: u32 = 51u;
// LIQUID_GEOMETRY_CONSTANTS
const LIQUID_MATERIAL_MASK: u32 = ~(LIQUID_DEPTH_WRITE_BIT | LIQUID_TOP_INSET_BIT | LIQUID_TWO_SIDED_BIT);

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
@group(0) @binding(14) var<storage, read> transparent_refs: array<TransparentDrawRef>;
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;
#ifdef ENHANCED
@group(0) @binding(ENHANCED_COLOR_TEXTURE_BINDING_0) var enhanced_color_page_0: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_COLOR_TEXTURE_BINDING_1) var enhanced_color_page_1: texture_2d_array<f32>;
@group(0) @binding(ENHANCED_SAMPLER_BINDING) var enhanced_sampler: sampler;
@group(0) @binding(ENHANCED_TEXTURE_REF_BINDING) var<storage, read> enhanced_texture_refs: array<u32>;
#endif

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) current_texture: u32,
    @location(2) @interpolate(flat) next_texture: u32,
    @location(3) @interpolate(flat) frame_blend: f32,
    @location(4) water_tint: vec4<f32>,
    @location(5) lighting: vec3<f32>,
#ifdef ENHANCED
    @location(6) sky_light: f32,
    @location(7) ambient_occlusion: f32,
#endif
    @location(8) @interpolate(flat) depth_write_route: u32,
    @location(9) world_position: vec3<f32>,
    @location(12) native_light_levels: vec2<f32>,
    @location(13) @interpolate(flat) native_face_shade: f32,
    @location(14) @interpolate(flat) two_sided: u32,
#ifdef ENHANCED
    @location(10) normal: vec3<f32>,
    @location(11) @interpolate(flat) surface_class: u32,
#endif
}

fn animation_sample(material: MaterialGpu) -> FrameSample {
    if (material.animation == 0xffffffffu) {
        return FrameSample(material.texture, material.texture, 0.0, 1.0);
    }
    let animation = animations[material.animation];
    let current_index = (clock.tick / animation.ticks_per_frame) % animation.frame_count;
    let current = animation_frames[animation.frame_start + current_index];
    if ((animation.flags & 1u) == 0u || animation.frame_count == 1u) {
        return FrameSample(current, current, 0.0, animation.uv_scale);
    }
    let next_index = (current_index + 1u) % animation.frame_count;
    let next = animation_frames[animation.frame_start + next_index];
    let blend = (
        f32(clock.tick % animation.ticks_per_frame) +
        clamp(clock.partial_tick, 0.0, 0.99999994)
    ) / f32(animation.ticks_per_frame);
    return FrameSample(current, next, blend, animation.uv_scale);
}

fn signed_i8(word: u32, shift: u32) -> i32 {
    return bitcast<i32>((word >> shift) << 24u) >> 24u;
}

// Task 12 winding: top NW/NE/SE/SW; bottom NW/SW/SE/NE;
// -X bottom-N/top-N/top-S/bottom-S; +X bottom-S/top-S/top-N/bottom-N;
// -Z bottom-E/top-E/top-W/bottom-W; +Z bottom-W/top-W/top-E/bottom-E.
fn liquid_corner(geometry: u32, height_word: u32, corner: u32, packed_material: u32) -> vec3<f32> {
    let face = (geometry >> 12u) & 7u;
    let origin = vec3<f32>(
        f32(geometry & 15u),
        f32((geometry >> 4u) & 15u),
        f32((geometry >> 8u) & 15u),
    );
    var height = f32((height_word >> (corner * 8u)) & 255u) / 255.0;
    var xz = LIQUID_FACE_XZ[face * 4u + corner];
    // Vanilla lowers exposed tops in-place; side upper vertices reuse them.
    // Covered tops retain their heights. The ordinary bottom face is untouched.
    if ((packed_material & LIQUID_TOP_INSET_BIT) != 0u && face != 2u && (face == 3u || corner == 1u || corner == 2u)) {
        height -= LIQUID_FACE_INSET;
    }
    if (face == 0u) { xz.x += LIQUID_FACE_INSET; }
    if (face == 1u) { xz.x -= LIQUID_FACE_INSET; }
    if (face == 4u) { xz.y += LIQUID_FACE_INSET; }
    if (face == 5u) { xz.y -= LIQUID_FACE_INSET; }
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
    uv_scale: f32,
) -> vec2<f32> {
    var uv = LIQUID_BASE_UV[face * 4u + corner];
    if (((SIDE_FACE_MASK >> face) & 1u) != 0u) {
        uv.y = SIDE_HEIGHT_BIAS + SIDE_HEIGHT_SCALE * height;
        uv *= uv_scale;
    }
    if (face == FLOW_FACE) {
        if (flow_x != 0 || flow_z != 0) {
            let radians = FLOW_ANGLE_DIRECTION * atan2(f32(flow_z), f32(flow_x)) + FLOW_ANGLE_OFFSET;
            let centered = (uv - vec2(0.5)) * uv_scale;
            let cosine = cos(radians);
            let sine = sin(radians);
            uv = vec2(
                centered.x * cosine - centered.y * sine,
                centered.x * sine + centered.y * cosine,
            ) + vec2(0.5);
        }
    }
    // Native falling states animate their flipbook, not a second timed UV scroll.
    return uv;
}

// Ordinary RenderChunk Transparent vertex shader (installed near-version Metal).
// Its distance parameter is native camera fade distance, not the fog end.
fn liquid_vertex_alpha(alpha: f32, camera_distance: f32, fade_distance: f32) -> f32 {
    if (fade_distance <= 0.0) { return alpha; }
    if (alpha < 0.95) {
        return mix(alpha, 1.0, clamp(camera_distance / max(fade_distance, 0.0001), 0.0, 1.0));
    }
    return alpha;
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
    let lighting_record_index = geometry_streams[liquid_word + 3u];
    let lighting_word = geometry_streams[lighting_record_index * 2u + corner / 2u];
    let light_sample = select(
        lighting_word & 0xffffu,
        lighting_word >> 16u,
        (corner & 1u) != 0u,
    );
    let face = (geometry >> 12u) & 7u;
    let local_position = liquid_corner(geometry, height_word, corner, packed_material);
    let chunk_origin = chunk_origins[draw_ref.metadata_index];
    let world_position = vec3<f32>(chunk_origin.value.xyz) + local_position;
    let block_coordinate = vec3<u32>(
        geometry & 15u,
        (geometry >> 4u) & 15u,
        (geometry >> 8u) & 15u,
    );

    let material = positional_material(packed_material & LIQUID_MATERIAL_MASK, chunk_origin.value.xyz + vec3<i32>(block_coordinate));
    let frame = animation_sample(material);

    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4(world_position, 1.0);
    out.uv = liquid_uv(
        face,
        corner,
        local_position.y - f32(block_coordinate.y),
        signed_i8(geometry, 16u),
        signed_i8(geometry, 24u),
        frame.uv_scale,
    );
    out.current_texture = frame.current;
    out.next_texture = frame.next;
    out.frame_blend = frame.blend;
    out.water_tint = blended_biome_tint(
        0x30u,
        0u,
        u32(chunk_origin.value.w),
        vec3<f32>(block_coordinate),
        vec3<f32>(chunk_origin.value.xyz),
    );
#ifndef ENHANCED
    out.water_tint.a = liquid_vertex_alpha(out.water_tint.a, distance(world_position, view.world_position), atmosphere.liquid_distance.x);
#endif
    out.lighting = light_colour(light_sample) * light_ao_factor((light_sample >> 8u) & 7u) * face_shade(face_normal(face), (light_sample & 2048u) != 0u);
    out.native_light_levels = terrain_light_levels(light_sample);
    out.native_face_shade = face_shade(face_normal(face), (light_sample & 2048u) != 0u);
#ifdef ENHANCED
    out.lighting = block_illumination(light_sample);
    out.sky_light = sky_illumination(light_sample);
    out.ambient_occlusion = light_ao_factor((light_sample >> 8u) & 7u);
#endif
    out.depth_write_route = packed_material >> 31u;
    out.two_sided = u32((packed_material & LIQUID_TWO_SIDED_BIT) != 0u);
    out.world_position = world_position;
#ifdef ENHANCED
    out.normal = face_normal(face);
    out.surface_class = material_class(packed_material & LIQUID_MATERIAL_MASK);
    if ((out.surface_class & CLASS_WATER) != 0u) {
        out.world_position = waved_water_position(world_position, out.normal.y > 0.5
            || (abs(out.normal.y) < 0.5 && local_position.y > f32(block_coordinate.y)));
        out.clip_position = view.clip_from_world * vec4(out.world_position, 1.0);
    }
#endif
    return out;
}

fn sample_texture_ref(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
#ifdef ENHANCED
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
#endif
    let layer = i32(texture_ref & 0x7ffu);
    var sampled: vec4<f32>;
    if ((texture_ref >> 31u) == 0u) {
        sampled = textureSampleGrad(block_textures_page_0, block_sampler, uv, layer, dx, dy);
    } else {
        sampled = textureSampleGrad(block_textures_page_1, block_sampler, uv, layer, dx, dy);
    }
#ifdef ENHANCED
    return sampled;
#else
    return tint_to_gamma(sampled);
#endif
}

fn apply_distance_fog(colour: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
#ifdef ENHANCED
    if (enhanced_physical_atmosphere()) { return colour; }
    let distance_to_camera = distance(world_position, view.world_position);
    let fog = clamp(
        (distance_to_camera - atmosphere.fog_color_start.w)
            / max(atmosphere.fog_end_time.x - atmosphere.fog_color_start.w, 0.0001),
        0.0,
        1.0,
    );
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
    return mix(colour, fog_colour, smoothstep(0.0, 1.0, fog));
#else
    let distance_to_camera = distance(world_position, view.world_position);
    let fog = clamp(
        (distance_to_camera - atmosphere.fog_color_start.w)
            / max(atmosphere.fog_end_time.x - atmosphere.fog_color_start.w, 0.0001),
        0.0,
        1.0,
    );
    return mix(colour, atmosphere.fog_color_start.rgb, fog);
#endif
}

#ifndef ENHANCED
// Native liquid vertex colours are byte-quantized tint * face shade, without AO.
// Ordinary RenderChunk composes texture, tint, lightmap and fog in gamma, then
// Native ordinary terrain blends these gamma values into an UNORM framebuffer.
fn native_liquid_colour(sampled_gamma: vec3<f32>, tint_linear: vec3<f32>, in: VertexOutput) -> vec3<f32> {
    let tint_gamma = tint_to_gamma(vec4(tint_linear, 1.0)).rgb;
    let face_tint = floor(clamp(tint_gamma * in.native_face_shade, vec3(0.0), vec3(1.0)) * 255.0 + vec3(0.0001)) / 255.0;
    let lit_gamma = sampled_gamma * face_tint * terrain_light_colour(in.native_light_levels);
    let fog_gamma = tint_to_gamma(vec4(atmosphere.fog_color_start.rgb, 1.0)).rgb;
    let distance_to_camera = distance(in.world_position, view.world_position);
    let fog = clamp((distance_to_camera - atmosphere.fog_color_start.w) / max(atmosphere.fog_end_time.x - atmosphere.fog_color_start.w, 0.0001), 0.0, 1.0);
    return mix(lit_gamma, fog_gamma, fog);
}
#endif

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    if (in.depth_write_route != 0u) { discard; }
    // Vanilla duplicates only faces marked by the liquid tessellator.
    // In particular, the bottom never has a reverse face at the ice surface.
    if (!front_facing && in.two_sided == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let current_sample = sample_texture_ref(in.current_texture, in.uv, dx, dy);
    var sampled = current_sample;
    if (in.frame_blend > 0.0) {
        let next_sample = sample_texture_ref(in.next_texture, in.uv, dx, dy);
        sampled = mix(current_sample, next_sample, in.frame_blend);
    }
#ifdef ENHANCED
    let water_footprint = max(length(dpdx(in.world_position.xz)), length(dpdy(in.world_position.xz)));
    if ((in.surface_class & CLASS_WATER) != 0u) {
        let water = shade_water(sampled.rgb * in.water_tint.rgb, sampled.a * in.water_tint.a, in.normal,
            in.world_position, in.clip_position, in.lighting, in.sky_light,
            in.ambient_occlusion, atmosphere.sky_zenith_rain.rgb, atmosphere.sky_horizon_thunder.rgb, water_footprint);
        return vec4(apply_distance_fog(water.rgb, in.world_position), water.a);
    }
    let colour = shade_surface(
        sampled.rgb * in.water_tint.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion,
        in.surface_class,
        in.normal,
        vec3(0.0, 0.8, 0.8),
    );
    // The background is fogged by the same transfer, so preserving source
    // alpha composes to one fog application instead of double-counting it.
    return vec4(apply_distance_fog(colour, in.world_position), sampled.a * in.water_tint.a);
#else
#ifdef NATIVE_GAMMA_BLEND
    return vec4(native_liquid_colour(sampled.rgb, in.water_tint.rgb, in), sampled.a * in.water_tint.a);
#else
    return tint_to_linear(vec4(native_liquid_colour(sampled.rgb, in.water_tint.rgb, in), sampled.a * in.water_tint.a));
#endif
#endif
}

@fragment
fn fragment_depth(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    if (in.depth_write_route == 0u) { discard; }
    if (!front_facing && in.two_sided == 0u) { discard; }
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let current_sample = sample_texture_ref(in.current_texture, in.uv, dx, dy);
    var sampled = current_sample;
    if (in.frame_blend > 0.0) {
        let next_sample = sample_texture_ref(in.next_texture, in.uv, dx, dy);
        sampled = mix(current_sample, next_sample, in.frame_blend);
    }
#ifdef ENHANCED
    let lit = shade_surface(
        sampled.rgb,
        in.normal,
        in.world_position,
        in.clip_position.xy,
        in.lighting,
        in.sky_light,
        in.ambient_occlusion,
        in.surface_class,
        in.normal,
        vec3(0.0, 0.8, 0.8),
    );
    return vec4(apply_distance_fog(lit, in.world_position), 1.0);
#else
    return tint_to_linear(vec4(native_liquid_colour(sampled.rgb, vec3(1.0), in), 1.0));
#endif
}
