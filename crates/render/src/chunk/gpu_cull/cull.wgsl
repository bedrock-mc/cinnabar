// Terrain cull: frustum, enabled bit, two-phase Hi-Z and facing runs into compacted,
// slot-ordered indirect args. `count` decides, `scan` offsets workgroups, `emit` writes.
// `cull_occlusion` instead writes one occluded bit per slot for CPU readback.

const WORKGROUP: u32 = CULL_WORKGROUP;
const SIDE: i32 = CULL_SIDE;
const HIZ_PADDING: f32 = CULL_HIZ_PADDING;
const RELATIVE_SLACK: f32 = CULL_RELATIVE_SLACK;
const ABSOLUTE_SLACK: f32 = CULL_ABSOLUTE_SLACK;
const BOUNDS_BIAS: i32 = 128;
const PHASE_LATE: u32 = 1u;
const OCCLUSION_WORDS: u32 = WORKGROUP / 32u;

// Scalar origin fields keep the record free of packed vec3 storage.
struct CullRecord {
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    base_vertex: i32,
    bounds_min: u32,
    bounds_max: u32,
    cube_start: u32,
    cube_end: u32,
    solid_ends: array<u32, 3>,
    model_start: u32,
    model_count: u32,
    liquid_start: u32,
    liquid_count: u32,
    live: u32,
}

struct CullView {
    planes: array<vec4<f32>, 5>,
    clip_from_rel: mat4x4<f32>,
    // Integer eye; w bit 0 keeps all faces, bits 1..3 flag a fractional eye per axis.
    camera: vec4<i32>,
    viewport: vec4<f32>,
    // Depth width, height, pyramid mips, Hi-Z enabled.
    depth: vec4<u32>,
    // Slot count, group count, phase, first count index.
    params: vec4<u32>,
    regions: vec4<u32>,
    index_counts: vec4<u32>,
}

@group(0) @binding(0) var<uniform> view: CullView;
@group(0) @binding(1) var<storage, read> records: array<CullRecord>;
@group(0) @binding(2) var<storage, read> enabled: array<u32>;
@group(0) @binding(3) var<storage, read_write> history: array<u32>;
@group(0) @binding(4) var<storage, read_write> record_draws: array<u32>;
@group(0) @binding(5) var<storage, read_write> group_sums: array<vec4<u32>>;
@group(0) @binding(6) var<storage, read_write> args: array<u32>;
@group(0) @binding(7) var<storage, read_write> draw_counts: array<u32>;
@group(0) @binding(8) var hiz: texture_2d<f32>;
@group(0) @binding(9) var<storage, read_write> occluded: array<u32>;

var<workgroup> sums: array<vec4<u32>, WORKGROUP>;
var<workgroup> occluded_bits: array<atomic<u32>, OCCLUSION_WORDS>;

fn record_origin(record: CullRecord) -> vec3<i32> {
    return vec3(record.origin_x, record.origin_y, record.origin_z);
}

fn slot_enabled(slot: u32) -> bool {
    return ((enabled[slot >> 5u] >> (slot & 31u)) & 1u) != 0u;
}

// Bevy's sphere then box test on the sub-chunk box, with slack so it never culls more.
fn in_frustum(origin: vec3<i32>) -> bool {
    let half = f32(SIDE) * 0.5;
    let center = vec3<f32>(origin) + vec3(half);
    let radius = length(vec3(half));
    for (var i = 0u; i < 5u; i++) {
        let plane = view.planes[i];
        let distance = dot(plane.xyz, center) + plane.w;
        let slack = RELATIVE_SLACK * (abs(plane.w) + dot(abs(plane.xyz * center), vec3(1.0)))
            + ABSOLUTE_SLACK;
        let extent = dot(abs(plane.xyz), vec3(half));
        if (distance + radius <= -slack || distance + extent <= -slack) {
            return false;
        }
    }
    return true;
}

fn unpack_bound(word: u32) -> vec3<f32> {
    return vec3<f32>(vec3<i32>(
        i32(word & 0xffu),
        i32((word >> 8u) & 0xffu),
        i32((word >> 16u) & 0xffu),
    ) - vec3(BOUNDS_BIAS));
}

