// Reverse-Z depth pyramid: every texel keeps the farthest (smallest) depth it covers.
// Level 0 texel x covers depth pixels 2x and 2x + 1, and level L texel x every pixel p with
// p >> (L + 1) == x. Power-of-two levels halve exactly. Only texels that cover a depth pixel are
// written: the cull never reads the rest. A texel takes the farthest depth of the source texels
// it covers inside the source's covered extent, which equals reading clamped to that extent.
//
// One dispatch builds up to HIZ_LEVELS consecutive levels: each workgroup of HIZ_SIDE² threads
// reduces one tile of 2·HIZ_SIDE texels per side of its first level, keeping each later level of
// the tile in workgroup memory, so a pyramid needs a few dispatches instead of one per level.
// A texel outside its level's covered extent covers no source texel and holds the neutral 1.0,
// so it never changes the texel above it.

@group(0) @binding(0) var depth_single: texture_depth_2d;
@group(0) @binding(1) var depth_multi: texture_depth_multisampled_2d;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var level_0: texture_storage_2d<r32float, write>;
@group(0) @binding(4) var level_1: texture_storage_2d<r32float, write>;
@group(0) @binding(5) var level_2: texture_storage_2d<r32float, write>;
@group(0) @binding(6) var level_3: texture_storage_2d<r32float, write>;

// The source's covered extent, then each built level's covered extent in `.xy`; a level this
// dispatch does not build has a zero extent.
struct HizBounds { source: vec4<u32>, extents: array<vec4<u32>, HIZ_LEVELS> }
@group(0) @binding(7) var<uniform> bounds: HizBounds;

var<workgroup> second: array<f32, HIZ_SIDE * HIZ_SIDE>;
var<workgroup> third: array<f32, HIZ_SIDE * HIZ_SIDE / 4u>;

// Child `offset` (0..4) of `texel` one level finer.
fn source_texel(texel: vec2<u32>, offset: u32) -> vec2<u32> {
    return texel * 2u + vec2(offset & 1u, offset >> 1u);
}

// True when `texel` lies in the source's covered extent.
fn in_source(texel: vec2<u32>) -> bool {
    return all(texel < bounds.source.xy);
}

// True when `texel` of built level `level` (0..HIZ_LEVELS) covers a depth pixel.
fn covered(texel: vec2<u32>, level: u32) -> bool {
    return all(texel < bounds.extents[level].xy);
}

// The first-level texel `offset` of thread `local` in workgroup `group`'s tile.
fn first_texel(group: vec2<u32>, local: vec2<u32>, offset: u32) -> vec2<u32> {
    return group * (HIZ_SIDE * 2u) + source_texel(local, offset);
}

// Writes the thread's four first-level texels, then reduces the tile's later levels.
fn reduce_tile(group: vec2<u32>, local: vec2<u32>, first: vec4<f32>) {
    for (var offset = 0u; offset < 4u; offset++) {
        let texel = first_texel(group, local, offset);
        if (covered(texel, 0u)) {
            textureStore(level_0, texel, vec4(first[offset]));
        }
    }
    let farthest = min(min(first.x, first.y), min(first.z, first.w));
    let texel_1 = group * HIZ_SIDE + local;
    if (covered(texel_1, 1u)) {
        textureStore(level_1, texel_1, vec4(farthest));
    }
    second[local.y * HIZ_SIDE + local.x] = farthest;
    workgroupBarrier();
    let half = HIZ_SIDE / 2u;
    if (all(local < vec2(half))) {
        let at = local.y * 2u * HIZ_SIDE + local.x * 2u;
        let value = min(
            min(second[at], second[at + 1u]),
            min(second[at + HIZ_SIDE], second[at + HIZ_SIDE + 1u]),
        );
        let texel_2 = group * half + local;
        if (covered(texel_2, 2u)) {
            textureStore(level_2, texel_2, vec4(value));
        }
        third[local.y * half + local.x] = value;
    }
    workgroupBarrier();
    let quarter = HIZ_SIDE / 4u;
    if (all(local < vec2(quarter))) {
        let at = local.y * 2u * half + local.x * 2u;
        let value = min(min(third[at], third[at + 1u]), min(third[at + half], third[at + half + 1u]));
        let texel_3 = group * quarter + local;
        if (covered(texel_3, 3u)) {
            textureStore(level_3, texel_3, vec4(value));
        }
    }
}

@compute @workgroup_size(HIZ_SIDE, HIZ_SIDE)
fn hiz_seed(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    var first = vec4(1.0);
    for (var texel_offset = 0u; texel_offset < 4u; texel_offset++) {
        let texel = first_texel(group.xy, local.xy, texel_offset);
        for (var offset = 0u; offset < 4u; offset++) {
            let pixel = source_texel(texel, offset);
            if (in_source(pixel)) {
                first[texel_offset] = min(first[texel_offset], textureLoad(depth_single, pixel, 0));
            }
        }
    }
    reduce_tile(group.xy, local.xy, first);
}

@compute @workgroup_size(HIZ_SIDE, HIZ_SIDE)
fn hiz_seed_multisampled(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_id) local: vec3<u32>,
) {
    let samples = textureNumSamples(depth_multi);
    var first = vec4(1.0);
    for (var texel_offset = 0u; texel_offset < 4u; texel_offset++) {
        let texel = first_texel(group.xy, local.xy, texel_offset);
        for (var offset = 0u; offset < 4u; offset++) {
            let pixel = source_texel(texel, offset);
            if (in_source(pixel)) {
                for (var sample = 0u; sample < samples; sample++) {
                    first[texel_offset] = min(
                        first[texel_offset],
                        textureLoad(depth_multi, pixel, i32(sample)),
                    );
                }
            }
        }
    }
    reduce_tile(group.xy, local.xy, first);
}

@compute @workgroup_size(HIZ_SIDE, HIZ_SIDE)
fn hiz_reduce(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    var first = vec4(1.0);
    for (var texel_offset = 0u; texel_offset < 4u; texel_offset++) {
        let texel = first_texel(group.xy, local.xy, texel_offset);
        for (var offset = 0u; offset < 4u; offset++) {
            let source = source_texel(texel, offset);
            if (in_source(source)) {
                first[texel_offset] = min(first[texel_offset], textureLoad(previous, source, 0).r);
            }
        }
    }
    reduce_tile(group.xy, local.xy, first);
}
