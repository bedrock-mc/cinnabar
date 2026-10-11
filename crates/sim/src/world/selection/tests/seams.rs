use super::*;
use std::collections::BTreeSet;

const TRANSVERSE_PROBE_OFFSET: f64 = 1.0 / 65536.0;

#[test]
fn multibox_internal_planes_keep_inside_origin_faces() {
    let pieces = cube_octants();
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, pieces.clone(), vec![]);
        let (store, registry, id) = fixture(space, &target);
        let view = PaletteWorld::new(&store, &registry, 0, Default::default());
        let whole = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        let (whole_store, whole_registry, _) = fixture(space, &whole);
        let whole_view = PaletteWorld::new(&whole_store, &whole_registry, 0, Default::default());
        let origin = Vec3::new(0.5, 0.5, 0.5);
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if [x, y, z] == [0; 3] {
                        continue;
                    }
                    let direction = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                    let reference = whole_view
                        .block_interaction_ray_current(origin, direction, 2.0)
                        .unwrap()
                        .unwrap();
                    let hit = view
                        .block_interaction_ray_current(origin, direction, 2.0)
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        (hit.runtime_id, hit.distance, hit.face, hit.hit_local),
                        (id, 0.0, reference.face, reference.hit_local)
                    );
                    let camera = view
                        .camera_visibility_ray(origin, direction, 2.0)
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        (camera.runtime_id, camera.distance, camera.face),
                        (id, 0.0, reference.face)
                    );
                }
            }
        }
    }
}

