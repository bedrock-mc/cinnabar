use super::*;
use protocol::{CustomBox, CustomSelection, CustomStateValue};

#[test]
fn mining_only_states_do_not_consume_physics_reservation() {
    let mut mining_only = slab();
    Arc::make_mut(&mut mining_only.visual).permutations = [protocol::CustomPermutation {
        condition: "1".into(),
        components: Default::default(),
        physical: protocol::CustomPhysicalComponents {
            mining: Some(Arc::new(protocol::CustomBlockMining {
                seconds_to_destroy: 0.5,
                ..Default::default()
            })),
            ..Default::default()
        },
    }]
    .into();
    let mut shape = slab();
    Arc::make_mut(&mut shape.visual).permutations[0]
        .physical
        .friction = Some(0.2);
    let mut blocks = CustomBlocks {
        blocks: std::iter::repeat_n(mining_only, 64)
            .chain([shape])
            .collect(),
        ..Default::default()
    };
    resolve_bounded(&mut blocks, 4);
    crate::block_mining::resolve(&mut blocks);
    for block in &blocks.blocks[..64] {
        assert_eq!(block.mining_for_state(1).unwrap().seconds_to_destroy, 0.5);
    }
    let shape = blocks.blocks.last().unwrap();
    assert!(shape.state_physics_complete);
    assert_eq!(
        shape.physics_for_state(1).collision_boxes.as_ref().unwrap()[0].min[1],
        0.5,
        "mining states must leave the shape reservation available"
    );
    assert_eq!(
        shape.physics_for_state(3).selection,
        CustomSelection::Disabled
    );
    assert_eq!(shape.physics_for_state(1).friction, Some(0.2));
}

#[test]
fn mining_resolves_independently_of_the_physics_reservation() {
    let mut block = slab();
    Arc::make_mut(&mut block.visual).permutations[0]
        .physical
        .mining = Some(Arc::new(protocol::CustomBlockMining {
        seconds_to_destroy: 0.5,
        ..Default::default()
    }));
    for physics_budget in [0, 4] {
        let mut blocks = CustomBlocks {
            blocks: [block.clone()].into(),
            ..Default::default()
        };
        resolve_bounded(&mut blocks, physics_budget);
        crate::block_mining::resolve(&mut blocks);
        let block = &blocks.blocks[0];
        assert_eq!(block.state_physics_complete, physics_budget > 0);
        assert_eq!(
            block.physics_for_state(1).collision_boxes.as_ref().unwrap()[0].min[1],
            if physics_budget > 0 { 0.5 } else { 0.0 }
        );
        assert_eq!(block.mining_for_state(1).unwrap().seconds_to_destroy, 0.5);
    }
}

#[test]
fn custom_mining_uncertainty_does_not_make_known_physics_incomplete() {
    for condition in [
        "loop(1, { t.x = 1; }); return t.x;",
        "q.block_state('test:missing')",
    ] {
        let mut block = slab();
        Arc::make_mut(&mut block.visual).permutations = [protocol::CustomPermutation {
            condition: condition.into(),
            components: Default::default(),
            physical: protocol::CustomPhysicalComponents {
                mining: Some(Arc::new(protocol::CustomBlockMining {
                    seconds_to_destroy: -1.0,
                    ..Default::default()
                })),
                ..Default::default()
            },
        }]
        .into();
        for budget in [0, MAX_RESOLVED_PHYSICS_STATES] {
            let mut blocks = CustomBlocks {
                blocks: [block.clone()].into(),
                ..Default::default()
            };
            resolve_bounded(&mut blocks, budget);
            crate::block_mining::resolve(&mut blocks);
            let block = &blocks.blocks[0];
            assert!(
                block.state_physics_complete,
                "mining cannot invalidate physical completeness"
            );
            for state in 0..block.state_count {
                assert_eq!(
                    block.mining_for_state(state).unwrap().seconds_to_destroy,
                    0.0
                );
            }
        }
    }
}

