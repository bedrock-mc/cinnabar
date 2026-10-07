// Full-screen camera overlays composited back to front in one pass.
// Fire follows the native cube and admitted texture timeline. Other procedural
// patterns remain provisional and need native measurement.
// Portal: vanilla's full-screen effect unit cube and the active atlas flipbook.

struct Layer {
    // rgb tint, alpha
    color: vec4<f32>,
    // kind, unused x3
    params: vec4<f32>,
}

struct Overlays {
    // layer count, clock seconds, textures present (0/1), unused
    header: vec4<f32>,
    // current frame, next frame, frame blend, fire present
    fire: vec4<f32>,
    // perspective ray scales X/Y
    projection: vec4<f32>,
    // current layer, next layer, frame blend, portal texture present
    portal_frames: vec4<f32>,
    portal_from_clip: mat4x4<f32>,
    layers: array<Layer, 8>,
}

@group(0) @binding(0) var<uniform> overlays: Overlays;
@group(0) @binding(1) var overlay_textures: texture_2d_array<f32>;
@group(0) @binding(2) var overlay_sampler: sampler;
@group(0) @binding(3) var fire_textures: texture_2d_array<f32>;
@group(0) @binding(4) var fire_sampler: sampler;
@group(0) @binding(5) var portal_textures: texture_2d_array<f32>;
@group(0) @binding(6) var portal_sampler: sampler;

const KIND_POWDER_SNOW: u32 = 1u;
const KIND_FIRE: u32 = 2u;
const KIND_PORTAL: u32 = 3u;
const KIND_PUMPKIN: u32 = 4u;
const KIND_SPYGLASS: u32 = 5u;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn overlay_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(cell);
    let b = hash(cell + vec2<f32>(1.0, 0.0));
    let c = hash(cell + vec2<f32>(0.0, 1.0));
    let d = hash(cell + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Five cube faces, Y rotation 45 degrees, translation (0,-.5,0), scale .7.
// Camera-ray intersection preserves perspective UVs and the open top.
fn camera_fire(uv: vec2<f32>) -> vec4<f32> {
    if overlays.fire.w < 0.5 {
        return vec4<f32>(0.0);
    }
    let ray = vec3<f32>((uv.x * 2.0 - 1.0) * overlays.projection.x,
                       (1.0 - uv.y * 2.0) * overlays.projection.y, 1.0);
    let c = sqrt(0.5);
    let direction = vec3<f32>(c * (ray.x - ray.z), ray.y, c * (ray.x + ray.z));
    let origin = vec3<f32>(0.0, 0.5 / 0.7, 0.0);
    let boundary = select(vec3<f32>(-1.0), vec3<f32>(1.0), direction >= vec3<f32>(0.0));
    let divisor = select(vec3<f32>(1.0e-10), direction, abs(direction) > vec3<f32>(1.0e-10));
    let exit = (boundary - origin) / divisor;
    let t = min(min(exit.x, exit.y), exit.z);
    let p = origin + direction * t;
    var texture_uv: vec2<f32>;
    if exit.y <= min(exit.x, exit.z) {
        if direction.y > 0.0 {
            return vec4<f32>(0.0);
        }
        texture_uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.z) * 0.5);
    } else if exit.x < exit.z {
        texture_uv = vec2<f32>((p.z + 1.0) * 0.5, (1.0 - p.y) * 0.5);
        if direction.x > 0.0 {
            texture_uv = vec2<f32>(texture_uv.x, 1.0 - texture_uv.y);
        }
    } else {
        texture_uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
        if direction.z < 0.0 {
            texture_uv = vec2<f32>(texture_uv.x, 1.0 - texture_uv.y);
        }
    }
    let current = textureSampleLevel(fire_textures, fire_sampler, texture_uv, i32(overlays.fire.x), 0.0);
    let next = textureSampleLevel(fire_textures, fire_sampler, texture_uv, i32(overlays.fire.y), 0.0);
    return mix(current, next, overlays.fire.z);
}

