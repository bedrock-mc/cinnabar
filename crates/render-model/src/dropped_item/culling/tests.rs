use super::*;
use crate::dropped_item::{
    block_mesh, cube_mesh, extruded_sprite_mesh, native_dropped_sprite_mesh,
};
use crate::{DroppedItemCube, ItemMeshVertex, OPAQUE_WHITE};
use std::sync::Arc;

/// Builds a block fixture from the emitted cube faces, including their original winding.
fn block_cube() -> DroppedItemBlock {
    let mesh = cube_mesh([0; 6], [OPAQUE_WHITE; 6], 1, 1).unwrap();
    let quads: Vec<_> = mesh
        .as_chunks::<6>()
        .0
        .iter()
        .map(|face| {
            let indices = if face[1].position == face[5].position {
                [0, 4, 1, 2]
            } else {
                [0, 1, 2, 5]
            };
            let [x, y, z] = face[0].normal;
            let flags = if y < 0.0 {
                1
            } else if y > 0.0 {
                2
            } else if x < 0.0 {
                3
            } else if x > 0.0 {
                4
            } else if z < 0.0 {
                5
            } else {
                6
            };
            assets::ModelQuad {
                positions: indices.map(|i| face[i].position.map(|v| ((v + 0.5) * 256.0) as i16)),
                uvs: [
                    [0, 0],
                    [block::UV_TILE_UNITS, 0],
                    [block::UV_TILE_UNITS, block::UV_TILE_UNITS],
                    [0, block::UV_TILE_UNITS],
                ],
                material: 0,
                flags,
            }
        })
        .collect();
    DroppedItemBlock {
        materials: Arc::from([(
            DroppedItemSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([255; 4]),
            },
            OPAQUE_WHITE,
        )]),
        quads: quads.into(),
        rotation: 0,
    }
}

/// Checks the geometric winding of every emitted triangle against its shading normal.
fn assert_outward(mesh: &[ItemMeshVertex]) {
    assert_eq!(mesh.len() % 3, 0);
    for triangle in mesh.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|v| Vec3::from_array(v.position));
        assert!(
            (b - a)
                .cross(c - a)
                .dot(Vec3::from_array(triangle[0].normal))
                > 0.0
        );
    }
}

#[test]
fn every_closed_builder_emits_outward_triangles_including_holes_and_native_mirroring() {
    assert_outward(&cube_mesh([0; 6], [OPAQUE_WHITE; 6], 1, 1).unwrap());
    for (width, height) in [(1, 1), (3, 3), (4, 2), (2, 4)] {
        let mut pixels = vec![255; width * height * 4];
        if width > 1 && height > 1 {
            pixels[(width + 1) * 4 + 3] = 0;
        }
        assert_outward(&extruded_sprite_mesh(width as u32, height as u32, &pixels, 32, 0).unwrap());
        assert_outward(
            &native_dropped_sprite_mesh(width as u32, height as u32, &pixels, 32, 0).unwrap(),
        );
    }
    let mut block = block_cube();
    assert!(DroppedItemModel::Block(block.clone()).can_cull_back_faces());
    for rotation in 0..4 {
        block.rotation = rotation;
        assert_outward(&block_mesh(&block, 0, 32).unwrap());
    }
}

#[test]
fn reversed_authored_faces_keep_their_position_uv_pairs_and_gain_outward_winding() {
    let mut block = block_cube();
    let expected = block_mesh(&block, 0, 32).unwrap();
    for quad in Arc::make_mut(&mut block.quads) {
        quad.positions.swap(1, 3);
        quad.uvs.swap(1, 3);
    }
    let actual = block_mesh(&block, 0, 32).unwrap();
    assert_eq!(actual, expected);
    assert!(DroppedItemModel::Block(block).can_cull_back_faces());
}