#[test]
fn custom_mining_resolves_beside_incomplete_physics() {
    let mut block = slab();
    let visual = Arc::make_mut(&mut block.visual);
    visual.components_incomplete = true;
    visual.permutations = [protocol::CustomPermutation {
        condition: "1".into(),
        components: Default::default(),
        physical: protocol::CustomPhysicalComponents {
            mining: Some(Arc::new(protocol::CustomBlockMining {
                seconds_to_destroy: 0.5,
                ..Default::default()
            })),
            ..Default::default()
        },
    }]
    .into();
    let mut blocks = CustomBlocks {
        blocks: [block].into(),
        ..Default::default()
    };
    resolve(&mut blocks);
    crate::block_mining::resolve(&mut blocks);
    let block = &blocks.blocks[0];
    assert!(block.state_physics.is_empty());
    assert!(!block.state_physics_complete);
    for state in 0..block.state_count {
        assert_eq!(
            block.mining_for_state(state).unwrap().seconds_to_destroy,
            0.5
        );
    }
}

#[test]
fn custom_mining_permutations_replace_the_whole_component() {
    let mut block = slab();
    block.mining = Arc::new(protocol::CustomBlockMining {
        seconds_to_destroy: 4.0,
        ..Default::default()
    });
    let visuals = Arc::make_mut(&mut block.visual);
    visuals.permutations = [protocol::CustomPermutation {
        condition: "1".into(),
        components: Default::default(),
        physical: protocol::CustomPhysicalComponents {
            mining: Some(Arc::new(protocol::CustomBlockMining {
                seconds_to_destroy: 0.5,
                ..Default::default()
            })),
            ..Default::default()
        },
    }]
    .into();
    let mut blocks = CustomBlocks {
        blocks: [block].into(),
        ..Default::default()
    };
    resolve(&mut blocks);
    crate::block_mining::resolve(&mut blocks);
    for state in 0..blocks.blocks[0].state_count {
        assert_eq!(
            blocks.blocks[0]
                .mining_for_state(state)
                .unwrap()
                .seconds_to_destroy,
            0.5
        );
    }
}

#[test]
fn custom_mining_unevaluated_conditions_read_as_zero() {
    for condition in [
        "loop(1, { t.x = 1; }); return t.x;",
        "q.block_state('test:missing')",
    ] {
        let mut block = slab();
        Arc::make_mut(&mut block.visual).permutations = [protocol::CustomPermutation {
            condition: condition.into(),
            components: Default::default(),
            physical: protocol::CustomPhysicalComponents {
                mining: Some(Arc::new(protocol::CustomBlockMining {
                    seconds_to_destroy: -1.0,
                    ..Default::default()
                })),
                ..Default::default()
            },
        }]
        .into();
        assert!(block.mining_for_state(0).is_none());
        let mut blocks = CustomBlocks {
            blocks: [block].into(),
            ..Default::default()
        };
        resolve(&mut blocks);
        crate::block_mining::resolve(&mut blocks);
        assert!(blocks.blocks[0].state_physics_complete);
        for state in 0..blocks.blocks[0].state_count {
            let mining = blocks.blocks[0].mining_for_state(state).unwrap();
            assert_eq!(mining.seconds_to_destroy, 0.0, "{condition} must not apply");
        }
    }
}

#[test]
fn custom_mining_unrelated_physics_gaps_keep_the_base_component() {
    let mut block = slab();
    Arc::make_mut(&mut block.visual)
        .permutations
        .iter_mut()
        .for_each(|permutation| permutation.condition = "q.unsupported()".into());
    let mut blocks = CustomBlocks {
        blocks: [block].into(),
        ..Default::default()
    };
    resolve(&mut blocks);
    assert!(!blocks.blocks[0].state_physics_complete);
    assert!(blocks.blocks[0].mining_for_state(0).is_some());
}

fn text(value: &str) -> Vec<u8> {
    let mut encoded = vec![u8::try_from(value.len()).unwrap()];
    encoded.extend(value.as_bytes());
    encoded
}

fn named(tag: u8, name: &str) -> Vec<u8> {
    let mut encoded = vec![tag];
    encoded.extend(text(name));
    encoded
}

