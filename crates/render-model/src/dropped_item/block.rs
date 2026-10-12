use super::{ItemMeshVertex, MAX_ITEM_LAYERS};
use crate::DroppedItemBlock;
use glam::Vec3;

/// Number of fixed-point UV units in one material tile.
pub(super) const UV_TILE_UNITS: u16 = 4096;

/// Expands the retained block templates into the ordinary lit item vertex format.
pub fn block_mesh(
    model: &DroppedItemBlock,
    first_layer: u32,
    layer_side: u32,
) -> Option<Vec<ItemMeshVertex>> {
    if layer_side == 0 || first_layer as usize + model.materials.len() > MAX_ITEM_LAYERS {
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
        if texture.width > layer_side || texture.height > layer_side {
            return None;
        }
        let Some(normal) = quad_normal(quad) else {
            continue;
        };
        let [a, b, c, d] = quad_order(quad, normal);
        let glint_scale = crate::ActorGlint::item(
            0.0,
            [
                texture.width as f32 / layer_side as f32,
                texture.height as f32 / layer_side as f32,
            ],
        )
        .uv_scale;
        for corner in [a, b, c, a, c, d] {
            vertices.push(ItemMeshVertex {
                position: rotate(quad.positions[corner].map(|p| f32::from(p) / 256.0 - 0.5)),
                uv: [
                    f32::from(quad.uvs[corner][0]) / f32::from(UV_TILE_UNITS)
                        * texture.width as f32
                        / layer_side as f32,
                    f32::from(quad.uvs[corner][1]) / f32::from(UV_TILE_UNITS)
                        * texture.height as f32
                        / layer_side as f32,
                ],
                normal: rotate(normal),
                layer: first_layer + quad.material,
                color: *color,
                glint_scale,
            });
        }
    }
    Some(vertices)
}

/// Returns the authored face normal, or the geometric normal for an unlabelled quad.
pub(super) fn quad_normal(quad: &assets::ModelQuad) -> Option<[f32; 3]> {
    Some(match quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK {
        1 => [0.0, -1.0, 0.0],
        2 => [0.0, 1.0, 0.0],
        3 => [-1.0, 0.0, 0.0],
        4 => [1.0, 0.0, 0.0],
        5 => [0.0, 0.0, -1.0],
        6 => [0.0, 0.0, 1.0],
        _ => {
            let [a, b, c, _] = quad.positions.map(|p| Vec3::from_array(p.map(f32::from)));
            (b - a).cross(c - a).try_normalize()?.to_array()
        }
    })
}

/// Orders the original corners about the normal while retaining every position and UV pair.
pub(super) fn quad_order(quad: &assets::ModelQuad, normal: [f32; 3]) -> [usize; 4] {
    let [a, b, c, _] = quad.positions.map(|p| Vec3::from_array(p.map(f32::from)));
    if (b - a).cross(c - a).dot(Vec3::from_array(normal)) < 0.0 {
        [0, 3, 2, 1]
    } else {
        [0, 1, 2, 3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn differently_sized_block_materials_keep_the_same_glint_coverage() {
        let quad = assets::ModelQuad {
            positions: [[0, 0, 128], [256, 0, 128], [256, 256, 128], [0, 256, 128]],
            uvs: [
                [0, 0],
                [UV_TILE_UNITS, 0],
                [UV_TILE_UNITS, UV_TILE_UNITS],
                [0, UV_TILE_UNITS],
            ],
            material: 0,
            flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
        };
        let materials = [(16, 32), (32, 16)].map(|(width, height)| {
            (
                crate::DroppedItemSprite {
                    width,
                    height,
                    rgba8: vec![255; (width * height * 4) as usize].into(),
                },
                crate::OPAQUE_WHITE,
            )
        });
        let model = DroppedItemBlock {
            materials: materials.into(),
            quads: Arc::from([
                quad,
                assets::ModelQuad {
                    material: 1,
                    ..quad
                },
            ]),
            rotation: 0,
        };
        let vertices = block_mesh(&model, 1, 32).unwrap();
        for (first, second) in vertices[..6].iter().zip(&vertices[6..]) {
            assert_ne!(first.glint_scale, second.glint_scale);
            for axis in 0..2 {
                assert_eq!(
                    first.uv[axis] * first.glint_scale[axis],
                    second.uv[axis] * second.glint_scale[axis]
                );
            }
        }
        let covered_span = |face: &[ItemMeshVertex], axis| {
            let max = face
                .iter()
                .map(|v| v.uv[axis] * v.glint_scale[axis])
                .fold(0.0, f32::max);
            let min = face
                .iter()
                .map(|v| v.uv[axis] * v.glint_scale[axis])
                .fold(f32::INFINITY, f32::min);
            max - min
        };
        for face in vertices.as_chunks::<6>().0 {
            for axis in 0..2 {
                assert_eq!(covered_span(face, axis), crate::ITEM_GLINT_UV_SCALE);
            }
        }
    }

    #[test]
    fn quantized_collapsed_faces_do_not_hide_drawable_faces() {
        let drawable = assets::ModelQuad {
            positions: [[36, 64, 36], [220, 64, 220], [220, 256, 220], [36, 256, 36]],
            uvs: [
                [0, UV_TILE_UNITS],
                [UV_TILE_UNITS, UV_TILE_UNITS],
                [UV_TILE_UNITS, 0],
                [0, 0],
            ],
            material: 0,
            flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
        };
        let collapsed = assets::ModelQuad {
            positions: [
                [128, 64, 128],
                [128, 64, 128],
                [128, 256, 128],
                [128, 256, 128],
            ],
            ..drawable
        };
        let model = DroppedItemBlock {
            materials: Arc::from([(
                crate::DroppedItemSprite {
                    width: 16,
                    height: 16,
                    rgba8: Arc::from([255; 16 * 16 * 4]),
                },
                crate::OPAQUE_WHITE,
            )]),
            quads: Arc::from([drawable, collapsed, drawable]),
            rotation: 0,
        };
        let vertices = block_mesh(&model, 1, 32).expect("drawable faces survive quantization");
        assert_eq!(vertices.len(), 12);
        assert!(
            vertices
                .iter()
                .all(|vertex| vertex.normal.iter().all(|axis| axis.is_finite()))
        );
        assert_eq!(vertices[0].position, [-0.359375, -0.25, -0.359375]);
        assert_eq!(vertices[6].position, vertices[0].position);
    }

    #[test]
    fn potted_planes_have_finite_unit_normals() {
        let model = DroppedItemBlock {
            materials: Arc::from([(
                crate::DroppedItemSprite {
                    width: 16,
                    height: 16,
                    rgba8: Arc::from([255; 16 * 16 * 4]),
                },
                crate::OPAQUE_WHITE,
            )]),
            quads: Arc::from([assets::ModelQuad {
                positions: [[36, 64, 36], [220, 64, 220], [220, 256, 220], [36, 256, 36]],
                uvs: [
                    [0, UV_TILE_UNITS],
                    [UV_TILE_UNITS, UV_TILE_UNITS],
                    [UV_TILE_UNITS, 0],
                    [0, 0],
                ],
                material: 0,
                flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
            }]),
            rotation: 0,
        };
        let vertices = block_mesh(&model, 1, 32).unwrap();
        assert_eq!(vertices.len(), 6);
        for vertex in vertices {
            assert!(vertex.normal.iter().all(|value| value.is_finite()));
            let squared_length: f32 = vertex.normal.iter().map(|value| value * value).sum();
            assert!((squared_length - 1.0).abs() < 1e-5);
            assert_eq!(vertex.normal[1], 0.0);
        }
    }
}