// Six unit-cube quads share the repeated portal texture tile.
fn portal_cube_uv(uv: vec2<f32>) -> vec2<f32> {
    let ray = (overlays.portal_from_clip * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 1.0, 1.0)).xyz;
    let magnitude = abs(ray);
    let hit = ray / max(max(magnitude.x, magnitude.y), magnitude.z);
    if magnitude.x >= magnitude.y && magnitude.x >= magnitude.z {
        return vec2<f32>(select(hit.z, -hit.z, ray.x > 0.0), -hit.y) * 0.5 + vec2<f32>(0.5);
    }
    if magnitude.y >= magnitude.z {
        return vec2<f32>(hit.x, -hit.z) * 0.5 + vec2<f32>(0.5);
    }
    return vec2<f32>(hit.x, -hit.y) * 0.5 + vec2<f32>(0.5);
}

// Returns rgb and coverage for one layer before its own alpha scale.
fn shade(kind: u32, tint: vec3<f32>, uv: vec2<f32>, aspect: f32, clock: f32, textured: bool) -> vec4<f32> {
    let centred = uv - vec2<f32>(0.5);
    let round_r = length(centred * vec2<f32>(aspect, 1.0));
    var result = vec4<f32>(tint, 1.0);
    if kind == KIND_POWDER_SNOW {
        result = vec4<f32>(tint, smoothstep(0.3, 0.75, round_r * 1.4));
    } else if kind == KIND_FIRE {
        let flame = camera_fire(uv);
        result = vec4<f32>(flame.rgb * tint, flame.a);
    } else if kind == KIND_PORTAL {
        let portal_uv = portal_cube_uv(uv);
        let current = textureSampleLevel(portal_textures, portal_sampler, portal_uv, i32(overlays.portal_frames.x), 0.0);
        let next = textureSampleLevel(portal_textures, portal_sampler, portal_uv, i32(overlays.portal_frames.y), 0.0);
        let texel = mix(current, next, overlays.portal_frames.z);
        result = vec4<f32>(texel.rgb * tint, texel.a * overlays.portal_frames.w);
    } else if kind == KIND_PUMPKIN {
        let texel = textureSampleLevel(overlay_textures, overlay_sampler, uv, 0, 0.0);
        let procedural = vec4<f32>(0.04, 0.02, 0.0, smoothstep(0.35, 0.8, round_r));
        result = select(procedural, vec4<f32>(texel.rgb * tint, texel.a), textured);
    } else if kind == KIND_SPYGLASS {
        // A square scope sized to the view height, centred, with black side bars.
        let scope = vec2<f32>(centred.x * aspect + 0.5, uv.y);
        let inside = scope.x >= 0.0 && scope.x <= 1.0;
        let texel = textureSampleLevel(overlay_textures, overlay_sampler, scope, 1, 0.0);
        let procedural = vec4<f32>(0.0, 0.0, 0.0, smoothstep(0.46, 0.5, length(scope - vec2<f32>(0.5))));
        let scoped = select(procedural, vec4<f32>(texel.rgb * tint, texel.a), textured);
        result = select(vec4<f32>(0.0, 0.0, 0.0, 1.0), scoped, inside);
    }
    return result;
}

@fragment
fn overlay_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let derivative = fwidth(in.uv);
    let aspect = max(derivative.y / max(derivative.x, 1.0e-8), 1.0e-3);
    let count = u32(overlays.header.x);
    let clock = overlays.header.y;
    let textured = overlays.header.z > 0.5;
    var rgb = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
        if i >= count {
            break;
        }
        let layer = overlays.layers[i];
        let shaded = shade(u32(layer.params.x), layer.color.rgb, in.uv, aspect, clock, textured);
        let src_alpha = clamp(shaded.a * layer.color.a, 0.0, 1.0);
        let out_alpha = src_alpha + alpha * (1.0 - src_alpha);
        rgb = (shaded.rgb * src_alpha + rgb * alpha * (1.0 - src_alpha)) / max(out_alpha, 1.0e-5);
        alpha = out_alpha;
    }
    return vec4<f32>(rgb, alpha);
}