fn boxes(min_y: f32, max_y: f32) -> Vec<u8> {
    let mut encoded = named(10, "minecraft:collision_box");
    encoded.extend(named(1, "enabled"));
    encoded.push(1);
    encoded.extend(named(9, "boxes"));
    encoded.extend([10, 2]);
    for (name, value) in [
        ("minX", 0.0_f32),
        ("minY", min_y),
        ("minZ", 0.0),
        ("maxX", 16.0),
        ("maxY", max_y),
        ("maxZ", 16.0),
    ] {
        encoded.extend(named(5, name));
        encoded.extend(value.to_le_bytes());
    }
    encoded.extend([0, 0]);
    encoded
}

fn selection(min_y: f32, size_y: f32) -> Vec<u8> {
    let mut encoded = named(10, "minecraft:selection_box");
    encoded.extend(named(1, "enabled"));
    encoded.push(1);
    for (name, values) in [
        ("origin", [-8.0_f32, min_y, -8.0]),
        ("size", [16.0, size_y, 16.0]),
    ] {
        encoded.extend(named(9, name));
        encoded.extend([5, 6]);
        for value in values {
            encoded.extend(value.to_le_bytes());
        }
    }
    encoded.push(0);
    encoded
}

fn slab() -> CustomBlock {
    let mut encoded = named(10, "");
    encoded.extend(named(10, "components"));
    encoded.extend(boxes(0.0, 8.0));
    encoded.extend(selection(0.0, 8.0));
    encoded.push(0);
    encoded.extend(named(9, "properties"));
    encoded.extend([10, 2]);
    encoded.extend(named(8, "name"));
    encoded.extend(text("minecraft:vertical_half"));
    encoded.extend(named(9, "enum"));
    encoded.extend([8, 8]);
    for value in ["bottom", "top", "double", "empty"] {
        encoded.extend(text(value));
    }
    encoded.push(0);
    encoded.extend(named(9, "permutations"));
    encoded.extend([10, 6]);
    for (state, min_y, max_y) in [("top", 8.0, 16.0), ("double", 0.0, 16.0)] {
        encoded.extend(named(8, "condition"));
        encoded.extend(text(&format!(
            "q.block_property('minecraft:vertical_half') == '{state}'"
        )));
        encoded.extend(named(10, "components"));
        encoded.extend(boxes(min_y, max_y));
        encoded.extend(selection(min_y, max_y - min_y));
        encoded.extend([0, 0]);
    }
    encoded.extend(named(8, "condition"));
    encoded.extend(text("q.block_state('minecraft:vertical_half') == 'empty'"));
    encoded.extend(named(10, "components"));
    for name in ["minecraft:collision_box", "minecraft:selection_box"] {
        encoded.extend(named(1, name));
        encoded.push(0);
    }
    encoded.extend([0, 0, 0]);
    protocol::CustomBlocks::from_definitions([("test:layered_slab", encoded.as_slice())]).blocks[0]
        .clone()
}

#[test]
fn slab_top_and_double_states_replace_base_collision_and_selection() {
    let block = slab();
    assert_eq!(block.state_count, 4);
    for (state, min_y, max_y) in [(0, 0.0, 0.5), (1, 0.5, 1.0), (2, 0.0, 1.0)] {
        let (collides, collision, selection) = state_physics(&block, state);
        assert!(collides);
        let expected = CustomBox {
            min: [0.0, min_y, 0.0],
            max: [1.0, max_y, 1.0],
        };
        assert_eq!(collision.as_deref(), Some([expected].as_slice()));
        assert_eq!(selection, CustomSelection::Box(expected));
    }
}

#[test]
fn a_matching_permutation_can_disable_collision_and_targeting() {
    let (collides, _, selection) = state_physics(&slab(), 3);
    assert!(!collides);
    assert_eq!(selection, CustomSelection::Disabled);
}

#[test]
fn preparation_materializes_state_physics_for_registry_consumers() {
    let mut blocks = CustomBlocks {
        blocks: Arc::from([slab()]),
        ..Default::default()
    };
    resolve(&mut blocks);
    assert_eq!(blocks.skipped, 0);
    let block = &blocks.blocks[0];
    assert_eq!(block.state_physics.len(), block.state_count as usize);
    for state in 0..block.state_count {
        let transformation = block.physics_for_state(state).transformation;
        let (collides, collision_boxes, selection) = state_physics(block, state);
        assert_eq!(
            block.physics_for_state(state),
            CustomBlockPhysics {
                random_offset: None,
                transformation,
                collides,
                collision_boxes,
                selection,
                friction: None,
            }
        );
    }
}

