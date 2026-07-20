use super::super::*;

/// Emits only the four horizontal attachment planes represented by Bedrock's
/// `vine_direction_bits`. The pinned Dragonfly codec defines bit order as
/// south, west, north, east; protocol 1001 carries no up/down attachment bit.
pub(in crate::compiler) fn vine_quads(material: u32, connections: u32) -> Vec<ModelQuad> {
    debug_assert!(connections <= 15);
    const PLANES: [(u32, u32, [[i16; 3]; 4]); 4] = [
        (
            1,
            6,
            [[0, 0, 255], [256, 0, 255], [256, 256, 255], [0, 256, 255]],
        ),
        (2, 3, [[1, 0, 0], [1, 0, 256], [1, 256, 256], [1, 256, 0]]),
        (4, 5, [[0, 0, 1], [0, 256, 1], [256, 256, 1], [256, 0, 1]]),
        (
            8,
            4,
            [[255, 0, 0], [255, 256, 0], [255, 256, 256], [255, 0, 256]],
        ),
    ];
    PLANES
        .into_iter()
        .filter(|(bit, _, _)| connections & bit != 0)
        .map(|(_, face, positions)| ModelQuad {
            positions,
            uvs: positions.map(|[x, y, z]| {
                let tangent = if matches!(face, 5 | 6) { x } else { z };
                [(tangent as u16) * 16, ((256 - y) as u16) * 16]
            }),
            material,
            // Vines remain visible from either side. Deliberately omit the
            // cull-face field: the support block is not a reason to drop the
            // attachment plane before alpha testing.
            flags: MODEL_QUAD_FLAG_TWO_SIDED | face,
        })
        .collect()
}
