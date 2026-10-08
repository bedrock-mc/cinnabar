use super::{CustomSelection, Definition, block_name_sort_key};

fn parse_definition(bytes: &[u8]) -> Option<Definition> {
    super::parse_definition(&crate::nbt_tree::read_root(bytes)?)
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
fn singleton_unnamed_properties_do_not_form_partial_state_identities() {
    for with_named_property in [false, true] {
        let mut nbt = named(10, "");
        nbt.extend(named(9, "properties"));
        nbt.extend([10, if with_named_property { 4 } else { 2 }]);
        if with_named_property {
            nbt.extend(named(8, "name"));
            nbt.extend(string("example:variant"));
            nbt.extend(named(9, "enum"));
            nbt.extend([8, 2]);
            nbt.extend(string("only"));
            nbt.push(0);
        }
        nbt.extend(named(9, "enum"));
        nbt.extend([8, 2]);
        nbt.extend(string("unnamed"));
        nbt.extend([0, 0]);
        let blocks =
            super::CustomBlocks::from_definitions([("example:incomplete", nbt.as_slice())]);
        assert_eq!(blocks.blocks.len(), 1);
        assert_eq!(blocks.skipped, 0);
        let block = &blocks.blocks[0];
        assert_eq!(block.state_count, 1);
        assert_eq!(
            block.visual.state_axes.len(),
            usize::from(with_named_property)
        );
        assert!(block.collides);
        assert!(
            block.hashed_states().is_empty(),
            "an unnamed singleton cannot create an identity; named_neighbor={with_named_property}"
        );
    }
}

#[test]
fn unsupported_property_and_trait_states_do_not_form_partial_identities() {
    for unsupported_trait in [false, true] {
        let mut nbt = named(10, "");
        if unsupported_trait {
            nbt.extend(named(9, "traits"));
            nbt.extend([10, 2]);
            nbt.extend(named(10, "enabled_states"));
            nbt.extend(named(1, "unknown_state"));
            nbt.extend([1, 0, 0]);
        } else {
            nbt.extend(named(9, "properties"));
            nbt.extend([10, 2]);
            nbt.extend(named(8, "name"));
            nbt.extend(string("example:unsupported"));
            nbt.extend(named(9, "enum"));
            nbt.extend([10, 4, 0, 0, 0]);
        }
        nbt.push(0);
        let blocks =
            super::CustomBlocks::from_definitions([("example:incomplete", nbt.as_slice())]);
        assert_eq!(blocks.blocks.len(), 1);
        let block = &blocks.blocks[0];
        assert_eq!(block.state_count, if unsupported_trait { 1 } else { 2 });
        assert!(block.collides);
        assert!(
            block.hashed_states().is_empty(),
            "unsupported states cannot form identities; unsupported_trait={unsupported_trait}"
        );
    }
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
fn network_legacy_light_filter_retains_transparent_plant_dampening() {
    for level in [0, 15] {
        let mut nbt = named(10, "");
        nbt.extend(named(10, "components"));
        nbt.extend(named(10, "minecraft:block_light_filter"));
        nbt.extend(named(1, "lightLevel"));
        nbt.extend([level, 0, 0, 0]);
        let visual = parse_definition(&nbt).expect("network definition").visual;
        assert_eq!(visual.base.light_dampening, Some(level));
    }
}

#[test]
fn network_material_lighting_choices_remain_distinct() {
    use crate::nbt_tree::Nbt;
    let components = |ao, dimming| {
        Nbt::Compound(vec![(
            "minecraft:material_instances".into(),
            Nbt::Compound(vec![(
                "materials".into(),
                Nbt::Compound(vec![(
                    "*".into(),
                    Nbt::Compound(vec![
                        ("texture".into(), Nbt::String("test:plant".into())),
                        ("ambient_occlusion".into(), Nbt::Byte(ao)),
                        ("face_dimming".into(), Nbt::Byte(dimming)),
                    ]),
                )]),
            )]),
        )])
    };
    let choices = [(0, 0), (0, 1), (1, 0), (1, 1)]
        .map(|(ao, dimming)| super::visual_components(Some(&components(ao, dimming))).materials);
    for (index, first) in choices.iter().enumerate() {
        for second in &choices[index + 1..] {
            assert_ne!(first, second);
        }
    }
}

#[test]
fn scalar_light_components_remain_lenient_for_odd_values() {
    use crate::nbt_tree::Nbt;
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
    use crate::nbt_tree::Nbt;
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
    use super::{Nbt, box_component};
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
