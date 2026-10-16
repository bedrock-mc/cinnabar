// Builds the shared blur pyramid for mod-surface backdrops, and copies the scene into the
// next main texture before backdrops draw over it.

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

@vertex
fn fullscreen_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// Dual-Kawase downsample: the 2x2 block under the output texel weighted four times, plus four
// bilinear taps one source texel out on the diagonals. Each level adds a variance of 0.75
// source texels squared per axis.
@fragment
fn downsample(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let source_size = vec2<f32>(textureDimensions(source));
    let output_size = max(vec2<f32>(1.0), floor(source_size * 0.5));
    let uv = position.xy / output_size;
    let offset = 1.0 / source_size;
    var sum = textureSampleLevel(source, source_sampler, uv, 0.0) * 4.0;
    sum += textureSampleLevel(source, source_sampler, uv - offset, 0.0);
    sum += textureSampleLevel(source, source_sampler, uv + offset, 0.0);
    sum += textureSampleLevel(source, source_sampler, uv + vec2<f32>(offset.x, -offset.y), 0.0);
    sum += textureSampleLevel(source, source_sampler, uv - vec2<f32>(offset.x, -offset.y), 0.0);
    return sum / 8.0;
}

@fragment
fn copy(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}
