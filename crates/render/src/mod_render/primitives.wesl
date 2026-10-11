import bevy_render::view::View;

// Mirrors `mod_render::geometry::ModVertex`; style ids match its constants.
struct ModVertex {
    anchor: vec4<f32>,
    shape: vec4<f32>,
    color: vec4<f32>,
    uv: vec4<f32>,
    style: vec4<f32>,
}

struct PrimitiveFrame {
    time: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> vertices: array<ModVertex>;
@group(0) @binding(2) var<uniform> primitive_frame: PrimitiveFrame;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) style: vec4<f32>,
}

const TAU: f32 = 6.2831853;

@vertex
fn mod_primitive_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let v = vertices[index];
    let anchor = v.anchor.xyz;
    let kind = u32(v.anchor.w + 0.5);
    let to_eye = view.world_position - anchor;
    let camera_right = view.world_from_view[0].xyz;
    let camera_up = view.world_from_view[1].xyz;
    var world = anchor;
    if kind == 1u {
        var side = cross(v.shape.xyz, to_eye);
        if dot(side, side) < 1e-10 {
            side = camera_right;
        }
        world = anchor + normalize(side) * v.uv.x * v.shape.w;
    } else if kind == 2u {
        world = anchor + camera_right * v.uv.x * v.shape.x + camera_up * v.uv.y * v.shape.y;
    } else if kind == 3u {
        var right = cross(vec3<f32>(0.0, 1.0, 0.0), to_eye);
        if dot(right, right) < 1e-10 {
            right = camera_right;
        }
        world = anchor + normalize(right) * v.uv.x * v.shape.x
            + vec3<f32>(0.0, v.uv.y * v.shape.y, 0.0);
    }
    var out: VertexOutput;
    out.position = view.clip_from_world * vec4<f32>(world, 1.0);
    out.color = v.color;
    out.uv = v.uv.xy;
    out.style = v.style;
    return out;
}

