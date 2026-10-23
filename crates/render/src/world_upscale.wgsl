// Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files(the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and / or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions :
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
// WGSL port of the FP32 EASU and RCAS filters in AMD FidelityFX FSR1.
// The scene is linear: use gamma 2.0 for filtering and SRTM for HDR range.

@group(0) @binding(0) var world_image: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
struct Constants {
    con0: vec4<f32>,
    con1: vec4<f32>,
    con2: vec4<f32>,
    con3: vec4<f32>,
    rcas: vec4<f32>,
};
@group(0) @binding(2) var<uniform> constants: Constants;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

/// Covers the output with one triangle, keeping texture coordinates top-left aligned.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

/// Keeps a basic reconstruction available if an FSR pipeline fails.
@fragment
fn bilinear_fragment(in: Vertex) -> @location(0) vec4<f32> {
    return textureSampleLevel(world_image, linear_sampler, in.uv, 0.0);
}

/// Converts linear scene samples to FSR's nonnegative perceptual input range.
fn perceptual(color: vec3<f32>) -> vec3<f32> {
    var c = max(color, vec3(0.0));
#ifdef FSR_HDR
    c /= 1.0 + max(c.r, max(c.g, c.b));
#endif
    return sqrt(c);
}

/// Restores linear scene colour after sharpening, including HDR highlights.
fn scene_color(color: vec3<f32>) -> vec3<f32> {
    var c = color * color;
#ifdef FSR_HDR
    c /= max(1.0 / 32768.0, 1.0 - max(c.r, max(c.g, c.b)));
#endif
    return c;
}

struct Gather {
    r: vec4<f32>,
    g: vec4<f32>,
    b: vec4<f32>,
};

/// Gathers the reference tap ordering and transforms each texel before filtering.
fn gather(p: vec2<f32>) -> Gather {
    let r = textureGather(0, world_image, linear_sampler, p);
    let g = textureGather(1, world_image, linear_sampler, p);
    let b = textureGather(2, world_image, linear_sampler, p);
    let x = perceptual(vec3(r.x, g.x, b.x));
    let y = perceptual(vec3(r.y, g.y, b.y));
    let z = perceptual(vec3(r.z, g.z, b.z));
    let w = perceptual(vec3(r.w, g.w, b.w));
    return Gather(vec4(x.r, y.r, z.r, w.r), vec4(x.g, y.g, z.g, w.g), vec4(x.b, y.b, z.b, w.b));
}

/// Approximates luma in the same channel proportions as EASU.
fn luma(c: vec3<f32>) -> f32 {
    return c.b * 0.5 + (c.r * 0.5 + c.g);
}

/// Accumulates one bilinearly weighted gradient and its edge strength.
fn easu_set(weight: f32, a: f32, b: f32, c: f32, d: f32, e: f32) -> vec3<f32> {
    let direction = vec2(d - b, e - a);
    let extent = max(abs(vec2(d - c, e - c)), abs(vec2(c - b, c - a)));
    let length = clamp(abs(direction) / max(extent, vec2(1e-8)), vec2(0.0), vec2(1.0));
    return vec3(direction, dot(length, length)) * weight;
}

/// Returns the weighted colour and weight of one anisotropic Lanczos tap.
fn easu_tap(offset: vec2<f32>, direction: vec2<f32>, length: vec2<f32>, lobe: f32, clip: f32, color: vec3<f32>) -> vec4<f32> {
    let v = vec2(dot(offset, direction), dot(offset, vec2(-direction.y, direction.x))) * length;
    let d2 = min(dot(v, v), clip);
    let base = 0.4 * d2 - 1.0;
    let window = lobe * d2 - 1.0;
    let weight = (1.5625 * base * base - 0.5625) * window * window;
    return vec4(color, 1.0) * weight;
}

