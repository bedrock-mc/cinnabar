// Distant terrain culling and compaction. The prelude comes first and `hiz_test.wgsl` last.
//
// `lod_sections` tests every section box against the frustum and the Hi-Z pyramid, and hides a
// section above level 0 over any resident chunk column, as only level-0 sections may draw
// there. The opaque chain then tests every cluster slot (its section, crack list, facing,
// resident chunk columns, frustum and Hi-Z) and compacts, in slot order, every visible cluster
// into next frame's pass A list and the newly visible ones into this frame's pass B list, which
// share one buffer from both ends. The translucent chain tests the clusters of the far-to-near
// order and compacts them in that order. Counts clamp to the lists' room and the dropped
// entries are counted. Visible sections and opaque clusters are also binned by screen size,
// and the empty quad slots of visible opaque clusters counted, for the renderer's statistics.

@group(0) @binding(0) var<uniform> view: LodView;
@group(0) @binding(1) var<storage, read> sections: array<u32>;
@group(0) @binding(2) var<storage, read> clusters: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read_write> flags: array<u32>;
@group(0) @binding(4) var<storage, read_write> group_sums: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read_write> lists: array<u32>;
@group(0) @binding(6) var<storage, read_write> args: array<atomic<u32>>;
@group(0) @binding(7) var<storage, read> order: array<u32>;
@group(0) @binding(8) var hiz: texture_2d<f32>;

var<workgroup> sums: array<vec2<u32>, WORKGROUP>;
// One workgroup's statistics: the size bins, then the empty quad slots.
var<workgroup> stats: array<atomic<u32>, STAT_SCRATCH_WORDS>;

fn field(word: u32, shift: u32, mask: u32) -> u32 {
    return (word >> shift) & mask;
}

fn capacity() -> u32 {
    return view.counts.w;
}

fn section_word(slot: u32, word: u32) -> u32 {
    return sections[slot * SECTION_RECORD_WORDS + word];
}

// The section's minimum corner in blocks relative to the integer eye.
fn section_origin(slot: u32, level: u32) -> vec3<f32> {
    let cells = vec3(
        bitcast<i32>(section_word(slot, 0u)),
        bitcast<i32>(section_word(slot, 1u)),
        bitcast<i32>(section_word(slot, 2u)),
    );
    let shift = vec3(firstTrailingBit(SECTION_SIDE) + level);
    return vec3<f32>((cells << shift) - view.camera.xyz);
}

// The box test against the left, right, bottom, top and near planes.
fn in_frustum(low: vec3<f32>, high: vec3<f32>) -> bool {
    for (var index = 0u; index < 5u; index++) {
        let plane = view.planes[index];
        let corner = select(low, high, plane.xyz >= vec3(0.0));
        if (dot(plane.xyz, corner) + plane.w < 0.0) {
            return false;
        }
    }
    return true;
}

fn box_visible(low: vec3<f32>, high: vec3<f32>) -> bool {
    return in_frustum(low, high)
        && hiz_box_test(low - vec3(HIZ_PADDING), high + vec3(HIZ_PADDING), false);
}

// True when `list` is drawn on `face` of a section with neighbour levels `info`.
fn list_drawn(info: u32, face: u32, list: u32) -> bool {
    if (list == LIST_MAIN) {
        return true;
    }
    let neighbours = field(info, SECTION_NEIGHBOURS_SHIFT, SECTION_NEIGHBOURS_MASK);
    let code = field(neighbours, face * NEIGHBOUR_LEVEL_BITS, (1u << NEIGHBOUR_LEVEL_BITS) - 1u);
    return (list == LIST_TO_COARSER && code == NEIGHBOUR_COARSER)
        || (list == LIST_TO_FINER && code == NEIGHBOUR_FINER);
}

fn column_resident(column: vec2<i32>) -> bool {
    let offset = abs(column - view.resident.xy);
    if (view.resident.w == 0 || any(offset >= vec2(view.resident.z))) {
        return false;
    }
    let side = RESIDENT_MASK_SIDE;
    let bit = (bitcast<u32>(column.y) & (side - 1u)) * side + (bitcast<u32>(column.x) & (side - 1u));
    let word = sections[MAX_SECTIONS * SECTION_RECORD_WORDS + bit / 32u];
    return ((word >> (bit % 32u)) & 1u) != 0u;
}