#[test]
fn unsupported_physical_conditions_are_counted_and_keep_base_components() {
    let mut block = slab();
    let visuals = Arc::make_mut(&mut block.visual);
    for permutation in &mut visuals.permutations {
        permutation.condition = "q.unsupported()".into();
    }
    let mut blocks = CustomBlocks {
        blocks: Arc::from([block]),
        ..Default::default()
    };
    resolve(&mut blocks);
    assert_eq!(blocks.skipped, 3);
    assert!(blocks.blocks[0].state_physics.is_empty());
    assert_eq!(
        blocks.blocks[0].physics_for_state(1),
        blocks.blocks[0].base_physics()
    );
}

#[test]
fn discarded_wire_permutations_keep_physics_incomplete() {
    let mut encoded = named(10, "");
    encoded.extend(named(9, "permutations"));
    encoded.push(10);
    let count = 4096_u32;
    let mut length = count << 1;
    while length >= 128 {
        encoded.push(length as u8 | 128);
        length >>= 7;
    }
    encoded.push(length as u8);
    for _ in 0..count {
        encoded.extend(named(8, "condition"));
        encoded.extend(text("1"));
        encoded.extend(named(10, "components"));
        encoded.extend(named(1, "minecraft:collision_box"));
        encoded.extend([0, 0, 0]);
    }
    encoded.push(0);
    let mut blocks = CustomBlocks::from_definitions([("test:oversized", encoded.as_slice())]);
    assert_eq!(blocks.blocks.len(), 1);
    assert!(blocks.skipped > 0);
    resolve(&mut blocks);
    assert_eq!(blocks.blocks[0].state_physics.len(), 1);
    assert!(!blocks.blocks[0].state_physics_complete);
}

#[test]
fn physical_completeness_rejects_partial_arrays_and_failed_evaluations() {
    for unsupported in [
        "q.unsupported()",
        "q.block_state('test:missing')",
        "1e30 * 1e30",
    ] {
        let mut block = slab();
        Arc::make_mut(&mut block.visual).permutations[0].condition = unsupported.into();
        let mut blocks = CustomBlocks {
            blocks: vec![block].into(),
            ..Default::default()
        };
        resolve(&mut blocks);
        let block = &blocks.blocks[0];
        assert_eq!(block.state_physics.len(), block.state_count as usize);
        assert!(!block.state_physics_complete, "{unsupported}");
        assert!(blocks.skipped > 0);
    }
    let mut complete = CustomBlocks {
        blocks: vec![slab()].into(),
        ..Default::default()
    };
    resolve(&mut complete);
    assert!(complete.blocks[0].state_physics_complete);
}

#[test]
fn later_matching_components_inherit_absent_fields_and_replace_present_fields() {
    let mut block = slab();
    let visuals = Arc::make_mut(&mut block.visual);
    let mut later = visuals.permutations[0].clone();
    later.physical.collision = None;
    later.physical.selection = Some(CustomSelection::Disabled);
    visuals.permutations = visuals
        .permutations
        .iter()
        .cloned()
        .chain([later])
        .collect();
    let mut blocks = CustomBlocks {
        blocks: [block].into(),
        ..Default::default()
    };
    resolve(&mut blocks);
    let physics = blocks.blocks[0].physics_for_state(1);
    assert_eq!(physics.collision_boxes.as_ref().unwrap()[0].min[1], 0.5);
    assert_eq!(physics.selection, CustomSelection::Disabled);
}

#[test]
fn aggregate_state_reservation_is_bounded_and_does_not_expand_for_rejected_blocks() {
    let mut single = slab();
    single.state_count = 1;
    Arc::make_mut(&mut single.visual).state_axes[0].values =
        Box::new([CustomStateValue::String("bottom".into())]);
    let mut blocks = CustomBlocks {
        blocks: [slab(), slab(), single].into(),
        ..Default::default()
    };
    resolve_bounded(&mut blocks, 5);
    assert_eq!(blocks.skipped, 1);
    assert_eq!(blocks.blocks[0].state_physics.len(), 4);
    assert!(blocks.blocks[1].state_physics.is_empty());
    assert_eq!(blocks.blocks[2].state_physics.len(), 1);
}

