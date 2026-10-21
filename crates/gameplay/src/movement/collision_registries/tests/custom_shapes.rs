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
fn custom_mining_components_resolve_in_both_runtime_identity_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut custom = definitions(&[[0.0, 0.0, 0.0, 16.0, 16.0, 16.0]]);
    let block = &mut Arc::make_mut(&mut custom.blocks)[0];
    block.tags = [Arc::from("minecraft:is_pickaxe_item_destructible")].into();
    block.mining = Arc::new(protocol::CustomBlockMining {
        seconds_to_destroy: 2.0,
        ..Default::default()
    });
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    registries.begin_session_hashed_custom_blocks(&custom);
    for (mode, id) in [
        (assets::NetworkIdMode::Sequential, range.start),
        (
            assets::NetworkIdMode::Hashed,
            custom.blocks[0].hashed_states()[0].hash,
        ),
    ] {
        let (mining, tools) = registries.custom_block_mining(mode, id).unwrap();
        assert_eq!(mining.seconds_to_destroy, 2.0);
        assert_eq!(
            tools,
            assets::compiled::block_destroy::ToolKind::Pickaxe.bit()
        );
    }
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
    assert_eq!(registries.begin_session_hashed_custom_blocks(&custom), 1);
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
fn default_component_boxes_follow_a_half_height_transformation_in_both_identity_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut bytes = named(10, "");
    bytes.extend(named(10, "components"));
    bytes.extend(named(10, "minecraft:collision_box"));
    bytes.extend(named(1, "enabled"));
    bytes.extend([1, 0]);
    bytes.extend(named(1, "minecraft:selection_box"));
    bytes.push(1);
    bytes.extend(named(10, "minecraft:transformation"));
    for (name, value) in [("SY", 0.5_f32), ("TY", -0.25)] {
        bytes.extend(named(5, name));
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 0, 0]);
    let custom = protocol::CustomBlocks::from_definitions([("test:half_height", bytes.as_slice())]);
    assert_eq!(custom.skipped, 0);
    let registration = registries.begin_session_blocks(&custom, false);
    let sequential = registration.custom_ids.unwrap().start;
    registries.begin_session_hashed_custom_blocks(&custom);
    let hash = custom.blocks[0].hashed_states()[0].hash;
    let expected = Aabb::new(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0));
    for (mode, registry, id) in [
        (
            assets::NetworkIdMode::Sequential,
            &registries.sequential,
            sequential,
        ),
        (assets::NetworkIdMode::Hashed, &registries.hashed, hash),
    ] {
        assert!(!registries.block_is_full_cube(mode, id));
        assert_eq!(registry.collision_shapes(id).unwrap(), [expected]);
        assert_eq!(registry.selection_shapes(id).unwrap(), [expected]);
        let above = Aabb::new(Vec3::new(0.25, 0.6, 0.25), Vec3::new(0.75, 0.9, 0.75));
        assert!(
            registry
                .collision_shapes(id)
                .unwrap()
                .iter()
                .all(|shape| !shape.intersects(above))
        );
    }
}

