struct Vertex {
    a: vec4<f32>, b: vec4<f32>, c: vec4<f32>,
    color_a: vec4<f32>, color_b: vec4<f32>, uv: vec4<f32>,
    data: vec4<f32>, shape: vec4<f32>,
}
struct Instance { transform: mat4x4<f32>, tint: vec4<f32>, line: vec4<f32>, params: array<vec4<f32>, 4> }
@group(0) @binding(1) var<storage, read> vertices: array<Vertex>;
@group(0) @binding(2) var<storage, read> instances: array<Instance>;
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
    return drawing_view.clip * instance.transform * vec4<f32>(p, 1.0);
}
fn direction(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let delta = b - a;
    if dot(delta, delta) < 1e-10 { return vec2<f32>(1.0, 0.0); }
    return normalize(delta);
}
fn rotate(v: vec2<f32>, angle: f32) -> vec2<f32> {
    return vec2<f32>(v.x*cos(angle)-v.y*sin(angle), v.x*sin(angle)+v.y*cos(angle));
}
/// Intersects offset edges; only the outside miter is limited to the selected join style.
fn miter(n0: vec2<f32>, n1: vec2<f32>, bounded: bool) -> vec2<f32> {
    let sum = n0+n1;
    // Reversals have no unique intersection.
    if dot(sum,sum) < 1e-10 { return n0; }
    let bisector = normalize(sum);
    var extent = 1.0/max(abs(dot(bisector,n0)),1e-5);
    if bounded { extent = min(MITER_LIMIT, extent); }
    return bisector * extent;
}
/// Gives each endpoint join at most half of either adjacent clipped segment.
fn inner_miter(n0: vec2<f32>, n1: vec2<f32>, lengths: vec2<f32>, radius: f32) -> vec2<f32> {
    let corner = miter(n0,n1,false);
    let advance = abs(n0.x*corner.y-n0.y*corner.x)*radius;
    // Projected lengths are twice their pixel length; neighboring joins share each segment.
    let available = min(lengths.x,lengths.y)*0.25;
    return corner * min(1.0,available/max(advance,1e-5));
}
@vertex
fn vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance_index: u32) -> MaterialInput {
    let vertex = vertices[index]; let instance = instances[instance_index];
    var output: MaterialInput;
    output.position = clip_position(instance, vertex.a.xyz);
    output.color = vertex.color_a * instance.tint;
    output.uv = vertex.uv.xy;
    output.local_position = vertex.a.xyz;
    output.distance = vertex.a.w;
    output.params0 = instance.params[0]; output.params1 = instance.params[1];
    output.params2 = instance.params[2]; output.params3 = instance.params[3];
    if vertex.shape.x < 0.5 { return output; }
    let a = clip_position(instance, vertex.a.xyz); let b = clip_position(instance, vertex.b.xyz);
    let pair = clip_pair(a, b);
    // A clipped/disabled fan is outside the depth clip volume, with finite coordinates.
    output.position = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    if !pair.valid { return output; }
    let viewport = max(drawing_view.viewport.xy, vec2<f32>(1.0));
    let pa = pair.a.xy / pair.a.w * viewport; let pb = pair.b.xy / pair.b.w * viewport;
    let tangent = direction(pa, pb); let normal = vec2<f32>(tangent.y, -tangent.x);
    let endpoint = vertex.shape.y;
    var center = mix(pair.a, pair.b, endpoint);
    var radius = instance.line.x * 0.5;
    if instance.line.y < 0.5 { radius = radius * abs(drawing_view.viewport.z) * viewport.y * 0.5 / center.w; }
    radius = min(radius, max(viewport.x,viewport.y));
    var offset = normal * vertex.shape.z;
    let t = mix(pair.ta, pair.tb, endpoint);
    output.color = mix(vertex.color_a, vertex.color_b, t) * instance.tint;
    output.uv = mix(vertex.uv.xy, vertex.uv.zw, t);
    output.local_position = mix(vertex.a.xyz, vertex.b.xyz, t);
    output.distance = mix(vertex.a.w, vertex.b.w, t);
    if vertex.shape.x < 1.5 && vertex.shape.w > 0.5 {
        // Only true polyline endpoints get caps; clipped endpoints stay perpendicular.
        if (endpoint < 0.5 && pair.ta == 0.0) || (endpoint > 0.5 && pair.tb == 1.0) {
            let c = clip_position(instance, vertex.c.xyz);
            var neighbor = clip_pair(c, a);
            if endpoint > 0.5 { neighbor = clip_pair(b, c); }
            if neighbor.valid {
                let neighbor_a = neighbor.a.xy/neighbor.a.w*viewport;
                let neighbor_b = neighbor.b.xy/neighbor.b.w*viewport;
                let other = direction(neighbor_a, neighbor_b);
                var incoming = other; var outgoing = tangent;
                if endpoint > 0.5 { incoming = tangent; outgoing = other; }
                let turn = incoming.x*outgoing.y-incoming.y*outgoing.x;
                let side = select(-1.0, 1.0, turn >= 0.0);
                if vertex.shape.z*side < 0.0 {
                    let n0 = vec2<f32>(incoming.y,-incoming.x)*side;
                    let n1 = vec2<f32>(outgoing.y,-outgoing.x)*side;
                    offset = -inner_miter(n0,n1,vec2<f32>(length(pb-pa),length(neighbor_b-neighbor_a)),radius);
                }
            }
        }
    } else if vertex.shape.x > 1.5 && vertex.shape.x < 2.5 {
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
        let next_b = next.b.xy/next.b.w*viewport;
        let outgoing = direction(pb, next_b);
        let turn = tangent.x*outgoing.y-tangent.y*outgoing.x;
        let side = select(-1.0, 1.0, turn >= 0.0);
        let n0 = normal*side; let n1 = vec2<f32>(outgoing.y,-outgoing.x)*side;
        // Both segment quads end at this inner corner; the fan owns the remaining wedge.
        offset = -inner_miter(n0,n1,vec2<f32>(length(pb-pa),length(next_b-pb)),radius);
        if vertex.shape.z >= 0.0 {
            let phase = vertex.shape.z;
            if instance.line.z > 1.5 {
                offset = rotate(n0, atan2(n0.x*n1.y-n0.y*n1.x, dot(n0,n1))*phase);
            } else if instance.line.z > 0.5 {
                let outer = miter(n0,n1,true);
                if phase < 0.5 { offset = mix(n0,outer,phase*2.0); }
                else { offset = mix(outer,n1,phase*2.0-1.0); }
            } else { offset = mix(n0,n1,phase); }
        }
        output.color = vertex.color_b*instance.tint;
        output.uv = vertex.uv.zw; output.local_position = vertex.b.xyz; output.distance = vertex.b.w;
    }
    output.position = vec4<f32>(center.xy + offset * radius * 2.0 / viewport * center.w, center.zw);
    return output;
}
