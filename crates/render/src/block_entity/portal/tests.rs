use super::*;
use assets::{BlockEntityPlacement, RuntimeBlockEntityAssets, encode_block_entity_catalog};
use bevy::math::Vec3;

fn assert_outward_winding(vertices: &[super::super::mesh::BlockEntityVertex]) {
    for triangle in vertices.as_chunks::<3>().0 {
        let [a, b, c] =
            std::array::from_fn::<_, 3, _>(|index| Vec3::from_array(triangle[index].position));
        let encoded = triangle[0].color;
        let normal = Vec3::new(encoded[0], encoded[1], encoded[2]) * 2.0 - Vec3::splat(1.0);
        assert!((b - a).cross(c - a).dot(normal) > 0.0);
    }
}

fn atlas() -> BlockEntityAtlas {
    let placements = [
        BlockEntityPlacement {
            name: STAR_TEXTURE.into(),
            x: 0,
            y: 0,
            width: 256,
            height: 256,
        },
        BlockEntityPlacement {
            name: COLOR_TEXTURE.into(),
            x: 256,
            y: 0,
            width: 4,
            height: 4,
        },
    ];
    let bytes =
        encode_block_entity_catalog(b"{}", 512, 256, &vec![255; 512 * 256 * 4], &placements)
            .unwrap();
    BlockEntityAtlas::from_assets(&RuntimeBlockEntityAssets::decode(&bytes).unwrap())
}

#[test]
fn portal_surface_uses_all_seventeen_coplanar_native_layers_and_byte_depths() {
    let atlas = atlas();
    let mut builder = MeshBuilder::new(atlas.size());
    builder.light = 0.0;
    emit(
        &mut builder,
        &atlas,
        [10, -3, 20],
        false,
        SceneClock::default(),
    );
    assert_eq!(builder.portal.len(), 17 * 6);
    assert_outward_winding(&builder.portal);
    assert!(builder.solid.is_empty() && builder.overlay.is_empty() && builder.crack.is_empty());
    let phases = [
        255, 239, 223, 207, 191, 175, 159, 143, 127, 111, 95, 79, 63, 47, 31, 15, 0,
    ];
    for (layer, quad) in builder.portal.as_chunks::<6>().0.iter().enumerate() {
        assert!(quad.iter().all(|vertex| vertex.position[1] == -2.25));
        for vertex in quad {
            assert_eq!(
                vertex.color,
                [
                    127.0 / 255.0,
                    1.0,
                    127.0 / 255.0,
                    phases[layer] as f32 / 255.0
                ]
            );
            assert!((10.0..=11.0).contains(&vertex.position[0]));
            assert!((20.0..=21.0).contains(&vertex.position[2]));
        }
        if layer > 0 {
            let cell = layer - 1;
            assert_eq!(
                quad[0].uv,
                [
                    (256.5 + (cell % 4) as f32) / atlas.size()[0] as f32,
                    (0.5 + (cell / 4) as f32) / atlas.size()[1] as f32
                ]
            );
        }
    }
}

#[test]
fn gateway_surface_has_six_full_cube_planes_without_flat_texture_or_lighting() {
    let atlas = atlas();
    let mut builder = MeshBuilder::new(atlas.size());
    builder.light = 0.0;
    emit(&mut builder, &atlas, [2, 3, 4], true, SceneClock::default());
    assert_eq!(builder.portal.len(), 17 * 6 * 6);
    assert_outward_winding(&builder.portal);
    for (layer, faces) in builder.portal.as_chunks::<{ 6 * 6 }>().0.iter().enumerate() {
        let expected = [
            [127, 255, 127],
            [127, 0, 127],
            [0, 127, 127],
            [255, 127, 127],
            [127, 127, 0],
            [127, 127, 255],
        ];
        for (quad, normal) in faces.as_chunks::<6>().0.iter().zip(expected) {
            for vertex in quad {
                assert_eq!(vertex.color[..3], normal.map(|value| value as f32 / 255.0));
                assert_eq!(vertex.color[3], layer_phase(layer));
            }
        }
    }
    let mut normals: Vec<_> = builder
        .portal
        .iter()
        .map(|vertex| vertex.color[..3].to_vec())
        .collect();
    normals.sort_by(|left, right| left.partial_cmp(right).unwrap());
    normals.dedup();
    assert_eq!(normals.len(), 6);
    for vertex in &builder.portal {
        for (axis, min) in [2.0, 3.0, 4.0].into_iter().enumerate() {
            assert!((min..=min + 1.0).contains(&vertex.position[axis]));
        }
    }
}

#[test]
fn star_wrap_bounds_follow_the_atlas_placement_and_dynamic_strip_height() {
    let atlas = atlas();
    let rect = star_rect(&atlas);
    assert_eq!(rect, [0.0, 0.0, 0.5, 256.0 / atlas.size()[1] as f32]);
}
