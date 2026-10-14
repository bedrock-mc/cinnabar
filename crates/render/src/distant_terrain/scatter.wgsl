// Applies one frame's staged writes: one workgroup per command copies a run of staged words,
// or fills a run with one word, into its destination buffer. The prelude comes first.

@group(0) @binding(0) var<storage, read> staging: array<u32>;
@group(0) @binding(1) var<storage, read_write> pool: array<u32>;
@group(0) @binding(2) var<storage, read_write> clusters: array<u32>;
@group(0) @binding(3) var<storage, read_write> sections: array<u32>;
@group(0) @binding(4) var<storage, read_write> flags: array<u32>;
@group(0) @binding(5) var<storage, read_write> order: array<u32>;

fn put(destination: u32, index: u32, value: u32) {
    switch destination {
        case TARGET_POOL: {
            pool[index] = value;
        }
        case TARGET_CLUSTERS: {
            clusters[index] = value;
        }
        case TARGET_SECTIONS: {
            sections[index] = value;
        }
        case TARGET_FLAGS: {
            flags[index] = value;
        }
        case TARGET_ORDER: {
            order[index] = value;
        }
        default: {}
    }
}

@compute @workgroup_size(SCATTER_WORKGROUP)
fn lod_scatter(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {
    if (wid.x >= staging[0]) {
        return;
    }
    let base = STAGING_HEADER_WORDS + wid.x * COMMAND_WORDS;
    let destination = staging[base];
    let first = staging[base + 1u];
    let source = staging[base + 2u];
    let raw = staging[base + 3u];
    let fill = (raw & FILL_FLAG) != 0u;
    let len = raw & ~FILL_FLAG;
    for (var index = lid; index < len; index += SCATTER_WORKGROUP) {
        var value = source;
        if (!fill) {
            value = staging[source + index];
        }
        put(destination, first + index, value);
    }
}