#[path = "tests/fixture.rs"]
mod fixture;

/// A friction-only permutation is physical and replaces the base value for its states.
#[test]
fn friction_permutations_override_the_base_component() {
    let mut encoded = named(10, "");
    encoded.extend(named(10, "components"));
    encoded.extend(named(10, "minecraft:friction"));
    encoded.extend(named(5, "value"));
    encoded.extend(0.4_f32.to_le_bytes());
    encoded.extend([0, 0]);
    encoded.extend(named(9, "properties"));
    encoded.extend([10, 2]);
    encoded.extend(named(8, "name"));
    encoded.extend(text("test:surface"));
    encoded.extend(named(9, "enum"));
    encoded.extend([8, 4]);
    encoded.extend(text("dry"));
    encoded.extend(text("icy"));
    encoded.push(0);
    encoded.extend(named(9, "permutations"));
    encoded.extend([10, 2]);
    encoded.extend(named(8, "condition"));
    encoded.extend(text("q.block_state('test:surface') == 'icy'"));
    encoded.extend(named(10, "components"));
    encoded.extend(named(10, "minecraft:friction"));
    encoded.extend(named(5, "value"));
    encoded.extend(0.02_f32.to_le_bytes());
    encoded.extend([0, 0, 0, 0]);
    let mut blocks = CustomBlocks::from_definitions([("test:ice", encoded.as_slice())]);
    assert_eq!(blocks.skipped, 0);
    resolve(&mut blocks);
    assert_eq!(blocks.skipped, 0);
    let block = &blocks.blocks[0];
    assert_eq!(block.physics_for_state(0).friction, Some(0.4));
    assert_eq!(block.physics_for_state(1).friction, Some(0.02));
}

#[test]
fn random_offset_state_overrides_are_prepared_once_in_palette_order() {
    let mut block = slab();
    let base = block_transform::random_offset::BAMBOO;
    let zero = block_transform::random_offset::RandomOffsetComponent::default();
    let visual = Arc::make_mut(&mut block.visual);
    visual.base.random_offset = Some(base);
    visual.permutations = Box::new([protocol::CustomPermutation {
        condition: "q.block_state('minecraft:vertical_half') == 'top'".into(),
        components: protocol::CustomVisualComponents {
            random_offset: Some(zero),
            ..Default::default()
        },
        physical: protocol::CustomPhysicalComponents {
            random_offset: Some(zero),
            ..Default::default()
        },
    }]);
    let mut blocks = CustomBlocks {
        blocks: Arc::from([block]),
        ..Default::default()
    };
    resolve(&mut blocks);
    assert_eq!(blocks.skipped, 0);
    for (state, expected) in [(0, base), (1, zero), (2, base), (3, base)] {
        assert_eq!(
            blocks.blocks[0].physics_for_state(state).random_offset,
            Some(expected)
        );
    }
}

/// Zigzag VarInt element count for a network NBT list.
fn count(elements: usize) -> Vec<u8> {
    let mut zigzag = (elements as i64) << 1;
    let mut encoded = Vec::new();
    loop {
        let byte = (zigzag & 0x7f) as u8;
        zigzag >>= 7;
        if zigzag == 0 {
            encoded.push(byte);
            return encoded;
        }
        encoded.push(byte | 0x80);
    }
}

