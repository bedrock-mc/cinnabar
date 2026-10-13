use super::*;

/// An undamaged-or-`damage` diamond sword carrying `enchantments`.
fn sword<'a>(damage: u32, enchantments: &'a [(i16, i16)]) -> AnvilItem<'a> {
    AnvilItem {
        identifier: "minecraft:diamond_sword",
        count: 1,
        damage,
        max_damage: Some(1561),
        enchantments,
        ..AnvilItem::default()
    }
}

/// An enchanted book carrying `enchantments`.
fn book(enchantments: &[(i16, i16)]) -> AnvilItem<'_> {
    AnvilItem {
        identifier: "minecraft:enchanted_book",
        count: 1,
        enchantments,
        ..AnvilItem::default()
    }
}

/// Evaluates in survival with the name box holding the sword's own name.
fn evaluate(left: &AnvilItem<'_>, right: Option<&AnvilItem<'_>>) -> Option<AnvilOutcome> {
    evaluate_anvil(left, right, "Diamond Sword", "Diamond Sword", false)
}

#[test]
fn materials_repair_a_quarter_each_and_cost_a_level_each() {
    let diamonds = AnvilItem {
        identifier: "minecraft:diamond",
        count: 10,
        ..AnvilItem::default()
    };
    let outcome = evaluate(&sword(1000, &[]), Some(&diamonds)).unwrap();
    assert_eq!(outcome.material_used, 3, "390 + 390 + 220 repairs 1000");
    assert_eq!(outcome.damage, Some(0));
    assert_eq!(outcome.cost, 3);
    assert_eq!(outcome.repair_cost, 1);
    assert_eq!(
        evaluate(&sword(0, &[]), Some(&diamonds)),
        None,
        "nothing to repair"
    );
    for creative in [false, true] {
        assert_eq!(
            evaluate_anvil(
                &sword(0, &[]),
                Some(&diamonds),
                "Blade",
                "Diamond Sword",
                creative
            ),
            None,
            "a repair material that changes nothing also blocks renaming"
        );
        let renamed = evaluate_anvil(&sword(0, &[]), None, "Blade", "Diamond Sword", creative)
            .expect("renaming alone needs no repair work");
        assert_eq!(renamed.custom_name, Some(Some("Blade".into())));
        assert_eq!(renamed.damage, None);
        assert_eq!(renamed.material_used, 0);
        assert_eq!(renamed.cost, 1);
    }
}

#[test]
fn books_merge_equal_levels_upward_and_cost_by_book_multiplier() {
    let outcome = evaluate(&sword(0, &[(9, 4)]), Some(&book(&[(9, 4)]))).unwrap();
    assert_eq!(outcome.enchantments, Some(vec![(9, 5)]));
    assert_eq!(outcome.cost, 1, "one sharpness level at book multiplier 1");
    assert_eq!(outcome.damage, None);
    let chestplate = AnvilItem {
        identifier: "minecraft:diamond_chestplate",
        max_damage: Some(528),
        ..sword(0, &[])
    };
    let outcome = evaluate(&chestplate, Some(&book(&[(5, 2)]))).unwrap();
    assert_eq!(outcome.cost, 8, "thorns II at book multiplier 4");
    let trident = AnvilItem {
        identifier: "minecraft:trident",
        max_damage: Some(250),
        ..sword(0, &[])
    };
    let outcome = evaluate(&trident, Some(&book(&[(29, 3)]))).unwrap();
    assert_eq!(outcome.cost, 3, "impaling is uncommon: book multiplier 1");
    assert_eq!(
        evaluate(&sword(0, &[(9, 5)]), Some(&book(&[(9, 5)]))),
        None,
        "an enchantment at its cap takes nothing more"
    );
}

#[test]
fn a_book_that_adds_nothing_blocks_renaming_in_every_game_mode() {
    let protection = book(&[(0, 2)]);
    for creative in [false, true] {
        assert_eq!(
            evaluate_anvil(
                &sword(0, &[]),
                Some(&protection),
                "Blade",
                "Diamond Sword",
                creative
            ),
            None
        );
    }
}

#[test]
fn incompatible_or_inapplicable_enchantments_are_refused() {
    assert_eq!(
        evaluate(&sword(0, &[(9, 1)]), Some(&book(&[(10, 1)]))),
        None
    );
    assert_eq!(evaluate(&sword(0, &[]), Some(&book(&[(2, 1)]))), None);
}

#[test]
fn combining_two_damaged_copies_adds_durability_and_a_bonus() {
    let outcome = evaluate(&sword(1000, &[]), Some(&sword(1000, &[]))).unwrap();
    // 561 + 561 + 187 remaining.
    assert_eq!(outcome.damage, Some(252));
    assert_eq!(outcome.cost, 2);
    assert_eq!(outcome.material_used, 1);
    let stick = AnvilItem {
        identifier: "minecraft:stick",
        count: 1,
        ..AnvilItem::default()
    };
    assert_eq!(evaluate(&sword(1000, &[]), Some(&stick)), None);
}