#[test]
fn multibox_internal_planes_keep_boundary_entry_faces() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, cube_octants(), vec![]);
        let (store, registry, _) = fixture(space, &target);
        let view = PaletteWorld::new(&store, &registry, 0, Default::default());
        let whole = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        let (whole_store, whole_registry, _) = fixture(space, &whole);
        let whole_view = PaletteWorld::new(&whole_store, &whole_registry, 0, Default::default());
        for axis in 0..3 {
            let a = (axis + 1) % 3;
            let b = (axis + 2) % 3;
            for side in [0.0, 1.0] {
                for u in [0.25, 0.5, 0.75] {
                    for v in [0.25, 0.5, 0.75] {
                        let mut point = Vec3::ZERO;
                        point[axis] = side;
                        point[a] = u;
                        point[b] = v;
                        for du in [-0.25, 0.0, 0.25] {
                            for dv in [-0.25, 0.0, 0.25] {
                                let mut direction = Vec3::ZERO;
                                direction[axis] = if side == 0.0 { 1.0 } else { -1.0 };
                                direction[a] = du;
                                direction[b] = dv;
                                let origin = point - direction * 2.0;
                                let reference = whole_view
                                    .block_interaction_ray_current(origin, direction, 4.0)
                                    .unwrap()
                                    .unwrap();
                                let hit = view
                                    .block_interaction_ray_current(origin, direction, 4.0)
                                    .unwrap()
                                    .unwrap();
                                assert_eq!(
                                    (hit.runtime_id, hit.distance, hit.face, hit.hit_local),
                                    (
                                        reference.runtime_id,
                                        reference.distance,
                                        reference.face,
                                        reference.hit_local
                                    ),
                                    "{origin:?} {direction:?}"
                                );
                                let camera = view
                                    .camera_visibility_ray(origin, direction, 4.0)
                                    .unwrap()
                                    .unwrap();
                                assert_eq!(
                                    (camera.runtime_id, camera.distance, camera.face),
                                    (reference.runtime_id, reference.distance, reference.face)
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Splits a unit cube along all three internal midpoint planes.
fn cube_octants() -> Vec<ShapeBox> {
    (0..8)
        .map(|index| {
            let min =
                [index & 1, (index >> 1) & 1, (index >> 2) & 1].map(|value| f64::from(value) * 0.5);
            ShapeBox::new(min, min.map(|value| value + 0.5))
        })
        .collect()
}

#[test]
fn compiled_stair_internal_planes_remain_pickable() {
    let (records, registry) = compiled_fixture();
    let mut states = 0;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
    {
        let physics = registry.by_sequential_id(record.sequential_id).unwrap();
        check_internal_planes(&physics.shapes.as_ref().unwrap().selection);
        // Corner movement boxes exercise the same union clipper independently of picking state.
        let scale = f64::from(assets::block_shapes::COLLISION_UNITS_PER_BLOCK);
        let corners: Vec<_> = physics
            .boxes
            .iter()
            .map(|shape| {
                ShapeBox::new(
                    [shape.min_x, shape.min_y, shape.min_z].map(|value| f64::from(value) / scale),
                    [shape.max_x, shape.max_y, shape.max_z].map(|value| f64::from(value) / scale),
                )
            })
            .collect();
        check_internal_planes(&corners);
        states += 1;
    }
    assert!(
        states >= 40,
        "all directions, halves and five corner states must be covered"
    );
}

#[test]
fn other_compiled_multibox_internal_planes_remain_pickable() {
    let (records, registry) = compiled_fixture();
    let mut checked = BTreeSet::new();
    let mut families = BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.model_family != assets::ModelFamily::Stair)
    {
        let selection = &registry
            .by_sequential_id(record.sequential_id)
            .unwrap()
            .shapes
            .as_ref()
            .unwrap()
            .selection;
        if selection.len() > 1 {
            families.insert(record.name.to_string());
            if checked.insert(format!("{selection:?}")) {
                check_internal_planes(selection);
            }
        }
    }
    for name in [
        "hopper",
        "cauldron",
        "composter",
        "scaffolding",
        "glow_lichen",
        "sculk_vein",
        "resin_clump",
        "piston_arm_collision",
        "sticky_piston_arm_collision",
    ] {
        assert!(
            families.contains(&format!("minecraft:{name}")),
            "missing {name}"
        );
    }
}

#[test]
fn multibox_outer_faces_and_unfilled_seam_quadrants_remain_unpickable() {
    let boxes = vec![
        ShapeBox::new([0.0; 3], [1.0, 0.5, 0.5]),
        ShapeBox::new([0.0, 0.5, 0.5], [1.0; 3]),
    ];
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, boxes.clone(), vec![]);
        let (store, registry, _) = fixture(space, &target);
        let view = PaletteWorld::new(&store, &registry, 0, Default::default());
        for (y, z) in [
            (0.0, 0.25),
            (1.0, 0.75),
            (0.25, 0.0),
            (0.75, 1.0),
            (0.5, 0.5),
        ] {
            let origin = Vec3::new(-2.0, y, z);
            assert!(
                view.block_interaction_ray_current(origin, Vec3::new(1.0, 0.0, 0.0), 4.0)
                    .unwrap()
                    .is_none()
            );
            assert!(
                view.camera_visibility_ray(origin, Vec3::new(1.0, 0.0, 0.0), 4.0)
                    .unwrap()
                    .is_none()
            );
        }
    }
}

/// Loads the shipped geometry bound to the active block registry.
fn compiled_fixture() -> (Box<[assets::RegistryRecord]>, assets::PhysicsRegistry) {
    let breg = assets::pinned_block_registry_bytes();
    let protocol = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(breg, protocol).unwrap();
    let registry = assets::read_physics_registry_for_protocol(
        include_bytes!("../../../../../assets/data/block-physics-v2193.bin"),
        breg,
        &records,
        protocol,
    )
    .unwrap();
    (records, registry)
}

/// Checks rays on every box plane and plane intersection that lies inside the union.
fn check_internal_planes(boxes: &[ShapeBox]) {
    let coordinates: [Vec<f64>; 3] = std::array::from_fn(|axis| {
        let mut values: Vec<_> = boxes
            .iter()
            .flat_map(|shape| [shape.min[axis], shape.max[axis]])
            .collect();
        values.sort_by(f64::total_cmp);
        values.dedup();
        values
    });
    let probes: [Vec<f64>; 3] = std::array::from_fn(|axis| {
        let mut values = coordinates[axis].clone();
        values.extend(
            coordinates[axis]
                .windows(2)
                .map(|pair| (pair[0] + pair[1]) / 2.0),
        );
        values
    });
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, boxes.to_vec(), vec![]);
        let (store, registry, id) = fixture(space, &target);
        let view = PaletteWorld::new(&store, &registry, 0, Default::default());
        for axis in 0..3 {
            let a = (axis + 1) % 3;
            let b = (axis + 2) % 3;
            for &u in &probes[a] {
                for &v in &probes[b] {
                    for sign in [-1.0, 1.0] {
                        let mut segments: Vec<_> = coordinates[axis].windows(2).collect();
                        if sign < 0.0 {
                            segments.reverse();
                        }
                        let Some(segment) = segments.into_iter().find(|segment| {
                            let mut point = [0.0; 3];
                            point[axis] = (segment[0] + segment[1]) / 2.0;
                            point[a] = u;
                            point[b] = v;
                            // Four nearby samples distinguish solid internal seams from exposed edges.
                            [-1.0, 1.0].into_iter().all(|du| {
                                [-1.0, 1.0].into_iter().all(|dv| {
                                    let mut sample = point;
                                    sample[a] += du * TRANSVERSE_PROBE_OFFSET;
                                    sample[b] += dv * TRANSVERSE_PROBE_OFFSET;
                                    boxes.iter().any(|shape| {
                                        (0..3).all(|i| {
                                            shape.min[i] < sample[i] && sample[i] < shape.max[i]
                                        })
                                    })
                                })
                            })
                        }) else {
                            continue;
                        };
                        let mut origin = Vec3::ZERO;
                        origin[a] = u;
                        origin[b] = v;
                        origin[axis] = if sign > 0.0 { -2.0 } else { 3.0 };
                        let mut direction = Vec3::ZERO;
                        direction[axis] = sign;
                        let entry = if sign > 0.0 { segment[0] } else { segment[1] };
                        let distance = (entry - origin[axis]).abs();
                        let hit = view
                            .block_interaction_ray_current(origin, direction, 6.0)
                            .unwrap()
                            .unwrap();
                        assert_eq!(
                            (hit.runtime_id, hit.distance),
                            (id, distance),
                            "{boxes:?} {origin:?} {direction:?}"
                        );
                        let camera = view
                            .camera_visibility_ray(origin, direction, 6.0)
                            .unwrap()
                            .unwrap();
                        assert_eq!(
                            (camera.runtime_id, camera.distance, camera.face),
                            (id, distance, hit.face)
                        );
                    }
                }
            }
        }
    }
}
