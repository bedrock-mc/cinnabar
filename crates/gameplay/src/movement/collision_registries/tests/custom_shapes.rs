use std::sync::Arc;

use sim::{Aabb, Vec3};

use super::{BREG_V2193, active_content_registry_protocol, bind, synthetic_preg};

/// Encodes a named NBT tag header for the custom-block fixture.
fn named(tag: u8, name: &str) -> Vec<u8> {
    let mut bytes = vec![tag, u8::try_from(name.len()).unwrap()];
    bytes.extend(name.as_bytes());
    bytes
}

/// Creates a custom block with collision primitives and omitted selection data.
fn definitions(shapes: &[[f32; 6]]) -> protocol::CustomBlocks {
    let mut bytes = named(10, "");
    bytes.extend(named(10, "components"));
    bytes.extend(named(10, "minecraft:collision_box"));
    bytes.extend(named(1, "enabled"));
    bytes.push(1);
    bytes.extend(named(9, "boxes"));
    bytes.extend([10, u8::try_from(shapes.len() * 2).unwrap()]);
    for shape in shapes {
        for (name, coordinate) in ["minX", "minY", "minZ", "maxX", "maxY", "maxZ"]
            .into_iter()
            .zip(shape)
        {
            bytes.extend(named(5, name));
            bytes.extend(coordinate.to_le_bytes());
        }
        bytes.push(0);
    }
    bytes.extend([0, 0, 0]);
    protocol::CustomBlocks::from_definitions([("test:shaped_block", bytes.as_slice())])
}

#[test]
fn native_collision_boxes_register_separate_primitives_in_both_identity_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let custom = definitions(&[
        [0.0, 0.0, 0.0, 4.0, 10.0, 16.0],
        [12.0, 0.0, 0.0, 16.0, 8.0, 16.0],
    ]);
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(
        registries.begin_session_hashed_custom_blocks(&custom),
        Some(1)
    );
    let hash = custom.blocks[0].hashed_states()[0].hash;
    let expected = [
        Aabb::new(Vec3::new(0.75, 0.0, 0.0), Vec3::new(1.0, 0.625, 1.0)),
        Aabb::new(Vec3::ZERO, Vec3::new(0.25, 0.5, 1.0)),
    ];
    let gap = Aabb::new(Vec3::new(0.4, 0.1, 0.2), Vec3::new(0.6, 0.4, 0.8));
    for (registry, id) in [
        (&registries.sequential, range.start),
        (&registries.hashed, hash),
    ] {
        let collision = registry.collision_shapes(id).unwrap();
        assert_eq!(
            collision, expected,
            "independent boxes must not become a filled union"
        );
        assert!(collision.iter().all(|shape| !shape.intersects(gap)));
        assert_eq!(
            registry.selection_shapes(id).unwrap(),
            [Aabb::new(Vec3::ZERO, Vec3::ONE)]
        );
    }
}

#[test]
fn resolved_state_collision_and_selection_reach_both_identity_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut custom = definitions(&[[0.0, 0.0, 0.0, 16.0, 8.0, 16.0]]);
    let block = &mut std::sync::Arc::make_mut(&mut custom.blocks)[0];
    block.state_count = 3;
    std::sync::Arc::make_mut(&mut block.visual).state_axes =
        Box::new([protocol::CustomStateAxis {
            name: "test:half".into(),
            values: ["bottom", "top", "empty"]
                .map(|value| protocol::CustomStateValue::String(value.into()))
                .into(),
        }]);
    let lower = protocol::CustomBox {
        min: [0.0; 3],
        max: [1.0, 0.5, 1.0],
    };
    let upper = protocol::CustomBox {
        min: [0.0, 0.5, 0.0],
        max: [1.0; 3],
    };
    block.state_physics = [
        protocol::CustomBlockPhysics {
            random_offset: None,
            transformation: None,
            collides: true,
            collision_boxes: Some([lower].into()),
            selection: protocol::CustomSelection::Default,
        },
        protocol::CustomBlockPhysics {
            random_offset: None,
            transformation: None,
            collides: true,
            collision_boxes: Some([upper].into()),
            selection: protocol::CustomSelection::Box(upper),
        },
        protocol::CustomBlockPhysics {
            random_offset: None,
            transformation: None,
            collides: false,
            collision_boxes: None,
            selection: protocol::CustomSelection::Disabled,
        },
    ]
    .into();
    let hashes = block.hashed_states();
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(
        registries.begin_session_hashed_custom_blocks(&custom),
        Some(3)
    );
    for (index, shape) in [Some(lower), Some(upper), None].into_iter().enumerate() {
        let expected: Vec<_> = shape
            .into_iter()
            .map(|shape| {
                Aabb::new(
                    Vec3::new(
                        f64::from(shape.min[0]),
                        f64::from(shape.min[1]),
                        f64::from(shape.min[2]),
                    ),
                    Vec3::new(
                        f64::from(shape.max[0]),
                        f64::from(shape.max[1]),
                        f64::from(shape.max[2]),
                    ),
                )
            })
            .collect();
        for (registry, id) in [
            (&registries.sequential, range.start + index as u32),
            (&registries.hashed, hashes[index].hash),
        ] {
            assert_eq!(registry.collision_shapes(id).unwrap(), expected);
            let selection = match index {
                0 => vec![Aabb::new(Vec3::ZERO, Vec3::ONE)],
                1 => expected.clone(),
                _ => Vec::new(),
            };
            assert_eq!(registry.selection_shapes(id).unwrap(), selection);
        }
    }
}

