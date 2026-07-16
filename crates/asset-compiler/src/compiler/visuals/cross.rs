use super::super::*;

pub(in crate::compiler) fn model_variant(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<u32> {
    let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
    let key = key?;
    pack.terrain
        .get_for_model_record(&key, record)
        .map(|(_, variant)| variant)
}

pub(in crate::compiler) fn crossed_quads(materials: [u32; 2]) -> [ModelQuad; 2] {
    let uvs = [[0, 4096], [4096, 4096], [4096, 0], [0, 0]];
    [
        ModelQuad {
            positions: [[0, 0, 0], [256, 0, 256], [256, 256, 256], [0, 256, 0]],
            uvs,
            material: materials[0],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
        ModelQuad {
            positions: [[256, 0, 0], [0, 0, 256], [0, 256, 256], [256, 256, 0]],
            uvs,
            material: materials[1],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        },
    ]
}
