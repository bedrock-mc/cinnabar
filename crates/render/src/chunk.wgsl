#import cinnabar::material::{MaterialGpu, materials, positional_material, material_face_is_visible, material_uses_native_leaf_colour, material_uses_overlay_mask, material_leaf_uv_flags, material_leaf_shade}
#ifdef ENHANCED_SHADOW
#import cinnabar::enhanced_caster::caster_clip
#endif
#import bevy_render::view::View
#import cinnabar::biome_tint::{blended_biome_tint, blended_biome_tint_gamma, uniform_biome_tint_gamma}
#import cinnabar::lighting::{light_ao_factor, light_colour, lit_colour, material_ambient_occlusion, material_face_shade, tint_to_gamma, tint_to_linear, terrain_light_levels, terrain_light_colour}
#ifdef ALPHA_TO_COVERAGE
#import cinnabar::lighting::{cutout_alpha_array, cutout_coverage}
#endif
#ifdef ENHANCED
#import cinnabar::enhanced_view::{sky_illumination, material_class, shade_surface, waved_position}
#endif

struct PackedQuad {
    geometry: u32,
    material_id: u32,
}

struct ChunkOrigin {
    value: vec4<i32>,
    cube_bases: vec4<u32>,
}


// ANIMATION_GPU_LAYOUT

struct AnimationClockGpu {
    tick: u32,
    partial_tick: f32,
    padding_0: u32,
    padding_1: u32,
}

struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>,
    moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>,
    sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>,
    fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>,
    sky_extra: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> quads: array<PackedQuad>;
@group(0) @binding(2) var<storage, read> chunk_origins: array<ChunkOrigin>;
@group(0) @binding(4) var block_textures_page_0: texture_2d_array<f32>;
@group(0) @binding(5) var block_textures_page_1: texture_2d_array<f32>;
@group(0) @binding(6) var block_sampler: sampler;
@group(0) @binding(9) var<storage, read> animations: array<AnimationGpu>;
@group(0) @binding(10) var<storage, read> animation_frames: array<u32>;
@group(0) @binding(11) var<uniform> clock: AnimationClockGpu;
@group(0) @binding(13) var<storage, read> geometry_streams: array<u32>;
@group(0) @binding(15) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(NATIVE_LEAF_TEXTURE_BINDING_0) var native_leaf_textures_page_0: texture_2d_array<f32>;
@group(0) @binding(NATIVE_LEAF_TEXTURE_BINDING_1) var native_leaf_textures_page_1: texture_2d_array<f32>;
@group(0) @binding(NATIVE_LEAF_SAMPLER_BINDING) var native_leaf_sampler: sampler;

struct AnimationFrameSampleGpu {
    current_texture: u32,
    next_texture: u32,
    blend: f32,
}

fn select_animation_frames_gpu(material: MaterialGpu) -> AnimationFrameSampleGpu {
    if (material.animation == 0xffffffffu) {
        return AnimationFrameSampleGpu(material.texture, material.texture, 0.0);
    }
    let animation = animations[material.animation];
    let current_index =
        (clock.tick / animation.ticks_per_frame) % animation.frame_count;
    let current_texture = animation_frames[animation.frame_start + current_index];
    if ((animation.flags & 1u) == 0u || animation.frame_count == 1u) {
        return AnimationFrameSampleGpu(current_texture, current_texture, 0.0);
    }
    let next_index = (current_index + 1u) % animation.frame_count;
    let next_texture = animation_frames[animation.frame_start + next_index];
    let frame_tick = clock.tick % animation.ticks_per_frame;
    let blend = (f32(frame_tick) + clamp(clock.partial_tick, 0.0, 0.99999994)) /
        f32(animation.ticks_per_frame);
    return AnimationFrameSampleGpu(current_texture, next_texture, blend);
}

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
    @location(8) world_position: vec3<f32>,
    @location(9) lighting: vec3<f32>,
#ifdef ENHANCED
    @location(10) sky_light: f32,
    @location(11) ambient_occlusion: f32,
    @location(12) @interpolate(flat) surface_class: u32,
#else
    @location(10) native_light_levels: vec2<f32>,
    @location(11) native_ao_face: f32,
    @location(12) @interpolate(flat) uniform_tint_gamma: vec4<f32>,
#endif
}

