use super::{ItemMeshVertex, MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE};
use render_model::DroppedItemBlock;

/// Expands the retained block templates into the ordinary lit item vertex format.
pub(crate) fn block_mesh(
    model: &DroppedItemBlock,
    first_layer: u32,
) -> Option<Vec<ItemMeshVertex>> {
    if first_layer as usize + model.materials.len() > MAX_ITEM_LAYERS {
        return None;
    }
    let rotate = |[x, y, z]: [f32; 3]| match model.rotation & 3 {
        1 => [-z, y, x],
        2 => [-x, y, -z],
        3 => [z, y, -x],
        _ => [x, y, z],
    };
    let mut vertices = Vec::with_capacity(model.quads.len() * 6);
    for quad in model.quads.iter() {
        let (texture, color) = model.materials.get(quad.material as usize)?;
        let normal = match quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK {
            1 => [0.0, -1.0, 0.0],
            2 => [0.0, 1.0, 0.0],
            3 => [-1.0, 0.0, 0.0],
            4 => [1.0, 0.0, 0.0],
            5 => [0.0, 0.0, -1.0],
            6 => [0.0, 0.0, 1.0],
            _ => [0.0; 3],
        };
        for corner in [0, 1, 2, 0, 2, 3] {
            vertices.push(ItemMeshVertex {
                position: rotate(quad.positions[corner].map(|p| f32::from(p) / 256.0 - 0.5)),
                uv: [
                    f32::from(quad.uvs[corner][0]) / 4096.0 * texture.width as f32
                        / MAX_ITEM_SPRITE_SIDE as f32,
                    f32::from(quad.uvs[corner][1]) / 4096.0 * texture.height as f32
                        / MAX_ITEM_SPRITE_SIDE as f32,
                ],
                normal: rotate(normal),
                layer: first_layer + quad.material,
                color: *color,
            });
        }
    }
    Some(vertices)
}
