use super::super::*;

pub(in crate::compiler) fn kelp_quads(materials: [u32; 6]) -> [ModelQuad; 6] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    let diagonal_a = [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]];
    let diagonal_b = [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]];
    let reverse_a = [diagonal_a[1], diagonal_a[0], diagonal_a[3], diagonal_a[2]];
    let reverse_b = [diagonal_b[1], diagonal_b[0], diagonal_b[3], diagonal_b[2]];
    [
        ModelQuad {
            positions: diagonal_a,
            uvs,
            material: materials[0],
            flags: 0,
        },
        ModelQuad {
            positions: diagonal_b,
            uvs,
            material: materials[1],
            flags: 0,
        },
        ModelQuad {
            positions: reverse_a,
            uvs,
            material: materials[2],
            flags: 0,
        },
        ModelQuad {
            positions: reverse_b,
            uvs,
            material: materials[3],
            flags: 0,
        },
        ModelQuad {
            positions: diagonal_a,
            uvs,
            material: materials[4],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: diagonal_b,
            uvs,
            material: materials[5],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
    ]
}
