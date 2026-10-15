use super::*;

/// An original blank atlas at either stock or replacement-texture resolution.
fn atlas(side: u32) -> BlockEntityAtlas {
    let placements: Vec<_> = ["base", "stripe_bottom"]
        .into_iter()
        .enumerate()
        .map(|(index, pattern)| assets::BlockEntityPlacement {
            name: format!("{BANNER_PATTERN_TEXTURE_PREFIX}{pattern}").into(),
            x: index as u32 * side,
            y: 0,
            width: side,
            height: side,
        })
        .collect();
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        side * 2,
        side,
        &vec![255; (side * side * 8) as usize],
        &placements,
    )
    .unwrap();
    BlockEntityAtlas::from_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap())
}

/// Compares emitted cloth corners with the mounted, animated attachment contract.
#[test]
fn cloth_attaches_to_crossbar_for_every_mount_and_wave_phase() {
    let mut mounts: Vec<_> = (0..16)
        .map(|rotation| BannerMount::Standing {
            rotation_degrees: rotation as f32 * 22.5,
        })
        .collect();
    mounts
        .extend([Facing::North, Facing::South, Facing::West, Facing::East].map(BannerMount::Wall));
    for side in [64, 128] {
        let atlas = atlas(side);
        for &mount in &mounts {
            let model = BannerModel {
                mount,
                base: [1.0, 0.0, 0.0],
                layers: vec![BannerLayer {
                    pattern: "stripe_bottom",
                    color: [0.0, 1.0, 0.0],
                }],
            };
            let (origin, yaw) = match mount {
                BannerMount::Standing { rotation_degrees } => {
                    (Vec3::new(0.5, 0.48, 0.5), 180.0 - rotation_degrees)
                }
                BannerMount::Wall(facing) => (
                    Vec3::new(0.5, -0.16, 0.5)
                        + Mat4::from_rotation_y(facing.yaw_degrees().to_radians())
                            .transform_vector3(Vec3::new(0.0, -0.3125, 0.4375)),
                    facing.yaw_degrees(),
                ),
            };
            let unmount =
                Mat4::from_rotation_y(-yaw.to_radians()) * Mat4::from_translation(-origin);
            for ticks in [0.0, 25.0, 50.0, 75.0, 100.0] {
                let mut builder = MeshBuilder::new(atlas.size());
                emit(&mut builder, &atlas, [0; 3], &model, SceneClock { ticks });
                let angle = (2.25 - 1.8 * (std::f32::consts::TAU * ticks as f32 / 100.0).cos())
                    .to_radians();
                let uncloth = Mat4::from_rotation_x(-angle)
                    * Mat4::from_translation(Vec3::new(0.0, -1.28, 0.0))
                    * unmount;
                let cloth: Vec<_> = builder
                    .solid
                    .iter()
                    .filter(|vertex| vertex.color[0] > 0.0 && vertex.color[1] == 0.0)
                    .map(|vertex| uncloth.transform_point3(Vec3::from(vertex.position)))
                    .collect();
                assert!(!cloth.is_empty());
                for corner in [
                    [-0.4, 0.0, -0.08],
                    [0.4, 0.0, -0.04],
                    [-0.4, -1.6, -0.08],
                    [0.4, -1.6, -0.04],
                ] {
                    assert!(
                        cloth
                            .iter()
                            .any(|point| point.distance(Vec3::from(corner)) < 1.0e-5),
                        "missing cloth corner {corner:?}: {mount:?}, tick {ticks}, texture {side}"
                    );
                }
                let bar_top = builder
                    .solid
                    .iter()
                    .filter(|vertex| vertex.color[0] == vertex.color[1])
                    .map(|vertex| unmount.transform_point3(Vec3::from(vertex.position)).y)
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!((bar_top - 1.28).abs() < 1.0e-5, "{mount:?}: {bar_top}");
                let overlay: Vec<_> = builder
                    .solid
                    .iter()
                    .filter(|vertex| vertex.color[1] > 0.0 && vertex.color[0] == 0.0)
                    .map(|vertex| uncloth.transform_point3(Vec3::from(vertex.position)))
                    .collect();
                assert!(!overlay.is_empty());
                for point in overlay {
                    // Inflation separates artwork layers around the same cloth and hinge.
                    assert!((-0.401..=0.401).contains(&point.x));
                    assert!((-1.601..=0.001).contains(&point.y));
                    assert!((-0.081..=-0.039).contains(&point.z));
                }
            }
        }
    }
}
