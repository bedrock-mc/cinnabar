@group(0) @binding(0) var<storage, read_write> units: array<vec4<u32>>;
@group(0) @binding(1) var<storage, read> occluded: array<u32>;

// The reference offset follows every retained descriptor, including unselected slots.
@compute @workgroup_size(MASK_WORKGROUP)
fn clear_units(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = id.x + id.y * groups.x * MASK_WORKGROUP;
    if (1u + index >= units[0].x) { return; }
    units[1u + index].w = 0u;
}

@compute @workgroup_size(MASK_WORKGROUP)
fn mask_units(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = id.x + id.y * groups.x * MASK_WORKGROUP;
    if (index >= units[0].y) { return; }
    let references = units[0].x;
    let slot = units[references + index / 4u][index % 4u];
    let origin = units[1u + slot].z;
    var hidden = 0u;
    if (origin / 32u < arrayLength(&occluded)) {
        hidden = (occluded[origin / 32u] >> (origin % 32u)) & 1u;
    }
    units[1u + slot].w = hidden;
}