/// Encodes a pixel box component, including its network enable flag.
fn pixel_component(name: &str, origin: [f32; 3], size: [f32; 3]) -> Vec<u8> {
    let mut bytes = named(10, name);
    bytes.extend(named(1, "enabled"));
    bytes.push(1);
    for (name, values) in [("origin", origin), ("size", size)] {
        bytes.extend(named(9, name));
        bytes.extend([5, 6]);
        for value in values {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes.push(0);
    bytes
}

#[test]
fn half_height_collision_and_independent_selection_reach_world_queries() {
    use sim::{CollisionWorld, PaletteWorld};
    use world::{BlockUpdate, ChunkStore, SubChunkKey};

    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut bytes = named(10, "");
    bytes.extend(named(10, "components"));
    bytes.extend(pixel_component(
        "minecraft:collision_box",
        [-8.0, 0.0, -8.0],
        [16.0, 8.0, 16.0],
    ));
    bytes.extend(pixel_component(
        "minecraft:selection_box",
        [-6.0, 0.0, -2.0],
        [4.0, 6.0, 8.0],
    ));
    bytes.extend([0, 0]);
    let custom = protocol::CustomBlocks::from_definitions([("test:half_height", bytes.as_slice())]);
    let sequential = registries
        .begin_session_blocks(&custom, false)
        .custom_ids
        .unwrap()
        .start;
    registries.begin_session_hashed_custom_blocks(&custom);
    let hash = custom.blocks[0].hashed_states()[0].hash;
    let air = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:air")
        .unwrap();
    let selection = Aabb::new(Vec3::new(8.625, 8.0, 8.375), Vec3::new(8.875, 8.375, 8.875));
    for (registry, id, air) in [
        (&registries.sequential, sequential, air.sequential_id),
        (&registries.hashed, hash, air.network_hash),
    ] {
        let mut store = ChunkStore::new();
        let key = SubChunkKey::new(0, 0, 0, 0);
        store.apply_request_mode_air(key).unwrap();
        store.mark_sub_chunk_loaded(key).unwrap();
        store
            .update_block(key, BlockUpdate::new(8, 8, 8, 0, id), air)
            .unwrap();
        let world = PaletteWorld::new(&store, registry, 0, Default::default());
        let above = Aabb::new(Vec3::new(8.25, 8.6, 8.25), Vec3::new(8.75, 8.9, 8.75));
        assert!(world.collision_boxes(above).unwrap().value.is_empty());
        let inside = Aabb::new(Vec3::new(8.25, 8.1, 8.25), Vec3::new(8.75, 8.4, 8.75));
        assert_eq!(
            world.collision_boxes(inside).unwrap().value,
            [Aabb::new(
                Vec3::new(8.0, 8.0, 8.0),
                Vec3::new(9.0, 8.5, 9.0)
            )]
        );
        let hit = world
            .block_interaction_ray_current(
                Vec3::new(8.75, 10.0, 8.5),
                Vec3::new(0.0, -1.0, 0.0),
                3.0,
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.runtime_id, id);
        assert_eq!(hit.hit_local.y, 0.375);
        assert_eq!(world.block_outline([8, 8, 8], id).unwrap(), Some(selection));
    }
}

#[test]
fn selection_never_expands_disabled_or_partial_custom_collision() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut custom = definitions(&[[0.0, 0.0, 0.0, 16.0, 8.0, 16.0]]);
    for collides in [true, false] {
        Arc::make_mut(&mut custom.blocks)[0].collides = collides;
        let sequential = registries
            .begin_session_blocks(&custom, false)
            .custom_ids
            .unwrap()
            .start;
        registries.begin_session_hashed_custom_blocks(&custom);
        let hash = custom.blocks[0].hashed_states()[0].hash;
        for (registry, id) in [
            (&registries.sequential, sequential),
            (&registries.hashed, hash),
        ] {
            let expected = if collides {
                vec![Aabb::new(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0))]
            } else {
                Vec::new()
            };
            assert_eq!(registry.collision_shapes(id).unwrap(), expected);
            assert_eq!(
                registry.selection_shapes(id).unwrap(),
                [Aabb::new(Vec3::ZERO, Vec3::ONE)]
            );
        }
    }
}

