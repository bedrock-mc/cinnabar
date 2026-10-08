use sim::{Aabb, Vec3};

use super::{BREG_V2193, active_content_registry_protocol, bind, synthetic_preg};

fn named(tag: u8, name: &str) -> Vec<u8> {
    let mut bytes = vec![tag, u8::try_from(name.len()).unwrap()];
    bytes.extend(name.as_bytes());
    bytes
}

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
        assert_eq!(registry.selection_shapes(id).unwrap(), expected);
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
            collides: true,
            collision_boxes: Some([lower].into()),
            selection: protocol::CustomSelection::Default,
        },
        protocol::CustomBlockPhysics {
            collides: true,
            collision_boxes: Some([upper].into()),
            selection: protocol::CustomSelection::Box(upper),
        },
        protocol::CustomBlockPhysics {
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
            assert_eq!(registry.selection_shapes(id).unwrap(), expected);
        }
    }
}
