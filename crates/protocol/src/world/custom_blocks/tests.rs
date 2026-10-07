use super::{
    CustomBlock, CustomBlockVisuals, CustomSelection, CustomStateAxis, CustomStateValue,
    Definition, Nbt, block_name_sort_key,
};

fn parse_definition(bytes: &[u8]) -> Option<Definition> {
    super::parse_definition(&crate::nbt_tree::read_root(bytes)?)
}

// Every state appears once, in palette order, with a distinct hash.
#[test]
fn hashed_states_enumerate_axes_and_hash_distinctly() {
    let block = CustomBlock {
        name: "ns:b".into(),
        tags: Default::default(),
        state_count: 6,
        collides: true,
        collision_box: None,
        selection: CustomSelection::Default,
        visual: std::sync::Arc::new(CustomBlockVisuals {
            state_axes: Box::new([
                CustomStateAxis {
                    name: "ns:a".into(),
                    values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
                },
                CustomStateAxis {
                    name: "ns:c".into(),
                    values: Box::new([
                        CustomStateValue::Int(0),
                        CustomStateValue::Int(1),
                        CustomStateValue::Int(2),
                    ]),
                },
            ]),
            ..CustomBlockVisuals::default()
        }),
    };
    let states = block.hashed_states();
    assert_eq!(states.len(), 6);
    assert_eq!(
        states[1].values.as_ref(),
        [CustomStateValue::Bool(true), CustomStateValue::Int(0)],
        "the first axis varies fastest"
    );
    let hashes: std::collections::HashSet<_> = states.iter().map(|state| state.hash).collect();
    assert_eq!(hashes.len(), 6);
    let plain = CustomBlock {
        visual: std::sync::Arc::new(CustomBlockVisuals::default()),
        ..block
    };
    assert_eq!(plain.hashed_states().len(), 1);
}

fn string(value: &str) -> Vec<u8> {
    let mut bytes = vec![value.len() as u8];
    bytes.extend_from_slice(value.as_bytes());
    bytes
}

fn named(tag: u8, name: &str) -> Vec<u8> {
    let mut bytes = vec![tag];
    bytes.extend(string(name));
    bytes
}

#[test]
fn block_target_tags_survive_wire_decode_and_odd_entries_are_counted() {
    let mut nbt = named(10, "");
    nbt.extend(named(9, "blockTags"));
    nbt.extend([8, 6]);
    nbt.extend(string("test:target"));
    nbt.extend(string(""));
    nbt.extend(string("test:target"));
    nbt.push(0);
    let blocks = super::CustomBlocks::from_definitions([("test:block", nbt.as_slice())]);
    assert_eq!(blocks.blocks.len(), 1);
    assert_eq!(
        blocks.blocks[0].tags.as_ref(),
        [std::sync::Arc::from("test:target")]
    );
    assert_eq!(blocks.skipped, 1);

    let mut nbt = named(10, "");
    nbt.extend(named(8, "blockTags"));
    nbt.extend(string("not a list"));
    nbt.push(0);
    let blocks = super::CustomBlocks::from_definitions([("test:block", nbt.as_slice())]);
    assert_eq!(blocks.blocks.len(), 1);
    assert!(blocks.blocks[0].tags.is_empty());
    assert_eq!(blocks.skipped, 1);
}

#[test]
fn placement_trait_and_enum_properties_multiply_states() {
    let mut nbt = named(10, "");
    nbt.extend(named(9, "properties"));
    nbt.extend([10, 4]);
    for values in [2_u8, 3] {
        nbt.extend(named(9, "enum"));
        nbt.extend([8, values * 2]);
        for index in 0..values {
            nbt.extend(string(&index.to_string()));
        }
        nbt.push(0);
    }
    nbt.extend(named(9, "traits"));
    nbt.extend([10, 2]);
    nbt.extend(named(10, "enabled_states"));
    nbt.extend(named(1, "cardinal_direction"));
    nbt.extend([1, 0, 0]);
    nbt.extend(named(10, "components"));
    nbt.extend(named(1, "minecraft:collision_box"));
    nbt.extend([0, 0, 0]);
    let definition = parse_definition(&nbt).expect("definition");
    assert_eq!(
        (definition.state_count, definition.collides),
        (2 * 3 * 4, false)
    );
    let axes = &definition.visual.state_axes;
    assert_eq!(axes.len(), 1, "unnamed properties carry no axis");
    assert_eq!(axes[0].name.as_ref(), "minecraft:cardinal_direction");
    assert_eq!(
        axes[0].values[0],
        super::CustomStateValue::String("south".into())
    );
}