#[test]
fn renaming_costs_a_level_and_alone_keeps_the_penalty() {
    let worked = AnvilItem {
        repair_cost: 3,
        custom_name: Some("Old"),
        ..sword(0, &[])
    };
    let outcome = evaluate_anvil(&worked, None, "New", "Old", false).unwrap();
    assert_eq!(outcome.custom_name, Some(Some("New".into())));
    assert_eq!(outcome.cost, 4);
    assert_eq!(outcome.repair_cost, 3, "a rename alone does not double it");
    assert_eq!(evaluate_anvil(&worked, None, "Old", "Old", false), None);
    let cleared = evaluate_anvil(&worked, None, "", "Old", false).unwrap();
    assert_eq!(cleared.custom_name, Some(None));
    let renamed_merge =
        evaluate_anvil(&worked, Some(&book(&[(17, 1)])), "New", "Old", false).unwrap();
    assert_eq!(
        renamed_merge.repair_cost, 7,
        "real work doubles it plus one"
    );
}

#[test]
fn survival_refuses_forty_levels_but_creative_does_not() {
    let worked = AnvilItem {
        repair_cost: 63,
        ..sword(0, &[])
    };
    let unbreaking = book(&[(17, 3)]);
    let survival = evaluate_anvil(&worked, Some(&unbreaking), "", "", false).unwrap();
    assert!(survival.too_expensive);
    let creative = evaluate_anvil(&worked, Some(&unbreaking), "", "", true).unwrap();
    assert!(!creative.too_expensive);
    let rename = evaluate_anvil(&worked, None, "x", "", false).unwrap();
    assert_eq!(rename.cost, 39, "renaming alone stays affordable");
    assert!(!rename.too_expensive);
}

#[test]
fn maximum_prior_work_is_saturated_before_the_rename_cap() {
    let worked = AnvilItem {
        repair_cost: i32::MAX,
        custom_name: Some("Old"),
        ..sword(0, &[])
    };
    let renamed = evaluate_anvil(&worked, None, "New", "Old", false).unwrap();
    assert_eq!(renamed.cost, ANVIL_TOO_EXPENSIVE - 1);
    assert_eq!(renamed.repair_cost, i32::MAX);
    assert!(!renamed.too_expensive);

    for right_penalty in [0, i32::MAX] {
        let right = AnvilItem {
            repair_cost: right_penalty,
            ..book(&[(17, 1)])
        };
        let merged = evaluate_anvil(&worked, Some(&right), "Old", "Old", false).unwrap();
        assert_eq!(merged.cost, i32::MAX);
        assert_eq!(merged.repair_cost, i32::MAX);
        assert!(merged.too_expensive);
        let creative = evaluate_anvil(&worked, Some(&right), "Old", "Old", true).unwrap();
        assert_eq!(creative.cost, i32::MAX);
        assert!(!creative.too_expensive);
    }
}

#[test]
fn merging_keeps_existing_enchantment_levels_outside_the_tooltip_range() {
    let outcome = evaluate(&sword(0, &[(9, 300), (100, -1)]), Some(&book(&[(17, 1)]))).unwrap();
    assert_eq!(
        outcome.enchantments,
        Some(vec![(9, 300), (100, -1), (17, 1)])
    );
    assert_eq!(outcome.cost, 1);
}

#[test]
fn combining_large_durability_values_does_not_wrap() {
    let left = AnvilItem {
        max_damage: Some(u32::MAX),
        ..sword(1, &[])
    };
    let outcome = evaluate(&left, Some(&left)).unwrap();
    assert_eq!(outcome.damage, Some(0));
    assert_eq!(outcome.cost, 2);
}

#[test]
fn large_existing_enchantment_lists_do_not_overflow_cost_sums() {
    let levels = vec![(17, i16::MAX); 65_540];
    let outcome = evaluate(&sword(0, &levels), Some(&book(&[(9, 1)]))).unwrap();
    assert_eq!(outcome.cost, 1);
    assert_eq!(outcome.enchantments.unwrap().len(), levels.len() + 1);
}

#[test]
fn repair_materials_match_their_tier() {
    assert!(repairs_with(
        "minecraft:wooden_pickaxe",
        "minecraft:oak_planks"
    ));
    assert!(repairs_with(
        "minecraft:iron_chestplate",
        "minecraft:iron_ingot"
    ));
    assert!(repairs_with(
        "minecraft:chainmail_boots",
        "minecraft:iron_ingot"
    ));
    assert!(repairs_with(
        "minecraft:elytra",
        "minecraft:phantom_membrane"
    ));
    assert!(!repairs_with(
        "minecraft:diamond_sword",
        "minecraft:iron_ingot"
    ));
}

#[test]
fn large_lists_preserve_unknown_entries_and_merge_repeated_known_enchantments() {
    let unknown = vec![(100, 1); 40_000];
    let repeated = vec![(17, 1); 40_000];
    let outcome = evaluate(&sword(0, &unknown), Some(&book(&repeated))).unwrap();
    let merged = outcome.enchantments.unwrap();
    assert_eq!(&merged[..unknown.len()], unknown.as_slice());
    assert_eq!(&merged[unknown.len()..], &[(17, 2)]);
    assert_eq!(outcome.cost, 2);
}