// True when any chunk column under the footprint of the section in `slot`, at `level`, is
// resident. Mask words hold 32 columns of a row, so each row reads a word per 32 columns.
fn any_column_resident(slot: u32, level: u32) -> bool {
    if (view.resident.w == 0) {
        return false;
    }
    let span = i32(SECTION_CHUNKS << level);
    let base = vec2(
        bitcast<i32>(section_word(slot, 0u)) * span,
        bitcast<i32>(section_word(slot, 2u)) * span,
    );
    // Columns the mask answers lie less than its radius from its centre.
    let low = max(base, view.resident.xy - vec2(view.resident.z - 1));
    let high = min(base + vec2(span), view.resident.xy + vec2(view.resident.z));
    let side = RESIDENT_MASK_SIDE;
    for (var z = low.y; z < high.y; z++) {
        let row = (bitcast<u32>(z) & (side - 1u)) * side;
        var x = low.x;
        while (x < high.x) {
            let bit = row + (bitcast<u32>(x) & (side - 1u));
            let shift = bit % 32u;
            let count = min(u32(high.x - x), 32u - shift);
            let word = sections[MAX_SECTIONS * SECTION_RECORD_WORDS + bit / 32u] >> shift;
            if ((word & select((1u << count) - 1u, 0xffffffffu, count == 32u)) != 0u) {
                return true;
            }
            x += i32(count);
        }
    }
    return false;
}