fn quad_corner(face: u32, corner: u32, origin: vec3<f32>, width: f32, height: f32) -> vec3<f32> {
    var corners = array<vec3<f32>, 4>(origin, origin, origin, origin);
    switch face {
        case 0u: {
            corners = array<vec3<f32>, 4>(
                origin,
                origin + vec3(0.0, 0.0, width),
                origin + vec3(0.0, height, width),
                origin + vec3(0.0, height, 0.0),
            );
        }
        case 1u: {
            let base = origin + vec3(1.0, 0.0, 0.0);
            corners = array<vec3<f32>, 4>(
                base,
                base + vec3(0.0, height, 0.0),
                base + vec3(0.0, height, width),
                base + vec3(0.0, 0.0, width),
            );
        }
        case 2u: {
            corners = array<vec3<f32>, 4>(
                origin,
                origin + vec3(width, 0.0, 0.0),
                origin + vec3(width, 0.0, height),
                origin + vec3(0.0, 0.0, height),
            );
        }
        case 3u: {
            let base = origin + vec3(0.0, 1.0, 0.0);
            corners = array<vec3<f32>, 4>(
                base,
                base + vec3(0.0, 0.0, height),
                base + vec3(width, 0.0, height),
                base + vec3(width, 0.0, 0.0),
            );
        }
        case 4u: {
            corners = array<vec3<f32>, 4>(
                origin,
                origin + vec3(0.0, height, 0.0),
                origin + vec3(width, height, 0.0),
                origin + vec3(width, 0.0, 0.0),
            );
        }
        default: {
            let base = origin + vec3(0.0, 0.0, 1.0);
            corners = array<vec3<f32>, 4>(
                base,
                base + vec3(width, 0.0, 0.0),
                base + vec3(width, height, 0.0),
                base + vec3(0.0, height, 0.0),
            );
        }
    }
    return corners[corner];
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

fn greedy_uv(face: u32, corner: u32, width: f32, height: f32, flags: u32) -> vec2<f32> {
    // Native cube UV axes: West +Z/-Y, East -Z/-Y, North -X/-Y,
    // South +X/-Y, Down +X/-Z, Up +X/+Z. Opposing cutout faces
    // must not share the same projected alpha mask.
    let horizontal_standard = array<vec2<f32>, 4>(
        vec2(0.0, height), vec2(width, height),
        vec2(width, 0.0), vec2(0.0, 0.0),
    );
    let horizontal_transposed = array<vec2<f32>, 4>(
        vec2(0.0, 0.0), vec2(0.0, height),
        vec2(width, height), vec2(width, 0.0),
    );
    let vertical_standard = array<vec2<f32>, 4>(
        vec2(0.0, height), vec2(width, height),
        vec2(width, 0.0), vec2(0.0, 0.0),
    );
    let vertical_transposed = array<vec2<f32>, 4>(
        vec2(width, height), vec2(width, 0.0),
        vec2(0.0, 0.0), vec2(0.0, height),
    );
    var uv = horizontal_standard[corner];
    switch face {
        case 0u, 5u: { uv = vertical_standard[corner]; }
        case 1u, 4u: { uv = vertical_transposed[corner]; }
        case 3u: { uv = horizontal_transposed[corner]; }
        default: {}
    }

    var extents = vec2(width, height);
    switch flags & 3u {
        case 1u: {
            uv = vec2(uv.y, width - uv.x);
            extents = vec2(height, width);
        }
        case 2u: {
            uv = vec2(width - uv.x, height - uv.y);
        }
        case 3u: {
            uv = vec2(height - uv.y, uv.x);
            extents = vec2(height, width);
        }
        default: {}
    }
    if ((flags & 4u) != 0u) {
        uv.x = extents.x - uv.x;
    }
    if ((flags & 8u) != 0u) {
        uv.y = extents.y - uv.y;
    }
    return uv;
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    return cube_vertex(vertex_index, instance_index);
}

// `vertex_index / 4` selects the chunk origin and `instance_index` the packed quad.
fn cube_vertex(vertex_index: u32, instance_index: u32) -> VertexOutput {
    let quad = quads[instance_index];
    let geometry = quad.geometry;
    let local_origin = vec3<f32>(
        f32(geometry & 0x1fu),
        f32((geometry >> 5u) & 0x1fu),
        f32((geometry >> 10u) & 0x1fu),
    );
    let face = (geometry >> 15u) & 0x7u;
    let width = f32(((geometry >> 18u) & 0xfu) + 1u);
    let height = f32(((geometry >> 22u) & 0xfu) + 1u);
    let metadata_index = vertex_index / 4u;
    let corner = vertex_index & 3u;
    let chunk_origin = chunk_origins[metadata_index];
    let local_quad_index = instance_index - chunk_origin.cube_bases.x;
    let lighting_record_index = chunk_origin.cube_bases.y + local_quad_index;
    let lighting_word = geometry_streams[lighting_record_index * 2u + corner / 2u];
    let light_sample = select(
        lighting_word & 0xffffu,
        lighting_word >> 16u,
        (corner & 1u) != 0u,
    );
    let local_position = quad_corner(face, corner, local_origin, width, height);
    let world_position = vec3<f32>(chunk_origin.value.xyz) + local_position;
    let material = positional_material(quad.material_id, chunk_origin.value.xyz + vec3<i32>(local_origin));
    let animation_sample = select_animation_frames_gpu(material);

    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4(world_position, 1.0);
#ifdef ENHANCED_SHADOW
    out.clip_position = caster_clip(world_position, quad.material_id, 1.0);
#endif
    let uv_flags = material_leaf_uv_flags(material.flags, chunk_origin.value.xyz + vec3<i32>(local_origin));
    out.uv = greedy_uv(face, corner, width, height, uv_flags);
    out.current_texture = animation_sample.current_texture;
    out.normal = face_normal(face);
    out.material_flags = material.flags;
    out.local_position = local_position;
    out.biome_record = u32(chunk_origin.value.w);
    out.next_texture = animation_sample.next_texture;
    out.frame_blend = animation_sample.blend;
    out.world_position = world_position;
    let ao = material_ambient_occlusion(light_ao_factor((light_sample >> 8u) & 7u), material.flags);
    let dimming = material_face_shade(out.normal, (light_sample & 2048u) != 0u, material.flags);
    out.lighting = light_colour(light_sample) * ao * dimming;
#ifdef ENHANCED
    out.sky_light = sky_illumination(light_sample);
    out.ambient_occlusion = ao;
#else
    // Native RenderChunk samples its gamma lightmap after applying vertex AO.
    // Retain separate level and AO interpolants: table lookup occurs in the
    // fragment stage, not before interpolating its nonlinear RGB output.
    out.native_light_levels = terrain_light_levels(light_sample);
    out.native_ao_face = material_leaf_shade(ao * dimming, material.flags);
    out.uniform_tint_gamma = vec4(0.0);
#ifndef ENHANCED_SHADOW
#ifndef OPAQUE_OVERDRAW
    out.uniform_tint_gamma = uniform_biome_tint_gamma(material.flags & 0x30u, material.flags, out.biome_record);
#endif
#endif
#endif
#ifdef ENHANCED
    out.surface_class = material_class(quad.material_id);
    out.world_position = waved_position(world_position, out.surface_class, 1.0);
    out.clip_position = view.clip_from_world * vec4(out.world_position, 1.0);
#endif
    return out;
}

fn apply_material_tint(
    sampled: vec4<f32>,
    material_flags: u32,
    biome_record: u32,
    local_position: vec3<f32>,
    normal: vec3<f32>,
    world_origin: vec3<f32>,
) -> vec4<f32> {
    let tint_kind = material_flags & 0x30u;
    if (tint_kind != 0u) {
        let tinted = sampled.rgb * blended_biome_tint(
            tint_kind,
            material_flags,
            biome_record,
            local_position - normal * 0.001,
            world_origin,
        ).rgb;
        if (material_uses_overlay_mask(material_flags)) {
            // Grass-side alpha is an overlay weight, not transparency. Its
            // alpha-zero RGB contains the opaque dirt base.
            return vec4(mix(sampled.rgb, tinted, sampled.a), 1.0);
        }
        return vec4(tinted, 1.0);
    }
    return vec4(sampled.rgb, 1.0);
}

fn sample_texture_ref(
    texture_ref: u32,
    uv: vec2<f32>,
    uv_dx: vec2<f32>,
    uv_dy: vec2<f32>,
) -> vec4<f32> {
    let page = texture_ref >> 31u;
    let layer = i32(texture_ref & 0x7ffu);
#ifdef ENHANCED
    if (page == 0u) {
        return textureSampleGrad(block_textures_page_0, block_sampler, uv, layer, uv_dx, uv_dy);
    }
    return textureSampleGrad(block_textures_page_1, block_sampler, uv, layer, uv_dx, uv_dy);
#else
    // Mips and animation frames interpolate the original encoded texture bytes.
    if (page == 0u) {
        return textureSampleGrad(native_leaf_textures_page_0, block_sampler, uv, layer, uv_dx, uv_dy);
    }
    return textureSampleGrad(native_leaf_textures_page_1, block_sampler, uv, layer, uv_dx, uv_dy);
#endif
}

fn sample_material_texture_ref(
    texture_ref: u32,
    uv: vec2<f32>,
    uv_dx: vec2<f32>,
    uv_dy: vec2<f32>,
    material_flags: u32,
) -> vec4<f32> {
#ifdef ENHANCED
#else
    if (material_uses_native_leaf_colour(material_flags)) {
        let layer = i32(texture_ref & 0x7ffu);
        if ((texture_ref >> 31u) == 0u) {
            return textureSampleGrad(native_leaf_textures_page_0, native_leaf_sampler, uv, layer, uv_dx, uv_dy);
        }
        return textureSampleGrad(native_leaf_textures_page_1, native_leaf_sampler, uv, layer, uv_dx, uv_dy);
    }
#endif
    return sample_texture_ref(texture_ref, uv, uv_dx, uv_dy);
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

// Current atlas creation/upload uses RGBA8_UNORM,
// and vanilla Metal swapchain uses BGRA8_UNORM.
// Installed 1.26.51.01 RenderChunk Metal applies palette, AO and lightmap,
// then gamma fog, without a transfer function. Preserve Bevy's existing
// sRGB texture/output contract at the renderer boundary, not in its lighting.
fn native_leaf_colour(
    texture_gamma: vec3<f32>,
    tint_gamma: vec3<f32>,
    ao_face: f32,
    lightmap_gamma: vec3<f32>,
    fog_linear: vec3<f32>,
    fog_amount: f32,
) -> vec3<f32> {
    let fog_gamma = tint_to_gamma(vec4(fog_linear, 1.0)).rgb;
    let lit_gamma = ((texture_gamma * tint_gamma) * ao_face) * lightmap_gamma;
    return tint_to_linear(vec4(mix(lit_gamma, fog_gamma, fog_amount), 1.0)).rgb;
}

// Terrain lighting operates on encoded RGB before the framebuffer conversion.
fn native_cube_colour(
    texture: vec4<f32>, flags: u32, tint_gamma: vec3<f32>,
    ao_face: f32, lightmap_gamma: vec3<f32>, fog_linear: vec3<f32>, fog_amount: f32,
) -> vec4<f32> {
    let texture_gamma = texture.rgb;
    // Current atlas overlay and installed near-version opaque
    // RenderChunk agree: alpha masks the RGB tint, then output is opaque.
    // Apply this in the same gamma domain as the native terrain product.
    let overlay = material_uses_overlay_mask(flags);
    let masked_tint = select(tint_gamma, mix(vec3(1.0), tint_gamma, texture.a), overlay);
    let colour = native_leaf_colour(texture_gamma, masked_tint, ao_face, lightmap_gamma, fog_linear, fog_amount);
    let alpha = select(texture.a, 1.0, overlay || material_uses_native_leaf_colour(flags));
    return vec4(colour, alpha);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    if (!material_face_is_visible(in.material_flags, front)) { discard; }
    let sampled = sample_cube_texture(in, uv_dx, uv_dy);
#ifndef ALPHA_TO_COVERAGE
    if ((in.material_flags & (1u << 8u)) != 0u && sampled.a < 0.5) {
        discard;
    }
#endif
#ifdef OPAQUE_OVERDRAW
    return vec4(1.0);
#else
    let shaded = shade_cube(in, sampled);
#ifdef ALPHA_TO_COVERAGE
    // Opaque overlay alpha weights tint; only cutout materials contribute coverage.
    var coverage = 1.0;
    if ((in.material_flags & (1u << 8u)) != 0u) {
        var footprint = cube_alpha_footprint(in.current_texture, in.uv, uv_dx, uv_dy, sampled.a, in.material_flags);
        if (in.frame_blend > 0.0) {
            footprint = mix(footprint, cube_alpha_footprint(in.next_texture, in.uv, uv_dx, uv_dy, sampled.a, in.material_flags), in.frame_blend);
        }
        coverage = cutout_coverage(footprint, 0.5);
    }
    return vec4(shaded.rgb, coverage);
#else
    return shaded;
#endif
#endif
}

#ifdef ALPHA_TO_COVERAGE
// Coverage reads the same page and layer as the nearest RGB texture sample.
fn cube_alpha_footprint(texture_ref: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, sampled_alpha: f32, flags: u32) -> vec2<f32> {
    let layer = i32(texture_ref & 0x7ffu);
#ifdef ENHANCED
    if ((texture_ref >> 31u) == 0u) { return cutout_alpha_array(block_textures_page_0, uv, layer, dx, dy, sampled_alpha, true); }
    return cutout_alpha_array(block_textures_page_1, uv, layer, dx, dy, sampled_alpha, true);
#else
    if ((texture_ref >> 31u) == 0u) { return cutout_alpha_array(native_leaf_textures_page_0, uv, layer, dx, dy, sampled_alpha, !material_uses_native_leaf_colour(flags)); }
    return cutout_alpha_array(native_leaf_textures_page_1, uv, layer, dx, dy, sampled_alpha, !material_uses_native_leaf_colour(flags));
#endif
}
#endif

// Single-sided opaque runs: back-face culling and the mesher's material partition
// stand in for both discards, keeping early depth and hidden-surface removal.
@fragment
fn fragment_solid(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef OPAQUE_OVERDRAW
    return vec4(1.0);
#else
    return shade_cube(in, sample_cube_texture(in, dpdx(in.uv), dpdy(in.uv)));
#endif
}

fn sample_cube_texture(in: VertexOutput, uv_dx: vec2<f32>, uv_dy: vec2<f32>) -> vec4<f32> {
    let current_sample = sample_material_texture_ref(in.current_texture, in.uv, uv_dx, uv_dy, in.material_flags);
    var sampled = current_sample;
    if (in.frame_blend > 0.0) {
        let next_sample = sample_material_texture_ref(in.next_texture, in.uv, uv_dx, uv_dy, in.material_flags);
        sampled = mix(current_sample, next_sample, in.frame_blend);
    }
    return sampled;
}

#ifndef ENHANCED
// Mixed records and position-noise grass keep their original per-block fragment lookup.
fn ordinary_cube_tint_gamma(in: VertexOutput) -> vec3<f32> {
    if (in.uniform_tint_gamma.a != 0.0) { return in.uniform_tint_gamma.rgb; }
    var tint_gamma = vec3(1.0);
    let tint_kind = in.material_flags & 0x30u;
    if (tint_kind != 0u) {
        tint_gamma = blended_biome_tint_gamma(
            tint_kind,
            in.material_flags,
            in.biome_record,
            in.local_position - in.normal * 0.001,
            in.world_position - in.local_position,
        ).rgb;
    }
    return tint_gamma;
}
#endif

fn shade_cube(in: VertexOutput, sampled: vec4<f32>) -> vec4<f32> {
#ifdef ENHANCED
    let colour = apply_material_tint(
        sampled,
        in.material_flags,
        in.biome_record,
        in.local_position,
        in.normal,
        in.world_position - in.local_position,
    );
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
    let tint_gamma = ordinary_cube_tint_gamma(in);
    let native_colour = native_cube_colour(
        sampled,
        in.material_flags,
        tint_gamma,
        in.native_ao_face,
        terrain_light_colour(in.native_light_levels),
        atmosphere.fog_color_start.rgb,
        distance_fog_amount(in.world_position),
    );
    return native_colour;
#endif
}
#ifdef ENHANCED_SHADOW

// Alpha-tested terrain depth; opaque texels cast independently of baked light.
@fragment
fn fragment_shadow(in: VertexOutput, @builtin(front_facing) front: bool) {
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    if (!material_face_is_visible(in.material_flags, front)) { discard; }
    var sampled = sample_material_texture_ref(in.current_texture, in.uv, dx, dy, in.material_flags);
    if (in.frame_blend > 0.0) {
        sampled = mix(sampled, sample_material_texture_ref(in.next_texture, in.uv, dx, dy, in.material_flags), in.frame_blend);
    }
    if ((in.material_flags & (1u << 8u)) != 0u && sampled.a < 0.5) { discard; }
}
#endif
