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
