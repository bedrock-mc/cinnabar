// Reverse-Z depth pyramid: every texel keeps the farthest (smallest) depth it covers.
// Level 0 texel x covers depth pixels 2x and 2x + 1, and level L texel x every pixel p with
// p >> (L + 1) == x. Power-of-two levels halve exactly; padding texels read clamped edges.

@group(0) @binding(0) var depth_single: texture_depth_2d;
@group(0) @binding(1) var depth_multi: texture_depth_multisampled_2d;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var destination: texture_storage_2d<r32float, write>;

fn source_texel(base: vec2<u32>, offset: u32, last: vec2<u32>) -> vec2<u32> {
    return min(base + vec2(offset & 1u, offset >> 1u), last);
}

@compute @workgroup_size(8, 8)
fn hiz_seed(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (any(gid.xy >= textureDimensions(destination))) {
        return;
    }
    let last = textureDimensions(depth_single) - vec2(1u);
    var farthest = 1.0;
    for (var offset = 0u; offset < 4u; offset++) {
        farthest = min(farthest, textureLoad(depth_single, source_texel(gid.xy * 2u, offset, last), 0));
    }
    textureStore(destination, gid.xy, vec4(farthest));
}

@compute @workgroup_size(8, 8)
fn hiz_seed_multisampled(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (any(gid.xy >= textureDimensions(destination))) {
        return;
    }
    let last = textureDimensions(depth_multi) - vec2(1u);
    let samples = textureNumSamples(depth_multi);
    var farthest = 1.0;
    for (var offset = 0u; offset < 4u; offset++) {
        let texel = source_texel(gid.xy * 2u, offset, last);
        for (var sample = 0u; sample < samples; sample++) {
            farthest = min(farthest, textureLoad(depth_multi, texel, i32(sample)));
        }
    }
    textureStore(destination, gid.xy, vec4(farthest));
}

@compute @workgroup_size(8, 8)
fn hiz_reduce(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (any(gid.xy >= textureDimensions(destination))) {
        return;
    }
    let last = textureDimensions(previous) - vec2(1u);
    var farthest = 1.0;
    for (var offset = 0u; offset < 4u; offset++) {
        farthest = min(farthest, textureLoad(previous, source_texel(gid.xy * 2u, offset, last), 0).r);
    }
    textureStore(destination, gid.xy, vec4(farthest));
}