#[test]
fn random_offset_state_overrides_reach_selection_and_collision_in_both_id_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut custom = definitions(&[[6.0, 0.0, 6.0, 10.0, 16.0, 10.0]]);
    let block = &mut std::sync::Arc::make_mut(&mut custom.blocks)[0];
    block.state_count = 2;
    let visual = std::sync::Arc::make_mut(&mut block.visual);
    visual.state_axes = Box::new([protocol::CustomStateAxis {
        name: "test:offset".into(),
        values: [false, true].map(protocol::CustomStateValue::Bool).into(),
    }]);
    let components = [
        block_transform::random_offset::BAMBOO,
        block_transform::random_offset::RandomOffsetComponent::default(),
    ];
    block.state_physics = components
        .map(|offset| {
            let mut physics = block.base_physics();
            physics.random_offset = Some(offset);
            physics
        })
        .into();
    let hashes = block.hashed_states();
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(
        registries.begin_session_hashed_custom_blocks(&custom),
        Some(2)
    );
    for (index, component) in components.into_iter().enumerate() {
        for (registry, id) in [
            (&registries.sequential, range.start + index as u32),
            (&registries.hashed, hashes[index].hash),
        ] {
            assert_eq!(
                registry.selection_shapes(id).unwrap(),
                [Aabb::new(Vec3::ZERO, Vec3::ONE)]
            );
            for position in [[-1, 8, -1], [i32::MIN, 0, i32::MAX], [16, 100, -32]] {
                let offset = component.offset(position);
                let expected = Vec3::new(
                    position[0] as f64 + f64::from(offset[0]),
                    position[1] as f64 + f64::from(offset[1]),
                    position[2] as f64 + f64::from(offset[2]),
                );
                assert_eq!(registry.block_shape_offset(id, position), Some(expected));
            }
            let air = records
                .iter()
                .find(|record| record.name.as_ref() == "minecraft:air")
                .unwrap();
            let air = if id == hashes[index].hash {
                air.network_hash
            } else {
                air.sequential_id
            };
            let mut store = world::ChunkStore::new();
            for x in -1..=1 {
                for z in -1..=1 {
                    store
                        .mark_chunk_loaded(world::ChunkKey::new(0, x, z))
                        .unwrap();
                }
            }
            store
                .update_block(
                    world::SubChunkKey::new(0, 0, 0, 0),
                    world::BlockUpdate::new(1, 8, 0, 0, id),
                    air,
                )
                .unwrap();
            let world = sim::PaletteWorld::new(&store, registry, 0, Default::default());
            let offset = registry.block_shape_offset(id, [1, 8, 0]).unwrap();
            let shape = registry.collision_shapes(id).unwrap()[0].translated(offset);
            use sim::CollisionWorld;
            assert_eq!(
                world
                    .collision_boxes(Aabb::new(
                        Vec3::new(0.0, 8.0, -1.0),
                        Vec3::new(3.0, 9.0, 2.0)
                    ))
                    .unwrap()
                    .value,
                [shape]
            );
            let selection = Aabb::new(Vec3::ZERO, Vec3::ONE).translated(offset);
            let origin = Vec3::new((shape.min.x + shape.max.x) * 0.5, 8.5, 2.0);
            let hit = world
                .block_interaction_ray_current(origin, Vec3::new(0.0, 0.0, -1.0), 4.0)
                .unwrap()
                .unwrap();
            assert_eq!((hit.block_pos, hit.runtime_id), ([1, 8, 0], id));
            assert!((hit.distance - (origin.z - selection.max.z)).abs() < 1e-12);
            let (camera, _) = world
                .camera_segment_entry(origin, Vec3::new(0.0, 0.0, -3.0))
                .unwrap();
            assert!((camera.unwrap() - (origin.z - shape.max.z) / 3.0).abs() < 1e-12);
        }
    }
}

