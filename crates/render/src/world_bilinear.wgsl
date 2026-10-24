// Independent bilinear fallback for a failed FSR shader module.
@group(0) @binding(0) var world_image: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

/// Covers the output without depending on the FSR shader module.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

/// Reconstructs the world with one bilinear sample per output pixel.
@fragment
fn bilinear_fragment(in: Vertex) -> @location(0) vec4<f32> {
    return textureSampleLevel(world_image, linear_sampler, in.uv, 0.0);
}

