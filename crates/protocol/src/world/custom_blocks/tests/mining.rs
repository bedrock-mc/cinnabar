use super::*;

/// Supplies a disabled mining override without a live server fixture.
fn mining_components() -> Nbt {
    compound(vec![(
        "minecraft:destructible_by_mining",
        compound(vec![("value", Nbt::Float(-1.0))]),
    )])
}

#[test]
fn custom_mining_unprepared_permutations_do_not_use_the_base_value() {
    let root = compound(vec![(
        "permutations",
        Nbt::List(vec![compound(vec![
            ("condition", text("return 1;")),
            ("components", mining_components()),
        ])]),
    )]);
    let block = custom_block("test:unprepared_mining", &root);
    assert!(block.mining_for_state(0).is_none());
}

#[test]
fn custom_mining_unrelated_discarded_permutations_keep_the_base_value() {
    let permutations = std::iter::repeat_with(|| compound(vec![("condition", text("1"))]))
        .take(super::super::MAX_PERMUTATIONS + 1)
        .collect();
    let block = custom_block(
        "test:truncated_visuals",
        &compound(vec![("permutations", Nbt::List(permutations))]),
    );
    assert!(block.visual.components_incomplete);
    assert_eq!(block.mining_for_state(0).unwrap().seconds_to_destroy, 0.0);
}

// A malformed item entry or a permutation without a condition must not leave the block unminable.
#[test]
fn custom_mining_odd_entries_and_conditionless_permutations_keep_the_block_minable() {
    let speeds = compound(vec![
        ("value", Nbt::Float(2.0)),
        (
            "item_specific_speeds",
            Nbt::List(vec![
                compound(vec![("destroy_speed", Nbt::Float(0.5))]),
                text("test:not_a_compound"),
            ]),
        ),
    ]);
    for root in [
        compound(vec![(
            "components",
            compound(vec![("minecraft:destructible_by_mining", speeds)]),
        )]),
        compound(vec![
            (
                "components",
                compound(vec![(
                    "minecraft:destructible_by_mining",
                    compound(vec![("value", Nbt::Float(2.0))]),
                )]),
            ),
            (
                "permutations",
                Nbt::List(vec![compound(vec![("components", mining_components())])]),
            ),
        ]),
    ] {
        let block = custom_block("test:odd_mining", &root);
        assert!(!block.visual.components_incomplete);
        assert_eq!(block.mining_for_state(0).unwrap().seconds_to_destroy, 2.0);
    }
}

#[test]
fn custom_mining_nonfinite_values_never_complete() {
    let root = compound(vec![(
        "components",
        compound(vec![(
            "minecraft:destructible_by_mining",
            compound(vec![("value", Nbt::Float(f64::NAN))]),
        )]),
    )]);
    let block = custom_block("test:nonfinite_mining", &root);
    assert!(block.mining_for_state(0).unwrap().seconds_to_destroy < 0.0);
}

#[test]
fn custom_mining_cache_overrides_the_base_value() {
    let mut block = custom_block("test:cached_mining", &compound(vec![]));
    block.state_mining = [std::sync::Arc::new(super::super::CustomBlockMining {
        seconds_to_destroy: 2.0,
        ..Default::default()
    })]
    .into();
    assert_eq!(block.mining_for_state(0).unwrap().seconds_to_destroy, 2.0);
}
