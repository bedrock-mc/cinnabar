use crate::{
    BlockFace, BlockFlags, BlockVisualId, ModelQuad, NetworkIdMode, RuntimeAssets, VisualKind,
    VisualSupport,
};

/// Returns an exact occluding model's six outward unit-cube faces in canonical face order.
/// Geometry, sprite corners and per-face material ownership are checked without allocation;
/// callers separately admit the materials they can render.
pub fn full_cube_model_quads(
    runtime: &RuntimeAssets,
    visual: BlockVisualId,
) -> Option<&[ModelQuad; 6]> {
    let block = runtime.resolve(NetworkIdMode::Sequential, visual.0);
    if !block.is_known()
        || block.kind() != VisualKind::Model
        || block.support() != VisualSupport::Exact
        || !block.flags().contains(BlockFlags::OCCLUDES_FULL_FACE)
        || block.variant() != 0
    {
        return None;
    }
    let template = runtime
        .model_templates()
        .get(block.model_template()? as usize)?;
    if template.flags != 0 || template.quad_count != BlockFace::ALL.len() as u32 {
        return None;
    }
    let start = template.quad_start as usize;
    let quads: &[ModelQuad; 6] = runtime
        .model_quads()
        .get(start..start.checked_add(BlockFace::ALL.len())?)?
        .try_into()
        .ok()?;
    for (index, (face, quad)) in BlockFace::ALL.into_iter().zip(quads).enumerate() {
        if !crate::model::unit_cube_quad_geometry_is_valid(
            index,
            quad.positions,
            quad.flags & crate::MODEL_QUAD_FLAG_FACE_MASK,
        ) || quad.flags
            & !(crate::MODEL_QUAD_FLAG_FACE_MASK | crate::MODEL_QUAD_FLAG_CULL_FACE_MASK)
            != 0
            || quad.material != block.face(face).material_id()
            || quad.material == crate::DIAGNOSTIC_MATERIAL
            || quad
                .uvs
                .iter()
                .flatten()
                .any(|component| !matches!(component, 0 | 4096))
        {
            return None;
        }
        // Every edge spans exactly one sprite axis, so folded or collapsed UVs fail closed.
        for edge in 0..4 {
            let [u, v] = quad.uvs[edge];
            let [next_u, next_v] = quad.uvs[(edge + 1) % 4];
            if (u == next_u) == (v == next_v) {
                return None;
            }
            if quad.uvs[..edge].contains(&quad.uvs[edge]) {
                return None;
            }
        }
    }
    Some(quads)
}