// Visible unless every point of the padded box lies behind the farthest pyramid depth it covers.
// `strict` also keeps a box reaching past the viewport, whose hidden part this depth never saw.
fn hiz_test(record: CullRecord, strict: bool) -> bool {
    if (view.depth.w == 0u) {
        return true;
    }
    let low = unpack_bound(record.bounds_min) - vec3(HIZ_PADDING);
    let high = unpack_bound(record.bounds_max) + vec3(HIZ_PADDING);
    let base = vec3<f32>(record_origin(record) - view.camera.xyz);
    var ndc_min = vec2(1.0e30);
    var ndc_max = vec2(-1.0e30);
    var nearest = 0.0;
    for (var corner = 0u; corner < 8u; corner++) {
        let pick = vec3((corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u);
        let clip = view.clip_from_rel * vec4(base + select(low, high, pick), 1.0);
        if (!(clip.w > 1.0e-4)) {
            return true;
        }
        let ndc = clip.xyz / clip.w;
        ndc_min = min(ndc_min, ndc.xy);
        ndc_max = max(ndc_max, ndc.xy);
        nearest = max(nearest, ndc.z);
    }
    if (strict && (any(ndc_min < vec2(-1.0)) || any(ndc_max > vec2(1.0)))) {
        return true;
    }
    let lo = clamp(ndc_min, vec2(-1.0), vec2(1.0));
    let hi = clamp(ndc_max, vec2(-1.0), vec2(1.0));
    let vp = view.viewport;
    let left = vp.x + (lo.x * 0.5 + 0.5) * vp.z;
    let right = vp.x + (hi.x * 0.5 + 0.5) * vp.z;
    let top = vp.y + (0.5 - hi.y * 0.5) * vp.w;
    let bottom = vp.y + (0.5 - lo.y * 0.5) * vp.w;
    let limit = vec2<f32>(view.depth.xy) - vec2(1.0);
    // One extra pixel each side absorbs rasteriser rounding.
    let p0 = vec2<u32>(clamp(floor(vec2(left, top)) - vec2(1.0), vec2(0.0), limit));
    let p1 = vec2<u32>(clamp(floor(vec2(right, bottom)) + vec2(1.0), vec2(0.0), limit));
    // The finest level where the rectangle spans at most 4x4 texels.
    var level = 0u;
    loop {
        let shift = level + 1u;
        let span = (p1 >> vec2(shift)) - (p0 >> vec2(shift));
        if ((span.x <= 3u && span.y <= 3u) || level + 1u >= view.depth.z) {
            break;
        }
        level++;
    }
    let shift = level + 1u;
    let last = textureDimensions(hiz, level) - vec2(1u);
    let t0 = min(p0 >> vec2(shift), last);
    let t1 = min(p1 >> vec2(shift), last);
    var farthest = 1.0;
    for (var y = t0.y; y <= t1.y; y++) {
        for (var x = t0.x; x <= t1.x; x++) {
            farthest = min(farthest, textureLoad(hiz, vec2(x, y), i32(level)).r);
        }
    }
    return nearest >= farthest;
}

fn hiz_visible(record: CullRecord) -> bool {
    return hiz_test(record, false);
}

fn facing_mask(origin: vec3<i32>) -> u32 {
    let flags = u32(view.camera.w);
    if ((flags & 1u) != 0u) {
        return 0x3fu;
    }
    var mask = 0u;
    for (var axis = 0u; axis < 3u; axis++) {
        let eye = view.camera[axis];
        let low = origin[axis];
        if (eye < low + SIDE) {
            mask |= 1u << (2u * axis);
        }
        if (eye > low || (eye == low && ((flags >> (axis + 1u)) & 1u) != 0u)) {
            mask |= 1u << (2u * axis + 1u);
        }
    }
    return mask;
}

fn solid_end(record: CullRecord, slot: u32) -> u32 {
    return (record.solid_ends[slot / 2u] >> (16u * (slot % 2u))) & 0xffffu;
}

fn solid_slot_face(slot: u32) -> u32 {
    return select(2u * (slot - 3u) + 1u, 2u * slot, slot < 3u);
}

// Counts merged facing runs, writing their args from `base` when `write` is set.
fn solid_runs(record: CullRecord, write: bool, base: u32) -> u32 {
    if (record.cube_start == record.cube_end) {
        return 0u;
    }
    let facing = facing_mask(record_origin(record));
    var runs = 0u;
    var run_start = 0u;
    var run_end = 0u;
    var open = false;
    for (var slot = 0u; slot < 6u; slot++) {
        var start = 0u;
        if (slot != 0u) {
            start = solid_end(record, slot - 1u);
        }
        let end = solid_end(record, slot);
        if ((facing & (1u << solid_slot_face(slot))) == 0u || start == end) {
            continue;
        }
        if (open && run_end == start) {
            run_end = end;
            continue;
        }
        if (open) {
            if (write) {
                write_args(base + runs, 0u, record, record.cube_start + run_start, run_end - run_start);
            }
            runs++;
        }
        run_start = start;
        run_end = end;
        open = true;
    }
    if (open) {
        if (write) {
            write_args(base + runs, 0u, record, record.cube_start + run_start, run_end - run_start);
        }
        runs++;
    }
    return runs;
}

fn write_args(index: u32, stream: u32, record: CullRecord, first: u32, count: u32) {
    let word = view.regions[stream] + index * 5u;
    args[word] = view.index_counts[stream];
    args[word + 1u] = count;
    args[word + 2u] = 0u;
    args[word + 3u] = bitcast<u32>(record.base_vertex);
    args[word + 4u] = first;
}

fn cutout_range(record: CullRecord) -> vec2<u32> {
    if (record.cube_start == record.cube_end) {
        return vec2(0u);
    }
    let first = record.cube_start + solid_end(record, 5u);
    return vec2(first, record.cube_end - first);
}

fn unpack_draws(draws: u32) -> vec4<u32> {
    return vec4(draws & 3u, (draws >> 2u) & 1u, (draws >> 3u) & 1u, (draws >> 4u) & 1u);
}

fn record_decision(slot: u32) -> u32 {
    let record = records[slot];
    let late = view.params.z == PHASE_LATE;
    let visible = record.live != 0u && slot_enabled(slot) && in_frustum(record_origin(record));
    var draw = false;
    if (late) {
        let kept = visible && hiz_visible(record);
        draw = kept && history[slot] == 0u;
        history[slot] = u32(kept);
    } else {
        draw = visible && history[slot] != 0u;
    }
    if (!draw) {
        return 0u;
    }
    let solid = solid_runs(record, false, 0u);
    let cutout = u32(cutout_range(record).y != 0u);
    let model = u32(record.model_count != 0u);
    let liquid = u32(record.liquid_count != 0u);
    return solid | (cutout << 2u) | (model << 3u) | (liquid << 4u);
}

fn inclusive_scan(lid: u32) {
    for (var step = 1u; step < WORKGROUP; step <<= 1u) {
        var value = sums[lid];
        if (lid >= step) {
            value += sums[lid - step];
        }
        workgroupBarrier();
        sums[lid] = value;
        workgroupBarrier();
    }
}

@compute @workgroup_size(WORKGROUP)
fn cull_count(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    var draws = 0u;
    if (gid.x < view.params.x) {
        draws = record_decision(gid.x);
        record_draws[gid.x] = draws;
    }
    sums[lid] = unpack_draws(draws);
    workgroupBarrier();
    for (var stride = WORKGROUP / 2u; stride > 0u; stride >>= 1u) {
        if (lid < stride) {
            sums[lid] += sums[lid + stride];
        }
        workgroupBarrier();
    }
    if (lid == 0u) {
        group_sums[wid.x] = sums[0];
    }
}

// One workgroup turns group totals into exclusive offsets and publishes the draw counts.
@compute @workgroup_size(WORKGROUP)
fn cull_scan(@builtin(local_invocation_index) lid: u32) {
    let groups = view.params.y;
    let per = (groups + WORKGROUP - 1u) / WORKGROUP;
    let begin = min(lid * per, groups);
    let end = min(begin + per, groups);
    var local = vec4(0u);
    for (var group = begin; group < end; group++) {
        local += group_sums[group];
    }
    sums[lid] = local;
    workgroupBarrier();
    inclusive_scan(lid);
    var running = sums[lid] - local;
    for (var group = begin; group < end; group++) {
        let total = group_sums[group];
        group_sums[group] = running;
        running += total;
    }
    if (lid == WORKGROUP - 1u) {
        let total = sums[lid];
        let first = view.params.w;
        draw_counts[first] = total.x;
        draw_counts[first + 1u] = total.y;
        draw_counts[first + 2u] = total.z;
        draw_counts[first + 3u] = total.w;
    }
}

@compute @workgroup_size(WORKGROUP)
fn cull_emit(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    var draws = 0u;
    if (gid.x < view.params.x) {
        draws = record_draws[gid.x];
    }
    let mine = unpack_draws(draws);
    sums[lid] = mine;
    workgroupBarrier();
    inclusive_scan(lid);
    if (draws == 0u) {
        return;
    }
    let offset = group_sums[wid.x] + sums[lid] - mine;
    let record = records[gid.x];
    solid_runs(record, true, offset.x);
    if (mine.y != 0u) {
        let cutout = cutout_range(record);
        write_args(offset.y, 1u, record, cutout.x, cutout.y);
    }
    if (mine.z != 0u) {
        write_args(offset.z, 2u, record, record.model_start, record.model_count);
    }
    if (mine.w != 0u) {
        write_args(offset.w, 3u, record, record.liquid_start, record.liquid_count);
    }
}

// Bit `slot` set when the live record is wholly on screen and behind this frame's pyramid.
@compute @workgroup_size(WORKGROUP)
fn cull_occlusion(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    if (lid < OCCLUSION_WORDS) {
        atomicStore(&occluded_bits[lid], 0u);
    }
    workgroupBarrier();
    if (gid.x < view.params.x) {
        let record = records[gid.x];
        if (record.live != 0u && !hiz_test(record, true)) {
            atomicOr(&occluded_bits[lid / 32u], 1u << (lid % 32u));
        }
    }
    workgroupBarrier();
    let word = wid.x * OCCLUSION_WORDS + lid;
    if (lid < OCCLUSION_WORDS && word < arrayLength(&occluded)) {
        occluded[word] = atomicLoad(&occluded_bits[lid]);
    }
}
