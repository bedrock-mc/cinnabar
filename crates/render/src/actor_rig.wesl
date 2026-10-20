// Actor rig skinning shared by the actor pass and the first-person hand pass: bones are 3x4
// row matrices, a vertex is posed between its previous and current bone transforms at the
// instance's partial tick, then placed by `world_from_actor`.

struct BoneMatrix {
    row_0: vec4<f32>,
    row_1: vec4<f32>,
    row_2: vec4<f32>,
}

fn transform_point(matrix: BoneMatrix, point: vec3<f32>) -> vec3<f32> {
    let homogeneous = vec4(point, 1.0);
    return vec3(
        dot(matrix.row_0, homogeneous),
        dot(matrix.row_1, homogeneous),
        dot(matrix.row_2, homogeneous),
    );
}

fn transform_direction(matrix: BoneMatrix, direction: vec3<f32>) -> vec3<f32> {
    return vec3(
        dot(matrix.row_0.xyz, direction),
        dot(matrix.row_1.xyz, direction),
        dot(matrix.row_2.xyz, direction),
    );
}

// A rig-local point posed between the previous and current bone transforms.
fn posed_point(previous: BoneMatrix, current: BoneMatrix, local: vec3<f32>, partial_tick: f32) -> vec3<f32> {
    return mix(transform_point(previous, local), transform_point(current, local), partial_tick);
}

// A rig-local normal posed between the previous and current bone transforms.
fn posed_normal(previous: BoneMatrix, current: BoneMatrix, local_normal: vec3<f32>, partial_tick: f32) -> vec3<f32> {
    return normalize(mix(
        transform_direction(previous, local_normal),
        transform_direction(current, local_normal),
        partial_tick,
    ));
}

// The instance's `world_from_actor` rows applied to a posed point.
fn placed_point(placement: BoneMatrix, posed: vec3<f32>) -> vec3<f32> {
    return transform_point(placement, posed);
}

// The instance's `world_from_actor` rows applied to a posed normal.
fn placed_normal(placement: BoneMatrix, posed: vec3<f32>) -> vec3<f32> {
    return normalize(transform_direction(placement, posed));
}
