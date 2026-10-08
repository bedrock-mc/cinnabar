@group(0) @binding(0) var source_single: texture_depth_2d;
@group(0) @binding(1) var source_multi: texture_depth_multisampled_2d;

// Covers the attachment with a single triangle and no shared diagonal edge.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(corner * 2.0 - vec2(1.0), 0.0, 1.0);
}

// Preserves depth when the scene already has one sample per pixel.
@fragment
fn single(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(source_single, vec2<i32>(position.xy), 0);
}

// Reverse-Z effects need the nearest covered surface, including partial edge coverage.
@fragment
fn multisampled(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    var nearest = 0.0;
    for (var sample = 0u; sample < textureNumSamples(source_multi); sample++) {
        nearest = max(nearest, textureLoad(source_multi, vec2<i32>(position.xy), i32(sample)));
    }
    return nearest;
}
