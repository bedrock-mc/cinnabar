// Reverse-Z depth pyramid: every texel keeps the farthest (smallest) depth it covers.
// Level 0 texel x covers depth pixels 2x and 2x + 1, and level L texel x every pixel p with
// p >> (L + 1) == x. Power-of-two levels halve exactly. Only texels that cover a depth pixel are
// written: the cull never reads the rest, and reads clamp to the source's covered extent, so a
// trailing texel takes the farthest depth of the edge pixels it reaches.

@group(0) @binding(0) var depth_single: texture_depth_2d;
@group(0) @binding(1) var depth_multi: texture_depth_multisampled_2d;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var destination: texture_storage_2d<r32float, write>;

// The source's last covered texel, then the destination's covered extent.
struct HizBounds { source_last: vec2<u32>, extent: vec2<u32> }
@group(0) @binding(4) var<uniform> bounds: HizBounds;

fn source_texel(base: vec2<u32>, offset: u32) -> vec2<u32> {
    return min(base + vec2(offset & 1u, offset >> 1u), bounds.source_last);
}

@compute @workgroup_size(8, 8)
fn hiz_seed(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (any(gid.xy >= bounds.extent)) {
        return;
    }
    var farthest = 1.0;
    for (var offset = 0u; offset < 4u; offset++) {
        farthest = min(farthest, textureLoad(depth_single, source_texel(gid.xy * 2u, offset), 0));
    }
    textureStore(destination, gid.xy, vec4(farthest));
}

@compute @workgroup_size(8, 8)
fn hiz_seed_multisampled(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (any(gid.xy >= bounds.extent)) {
        return;
    }
    let samples = textureNumSamples(depth_multi);
    var farthest = 1.0;
    for (var offset = 0u; offset < 4u; offset++) {
        let texel = source_texel(gid.xy * 2u, offset);
        for (var sample = 0u; sample < samples; sample++) {
            farthest = min(farthest, textureLoad(depth_multi, texel, i32(sample)));
        }
    }
    textureStore(destination, gid.xy, vec4(farthest));
}

@compute @workgroup_size(8, 8)
fn hiz_reduce(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (any(gid.xy >= bounds.extent)) {
        return;
    }
    var farthest = 1.0;
    for (var offset = 0u; offset < 4u; offset++) {
        farthest = min(farthest, textureLoad(previous, source_texel(gid.xy * 2u, offset), 0).r);
    }
    textureStore(destination, gid.xy, vec4(farthest));
}