/// A face-attached block that orients only through `minecraft:transformation`,
/// as `owlscubed_pob:backrooms_wallpaper_slab` does on the wire: the face
/// permutations carry no boxes of their own.
fn face_attached() -> protocol::CustomBlocks {
    let mut encoded = named(10, "");
    encoded.extend(named(10, "components"));
    // The block's default face is `up`; the base box is that top slab.
    encoded.extend(boxes(8.0, 16.0));
    encoded.extend(selection(8.0, 8.0));
    encoded.push(0);
    encoded.extend(named(9, "properties"));
    encoded.extend([10]);
    encoded.extend(count(1));
    encoded.extend(named(8, "name"));
    encoded.extend(text("minecraft:block_face"));
    encoded.extend(named(9, "enum"));
    encoded.extend([8]);
    encoded.extend(count(6));
    for face in ["down", "up", "north", "south", "west", "east"] {
        encoded.extend(text(face));
    }
    encoded.push(0);
    encoded.extend(named(9, "permutations"));
    encoded.extend([10]);
    encoded.extend(count(5));
    for (face, rotation) in [
        ("north", [1, 2, 0]),
        ("south", [1, 0, 0]),
        ("east", [0, 1, 3]),
        ("west", [2, 1, 3]),
        ("down", [2, 0, 0]),
    ] {
        encoded.extend(named(8, "condition"));
        encoded.extend(text(&format!(
            "q.block_state('minecraft:block_face') == '{face}'"
        )));
        encoded.extend(named(10, "components"));
        encoded.extend(named(10, "minecraft:transformation"));
        for (key, value) in [
            ("RX", rotation[0]),
            ("RY", rotation[1]),
            ("RZ", rotation[2]),
            ("SX", 1),
            ("SY", 1),
            ("SZ", 1),
            ("TX", 0),
            ("TY", 0),
            ("TZ", 0),
        ] {
            encoded.extend(named(3, key));
            encoded.extend(count(value));
        }
        // Close the transformation and component compounds, then the permutation.
        encoded.extend([0, 0, 0]);
    }
    encoded.push(0);
    protocol::CustomBlocks::from_definitions([("test:face_facing", encoded.as_slice())])
}

/// Transformation-only permutations are physical: each face state keeps its own turn.
#[test]
fn every_face_state_carries_its_own_turn() {
    let mut blocks = face_attached();
    resolve(&mut blocks);
    assert_eq!(blocks.skipped, 0);
    let block = &blocks.blocks[0];
    assert_eq!(block.state_physics.len(), 6);
    let expected: [(u32, Option<[i32; 3]>); 6] = [
        (0, Some([2, 0, 0])),
        (1, None), // up: the default face, no permutation
        (2, Some([1, 2, 0])),
        (3, Some([1, 0, 0])),
        (4, Some([2, 1, 3])),
        (5, Some([0, 1, 3])),
    ];
    for (state, rotation) in expected {
        assert_eq!(
            block
                .physics_for_state(state)
                .transformation
                .map(|t| t.rotation),
            rotation,
            "turn for state {state}"
        );
    }
}

/// A later permutation without a transformation replaces the boxes and inherits the turn.
#[test]
fn a_later_permutation_without_a_transform_inherits_the_earlier_turn() {
    let mut blocks = face_attached();
    resolve(&mut blocks);
    let block = Arc::make_mut(&mut blocks.blocks);
    let visuals = Arc::make_mut(&mut block[0].visual);
    let mut later = protocol::CustomPermutation {
        condition: "q.block_state('minecraft:block_face') == 'east'".into(),
        components: protocol::CustomVisualComponents::default(),
        physical: protocol::CustomPhysicalComponents::default(),
    };
    later.physical.collision = Some(protocol::CustomCollision {
        enabled: true,
        boxes: Some(
            [protocol::CustomBox {
                min: [0.0; 3],
                max: [1.0; 3],
            }]
            .into(),
        ),
    });
    visuals.permutations = visuals
        .permutations
        .iter()
        .cloned()
        .chain([later])
        .collect();
    let mut blocks = CustomBlocks {
        blocks: blocks.blocks.clone(),
        ..Default::default()
    };
    resolve(&mut blocks);
    let east = blocks.blocks[0]
        .physics_for_state(5)
        .transformation
        .expect("the earlier permutation still contributes its transform");
    assert_eq!(east.rotation, [0, 1, 3]);
    assert_eq!(
        blocks.blocks[0]
            .physics_for_state(5)
            .collision_boxes
            .as_deref(),
        Some(
            [protocol::CustomBox {
                min: [0.0; 3],
                max: [1.0; 3]
            }]
            .as_slice()
        )
    );
}

