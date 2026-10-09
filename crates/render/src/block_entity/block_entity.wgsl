#import bevy_render::view::View
#import cinnabar::lighting::{actor_lighting, actor_distance_fog, tint_to_gamma, tint_to_linear}
#ifdef ALPHA_TO_COVERAGE
#import cinnabar::lighting::{cutout_alpha_2d}
#endif

// Packed Rust BlockEntityVertex: position, atlas UV, RGBA, world normal, actor light.
const BLOCK_ENTITY_ALPHA_THRESHOLD: f32 = 0.5;

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> vertex_words: array<u32>;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;
struct PortalParameters {
    star_rect: vec4<f32>,
    time: vec4<f32>,
    fog_color_start: vec4<f32>,
    fog_end: vec4<f32>,
}
@group(0) @binding(4) var<uniform> portal: PortalParameters;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) native_lighting: vec3<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) @interpolate(flat) actor_light: u32,
}

fn vertex_f32(index: u32) -> f32 { return bitcast<f32>(vertex_words[index]); }

fn block_vertex(vertex_index: u32, overlay_bias: bool) -> VertexOutput {
    let base = vertex_index * BLOCK_ENTITY_VERTEX_WORDS;
    var position = vec3(vertex_f32(base), vertex_f32(base + 1u), vertex_f32(base + 2u));
    let actor_light = vertex_words[base + 12u];
    let normal = vec3(vertex_f32(base + 9u), vertex_f32(base + 10u), vertex_f32(base + 11u));
    if (overlay_bias && actor_light == 0u && any(normal != vec3(0.0))) {
        let direction = select(-1.0, 1.0, dot(view.world_position - position, normal) >= 0.0);
        position += normal * (direction * BLOCK_OVERLAY_FACE_OFFSET);
    }
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(position, 1.0);
    out.uv = vec2(vertex_f32(base + 3u), vertex_f32(base + 4u));
    out.color = vec4(
        vertex_f32(base + 5u),
        vertex_f32(base + 6u),
        vertex_f32(base + 7u),
        vertex_f32(base + 8u),
    );
    out.world_position = position;
    out.actor_light = actor_light;
    out.native_lighting = actor_lighting(out.actor_light, normal, 0.0);
    return out;
}

@vertex
fn block_entity_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    return block_vertex(vertex_index, false);
}

@vertex
fn block_overlay_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    return block_vertex(vertex_index, true);
}

struct SelectionLineOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

const SELECTION_STROKE_PIXELS: f32 = 2.0;

fn selection_endpoint(index: u32) -> vec4<f32> {
    let base = index * BLOCK_ENTITY_VERTEX_WORDS;
    let world = vec3(vertex_f32(base), vertex_f32(base + 1u), vertex_f32(base + 2u));
    var clip = view.clip_from_world * vec4(world, 1.0);
    clip.z += 0.00005;
    return clip;
}

@vertex
fn selection_line_vertex(@builtin(vertex_index) vertex_index: u32) -> SelectionLineOutput {
    let first = vertex_index / BLOCK_SELECTION_VERTICES_PER_EDGE * 2u;
    var a = selection_endpoint(first);
    var b = selection_endpoint(first + 1u);
    // Clip the centerline before dividing by w, including a camera inside the box.
    let da = a.w - a.z;
    let db = b.w - b.z;
    var out: SelectionLineOutput;
    if (da < 0.0 && db < 0.0) {
        out.position = vec4(0.0, 0.0, -1.0, 1.0);
        out.color = vec4(0.0);
        return out;
    }
    if (da < 0.0) { a = mix(a, b, da / (da - db)); }
    else if (db < 0.0) { b = mix(b, a, db / (db - da)); }
    let direction = (b.xy / b.w - a.xy / a.w) * view.viewport.zw;
    let length_squared = dot(direction, direction);
    if (length_squared < 0.000001) {
        out.position = vec4(0.0, 0.0, -1.0, 1.0);
        out.color = vec4(0.0);
        return out;
    }
    let perpendicular = vec2(-direction.y, direction.x) * inverseSqrt(length_squared);
    let corner = array(0u, 1u, 2u, 0u, 2u, 3u)[vertex_index % BLOCK_SELECTION_VERTICES_PER_EDGE];
    var endpoint = select(a, b, corner == 1u || corner == 2u);
    let sign = select(-1.0, 1.0, corner >= 2u);
    endpoint = vec4(endpoint.xy + perpendicular * sign * SELECTION_STROKE_PIXELS / view.viewport.zw * endpoint.w, endpoint.zw);
    out.position = endpoint;
    let base = first * BLOCK_ENTITY_VERTEX_WORDS;
    out.color = vec4(vertex_f32(base + 5u), vertex_f32(base + 6u), vertex_f32(base + 7u), vertex_f32(base + 8u));
    return out;
}

