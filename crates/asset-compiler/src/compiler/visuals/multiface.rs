use super::super::*;

/// Emits Bedrock's six independent multi-face attachment planes. This is a
/// distinct state contract from vines: bits include down/up and mask zero is
/// canonicalized by the caller to the all-face fallback.
pub(in crate::compiler) fn multiface_quads(
    material: u32,
    connections: u32,
    family: ModelFamily,
) -> Vec<ModelQuad> {
    debug_assert!((1..=63).contains(&connections));
    const GLOW_LICHEN_PLANES: [(u32, u32, [[i16; 3]; 4]); 6] = [
        (1, 1, [[0, 1, 0], [0, 1, 256], [256, 1, 256], [256, 1, 0]]),
        (
            2,
            2,
            [[0, 255, 0], [256, 255, 0], [256, 255, 256], [0, 255, 256]],
        ),
        (
            4,
            6,
            [[0, 0, 255], [256, 0, 255], [256, 256, 255], [0, 256, 255]],
        ),
        (8, 3, [[1, 0, 0], [1, 0, 256], [1, 256, 256], [1, 256, 0]]),
        (16, 5, [[0, 0, 1], [0, 256, 1], [256, 256, 1], [256, 0, 1]]),
        (
            32,
            4,
            [[255, 0, 0], [255, 256, 0], [255, 256, 256], [255, 0, 256]],
        ),
    ];
    const SCULK_VEIN_PLANES: [(u32, u32, [[i16; 3]; 4]); 6] = [
        GLOW_LICHEN_PLANES[0],
        GLOW_LICHEN_PLANES[1],
        (4, 5, GLOW_LICHEN_PLANES[4].2),
        (8, 6, GLOW_LICHEN_PLANES[2].2),
        (16, 3, GLOW_LICHEN_PLANES[3].2),
        (32, 4, GLOW_LICHEN_PLANES[5].2),
    ];
    let planes = match family {
        ModelFamily::GlowLichen | ModelFamily::ResinClump => GLOW_LICHEN_PLANES,
        ModelFamily::SculkVein => SCULK_VEIN_PLANES,
        _ => unreachable!("multiface geometry requested for an unrelated family"),
    };
    planes
        .into_iter()
        .filter(|(bit, _, _)| connections & bit != 0)
        .map(|(_, face, positions)| ModelQuad {
            positions,
            uvs: positions.map(|[x, y, z]| {
                if matches!(face, 1 | 2) {
                    [(x as u16) * 16, (z as u16) * 16]
                } else {
                    let tangent = if matches!(face, 5 | 6) { x } else { z };
                    [(tangent as u16) * 16, ((256 - y) as u16) * 16]
                }
            }),
            material,
            // Both families are paper-thin overlays. Support faces must not
            // remove them before alpha testing, and either side remains visible.
            flags: MODEL_QUAD_FLAG_TWO_SIDED | face,
        })
        .collect()
}