#[test]
fn open_two_sided_degenerate_and_inward_blocks_keep_culling_off() {
    let block = block_cube();
    let mut open = block.clone();
    open.quads = Arc::from(&open.quads[..5]);
    assert!(!DroppedItemModel::Block(open).can_cull_back_faces());
    let mut two_sided = block.clone();
    Arc::make_mut(&mut two_sided.quads)[0].flags |= assets::MODEL_QUAD_FLAG_TWO_SIDED;
    assert!(!DroppedItemModel::Block(two_sided).can_cull_back_faces());
    let mut degenerate = block.clone();
    Arc::make_mut(&mut degenerate.quads)[0].positions[1] = degenerate.quads[0].positions[0];
    assert!(!DroppedItemModel::Block(degenerate).can_cull_back_faces());
    let mut inward = block;
    let quad = &mut Arc::make_mut(&mut inward.quads)[0];
    quad.flags = 0;
    quad.positions.swap(1, 3);
    assert!(!DroppedItemModel::Block(inward).can_cull_back_faces());
}

#[test]
fn both_sprite_slabs_cull_binary_cutouts_but_not_partial_alpha() {
    let sprite = DroppedItemSprite {
        width: 3,
        height: 1,
        rgba8: Arc::from([255; 12]),
    };
    let models = |sprite: &DroppedItemSprite| {
        [
            DroppedItemModel::Sprite(sprite.clone()),
            DroppedItemModel::NativeSprite(sprite.clone()),
        ]
    };
    let mut asymmetric = sprite.clone();
    Arc::make_mut(&mut asymmetric.rgba8)[3] = 0;
    for model in models(&sprite).into_iter().chain(models(&asymmetric)) {
        assert!(model.can_cull_back_faces());
    }
    let mut translucent = sprite;
    Arc::make_mut(&mut translucent.rgba8)[3] = 128;
    for model in models(&translucent) {
        assert!(!model.can_cull_back_faces());
    }
}

#[test]
fn cube_cutouts_and_transparent_tints_keep_culling_off() {
    let mut cube = DroppedItemCube {
        tile: 1,
        faces: std::array::from_fn(|_| Arc::from([255; 4])),
        tints: [OPAQUE_WHITE; 6],
    };
    assert!(DroppedItemModel::Cube(cube.clone()).can_cull_back_faces());
    cube.tints[0] = 0x7fff_ffff;
    assert!(!DroppedItemModel::Cube(cube.clone()).can_cull_back_faces());
    cube.tints[0] = OPAQUE_WHITE;
    cube.faces[0] = Arc::from([255, 255, 255, 0]);
    assert!(!DroppedItemModel::Cube(cube).can_cull_back_faces());
}

#[test]
fn wrapped_block_uvs_keep_the_opaque_rear_face_available() {
    let mut block = block_cube();
    assert!(DroppedItemModel::Block(block.clone()).can_cull_back_faces());
    let front = Arc::make_mut(&mut block.quads)
        .iter_mut()
        .find(|quad| quad.flags == 6)
        .unwrap();
    for uv in &mut front.uvs {
        uv[0] += block::UV_TILE_UNITS;
    }
    assert!(!DroppedItemModel::Block(block.clone()).can_cull_back_faces());
    let layer_side = 32;
    let mesh = block_mesh(&block, 0, layer_side).unwrap();
    for (normal, expected_texel) in [(1.0, 1), (-1.0, 0)] {
        let face: Vec<_> = mesh.iter().filter(|v| v.normal[2] == normal).collect();
        assert_eq!(face.len(), 6);
        let u = face.iter().map(|v| v.uv[0]).sum::<f32>() / face.len() as f32;
        assert_eq!((u * layer_side as f32).floor() as u32, expected_texel);
    }
}

#[test]
fn either_uv_axis_cannot_extend_into_atlas_padding() {
    for axis in 0..2 {
        for coordinate in [block::UV_TILE_UNITS + 1, block::UV_TILE_UNITS * 2, u16::MAX] {
            let mut block = block_cube();
            Arc::make_mut(&mut block.quads)[0].uvs[0][axis] = coordinate;
            assert!(!DroppedItemModel::Block(block).can_cull_back_faces());
        }
    }
}

#[test]
fn constant_upper_edge_uvs_keep_culling_off() {
    for axis in 0..2 {
        for lower_corners in 0..2 {
            let mut block = block_cube();
            let quad = &mut Arc::make_mut(&mut block.quads)[0];
            for (corner, uv) in quad.uvs.iter_mut().enumerate() {
                uv[axis] = if corner < lower_corners {
                    0
                } else {
                    block::UV_TILE_UNITS
                };
            }
            assert!(!DroppedItemModel::Block(block).can_cull_back_faces());
        }
    }
}