@fragment
fn selection_line_fragment(input: SelectionLineOutput) -> @location(0) vec4<f32> {
    return input.color;
}

@fragment
fn block_entity_solid(input: VertexOutput) -> @location(0) vec4<f32> {
    var texel = textureSample(atlas, atlas_sampler, input.uv);
#ifdef ALPHA_TO_COVERAGE
    let cutout = cutout_alpha_2d(atlas, input.uv, dpdx(input.uv), dpdy(input.uv), texel, BLOCK_ENTITY_ALPHA_THRESHOLD);
    let coverage = cutout.coverage;
    if (texel.a < BLOCK_ENTITY_ALPHA_THRESHOLD && coverage > 0.0) { texel = cutout.colour; }
#else
    if (texel.a < BLOCK_ENTITY_ALPHA_THRESHOLD) {
        discard;
    }
    let coverage = 1.0;
#endif
    if (input.actor_light != 0u) {
        // Native mob_head:entity_alphatest uses Fancy world-normal shading and
        // the shared lightmap, composed in gamma before the output transfer.
        let gamma = tint_to_gamma(texel).rgb * input.color.rgb * input.native_lighting;
        return tint_to_linear(vec4(actor_distance_fog(gamma, input.world_position, view.world_position), coverage));
    }
    return vec4(texel.rgb * input.color.rgb, coverage);
}

@fragment
fn block_entity_overlay(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.uv.x < 0.0) { return input.color; }
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    let alpha = texel.a * input.color.a;
    if (alpha < 0.02) {
        discard;
    }
    return vec4(texel.rgb * input.color.rgb, alpha);
}

@fragment
fn block_entity_additive(input: VertexOutput) -> @location(0) vec4<f32> {
    let gamma = input.color.rgb * input.native_lighting;
    return tint_to_linear(vec4(actor_distance_fog(gamma, input.world_position, view.world_position), input.color.a));
}

@fragment
fn block_entity_crack(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.uv.x < 0.0) { return input.color; }
    let texel = textureSample(atlas, atlas_sampler, input.uv);
    if (texel.a < 0.02) {
        discard;
    }
    return vec4(texel.rgb, 1.0);
}

struct PortalOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) color_uv: vec2<f32>,
    @location(1) @interpolate(perspective, centroid) parallax_uv: vec2<f32>,
    @location(2) phase: f32,
    @location(3) fog: f32,
}

@vertex
fn portal_vertex(@builtin(vertex_index) vertex_index: u32) -> PortalOutput {
    let base = vertex_index * BLOCK_ENTITY_VERTEX_WORDS;
    let world_position = vec3(vertex_f32(base), vertex_f32(base + 1u), vertex_f32(base + 2u));
    let encoded_normal = vec3(vertex_f32(base + 5u), vertex_f32(base + 6u), vertex_f32(base + 7u));
    let normal = (encoded_normal - vec3(0.5)) * 2.0;
    let phase = vertex_f32(base + 8u);
    let depth = phase * 32.0;
    let ray = world_position - view.world_position;
    let intersection = dot(ray - depth * normal, normal) / dot(ray, normal);
    let projected = intersection * ray + view.world_position;
    let mask = abs(normal);
    let plane_uv = (projected.yz * mask.x + projected.xz * mask.y + projected.xy * mask.z) / 16.0;
    let angle = depth * 2.2439947;
    let sine = sin(angle);
    let cosine = cos(angle);
    var uv = vec2(plane_uv.x * cosine + plane_uv.y * sine, -plane_uv.x * sine + plane_uv.y * cosine);
    uv += vec2(cosine, sine) * depth;
    uv.y += portal.time.x / 256.0;
    // Native fmod keeps the sign; atlas wrapping happens after interpolation.
    uv -= trunc(uv / 64.0) * 64.0;
    var out: PortalOutput;
    out.position = view.clip_from_world * vec4(world_position, 1.0);
    out.color_uv = vec2(vertex_f32(base + 3u), vertex_f32(base + 4u));
    out.parallax_uv = uv;
    out.phase = phase;
    out.fog = clamp((length(ray) - portal.fog_color_start.w) / max(portal.fog_end.x - portal.fog_color_start.w, 0.0001), 0.0, 1.0);
    return out;
}

@fragment
fn portal_fragment(input: PortalOutput) -> @location(0) vec4<f32> {
    // One/OneMinusSrcAlpha clears the surface with the base, then adds the stars.
    if (input.phase * 32.0 > 31.0) {
        return vec4(portal.fog_color_start.rgb * input.fog, 1.0);
    }
    let uv = portal.star_rect.xy + fract(input.parallax_uv) * portal.star_rect.zw;
    let stars = textureSample(atlas, atlas_sampler, uv).rgb;
    let palette = textureSample(atlas, atlas_sampler, input.color_uv).rgb;
    return vec4(stars * palette * (1.0 - input.phase) * (1.0 - input.fog), 0.0);
}
