use super::*;
use crate::{Facing, SkullKind, SkullModel, SkullMount, floor_yaw_degrees, pack_actor_light};
use bevy::math::{Mat4, Vec3};

fn scene() -> BlockEntityScene {
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        80,
        64,
        &vec![255; 80 * 64 * 4],
        &[
            assets::BlockEntityPlacement {
                name: "textures/entity/steve".into(),
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            },
            assets::BlockEntityPlacement {
                name: "textures/environment/destroy_stage_0".into(),
                x: 64,
                y: 0,
                width: 16,
                height: 16,
            },
        ],
    )
    .unwrap();
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap());
    scene
}

fn skull(mount: SkullMount, block: u8, sky: u8) -> BlockEntitySubmission {
    BlockEntitySubmission {
        block: [0; 3],
        light: BlockEntityLight::Actor { block, sky },
        kind: BlockEntityKind::Skull(SkullModel {
            kind: SkullKind::Player,
            mount,
        }),
    }
}

#[test]
fn skull_base_and_hat_carry_rotated_outward_normals_without_terrain_shading() {
    let mut scene = scene();
    for (mount, yaw) in [
        (
            SkullMount::Floor {
                rotation_degrees: 0.0,
            },
            floor_yaw_degrees(0.0),
        ),
        (
            SkullMount::Floor {
                rotation_degrees: 45.0,
            },
            floor_yaw_degrees(45.0),
        ),
        (
            SkullMount::Floor {
                rotation_degrees: 90.0,
            },
            floor_yaw_degrees(90.0),
        ),
        (SkullMount::Wall(Facing::East), Facing::East.yaw_degrees()),
    ] {
        let frame = scene.update(SceneClock::default(), &[], &[skull(mount, 4, 11)]);
        assert_eq!(frame.solid.len(), 72, "base plus hat");
        let rotation = Mat4::from_rotation_y(yaw.to_radians());
        let normals = [
            Vec3::NEG_Z,
            Vec3::Z,
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
        ];
        for (face, vertices) in frame.solid.chunks_exact(6).enumerate() {
            let expected = rotation.transform_vector3(normals[face % normals.len()]);
            for vertex in vertices {
                assert!(Vec3::from_array(vertex.normal).abs_diff_eq(expected, 1.0e-5));
                assert_eq!(vertex.color, [1.0; 4], "native shader owns face shading");
                assert_eq!(vertex.actor_light, pack_actor_light(4, 11));
            }
        }
    }
}

#[test]
fn retained_skull_levels_invalidate_the_cache_and_do_not_light_later_cracks() {
    let mut scene = scene();
    let mount = SkullMount::Floor {
        rotation_degrees: 0.0,
    };
    let cracks = [CrackInstance {
        block: [2, 0, 0],
        stage: 0,
        shape: CrackShape::Cube,
    }];
    let first = scene
        .update(SceneClock::default(), &cracks, &[skull(mount, 0, 0)])
        .clone();
    assert!(
        first
            .solid
            .iter()
            .all(|vertex| vertex.actor_light == pack_actor_light(0, 0))
    );
    // Darkness is a valid native sample, distinct from the legacy/unlit zero sentinel.
    assert_ne!(pack_actor_light(0, 0), 0);
    let second = scene
        .update(SceneClock::default(), &cracks, &[skull(mount, 0, 15)])
        .clone();
    assert_ne!(first.revision, second.revision);
    assert!(
        second
            .solid
            .iter()
            .all(|vertex| vertex.actor_light == pack_actor_light(0, 15))
    );
    assert!(!second.crack.is_empty());
    assert!(second.crack.iter().all(|vertex| vertex.actor_light == 0));
    let cached = scene.update(SceneClock::default(), &cracks, &[skull(mount, 0, 15)]);
    assert_eq!(cached.revision, second.revision);
    assert_eq!(cached.solid, second.solid);
}