/// Reconstructs the scaled scene using the reference twelve-tap edge-adaptive kernel.
@fragment
fn easu_fragment(in: Vertex) -> @location(0) vec4<f32> {
    let position = floor(in.position.xy) * constants.con0.xy + constants.con0.zw;
    let origin = floor(position);
    let pp = position - origin;
    let p0 = origin * constants.con1.xy + constants.con1.zw;
    let bc = gather(p0);
    let ijfe = gather(p0 + constants.con2.xy);
    let klhg = gather(p0 + constants.con2.zw);
    let no = gather(p0 + constants.con3.xy);
    let b = vec3(bc.r.x, bc.g.x, bc.b.x);
    let c = vec3(bc.r.y, bc.g.y, bc.b.y);
    let i = vec3(ijfe.r.x, ijfe.g.x, ijfe.b.x);
    let j = vec3(ijfe.r.y, ijfe.g.y, ijfe.b.y);
    let f = vec3(ijfe.r.z, ijfe.g.z, ijfe.b.z);
    let e = vec3(ijfe.r.w, ijfe.g.w, ijfe.b.w);
    let k = vec3(klhg.r.x, klhg.g.x, klhg.b.x);
    let l = vec3(klhg.r.y, klhg.g.y, klhg.b.y);
    let h = vec3(klhg.r.z, klhg.g.z, klhg.b.z);
    let g = vec3(klhg.r.w, klhg.g.w, klhg.b.w);
    let o = vec3(no.r.z, no.g.z, no.b.z);
    let n = vec3(no.r.w, no.g.w, no.b.w);
    let edge = easu_set((1.0-pp.x)*(1.0-pp.y), luma(b), luma(e), luma(f), luma(g), luma(j))
        + easu_set(pp.x*(1.0-pp.y), luma(c), luma(f), luma(g), luma(h), luma(k))
        + easu_set((1.0-pp.x)*pp.y, luma(f), luma(i), luma(j), luma(k), luma(n))
        + easu_set(pp.x*pp.y, luma(g), luma(j), luma(k), luma(l), luma(o));
    var direction = vec2(1.0, 0.0);
    if dot(edge.xy, edge.xy) >= 1.0 / 32768.0 {
        direction = normalize(edge.xy);
    }
    let length = 0.25 * edge.z * edge.z;
    let stretch = dot(direction, direction) / max(abs(direction.x), abs(direction.y));
    let anisotropy = vec2(1.0 + (stretch - 1.0) * length, 1.0 - 0.5 * length);
    let lobe = 0.5 + (0.21 - 0.5) * length;
    let clip = 1.0 / lobe;
    let sum = easu_tap(vec2(0.0,-1.0)-pp, direction, anisotropy, lobe, clip, b)
        + easu_tap(vec2(1.0,-1.0)-pp, direction, anisotropy, lobe, clip, c)
        + easu_tap(vec2(-1.0,1.0)-pp, direction, anisotropy, lobe, clip, i)
        + easu_tap(vec2(0.0,1.0)-pp, direction, anisotropy, lobe, clip, j)
        + easu_tap(vec2(0.0,0.0)-pp, direction, anisotropy, lobe, clip, f)
        + easu_tap(vec2(-1.0,0.0)-pp, direction, anisotropy, lobe, clip, e)
        + easu_tap(vec2(1.0,1.0)-pp, direction, anisotropy, lobe, clip, k)
        + easu_tap(vec2(2.0,1.0)-pp, direction, anisotropy, lobe, clip, l)
        + easu_tap(vec2(2.0,0.0)-pp, direction, anisotropy, lobe, clip, h)
        + easu_tap(vec2(1.0,0.0)-pp, direction, anisotropy, lobe, clip, g)
        + easu_tap(vec2(1.0,2.0)-pp, direction, anisotropy, lobe, clip, o)
        + easu_tap(vec2(0.0,2.0)-pp, direction, anisotropy, lobe, clip, n);
    let low = min(min(f, g), min(j, k));
    let high = max(max(f, g), max(j, k));
    let color = clamp(sum.rgb / sum.a, low, high);
    let alpha = textureSampleLevel(world_image, linear_sampler, in.uv, 0.0).a;
    return vec4(scene_color(color), alpha);
}

/// Clamps RCAS reads at image edges rather than depending on out-of-bounds loads.
fn rcas_load(p: vec2<i32>) -> vec4<f32> {
    let sample = textureLoad(world_image, clamp(p, vec2(0), vec2<i32>(textureDimensions(world_image)) - 1), 0);
    return vec4(perceptual(sample.rgb), sample.a);
}

/// Sharpens the native intermediate; zero strength resolves EASU without sharpening.
@fragment
fn rcas_fragment(in: Vertex) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.position.xy);
    let b = rcas_load(p + vec2(0,-1)).rgb;
    let d = rcas_load(p + vec2(-1,0)).rgb;
    let e = rcas_load(p);
    let f = rcas_load(p + vec2(1,0)).rgb;
    let h = rcas_load(p + vec2(0,1)).rgb;
    let low = min(min(b, d), min(f, h));
    let high = max(max(b, d), max(f, h));
    // Guard the zero denominators in solid black and white neighbourhoods.
    let hit_min = min(low, e.rgb) / max(4.0 * high, vec3(1e-8));
    let hit_max = (1.0 - max(high, e.rgb)) / min(4.0 * low - 4.0, vec3(-1e-8));
    let lobes = max(-hit_min, hit_max);
    let lobe = max(-0.1875, min(max(lobes.r, max(lobes.g, lobes.b)), 0.0)) * constants.rcas.x;
    let color = (lobe * (b + d + f + h) + e.rgb) / (4.0 * lobe + 1.0);
    return vec4(scene_color(color), e.a);
}
