import bevy_render::view::View;

struct Vertex {
    a: vec4<f32>, b: vec4<f32>, c: vec4<f32>,
    color_a: vec4<f32>, color_b: vec4<f32>, uv: vec4<f32>,
    data: vec4<f32>, shape: vec4<f32>,
}
struct Instance { transform: mat4x4<f32>, tint: vec4<f32>, line: vec4<f32> }
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> vertices: array<Vertex>;
@group(0) @binding(2) var<storage, read> instances: array<Instance>;
struct Output { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> }
const MITER_LIMIT: f32 = 4.0;
struct Pair { a: vec4<f32>, b: vec4<f32>, ta: f32, tb: f32, valid: bool }

// Clip to WebGPU's near half-space before dividing by W, including orthographic views.
fn clip_pair(a: vec4<f32>, b: vec4<f32>) -> Pair {
    let da = a.w - a.z; let db = b.w - b.z;
    var result = Pair(a, b, 0.0, 1.0, da >= 0.0 || db >= 0.0);
    if da < 0.0 && db >= 0.0 { result.ta = da / (da - db); result.a = mix(a, b, result.ta); }
    if db < 0.0 && da >= 0.0 { result.tb = da / (da - db); result.b = mix(a, b, result.tb); }
    result.valid = result.valid && result.a.w > 1e-6 && result.b.w > 1e-6;
    return result;
}
fn clip_position(instance: Instance, p: vec3<f32>) -> vec4<f32> {
    let relative = instance.transform * vec4<f32>(p, 1.0);
    let camera = view.view_from_world * vec4<f32>(relative.xyz, 0.0);
    return view.clip_from_view * vec4<f32>(camera.xyz, 1.0);
}
fn direction(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let delta = b - a;
    if dot(delta, delta) < 1e-10 { return vec2<f32>(1.0, 0.0); }
    return normalize(delta);
}
fn rotate(v: vec2<f32>, angle: f32) -> vec2<f32> {
    return vec2<f32>(v.x*cos(angle)-v.y*sin(angle), v.x*sin(angle)+v.y*cos(angle));
}
@vertex
fn vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance_index: u32) -> Output {
    let vertex = vertices[index]; let instance = instances[instance_index];
    var output = Output(clip_position(instance, vertex.a.xyz), vertex.color_a * instance.tint);
    if vertex.shape.x < 0.5 { return output; }
    let a = clip_position(instance, vertex.a.xyz); let b = clip_position(instance, vertex.b.xyz);
    let pair = clip_pair(a, b);
    // A clipped/disabled fan is outside the depth clip volume, with finite coordinates.
    output.position = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    if !pair.valid { return output; }
    let viewport = max(view.viewport.zw, vec2<f32>(1.0));
    let pa = pair.a.xy / pair.a.w * viewport; let pb = pair.b.xy / pair.b.w * viewport;
    let tangent = direction(pa, pb); let normal = vec2<f32>(tangent.y, -tangent.x);
    let endpoint = vertex.shape.y;
    var center = mix(pair.a, pair.b, endpoint);
    var offset = normal * vertex.shape.z;
    let t = mix(pair.ta, pair.tb, endpoint);
    output.color = mix(vertex.color_a, vertex.color_b, t) * instance.tint;
    if vertex.shape.x > 1.5 && vertex.shape.x < 2.5 {
        if instance.line.w < 0.5 || (endpoint < 0.5 && pair.ta > 0.0) || (endpoint > 0.5 && pair.tb < 1.0) { return output; }
        offset = vec2<f32>(0.0);
        if vertex.shape.z >= 0.0 {
            offset = rotate(normal, -vertex.shape.z * 3.14159265);
            if endpoint > 0.5 { offset = -offset; }
        }
    } else if vertex.shape.x > 2.5 {
        if b.w <= 1e-6 || b.w < b.z { return output; }
        let c = clip_position(instance, vertex.c.xyz); let next = clip_pair(b, c);
        if !next.valid { return output; }
        center = b;
        let outgoing = direction(b.xy/b.w*viewport, next.b.xy/next.b.w*viewport);
        let turn = tangent.x*outgoing.y-tangent.y*outgoing.x;
        let side = select(-1.0, 1.0, turn >= 0.0);
        let n0 = normal*side; let n1 = vec2<f32>(outgoing.y,-outgoing.x)*side;
        offset = vec2<f32>(0.0);
        if vertex.shape.z >= 0.0 {
            let phase = vertex.shape.z;
            if instance.line.z > 1.5 {
                offset = rotate(n0, atan2(n0.x*n1.y-n0.y*n1.x, dot(n0,n1))*phase);
            } else if instance.line.z > 0.5 {
                let bisector = direction(vec2<f32>(0.0), n0+n1);
                let miter = bisector * min(MITER_LIMIT, 1.0/max(abs(dot(bisector,n0)),1.0/MITER_LIMIT));
                if phase < 0.5 { offset = mix(n0,miter,phase*2.0); }
                else { offset = mix(miter,n1,phase*2.0-1.0); }
            } else { offset = mix(n0,n1,phase); }
        }
        output.color = vertex.color_b*instance.tint;
    }
    var radius = instance.line.x * 0.5;
    if instance.line.y < 0.5 { radius = radius * abs(view.clip_from_view[1][1]) * viewport.y * 0.5 / center.w; }
    radius = min(radius, max(viewport.x,viewport.y));
    output.position = vec4<f32>(center.xy + offset * radius * 2.0 / viewport * center.w, center.zw);
    return output;
}
@fragment
fn fragment(input: Output) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color.rgb * input.color.a, input.color.a);
}
