use super::*;
use protocol::{CustomBox, CustomSelection};

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
                selection
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
