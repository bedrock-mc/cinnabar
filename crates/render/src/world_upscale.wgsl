// Original bilinear reconstruction with a small, locally bounded unsharp mask.
@group(0) @binding(0) var world_image: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

@fragment
fn fragment(in: Vertex) -> @location(0) vec4<f32> {
    let centre = textureSampleLevel(world_image, linear_sampler, in.uv, 0.0);
#ifdef SHARPEN
    let step = 1.0 / vec2<f32>(textureDimensions(world_image));
    let left = textureSampleLevel(world_image, linear_sampler, in.uv - vec2<f32>(step.x, 0.0), 0.0).rgb;
    let right = textureSampleLevel(world_image, linear_sampler, in.uv + vec2<f32>(step.x, 0.0), 0.0).rgb;
    let up = textureSampleLevel(world_image, linear_sampler, in.uv - vec2<f32>(0.0, step.y), 0.0).rgb;
    let down = textureSampleLevel(world_image, linear_sampler, in.uv + vec2<f32>(0.0, step.y), 0.0).rgb;
    let low = min(centre.rgb, min(min(left, right), min(up, down)));
    let high = max(centre.rgb, max(max(left, right), max(up, down)));
    let sharpened = centre.rgb + 0.2 * (centre.rgb - (left + right + up + down) * 0.25);
    return vec4<f32>(clamp(sharpened, low, high), centre.a);
#else
    return centre;
#endif
}
