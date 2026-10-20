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
    assert!(block.physics_for_state(0).mining.is_none());
}

#[test]
fn custom_mining_discarded_permutations_cannot_use_an_instant_base() {
    let mut permutations = std::iter::repeat_with(|| compound(vec![("condition", text("1"))]))
        .take(super::super::MAX_PERMUTATIONS)
        .collect::<Vec<_>>();
    permutations.push(compound(vec![
        ("condition", text("1")),
        ("components", mining_components()),
    ]));
    let root = compound(vec![("permutations", Nbt::List(permutations))]);
    let mut block = custom_block("test:truncated_mining", &root);
    assert!(!block.visual.components_incomplete);
    assert!(block.visual.mining_incomplete);
    assert!(block.mining_for_state(0).is_none());
    assert!(block.physics_for_state(0).mining.is_none());
    let mut cached = block.base_physics();
    cached.mining = Some(std::sync::Arc::clone(&block.mining));
    block.state_physics = [cached].into();
    assert!(block.mining_for_state(0).is_none());
    assert!(block.physics_for_state(0).mining.is_none());
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

#[test]
fn custom_mining_discarded_condition_and_invalid_values_stay_unresolved() {
    for root in [
        compound(vec![(
            "permutations",
            Nbt::List(vec![compound(vec![("components", mining_components())])]),
        )]),
        compound(vec![(
            "components",
            compound(vec![(
                "minecraft:destructible_by_mining",
                Nbt::Float(f64::NAN),
            )]),
        )]),
        compound(vec![(
            "permutations",
            Nbt::List(vec![compound(vec![
                ("condition", text("1")),
                (
                    "components",
                    compound(vec![(
                        "minecraft:destructible_by_mining",
                        Nbt::Float(f64::NAN),
                    )]),
                ),
            ])]),
        )]),
    ] {
        let block = custom_block("test:incomplete_mining", &root);
        assert!(!block.visual.components_incomplete);
        assert!(block.mining_for_state(0).is_none());
        assert!(block.base_physics().mining.is_none());
    }
}
