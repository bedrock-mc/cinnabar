use super::*;

#[test]
fn stage_names_clamp_to_the_last_texture() {
    assert_eq!(
        crack_texture_name(3),
        "textures/environment/destroy_stage_3"
    );
    assert_eq!(
        crack_texture_name(200),
        "textures/environment/destroy_stage_9"
    );
}

#[test]
fn wrapped_uvs_fold_into_one_tile_and_full_tile_stays_full() {
    assert_eq!(tile_fraction(4096), 1.0);
    assert_eq!(tile_fraction(2048), 0.5);
    assert!((tile_fraction(6144) - 0.5).abs() < 1.0e-6);
    assert_eq!(tile_fraction(8192), 1.0);
}

#[test]
fn out_of_range_templates_have_no_shape() {
    assert!(crack_shape_from_template(&RuntimeAssets::diagnostic(), 999, 0).is_none());
}

#[test]
fn every_partial_snow_top_crack_is_above_its_actual_surface() {
    for layer in 1..assets::TOP_SNOW_LAYER_COUNT {
        let height = f32::from(layer) / f32::from(assets::TOP_SNOW_LAYER_COUNT);
        let face = CrackQuad {
            corners: [
                [0.0, height, 0.0],
                [0.0, height, 1.0],
                [1.0, height, 1.0],
                [1.0, height, 0.0],
            ],
            uvs: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        };
        let mut builder = MeshBuilder::new([16, 16]);
        emit_model(
            &mut builder,
            Vec3::new(-3.0, 20.0, 7.0),
            super::super::atlas::AtlasRect {
                x: 0.0,
                y: 0.0,
                width: 16.0,
                height: 16.0,
            },
            &[face],
        );
        assert_eq!(builder.crack.len(), 6);
        for vertex in builder.crack {
            assert!(vertex.position[1] > 20.0 + height);
            assert!((vertex.position[1] - (20.0 + height + FACE_OFFSET)).abs() < 0.00001);
            assert!(vertex.uv.iter().all(|axis| (0.0..=1.0).contains(axis)));
        }
    }
}

#[test]
fn inset_stair_risers_push_into_empty_space_not_towards_the_cell_edge() {
    // A west-facing riser at x=.5 has its outside towards the open step at -X.
    let face = CrackQuad {
        corners: [
            [0.5, 0.5, 0.0],
            [0.5, 0.5, 1.0],
            [0.5, 1.0, 1.0],
            [0.5, 1.0, 0.0],
        ],
        uvs: [[0.0; 2]; 4],
    };
    assert_eq!(face.outward_offset(), Vec3::NEG_X * FACE_OFFSET);
    for variant in 0..4 {
        let rotated = CrackQuad {
            corners: [
                [128, 128, 0],
                [128, 128, 256],
                [128, 256, 256],
                [128, 256, 0],
            ]
            .map(|corner| rotate_corner(corner, variant)),
            ..face
        };
        assert_eq!(
            rotated.outward_offset(),
            [Vec3::NEG_X, Vec3::NEG_Z, Vec3::X, Vec3::Z][variant as usize] * FACE_OFFSET
        );
        assert_eq!(
            rotated.corners,
            [
                [128, 128, 0],
                [128, 128, 256],
                [128, 256, 256],
                [128, 256, 0],
            ]
            .map(|corner| rotate_corner(corner, variant | assets::BLOCK_VISUAL_VARIANT_TOP_SNOW))
        );
    }
}