#[test]
fn zero_volume_custom_collision_has_no_movement_primitives() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let custom = definitions(&[[0.0; 6]]);
    let sequential = registries
        .begin_session_blocks(&custom, false)
        .custom_ids
        .unwrap()
        .start;
    registries.begin_session_hashed_custom_blocks(&custom);
    let hash = custom.blocks[0].hashed_states()[0].hash;
    for (registry, id) in [
        (&registries.sequential, sequential),
        (&registries.hashed, hash),
    ] {
        assert!(registry.collision_shapes(id).unwrap().is_empty());
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
            friction: None,
        },
        protocol::CustomBlockPhysics {
            random_offset: None,
            transformation: None,
            collides: true,
            collision_boxes: Some([upper].into()),
            selection: protocol::CustomSelection::Box(upper),
            friction: None,
        },
        protocol::CustomBlockPhysics {
            random_offset: None,
            transformation: None,
            collides: false,
            collision_boxes: None,
            selection: protocol::CustomSelection::Disabled,
            friction: None,
        },
    ]
    .into();
    let hashes = block.hashed_states();
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(registries.begin_session_hashed_custom_blocks(&custom), 3);
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
    assert_eq!(registries.begin_session_hashed_custom_blocks(&custom), 2);
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
                // Displaced shapes add the offset to the owner cell in single precision.
                let offset = component.offset(position);
                let expected = Vec3::new(
                    f64::from(position[0] as f32 + offset[0]),
                    f64::from(position[1] as f32 + offset[1]),
                    f64::from(position[2] as f32 + offset[2]),
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
            // Camera segment fractions are computed in single precision.
            assert!(
                (camera.unwrap() - (origin.z - shape.max.z) / 3.0).abs() <= f64::from(f32::EPSILON)
            );
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
    registries.begin_session_hashed_custom_blocks(&custom);
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
        friction: None,
    });
    protocol::CustomBlocks {
        blocks: Arc::from([protocol::CustomBlock {
            name: Arc::from("test:face_facing"),
            tags: Arc::default(),
            state_count: 6,
            collides: true,
            collision_boxes: Some([base].into()),
            selection: CustomSelection::Box(base),
            friction: None,
            player_interact: false,
            visual: Arc::new(protocol::CustomBlockVisuals {
                placement: Default::default(),
                base: protocol::CustomVisualComponents::default(),
                item_visual: None,
                permutations: Box::default(),
                components_incomplete: false,
                state_axes: Box::new([CustomStateAxis {
                    name: Arc::from("minecraft:block_face"),
                    values: ["down", "up", "north", "south", "west", "east"]
                        .map(|face| CustomStateValue::String(Arc::from(face)))
                        .into(),
                }]),
                state_identity_incomplete: false,
                materialization_disabled: false,
            }),
            mining: Default::default(),
            state_physics_complete: false,
            state_physics: physics.into(),
            state_mining: Default::default(),
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
    assert_eq!(registries.begin_session_hashed_custom_blocks(&custom), 6);
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

/// Custom surfaces take friction from their own component in both identity spaces;
/// a block without one keeps the default and no surface or fluid response.
#[test]
fn custom_friction_components_set_surface_friction_in_both_identity_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut custom = definitions(&[[0.0, 0.0, 0.0, 16.0, 16.0, 16.0]]);
    let mut icy = custom.blocks[0].clone();
    icy.friction = Some(0.02);
    let mut plain = custom.blocks[0].clone();
    plain.name = "test:plain_block".into();
    custom.blocks = Arc::from([icy, plain]);
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(registries.begin_session_hashed_custom_blocks(&custom), 2);
    for (index, expected) in [(0, f64::from(1.0_f32 - 0.02)), (1, 0.6)] {
        let block = &custom.blocks[index];
        let sequential = range.start
            + custom.blocks[..index]
                .iter()
                .map(|block| block.state_count)
                .sum::<u32>();
        for (registry, id) in [
            (&registries.sequential, sequential),
            (&registries.hashed, block.hashed_states()[0].hash),
        ] {
            let facts = registry.movement_facts(id).unwrap();
            assert_eq!(facts.friction, expected, "{}", block.name);
            assert_eq!(facts.horizontal_speed_factor, 1.0);
            assert_eq!(facts.vertical_speed_factor, 1.0);
            assert_eq!(facts.fluid_height_blocks, 0.0);
            assert_eq!(facts.flags, sim::BlockPhysicsFlags::default());
            assert_eq!(facts.surface_response, sim::SurfaceResponse::None);
        }
    }
}

/// A server block whose `minecraft:custom_components` declares a player-interact handler is used
/// rather than built against, in both identity spaces; one without it stays a building surface.
#[test]
fn a_declared_player_interact_handler_makes_the_block_used_not_built_against() {
    let definition = |interact: bool| {
        let mut bytes = named(10, "");
        bytes.extend(named(10, "components"));
        if interact {
            bytes.extend(named(10, "minecraft:custom_components"));
            bytes.extend(named(1, "hasPlayerInteract"));
            bytes.push(1);
            bytes.push(0);
        }
        bytes.extend([0, 0]);
        bytes
    };
    let (used, plain) = (definition(true), definition(false));
    let custom = protocol::CustomBlocks::from_definitions([
        ("test:terminal", used.as_slice()),
        ("test:wall", plain.as_slice()),
    ]);
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    assert_eq!(registries.begin_session_hashed_custom_blocks(&custom), 2);
    for (offset, block) in custom.blocks.iter().enumerate() {
        let hash = block.hashed_states()[0].hash;
        let sequential = range.start + u32::try_from(offset).unwrap();
        for fact in [
            registries.block_facts(assets::NetworkIdMode::Sequential, sequential),
            registries.block_facts(assets::NetworkIdMode::Hashed, hash),
        ] {
            assert_eq!(
                fact.traits
                    .contains(assets::registry::BlockTraits::USES_LOCALLY),
                block.name.as_ref() == "test:terminal",
                "{}",
                block.name
            );
        }
    }
}
