use super::super::{Nbt, parse_definition};

/// Builds an original wire component fixture without a game pack.
fn compound<const N: usize>(fields: [(&str, Nbt); N]) -> Nbt {
    Nbt::Compound(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

#[test]
fn custom_placing_handlers_and_legacy_triggers_disable_prediction() {
    for placing in [0, 1] {
        let components = compound([(
            "minecraft:custom_components",
            compound([
                ("hasPlayerPlacing", Nbt::Byte(placing)),
                ("hasPlayerInteract", Nbt::Byte(1)),
            ]),
        )]);
        let definition = parse_definition(&compound([("components", components)])).unwrap();
        assert_eq!(definition.visual.placement.server_placing, placing != 0);
        assert!(definition.player_interact);
    }
    let definition = parse_definition(&compound([(
        "components",
        compound([("minecraft:on_player_placing", compound([]))]),
    )]))
    .unwrap();
    assert!(definition.visual.placement.server_placing);
}

#[test]
fn permutation_placing_overrides_apply_to_the_block_type() {
    let definition = parse_definition(&compound([(
        "permutations",
        Nbt::List(vec![compound([
            ("condition", Nbt::String("q.block_state('test:on')".into())),
            (
                "components",
                compound([(
                    "minecraft:custom_components",
                    compound([("hasPlayerPlacing", Nbt::Byte(1))]),
                )]),
            ),
        ])]),
    )]))
    .unwrap();
    assert!(definition.visual.placement.server_placing);
}

#[test]
fn omitted_permutations_and_physical_components_are_marked_incomplete() {
    for permutations in [
        (0..=super::super::MAX_PERMUTATIONS)
            .map(|_| compound([("condition", Nbt::String("1".into()))]))
            .collect(),
        vec![compound([])],
        vec![compound([
            ("condition", Nbt::String("1".into())),
            (
                "components",
                compound([(
                    "minecraft:collision_box",
                    compound([("boxes", Nbt::Byte(0))]),
                )]),
            ),
        ])],
    ] {
        let definition =
            parse_definition(&compound([("permutations", Nbt::List(permutations))])).unwrap();
        assert!(definition.visual.components_incomplete);
        assert!(definition.skipped > 0);
    }
    for component in ["minecraft:collision_box", "minecraft:transformation"] {
        let value = if component == "minecraft:collision_box" {
            compound([("boxes", Nbt::Byte(0))])
        } else {
            compound([("SX", Nbt::Float(f64::INFINITY))])
        };
        let definition =
            parse_definition(&compound([("components", compound([(component, value)]))])).unwrap();
        assert!(definition.visual.components_incomplete);
    }
    let complete = parse_definition(&compound([])).unwrap();
    assert!(!complete.visual.components_incomplete);
}

#[test]
fn placement_traits_retain_offset_and_exclude_authored_properties() {
    for (offset, expected) in [(0.0, 0.0), (89.0, 90.0), (-91.0, 270.0), (0.1, 0.1)] {
        let trait_ = compound([
            (
                "enabled_states",
                Nbt::List(vec![Nbt::String("minecraft:cardinal_direction".into())]),
            ),
            ("y_rotation_offset", Nbt::Float(offset)),
        ]);
        let definition = parse_definition(&compound([
            (
                "traits",
                Nbt::Compound(vec![("minecraft:placement_direction".into(), trait_)]),
            ),
            (
                "properties",
                Nbt::List(vec![compound([
                    ("name", Nbt::String("test:on".into())),
                    ("enum", Nbt::List(vec![Nbt::Byte(0), Nbt::Byte(1)])),
                ])]),
            ),
        ]))
        .unwrap();
        assert_eq!(definition.visual.placement.trait_state_count, 1);
        assert_eq!(definition.visual.placement.y_rotation_offset, expected);
        assert_eq!(definition.state_count, 8);
    }
}

#[test]
fn placement_traits_compose_named_states_with_connection_flags() {
    for compound_traits in [false, true] {
        for connection_states in [
            Nbt::Int(1),
            Nbt::List(vec![Nbt::String("minecraft:cardinal_connections".into())]),
            compound([("minecraft:cardinal_connections", Nbt::Byte(1))]),
        ] {
            let connection = if compound_traits {
                compound([("enabled_states", connection_states)])
            } else {
                compound([
                    ("name", Nbt::String("minecraft:connection".into())),
                    ("enabled_states", connection_states),
                ])
            };
            let traits = [
                (
                    "minecraft:placement_direction",
                    compound([
                        (
                            "enabled_states",
                            Nbt::List(vec![Nbt::String("minecraft:cardinal_direction".into())]),
                        ),
                        ("y_rotation_offset", Nbt::Float(90.0)),
                    ]),
                ),
                ("minecraft:connection", connection),
            ];
            let traits = if compound_traits {
                compound(traits)
            } else {
                Nbt::List(traits.into_iter().map(|(_, value)| value).collect())
            };
            let definition = parse_definition(&compound([
                ("traits", traits),
                (
                    "properties",
                    Nbt::List(vec![compound([
                        ("name", Nbt::String("test:on".into())),
                        ("enum", Nbt::List(vec![Nbt::Byte(0), Nbt::Byte(1)])),
                    ])]),
                ),
            ]))
            .unwrap();
            assert_eq!(definition.state_count, 4 * 16 * 2);
            assert!(!definition.visual.state_identity_incomplete);
            assert_eq!(definition.visual.placement.trait_state_count, 5);
            assert_eq!(definition.visual.placement.y_rotation_offset, 90.0);
            let axes = &definition.visual.state_axes;
            assert_eq!(axes.len(), 6);
            assert_eq!(axes[0].name.as_ref(), "minecraft:cardinal_direction");
            assert_eq!(axes[5].name.as_ref(), "test:on");
        }
    }
}
