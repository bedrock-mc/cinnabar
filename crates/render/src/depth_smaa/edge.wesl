@if(MULTISAMPLED)
@group(0) @binding(0) var scene_depth: texture_depth_multisampled_2d;
@if(!MULTISAMPLED)
@group(0) @binding(0) var scene_depth: texture_depth_2d;

// A full-screen triangle avoids a diagonal seam between primitives.
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(corner * 2.0 - vec2(1.0), 0.0, 1.0);
}

// Reverse-Z selects the nearest covered surface without inspecting colour texels.
fn depth_at(pixel: vec2<i32>) -> f32 {
    let point = clamp(pixel, vec2(0), vec2<i32>(textureDimensions(scene_depth)) - vec2(1));
    @if(MULTISAMPLED)
    var depth = 0.0;
    @if(MULTISAMPLED)
    for (var sample = 0u; sample < textureNumSamples(scene_depth); sample++) {
        depth = max(depth, textureLoad(scene_depth, point, i32(sample)));
    }
    @if(MULTISAMPLED)
    return depth;
    @if(!MULTISAMPLED)
    return textureLoad(scene_depth, point, 0);
}

// Depth jumps mark silhouettes; continuous slopes cannot smear authored texture edges.
@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let pixel = vec2<i32>(position.xy);
    let depth = depth_at(pixel);
    let adjacent = vec2(depth_at(pixel - vec2(1, 0)), depth_at(pixel - vec2(0, 1)));
    let threshold = 0.01 * max(vec2(depth), adjacent);
    let delta = abs(vec2(depth) - adjacent);
    let forward = vec2(depth_at(pixel + vec2(1, 0)), depth_at(pixel + vec2(0, 1)));
    let farther = vec2(depth_at(pixel - vec2(2, 0)), depth_at(pixel - vec2(0, 2)));
    var continuation = min(abs(vec2(depth) - forward), abs(adjacent - farther));
    let size = vec2<i32>(textureDimensions(scene_depth));
    if pixel.x < 2 { continuation.x = abs(depth - forward.x); }
    if pixel.y < 2 { continuation.y = abs(depth - forward.y); }
    if pixel.x >= size.x - 1 { continuation.x = abs(adjacent.x - farther.x); }
    if pixel.y >= size.y - 1 { continuation.y = abs(adjacent.y - farther.y); }
    let edges = vec2<f32>(delta > max(max(threshold, 2.0 * continuation), vec2(0.000001)));
    if all(edges == vec2(0.0)) { discard; }
    return edges;
}