#[test]
fn omitted_selection_remains_pickable_without_collision() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut custom = definitions(&[]);
    let block = &mut std::sync::Arc::make_mut(&mut custom.blocks)[0];
    block.collides = false;
    let hash = block.hashed_states()[0].hash;
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    registries
        .begin_session_hashed_custom_blocks(&custom)
        .unwrap();
    for (registry, id) in [
        (&registries.sequential, range.start),
        (&registries.hashed, hash),
    ] {
        assert!(registry.collision_shapes(id).unwrap().is_empty());
        assert_eq!(
            registry.selection_shapes(id).unwrap(),
            [Aabb::new(Vec3::ZERO, Vec3::ONE)]
        );
    }
}

/// Builds a face-attached block whose per-face turn is the only per-face signal,
/// as `owlscubed_pob:backrooms_wallpaper_slab` authors it on the wire.
fn face_facing() -> protocol::CustomBlocks {
    use protocol::{
        CustomBox, CustomSelection, CustomStateAxis, CustomStateValue, CustomTransformation,
    };
    let base = CustomBox {
        min: [0.0, 0.5, 0.0],
        max: [1.0, 1.0, 1.0],
    };
    let turns: [([i32; 3], &str); 6] = [
        ([2, 0, 0], "down"),
        ([0, 0, 0], "up"),
        ([1, 2, 0], "north"),
        ([1, 0, 0], "south"),
        ([2, 1, 3], "west"),
        ([0, 1, 3], "east"),
    ];
    let physics = turns.map(|(rotation, _)| protocol::CustomBlockPhysics {
        random_offset: None,
        // `up` is the default face and carries no authored turn.
        transformation: if rotation == [0; 3] {
            None
        } else {
            Some(CustomTransformation {
                rotation,
                scale: [1.0; 3],
                translation: [0.0; 3],
            })
        },
        collides: true,
        collision_boxes: Some([base].into()),
        selection: CustomSelection::Box(base),
    });
    protocol::CustomBlocks {
        blocks: Arc::from([protocol::CustomBlock {
            name: Arc::from("test:face_facing"),
            tags: Arc::default(),
            state_count: 6,
            collides: true,
            collision_boxes: Some([base].into()),
            selection: CustomSelection::Box(base),
            visual: Arc::new(protocol::CustomBlockVisuals {
                base: protocol::CustomVisualComponents::default(),
                permutations: Box::default(),
                state_axes: Box::new([CustomStateAxis {
                    name: Arc::from("minecraft:block_face"),
                    values: ["down", "up", "north", "south", "west", "east"]
                        .map(|face| CustomStateValue::String(Arc::from(face)))
                        .into(),
                }]),
                state_identity_incomplete: false,
                materialization_disabled: false,
            }),
            state_physics: physics.into(),
        }]),
        vanilla_blocks: Arc::default(),
        skipped: 0,
    }
}

#[test]
fn face_attached_block_boxes_follow_the_authored_turn_in_both_id_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let custom = face_facing();
    let hashes: Vec<u32> = custom.blocks[0]
        .hashed_states()
        .into_iter()
        .map(|state| state.hash)
        .collect();
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(
        registries.begin_session_hashed_custom_blocks(&custom),
        Some(6)
    );
    // Palette order is the authored face order, so `down` is state 0.
    let expected: [([f64; 3], [f64; 3]); 6] = [
        ([0.0, 0.0, 0.0], [1.0, 0.5, 1.0]), // down
        ([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]), // up, the default face
        ([0.0, 0.0, 0.0], [1.0, 1.0, 0.5]), // north
        ([0.0, 0.0, 0.5], [1.0, 1.0, 1.0]), // south
        ([0.0, 0.0, 0.0], [0.5, 1.0, 1.0]), // west
        ([0.5, 0.0, 0.0], [1.0, 1.0, 1.0]), // east
    ];
    for (index, (min, max)) in expected.into_iter().enumerate() {
        let shape = Aabb::new(
            Vec3::new(min[0], min[1], min[2]),
            Vec3::new(max[0], max[1], max[2]),
        );
        for (registry, id) in [
            (&registries.sequential, range.start + index as u32),
            (&registries.hashed, hashes[index]),
        ] {
            assert_eq!(
                registry.collision_shapes(id).unwrap(),
                [shape],
                "collision for face index {index}"
            );
            assert_eq!(
                registry.selection_shapes(id).unwrap(),
                [shape],
                "selection for face index {index}"
            );
        }
    }
}