// The screen rectangle's size in pixels of a camera-relative box, clamped to the viewport; a
// box reaching behind the eye spans the whole viewport.
fn screen_size(low: vec3<f32>, high: vec3<f32>) -> vec2<f32> {
    var ndc_min = vec2(1.0e30);
    var ndc_max = vec2(-1.0e30);
    for (var corner = 0u; corner < 8u; corner++) {
        let pick = vec3((corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u);
        let clip = view.clip_from_rel * vec4(select(low, high, pick), 1.0);
        if (!(clip.w > 1.0e-4)) {
            return view.viewport.zw;
        }
        ndc_min = min(ndc_min, clip.xy / clip.w);
        ndc_max = max(ndc_max, clip.xy / clip.w);
    }
    let span = clamp(ndc_max, vec2(-1.0), vec2(1.0)) - clamp(ndc_min, vec2(-1.0), vec2(1.0));
    return max(span, vec2(0.0)) * 0.5 * view.viewport.zw;
}

// The bin of `size`: 0 below `unit`, then one per doubling, the last open.
fn size_bin(size: f32, unit: f32) -> u32 {
    if (!(size >= unit)) {
        return 0u;
    }
    return min(1u + u32(floor(log2(size / unit))), SIZE_BINS - 1u);
}

// Clears the workgroup's statistics; every thread calls it before its first barrier.
fn clear_stats(lid: u32) {
    if (lid < STAT_SCRATCH_WORDS) {
        atomicStore(&stats[lid], 0u);
    }
}

// Adds the workgroup's statistics to the args words from `first`; call after a barrier that
// follows every update.
fn flush_stats(lid: u32, first: u32, words: u32) {
    if (lid < words) {
        let value = atomicLoad(&stats[lid]);
        if (value != 0u) {
            atomicAdd(&args[first + lid], value);
        }
    }
}

// True when every chunk column a level-0 cluster's voxels lie in is resident.
fn columns_resident(slot: u32, columns: u32) -> bool {
    let chunks = i32(SECTION_CHUNKS);
    let base = vec2(
        bitcast<i32>(section_word(slot, 0u)) * chunks,
        bitcast<i32>(section_word(slot, 2u)) * chunks,
    );
    for (var bit = 0u; bit < 4u; bit++) {
        if ((columns & (1u << bit)) != 0u
            && !column_resident(base + vec2(i32(bit & 1u), i32(bit >> 1u)))) {
            return false;
        }
    }
    return true;
}

// True when the cluster in slot `cluster` of `bucket` is drawn; `binned` adds a visible one to
// the workgroup's statistics.
fn cluster_visible(cluster: u32, bucket: u32, binned: bool) -> bool {
    let record = clusters[cluster];
    let slot = field(record.y, CLUSTER_SECTION_SHIFT, CLUSTER_SECTION_MASK);
    if (slot == 0u || slot >= view.counts.x || flags[3u * capacity() + slot] == 0u) {
        return false;
    }
    if (field(record.y, CLUSTER_BUCKET_SHIFT, CLUSTER_BUCKET_MASK) != bucket) {
        return false;
    }
    let info = section_word(slot, SECTION_INFO_WORD);
    let face = field(record.y, CLUSTER_FACE_SHIFT, CLUSTER_FACE_MASK);
    if (!list_drawn(info, face, field(record.y, CLUSTER_LIST_SHIFT, CLUSTER_LIST_MASK))) {
        return false;
    }
    let level = field(info, SECTION_LEVEL_SHIFT, SECTION_LEVEL_MASK);
    let scale = f32(1u << level);
    let origin = section_origin(slot, level);
    let low = origin + vec3<f32>(vec3(
        field(record.z, CLUSTER_MIN_MIN_X_SHIFT, CLUSTER_MIN_MIN_X_MASK),
        field(record.z, CLUSTER_MIN_MIN_Y_SHIFT, CLUSTER_MIN_MIN_Y_MASK),
        field(record.z, CLUSTER_MIN_MIN_Z_SHIFT, CLUSTER_MIN_MIN_Z_MASK),
    )) * scale;
    let high = origin + vec3<f32>(vec3(
        field(record.w, CLUSTER_MAX_MAX_X_SHIFT, CLUSTER_MAX_MAX_X_MASK),
        field(record.w, CLUSTER_MAX_MAX_Y_SHIFT, CLUSTER_MAX_MAX_Y_MASK),
        field(record.w, CLUSTER_MAX_MAX_Z_SHIFT, CLUSTER_MAX_MAX_Z_MASK),
    )) * scale;
    // Every quad of a cluster faces one way, on planes spanning its bounds: the cluster is
    // hidden only when the camera is behind its nearest-facing plane.
    let axis = face >> 1u;
    let plane = select(high[axis], low[axis], (face & 1u) == 1u) - view.camera_frac[axis];
    if (((face & 1u) == 1u && plane >= 0.0) || ((face & 1u) == 0u && plane <= 0.0)) {
        return false;
    }
    let columns = field(record.z, CLUSTER_MIN_CHUNK_COLUMNS_SHIFT, CLUSTER_MIN_CHUNK_COLUMNS_MASK);
    if (level == 0u && columns != 0u && columns_resident(slot, columns)) {
        return false;
    }
    if (!box_visible(low, high)) {
        return false;
    }
    if (binned) {
        let size = screen_size(low, high);
        atomicAdd(&stats[size_bin(max(size.x, size.y), CLUSTER_SIZE_UNIT_PX)], 1u);
        let empty = CLUSTER_QUADS - 1u - field(record.y, CLUSTER_COUNT_SHIFT, CLUSTER_COUNT_MASK);
        atomicAdd(&stats[SIZE_BINS], empty);
    }
    return true;
}

@compute @workgroup_size(WORKGROUP)
fn lod_sections(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {
    clear_stats(lid);
    workgroupBarrier();
    let slot = gid.x;
    if (slot < view.counts.x) {
        let info = section_word(slot, SECTION_INFO_WORD);
        var visible = false;
        if (slot != 0u && field(info, SECTION_LIVE_SHIFT, SECTION_LIVE_MASK) != 0u) {
            let level = field(info, SECTION_LEVEL_SHIFT, SECTION_LEVEL_MASK);
            let low = section_origin(slot, level);
            let high = low + vec3(f32(SECTION_SIDE << level));
            visible = (level == 0u || !any_column_resident(slot, level)) && box_visible(low, high);
            if (visible) {
                let size = screen_size(low, high);
                atomicAdd(&stats[size_bin(sqrt(size.x * size.y), SECTION_SIZE_UNIT_PX)], 1u);
            }
        }
        flags[3u * capacity() + slot] = u32(visible);
    }
    workgroupBarrier();
    flush_stats(lid, ARGS_SECTION_BINS, SIZE_BINS);
}

fn reduce_group(lid: u32, wid: u32) {
    workgroupBarrier();
    for (var stride = WORKGROUP / 2u; stride > 0u; stride >>= 1u) {
        if (lid < stride) {
            sums[lid] += sums[lid + stride];
        }
        workgroupBarrier();
    }
    if (lid == 0u) {
        group_sums[wid] = sums[0];
    }
}

fn inclusive_scan(lid: u32) {
    workgroupBarrier();
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

// One workgroup turns the totals of `count` items' groups into exclusive offsets; the last
// thread returns the grand total.
fn scan_groups(lid: u32, count: u32) -> vec2<u32> {
    let groups = (count + WORKGROUP - 1u) / WORKGROUP;
    let per = (groups + WORKGROUP - 1u) / WORKGROUP;
    let begin = min(lid * per, groups);
    let end = min(begin + per, groups);
    var local = vec2(0u);
    for (var group = begin; group < end; group++) {
        local += group_sums[group];
    }
    sums[lid] = local;
    inclusive_scan(lid);
    var running = sums[lid] - local;
    for (var group = begin; group < end; group++) {
        let total = group_sums[group];
        group_sums[group] = running;
        running += total;
    }
    return sums[lid];
}

fn write_draw(at: u32, instances: u32) {
    atomicStore(&args[at], INDEX_COUNT);
    atomicStore(&args[at + 1u], instances);
    atomicStore(&args[at + 2u], 0u);
    atomicStore(&args[at + 3u], 0u);
    atomicStore(&args[at + 4u], 0u);
}

@compute @workgroup_size(WORKGROUP)
fn lod_count_opaque(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    clear_stats(lid);
    workgroupBarrier();
    var decision = vec2(0u);
    if (gid.x < view.counts.y) {
        let visible = cluster_visible(gid.x, BUCKET_OPAQUE, true);
        let drawn_in_a = flags[capacity() + gid.x] != 0u;
        decision = vec2(u32(visible), u32(visible && !drawn_in_a));
        flags[gid.x] = decision.x | (decision.y << 1u);
    }
    sums[lid] = decision;
    reduce_group(lid, wid.x);
    flush_stats(lid, ARGS_CLUSTER_BINS, STAT_SCRATCH_WORDS);
}

@compute @workgroup_size(WORKGROUP)
fn lod_scan_opaque(@builtin(local_invocation_index) lid: u32) {
    let total = scan_groups(lid, view.counts.y);
    if (lid == WORKGROUP - 1u) {
        let next_a = min(total.x, capacity());
        let b = min(total.y, capacity() - next_a);
        write_draw(ARGS_PASS_A, next_a);
        write_draw(ARGS_PASS_B, b);
        atomicStore(&args[ARGS_STATS], total.x);
        atomicStore(&args[ARGS_STATS + 1u], total.y);
        atomicStore(&args[ARGS_STATS + 3u], (total.x - next_a) + (total.y - b));
    }
}

@compute @workgroup_size(WORKGROUP)
fn lod_emit_opaque(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    var decision = vec2(0u);
    if (gid.x < view.counts.y) {
        let packed = flags[gid.x];
        decision = vec2(packed & 1u, (packed >> 1u) & 1u);
    }
    sums[lid] = decision;
    inclusive_scan(lid);
    if (gid.x >= view.counts.y) {
        return;
    }
    let at = group_sums[wid.x] + sums[lid] - decision;
    var listed = 0u;
    if (decision.x != 0u && at.x < atomicLoad(&args[ARGS_PASS_A + 1u])) {
        lists[at.x] = gid.x;
        listed = 1u;
    }
    if (decision.y != 0u && at.y < atomicLoad(&args[ARGS_PASS_B + 1u])) {
        lists[capacity() - 1u - at.y] = gid.x;
    }
    // Only clusters next frame's pass A draws count as drawn there.
    flags[capacity() + gid.x] = listed;
}

@compute @workgroup_size(WORKGROUP)
fn lod_count_translucent(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    var visible = 0u;
    if (gid.x < view.counts.z) {
        let cluster = order[gid.x];
        visible = u32(cluster < view.counts.y && cluster_visible(cluster, BUCKET_TRANSLUCENT, false));
        flags[2u * capacity() + gid.x] = visible;
    }
    sums[lid] = vec2(visible, 0u);
    reduce_group(lid, wid.x);
}

@compute @workgroup_size(WORKGROUP)
fn lod_scan_translucent(@builtin(local_invocation_index) lid: u32) {
    let total = scan_groups(lid, view.counts.z);
    if (lid == WORKGROUP - 1u) {
        let drawn = min(total.x, capacity());
        write_draw(ARGS_TRANSLUCENT, drawn);
        atomicStore(&args[ARGS_STATS + 2u], total.x);
        atomicAdd(&args[ARGS_STATS + 3u], total.x - drawn);
    }
}

@compute @workgroup_size(WORKGROUP)
fn lod_emit_translucent(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    var visible = 0u;
    if (gid.x < view.counts.z) {
        visible = flags[2u * capacity() + gid.x];
    }
    sums[lid] = vec2(visible, 0u);
    inclusive_scan(lid);
    if (gid.x >= view.counts.z || visible == 0u) {
        return;
    }
    let at = group_sums[wid.x].x + sums[lid].x - visible;
    if (at < atomicLoad(&args[ARGS_TRANSLUCENT + 1u])) {
        lists[capacity() + at] = order[gid.x];
    }
}