// Fence collision permutations resolve per connection state once the trait
// supplies the four boolean axes: the isolated post keeps its base box while
// a connected state takes the matching arm box.
#[test]
fn connection_states_select_per_state_collision() {
    use protocol::{
        CustomBlockVisuals, CustomPermutation, CustomPhysicalComponents, CustomStateAxis,
        CustomStateValue, CustomVisualComponents,
    };
    let post = protocol::CustomBox {
        min: [0.375, 0.0, 0.375],
        max: [0.625, 1.5, 0.625],
    };
    let arm = |direction: &str, boxes: protocol::CustomBox| CustomPermutation {
        physical: CustomPhysicalComponents {
            collision: Some(protocol::CustomCollision {
                enabled: true,
                boxes: Some(Arc::from([boxes])),
            }),
            selection: Some(CustomSelection::Box(boxes)),
            ..CustomPhysicalComponents::default()
        },
        condition: format!("q.block_state('minecraft:connection_{direction}')").into(),
        components: CustomVisualComponents::default(),
    };
    let block = CustomBlock {
        mining: Default::default(),
        sound: None,
        state_physics_complete: false,
        state_physics: Default::default(),
        state_mining: Default::default(),
        player_interact: false,
        name: "test:fence".into(),
        tags: Default::default(),
        state_count: 16,
        collides: true,
        collision_boxes: Some(Arc::from([post])),
        selection: CustomSelection::Box(post),
        friction: None,
        visual: Arc::new(CustomBlockVisuals {
            permutations: Box::new([
                arm(
                    "north",
                    protocol::CustomBox {
                        min: [0.375, 0.0, 0.0],
                        max: [0.625, 1.5, 0.625],
                    },
                ),
                arm(
                    "south",
                    protocol::CustomBox {
                        min: [0.375, 0.0, 0.375],
                        max: [0.625, 1.5, 1.0],
                    },
                ),
            ]),
            state_axes: ["north", "east", "south", "west"]
                .map(|direction| CustomStateAxis {
                    name: format!("minecraft:connection_{direction}").into(),
                    values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
                })
                .into(),
            ..CustomBlockVisuals::default()
        }),
    };
    // North varies fastest: state 1 connects north, state 4 connects south.
    let (collides, collision, selection) = state_physics(&block, 0);
    assert!(collides);
    assert_eq!(collision.as_deref(), Some([post].as_slice()));
    assert_eq!(selection, CustomSelection::Box(post));
    let north = protocol::CustomBox {
        min: [0.375, 0.0, 0.0],
        max: [0.625, 1.5, 0.625],
    };
    let (_, collision, selection) = state_physics(&block, 1);
    assert_eq!(collision.as_deref(), Some([north].as_slice()));
    assert_eq!(selection, CustomSelection::Box(north));
}

// A disabling mining override past the permutation bound cannot be read, so mining stays
// unknown instead of allowing an instant break; a huge truncated definition does no mining work.
#[test]
fn custom_mining_truncated_definitions_stay_unknown_without_evaluation() {
    for properties in [0_u32, 16] {
        let permutations: u32 = 1025;
        let mut encoded = named(10, "");
        encoded.extend(named(9, "properties"));
        encoded.push(10);
        encoded.extend(varint(properties << 1));
        for index in 0..properties {
            encoded.extend(named(8, "name"));
            encoded.extend(text(&format!("test:bit{index}")));
            encoded.extend(named(9, "enum"));
            encoded.extend([1, 4, 0, 1]);
            encoded.push(0);
        }
        encoded.extend(named(9, "permutations"));
        encoded.push(10);
        encoded.extend(varint(permutations << 1));
        for index in 0..permutations {
            encoded.extend(named(8, "condition"));
            encoded.extend(text("1"));
            if index + 1 == permutations {
                encoded.extend(named(10, "components"));
                encoded.extend(named(10, "minecraft:destructible_by_mining"));
                encoded.extend(named(5, "value"));
                encoded.extend((-1.0_f32).to_le_bytes());
                encoded.extend([0, 0]);
            }
            encoded.push(0);
        }
        encoded.push(0);
        let mut blocks = CustomBlocks::from_definitions([("test:many", encoded.as_slice())]);
        assert_eq!(blocks.blocks[0].state_count, 1 << properties);
        resolve(&mut blocks);
        crate::block_mining::resolve(&mut blocks);
        let block = &blocks.blocks[0];
        assert!(block.state_mining.is_empty(), "no state is evaluated");
        assert!(block.mining_for_state(0).is_none());
    }
}

/// Encodes an unsigned network VarInt.
fn varint(mut value: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    while value >= 0x80 {
        bytes.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
    bytes
}