fn hash(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

fn capsule(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, radius: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - radius;
}

fn silhouette(uv: vec2<f32>) -> f32 {
    let p = vec2<f32>(abs(uv.x), uv.y);
    var d = length(p - vec2<f32>(0.0, 0.72)) - 0.17;
    d = min(d, capsule(p, vec2<f32>(0.0, 0.42), vec2<f32>(0.0, -0.08), 0.2));
    d = min(d, capsule(p, vec2<f32>(0.1, -0.1), vec2<f32>(0.14, -0.95), 0.08));
    d = min(d, capsule(p, vec2<f32>(0.24, 0.4), vec2<f32>(0.32, -0.12), 0.06));
    return d;
}

/// Coverage, extra white, and glow (0 alpha-blended, 1 purely additive).
struct Shade {
    coverage: f32,
    white: f32,
    glow: f32,
}

fn shade(uv: vec2<f32>, style: vec4<f32>, time: f32, edge_feather: f32) -> Shade {
    let id = u32(style.x + 0.5);
    let progress = style.y;
    let r = length(uv);
    var s = Shade(0.0, 0.0, 0.0);
    switch id {
        case 0u: {
            // Telegraph: translucent fill growing to the rim; the leading edge marks impact time.
            let fill = select(0.0, 0.3, r <= progress);
            let rim = exp(-pow((r - 0.97) / 0.03, 2.0));
            let edge = exp(-pow((r - progress) / 0.025, 2.0)) * 0.9;
            let pulse = 0.85 + 0.15 * sin(time * 12.0);
            s = Shade(select(0.0, max(fill, max(rim, edge)) * pulse, r <= 1.0), edge * 0.5, 0.4);
        }
        case 1u: {
            s = Shade(1.0 - smoothstep(0.8, 1.0, r), 0.0, 0.0);
        }
        case 2u: {
            // Crater: scorched floor with jagged radial cracks whose embers cool with progress.
            let angle = atan2(uv.y, uv.x) / TAU + 0.5;
            let lanes = angle * 11.0;
            let lane = floor(lanes);
            let wobble = sin(r * 23.0 + lane * 3.1) * 0.08;
            let crack = exp(-pow((fract(lanes + wobble) - 0.5) / 0.05, 2.0))
                * step(hash(lane) * 0.4 + 0.55, 1.0 - r * 0.6) * (1.0 - smoothstep(0.75, 1.0, r));
            let floor_mask = 1.0 - smoothstep(0.32, 0.55, r);
            let cool = 1.0 - progress * progress;
            s = Shade(max(floor_mask * 0.85, crack) * cool, crack * (1.0 - progress) * 0.8, 0.0);
        }
        case 3u: {
            let width = 0.08 * (1.0 - progress) + 0.02;
            s = Shade(exp(-pow((r - progress) / width, 2.0)) * (1.0 - progress), 0.3, 0.8);
        }
        case 4u: {
            let angle = atan2(uv.y, uv.x);
            let grain = 0.55 + 0.45 * sin(angle * 13.0 + time * 2.0) * sin(angle * 7.0 - r * 9.0);
            let band = exp(-pow((r - progress * 0.9) / 0.18, 2.0));
            s = Shade(band * grain * (1.0 - progress), 0.0, 0.0);
        }
        case 10u: {
            let across = abs(uv.x);
            s = Shade((1.0 - across * across) * (1.0 - uv.y), pow(1.0 - across, 4.0) * 0.6, 0.6);
        }
        case 11u: {
            let across = abs(uv.x);
            let along = uv.y * style.z;
            let flow = 0.5 + 0.5 * sin(along * 3.0 - time * 30.0 + sin(along * 7.0 + time * 11.0));
            let core = exp(-across * across * 30.0) * style.w;
            let halo = exp(-across * across * 4.0) * (0.6 + 0.4 * flow);
            s = Shade(clamp(halo + core, 0.0, 1.0), core, 1.0);
        }
        case 20u: {
            s = Shade(1.0, 0.0, 0.0);
        }
        case 21u: {
            s = Shade(pow(max(1.0 - r, 0.0), 1.5), 0.0, 0.5);
        }
        case 22u: {
            s = Shade(exp(-pow((r - 0.8) / 0.1, 2.0)), 0.2, 0.7);
        }
        case 23u: {
            let ray = max(
                exp(-abs(uv.x) * 14.0) * (1.0 - abs(uv.y)),
                exp(-abs(uv.y) * 14.0) * (1.0 - abs(uv.x)),
            );
            let core = exp(-r * r * 10.0);
            s = Shade(clamp(ray + core, 0.0, 1.0), core, 1.0);
        }
        case 24u: {
            let d = silhouette(uv);
            let body = 1.0 - smoothstep(-0.02, 0.02, d);
            let rim = exp(-pow(d / 0.03, 2.0)) * 0.6;
            s = Shade(max(body * 0.7, rim), rim * 0.5, 0.3);
        }
        case 25u: {
            let sphere_r = r / 0.7;
            let z = sqrt(max(1.0 - sphere_r * sphere_r, 0.0));
            let normal = vec3<f32>(uv / 0.7, z);
            let light = 0.55 + 0.45 * dot(normal, normalize(vec3<f32>(-0.4, 0.6, 0.7)));
            let fresnel = pow(1.0 - z, 2.0);
            let body = select(0.0, light, sphere_r <= 1.0);
            let halo = exp(-pow(max(r - 0.7, 0.0) / 0.15, 2.0)) * 0.6;
            s = Shade(max(body, halo), select(0.0, fresnel + 0.4 * z * z, sphere_r <= 1.0), 0.6);
        }
        case 26u: {
            // Aura shell: a flickering, hollow flame envelope with rising streaks.
            let flicker = sin(uv.x * 9.0 + time * 7.0) * sin(uv.y * 6.0 - time * 9.0) * 0.08;
            let d = length(vec2<f32>(uv.x * (1.25 + 0.45 * uv.y), (uv.y + 0.15) * 0.85));
            let shell = smoothstep(1.0 + flicker, 0.72, d) - 0.65 * smoothstep(0.72, 0.25, d);
            let streak = pow(0.5 + 0.5 * sin(uv.x * 23.0 + hash(floor(uv.x * 4.0)) * 6.0), 6.0)
                * fract(uv.y * 0.5 - time * 1.7);
            s = Shade(clamp(shell + streak * 0.35 * smoothstep(1.0, 0.6, d), 0.0, 1.0), 0.15, 1.0);
        }
        case 30u: {
            let feather = max(edge_feather, 1e-5);
            s = Shade(1.0 - smoothstep(1.0 - feather, 1.0, abs(uv.x)), 0.0, 0.0);
        }
        default: {}
    }
    return s;
}

@fragment
fn mod_primitive_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = shade(in.uv, in.style, primitive_frame.time.x, fwidth(in.uv.x));
    let coverage = clamp(s.coverage, 0.0, 1.0) * in.color.a;
    if coverage <= 0.002 {
        discard;
    }
    let rgb = in.color.rgb + vec3<f32>(s.white);
    // Premultiplied: glow contributes colour while hardly occluding what lies behind.
    return vec4<f32>(rgb * coverage, coverage * (1.0 - s.glow));
}