fn compound(fields: Vec<(&str, Nbt)>) -> Nbt {
    Nbt::Compound(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn text(value: &str) -> Nbt {
    Nbt::String(value.to_owned())
}

fn trait_nbt(name: &str, states: &[(&str, i8)]) -> Nbt {
    compound(vec![
        ("name", text(name)),
        (
            "enabled_states",
            compound(
                states
                    .iter()
                    .map(|&(state, flag)| (state, Nbt::Byte(flag)))
                    .collect(),
            ),
        ),
    ])
}

fn placement_traits(direction_first: bool) -> Vec<Nbt> {
    let direction = trait_nbt(
        "minecraft:placement_direction",
        &[
            ("cardinal_direction", 1),
            ("corner_and_cardinal_direction", 0),
            ("facing_direction", 1),
            ("sixteen_way_rotation", 0),
        ],
    );
    let position = trait_nbt(
        "minecraft:placement_position",
        &[("block_face", 1), ("vertical_half", 1)],
    );
    if direction_first {
        vec![direction, position]
    } else {
        vec![position, direction]
    }
}

fn custom_block(name: &str, root: &Nbt) -> CustomBlock {
    let definition = super::parse_definition(root).expect("definition");
    CustomBlock {
        name: name.into(),
        tags: definition.tags,
        state_count: definition.state_count,
        collides: definition.collides,
        collision_box: definition.collision_box,
        selection: definition.selection,
        visual: std::sync::Arc::new(definition.visual),
    }
}

fn named_values(block: &CustomBlock, index: u32) -> Vec<(String, String)> {
    let values = block.state_values(index).expect("decodable state");
    block
        .visual
        .state_axes
        .iter()
        .zip(values.iter())
        .map(|(axis, value)| {
            let value = match value {
                CustomStateValue::String(text) => text.to_string(),
                CustomStateValue::Int(number) => number.to_string(),
                CustomStateValue::Bool(flag) => flag.to_string(),
            };
            (axis.name.to_string(), value)
        })
        .collect()
}

fn expect_state(block: &CustomBlock, index: u32, expected: &[(&str, &str)]) {
    let mut actual = named_values(block, index);
    actual.sort();
    let mut expected = expected
        .iter()
        .map(|&(name, value)| (name.to_owned(), value.to_owned()))
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(actual, expected, "{} state {index}", block.name);
}

// Vanilla palette, read back from a dedicated server with sequential ids: trait states
// precede properties, position precedes direction whatever the declared order, and the
// first-added state varies fastest.
#[test]
fn trait_states_lead_the_palette_in_vanilla_order() {
    const C: &str = "minecraft:cardinal_direction";
    const F: &str = "minecraft:facing_direction";
    const B: &str = "minecraft:block_face";
    const V: &str = "minecraft:vertical_half";
    for direction_first in [true, false] {
        let block = custom_block(
            "df:a_dirpos",
            &compound(vec![(
                "traits",
                Nbt::List(placement_traits(direction_first)),
            )]),
        );
        assert_eq!(block.state_count, 288);
        for (index, [cardinal, facing, face, half]) in [
            (0, ["south", "down", "down", "bottom"]),
            (1, ["south", "down", "up", "bottom"]),
            (5, ["south", "down", "east", "bottom"]),
            (6, ["south", "down", "down", "top"]),
            (12, ["west", "down", "down", "bottom"]),
            (47, ["east", "down", "east", "top"]),
            (48, ["south", "up", "down", "bottom"]),
            (143, ["east", "north", "east", "top"]),
            (144, ["south", "south", "down", "bottom"]),
            (287, ["east", "east", "east", "top"]),
        ] {
            expect_state(
                &block,
                index,
                &[(C, cardinal), (F, facing), (B, face), (V, half)],
            );
        }
    }
    let properties = compound(vec![
        (
            "traits",
            Nbt::List(vec![trait_nbt(
                "minecraft:placement_direction",
                &[("cardinal_direction", 1)],
            )]),
        ),
        (
            "properties",
            Nbt::List(vec![
                compound(vec![
                    ("enum", Nbt::List(vec![Nbt::Byte(0), Nbt::Byte(1)])),
                    ("name", text("df:zeta")),
                ]),
                compound(vec![
                    (
                        "enum",
                        Nbt::List(vec![Nbt::Int(0), Nbt::Int(1), Nbt::Int(2)]),
                    ),
                    ("name", text("df:alpha")),
                ]),
            ]),
        ),
    ]);
    let block = custom_block("df:c_props", &properties);
    assert_eq!(block.state_count, 24);
    for (index, [cardinal, zeta, alpha]) in [
        (1, ["west", "false", "0"]),
        (4, ["south", "true", "0"]),
        (8, ["south", "false", "1"]),
        (23, ["east", "true", "2"]),
    ] {
        expect_state(
            &block,
            index,
            &[(C, cardinal), ("df:zeta", zeta), ("df:alpha", alpha)],
        );
    }
}

// Hashed sessions enumerate the same palette order, so index and hash agree.
#[test]
fn hashed_states_follow_palette_order() {
    let block = custom_block(
        "df:a_dirpos",
        &compound(vec![("traits", Nbt::List(placement_traits(true)))]),
    );
    let states = block.hashed_states();
    assert_eq!(states.len(), 288);
    for index in [0_u32, 7, 143, 287] {
        assert_eq!(
            Some(states[index as usize].values.clone()),
            block.state_values(index)
        );
    }
}

// Unnamed properties still count toward the palette but cannot be decoded.
#[test]
fn states_the_axes_cannot_account_for_are_not_decoded() {
    let block = custom_block(
        "ns:b",
        &compound(vec![(
            "properties",
            Nbt::List(vec![compound(vec![(
                "enum",
                Nbt::List(vec![Nbt::Int(0), Nbt::Int(1)]),
            )])]),
        )]),
    );
    assert_eq!((block.state_count, block.state_values(0)), (2, None));
}

#[test]
fn geometry_bone_visibility_is_retained_per_component_set() {
    let geometry = |bones: Vec<(&str, Nbt)>| {
        compound(vec![(
            "minecraft:geometry",
            compound(vec![
                ("identifier", text("geometry.df_test")),
                ("bone_visibility", compound(bones)),
            ]),
        )])
    };
    let root = compound(vec![
        (
            "components",
            geometry(vec![
                (
                    "a",
                    text("q.block_state('minecraft:cardinal_direction') == 'north'"),
                ),
                ("c", text("0.000000")),
                ("d", Nbt::Byte(1)),
            ]),
        ),
        (
            "permutations",
            Nbt::List(vec![compound(vec![
                ("condition", text("q.block_state('df:s')")),
                ("components", geometry(vec![("a", text("0.000000"))])),
            ])]),
        ),
    ]);
    let visual = super::parse_definition(&root).expect("definition").visual;
    let bones = |components: &super::CustomVisualComponents| {
        components
            .bone_visibility
            .iter()
            .map(|(bone, expression)| (bone.to_string(), expression.to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bones(&visual.base),
        [
            (
                "a".to_owned(),
                "q.block_state('minecraft:cardinal_direction') == 'north'".to_owned()
            ),
            ("c".to_owned(), "0.000000".to_owned()),
            ("d".to_owned(), "1".to_owned()),
        ]
    );
    assert_eq!(
        bones(&visual.permutations[0].components),
        [("a".to_owned(), "0.000000".to_owned())]
    );
}

#[test]
fn geometry_bone_visibility_retains_versioned_network_expressions() {
    let expression = "q.block_state('test:raised') == 1";
    let mut nbt = named(10, "");
    nbt.extend(named(10, "components"));
    nbt.extend(named(10, "minecraft:geometry"));
    nbt.extend(string_field("identifier", "geometry.test_edge"));
    nbt.extend(named(10, "bone_visibility"));
    for (bone, value) in [("raised", expression), ("hidden", "0.000000")] {
        nbt.extend(named(10, bone));
        nbt.extend(string_field("expression", value));
        nbt.extend(named(3, "version"));
        nbt.extend([18, 0]);
    }
    nbt.extend(named(10, "unknown"));
    nbt.extend(string_field("unrelated", "0.000000"));
    nbt.push(0);
    nbt.extend(named(10, "malformed_expression"));
    nbt.extend(named(1, "expression"));
    nbt.extend([1, 0]);
    nbt.extend([0, 0, 0, 0]);

    let blocks = super::CustomBlocks::from_definitions([("test:edge", nbt.as_slice())]);
    assert_eq!(blocks.blocks.len(), 1, "odd bone entries keep the block");
    let base = &blocks.blocks[0].visual.base;
    assert_eq!(base.geometry.as_deref(), Some("geometry.test_edge"));
    assert_eq!(
        base.bone_visibility
            .iter()
            .map(|(bone, expression)| (bone.as_ref(), expression.as_ref()))
            .collect::<Vec<_>>(),
        [("raised", expression), ("hidden", "0.000000")],
        "structured expressions must reach state-dependent geometry evaluation"
    );
}

fn string_field(name: &str, value: &str) -> Vec<u8> {
    let mut bytes = named(8, name);
    bytes.extend(string(value));
    bytes
}

#[test]
fn network_light_descriptions_retain_zero_dampening_and_emission() {
    // Native serialization uses byte tags; accept numeric server variants too.
    for dampening_tag in [1, 3] {
        let mut nbt = named(10, "");
        nbt.extend(named(10, "components"));
        for (component, field, tag, level) in [
            (
                "minecraft:light_dampening",
                "lightLevel",
                dampening_tag,
                0_u8,
            ),
            ("minecraft:light_emission", "emission", 1, 13),
        ] {
            nbt.extend(named(10, component));
            nbt.extend(named(tag, field));
            nbt.extend([level, 0]); // Zero has the same byte/zigzag-int encoding.
        }
        nbt.extend([0, 0]);
        let visual = parse_definition(&nbt).expect("network definition").visual;
        assert_eq!(visual.base.light_dampening, Some(0));
        assert_eq!(visual.base.light_emission, Some(13));
    }
}

#[test]
fn scalar_light_components_remain_lenient_for_odd_values() {
    let components = |value| Nbt::Compound(vec![("minecraft:light_dampening".into(), value)]);
    for (value, expected) in [
        (Nbt::Int(0), Some(0)),
        (Nbt::Int(30), Some(15)),
        (Nbt::Int(-1), Some(0)),
        (Nbt::Float(f64::NAN), None),
        (Nbt::String("unknown".into()), None),
    ] {
        let visual = super::visual_components(Some(&components(value)));
        assert_eq!(visual.light_dampening, expected);
    }
}

#[test]
fn visual_components_and_permutations_are_retained() {
    let mut nbt = named(10, "");
    nbt.extend(named(10, "components"));
    nbt.extend(named(10, "minecraft:geometry"));
    nbt.extend(string_field("identifier", "geometry.ore"));
    nbt.push(0);
    nbt.extend(named(10, "minecraft:material_instances"));
    nbt.extend(named(10, "materials"));
    nbt.extend(named(10, "*"));
    nbt.extend(string_field("texture", "ore_top"));
    nbt.extend([0, 0, 0]);
    nbt.push(0);
    nbt.extend(named(9, "permutations"));
    nbt.extend([10, 2]);
    nbt.extend(string_field("condition", "q.block_state('x') == 'y'"));
    nbt.extend(named(10, "components"));
    nbt.extend(named(10, "minecraft:transformation"));
    nbt.extend(named(3, "RY"));
    nbt.push(4);
    nbt.extend(named(5, "SX"));
    nbt.extend(2.0_f32.to_le_bytes());
    nbt.extend([0, 0, 0]);
    nbt.push(0);
    let visual = parse_definition(&nbt).expect("definition").visual;
    assert_eq!(visual.base.geometry.as_deref(), Some("geometry.ore"));
    let materials = visual.base.materials.as_deref().expect("materials");
    assert_eq!(
        (materials[0].name.as_ref(), materials[0].texture.as_ref()),
        ("*", "ore_top")
    );
    let permutation = &visual.permutations[0];
    assert_eq!(permutation.condition.as_ref(), "q.block_state('x') == 'y'");
    let transform = permutation
        .components
        .transformation
        .expect("transformation");
    assert_eq!(
        transform.rotation,
        [0, 2, 0],
        "zigzag 4 is two quarter turns"
    );
    assert_eq!(transform.scale, [2.0, 1.0, 1.0]);
}

// Origin is bottom-centre in sixteenths; a full 16-cube maps to the unit block.
#[test]
fn collision_box_maps_sixteenths_to_block_units() {
    let list = |values: [f64; 3]| Nbt::List(values.map(Nbt::Float).into());
    let boxed = |origin, size| {
        Nbt::Compound(vec![
            ("origin".to_owned(), list(origin)),
            ("size".to_owned(), list(size)),
        ])
    };
    let full = super::box_component(&boxed([-8.0, 0.0, -8.0], [16.0, 16.0, 16.0])).unwrap();
    assert_eq!((full.min, full.max), ([0.0; 3], [1.0; 3]));
    let slab = super::box_component(&boxed([-8.0, 0.0, -8.0], [16.0, 8.0, 16.0])).unwrap();
    assert_eq!(slab.max, [1.0, 0.5, 1.0]);
    assert!(super::box_component(&boxed([0.0; 3], [0.0; 3])).is_none());
}

#[test]
fn review_custom_box_rejects_nonfinite_narrowed_and_computed_coordinates() {
    use super::box_component;
    let boxed = |origin: [f64; 3], size: [f64; 3]| {
        Nbt::Compound(vec![
            ("origin".into(), Nbt::List(origin.map(Nbt::Float).into())),
            ("size".into(), Nbt::List(size.map(Nbt::Float).into())),
        ])
    };
    assert!(box_component(&boxed([-1e100, 0.0, 0.0], [1e100, 16.0, 16.0])).is_none());
    assert!(box_component(&boxed([3e38, 0.0, 0.0], [3e38, 16.0, 16.0])).is_none());
}

// A disabled selection box makes the block untargetable; a box overrides the default.
#[test]
fn selection_box_component_is_parsed() {
    let selection = |body: Vec<u8>| {
        let mut nbt = named(10, "");
        nbt.extend(named(10, "components"));
        nbt.extend(named(10, "minecraft:selection_box"));
        nbt.extend(body);
        nbt.extend([0, 0, 0]);
        parse_definition(&nbt).expect("definition").selection
    };
    assert_eq!(
        selection(named(1, "enabled").into_iter().chain([0]).collect()),
        CustomSelection::Disabled
    );
    assert_eq!(selection(Vec::new()), CustomSelection::Default);
}

#[test]
fn truncated_definition_is_rejected() {
    assert!(parse_definition(&[10, 0, 9]).is_none());
}

// Vanilla definitions admit base states; custom definitions also need overlay visuals.
#[test]
fn only_vanilla_namespace_definitions_are_not_server_blocks() {
    let definition = |block_id: &[u8]| {
        let mut nbt = named(10, "");
        nbt.extend(named(10, "vanilla_block_data"));
        nbt.extend(named(3, "block_id"));
        nbt.extend_from_slice(block_id);
        nbt.extend([0, 0]);
        nbt
    };
    // Zigzag varints of 1464 and 10000.
    let vanilla = definition(&[0xf0, 0x16]);
    let server = definition(&[0xa0, 0x9c, 0x01]);
    let blocks = super::CustomBlocks::from_definitions([
        ("minecraft:light_gray_concrete_stairs", vanilla.as_slice()),
        ("benergistics:controller", server.as_slice()),
    ]);
    let names = blocks
        .blocks
        .iter()
        .map(|block| block.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        (names, blocks.skipped),
        (vec!["benergistics:controller"], 0)
    );
    assert_eq!(
        blocks.vanilla_blocks.as_ref(),
        &[std::sync::Arc::<str>::from(
            "minecraft:light_gray_concrete_stairs"
        )]
    );
}

#[test]
fn sort_key_is_fnv1_64_of_the_name() {
    assert_eq!(block_name_sort_key(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(block_name_sort_key("a"), 0xaf63_bd4c_8601_b7be);
}
