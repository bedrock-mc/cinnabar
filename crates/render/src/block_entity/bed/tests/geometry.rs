use super::super::*;
use crate::block_entity::{heads::HeadBox, mesh::BoxSpec};
use assets::{BlockEntityPlacement, RuntimeBlockEntityAssets, encode_block_entity_catalog};
use bevy::math::Vec3;

fn geometry() -> HeadModel {
    HeadModel {
        texture: [64.0, 64.0],
        boxes: [
            ([-16.0, 0.0, 0.0], [16.0, 32.0, 6.0], [0.0, 0.0]),
            ([-1.0, 3.0, 6.0], [1.0, 26.0, 3.0], [52.0, 6.0]),
            ([-16.0, 29.0, 6.0], [3.0; 3], [0.0, 38.0]),
        ]
        .into_iter()
        .map(|(origin, size, uv)| HeadBox {
            matrix: Mat4::IDENTITY,
            spec: BoxSpec::new(origin, size, uv),
        })
        .collect(),
    }
}

fn atlas() -> BlockEntityAtlas {
    let bytes = encode_block_entity_catalog(
        b"{}",
        64,
        64,
        &vec![255; 64 * 64 * 4],
        &[BlockEntityPlacement {
            name: "textures/entity/bed/red".into(),
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        }],
    )
    .unwrap();
    BlockEntityAtlas::from_assets(&RuntimeBlockEntityAssets::decode(&bytes).unwrap())
}

#[test]
fn bed_foot_does_not_duplicate_the_head_owned_model() {
    let atlas = atlas();
    let mut mesh = MeshBuilder::new(atlas.size());
    emit(
        &mut mesh,
        &atlas,
        [0; 3],
        &BedModel {
            color: "red",
            head: false,
            direction: 0,
        },
        Some(&geometry()),
    );
    assert!(mesh.solid.is_empty(), "the head owns the complete bed");
}

#[test]
fn bed_mattress_pillow_stays_in_the_head_cell_for_every_direction() {
    let atlas = atlas();
    let head = [10, 64, -20];
    for direction in 0..4 {
        let model = BedModel {
            color: "red",
            head: true,
            direction,
        };
        let mut mesh = MeshBuilder::new(atlas.size());
        emit(&mut mesh, &atlas, head, &model, Some(&geometry()));
        let foot = model.other_half_offset();
        let expected_min = Vec3::new(
            (head[0] + foot[0].min(0)) as f32,
            head[1] as f32,
            (head[2] + foot[2].min(0)) as f32,
        );
        let expected_max = Vec3::new(
            (head[0] + foot[0].max(0) + 1) as f32,
            head[1] as f32 + 9.0 / 16.0,
            (head[2] + foot[2].max(0) + 1) as f32,
        );
        let positions = mesh
            .solid
            .iter()
            .map(|vertex| Vec3::from_array(vertex.position));
        let min = positions.clone().reduce(Vec3::min).unwrap();
        let max = positions.reduce(Vec3::max).unwrap();
        assert!(min.abs_diff_eq(expected_min, 1.0e-4), "{direction} {min:?}");
        assert!(max.abs_diff_eq(expected_max, 1.0e-4), "{direction} {max:?}");

        let mattress = mesh
            .solid
            .chunks_exact(6)
            .find(|face| {
                face.iter()
                    .all(|vertex| (vertex.position[1] - expected_max.y).abs() < 1.0e-4)
            })
            .expect("mattress top");
        let pillow_edge: Vec<_> = mattress
            .iter()
            .filter(|vertex| (vertex.uv[1] * atlas.size()[1] as f32 - 6.0).abs() < 1.0e-4)
            .collect();
        assert!(!pillow_edge.is_empty());
        for vertex in pillow_edge {
            for axis in [0, 2] {
                let coordinate = vertex.position[axis];
                assert!(
                    coordinate >= head[axis] as f32 - 1.0e-4
                        && coordinate <= (head[axis] + 1) as f32 + 1.0e-4,
                    "direction {direction}: pillow left head cell: {:?}",
                    vertex.position
                );
            }
        }
    }
}

#[test]
fn bed_foot_end_keeps_the_authored_vertical_texture_orientation() {
    let atlas = atlas();
    let head = [10, 64, -20];
    for direction in 0..4 {
        let model = BedModel {
            color: "red",
            head: true,
            direction,
        };
        let mut mesh = MeshBuilder::new(atlas.size());
        mesh.actor_light = 1;
        emit(&mut mesh, &atlas, head, &model, Some(&geometry()));
        let outward = Vec3::from_array(model.other_half_offset().map(|value| value as f32));
        let foot_end = mesh
            .solid
            .chunks_exact(6)
            .find(|face| {
                Vec3::from_array(face[0].normal).abs_diff_eq(outward, 1.0e-4)
                    && face.iter().all(|vertex| {
                        vertex.position[1] >= head[1] as f32 + 3.0 / 16.0 - 1.0e-4
                            && vertex.position[1] <= head[1] as f32 + 9.0 / 16.0 + 1.0e-4
                    })
            })
            .expect("mattress foot end");
        for vertex in foot_end {
            let top = (vertex.position[1] - head[1] as f32 - 9.0 / 16.0).abs() < 1.0e-4;
            let expected_v = if top { 6.0 } else { 0.0 };
            let v = vertex.uv[1] * atlas.size()[1] as f32;
            assert!(
                (v - expected_v).abs() < 1.0e-4,
                "direction {direction}, position {:?}: expected V {expected_v}, got {v}",
                vertex.position
            );
        }
    }
}

#[test]
fn bed_head_covers_both_blocks_with_one_continuous_mattress_and_inset_frame() {
    let atlas = atlas();
    for direction in 0..4 {
        let mut mesh = MeshBuilder::new(atlas.size());
        emit(
            &mut mesh,
            &atlas,
            [0; 3],
            &BedModel {
                color: "red",
                head: true,
                direction,
            },
            Some(&geometry()),
        );
        let inverse = model_matrix([0; 3], [0.5, 0.0, 0.5], yaw_degrees(direction)).inverse();
        let local: Vec<_> = mesh
            .solid
            .iter()
            .map(|vertex| inverse.transform_point3(Vec3::from_array(vertex.position)))
            .collect();
        let min = local.iter().copied().reduce(Vec3::min).unwrap();
        let max = local.iter().copied().reduce(Vec3::max).unwrap();
        assert!(min.abs_diff_eq(Vec3::new(-8.0, 0.0, -24.0), 1.0e-4));
        assert!(max.abs_diff_eq(Vec3::new(8.0, 9.0, 8.0), 1.0e-4));
        let mattress = mesh
            .solid
            .chunks_exact(6)
            .find(|face| {
                face.iter().all(|vertex| {
                    (inverse
                        .transform_point3(Vec3::from_array(vertex.position))
                        .y
                        - 9.0)
                        .abs()
                        < 1.0e-4
                })
            })
            .unwrap();
        let uv_min = mattress
            .iter()
            .map(|vertex| {
                [
                    vertex.uv[0] * atlas.size()[0] as f32,
                    vertex.uv[1] * atlas.size()[1] as f32,
                ]
            })
            .fold([f32::INFINITY; 2], |min, uv| {
                [min[0].min(uv[0]), min[1].min(uv[1])]
            });
        let uv_max = mattress
            .iter()
            .map(|vertex| {
                [
                    vertex.uv[0] * atlas.size()[0] as f32,
                    vertex.uv[1] * atlas.size()[1] as f32,
                ]
            })
            .fold([f32::NEG_INFINITY; 2], |max, uv| {
                [max[0].max(uv[0]), max[1].max(uv[1])]
            });
        assert_eq!(uv_min, [6.0, 6.0]);
        assert_eq!(uv_max, [22.0, 38.0]);
        assert!(
            local.iter().any(|point| {
                (point.x - 7.0).abs() < 1.0e-4
                    && point.y.abs() < 1.0e-4
                    && (point.z + 21.0).abs() < 1.0e-4
            }),
            "the underside has its inset wooden side rail"
        );
    }
}
