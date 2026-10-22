use super::*;

const SHARPNESS: (i16, i16) = (9, 3);
const VANISHING: (i16, i16) = (28, 1);
const BINDING: (i16, i16) = (27, 1);

/// The session identity of a vanilla item.
fn kind(identifier: &str) -> Option<crate::SessionItemKind> {
    let pinned = assets::facts::ItemFacts::builtin().unwrap();
    static SESSION: std::sync::OnceLock<crate::SessionItemFacts> = std::sync::OnceLock::new();
    let session = SESSION.get_or_init(|| {
        crate::SessionItemFacts::from_registry(
            pinned,
            &protocol::ItemRegistryEvent {
                entries: protocol::vanilla_item_registry(),
            },
        )
        .unwrap()
    });
    session.kind(pinned.network_id(identifier).unwrap())
}

/// A diamond sword with `enchantments`, `damage` and prior-work `repair_cost`.
fn sword<'a>(enchantments: &'a [(i16, i16)], damage: u32, repair_cost: i32) -> GrindstoneItem<'a> {
    GrindstoneItem {
        kind: kind("minecraft:diamond_sword"),
        count: 1,
        damage,
        max_damage: Some(1561),
        enchantments,
        repair_cost,
        has_user_data: !enchantments.is_empty() || damage > 0 || repair_cost > 0,
        ..Default::default()
    }
}

/// A lone enchanted item keeps only its curses and its penalty follows the curse count.
#[test]
fn a_lone_item_keeps_curses_and_resets_its_penalty() {
    for (enchantments, kept, cost) in [
        (&[SHARPNESS][..], &[][..], 0),
        (&[SHARPNESS, VANISHING], &[VANISHING], 1),
        (&[SHARPNESS, VANISHING, BINDING], &[VANISHING, BINDING], 3),
    ] {
        for top in [true, false] {
            let item = sword(enchantments, 10, 7);
            let (a, b) = if top {
                (Some(&item), None)
            } else {
                (None, Some(&item))
            };
            let outcome = evaluate_grindstone(a, b).unwrap();
            assert_eq!(outcome.source, usize::from(!top));
            assert_eq!(outcome.enchantments, kept);
            assert_eq!(outcome.repair_cost, cost);
            assert_eq!(outcome.damage, None);
        }
    }
}

/// A lone item that would come out unchanged, or trimmed armor, makes nothing.
#[test]
fn unchanged_or_trimmed_items_make_nothing() {
    assert_eq!(evaluate_grindstone(Some(&sword(&[], 10, 0)), None), None);
    assert_eq!(
        evaluate_grindstone(Some(&sword(&[VANISHING], 0, 1)), None),
        None
    );
    assert!(evaluate_grindstone(Some(&sword(&[], 0, 4)), None).is_some());
    let trimmed = GrindstoneItem {
        trimmed: true,
        ..sword(&[SHARPNESS], 0, 0)
    };
    assert_eq!(evaluate_grindstone(Some(&trimmed), None), None);
}

/// Two copies merge their remaining durability plus a twentieth of the maximum.
#[test]
fn two_copies_merge_durability() {
    let worn = sword(&[SHARPNESS], 1000, 3);
    let plain = GrindstoneItem {
        has_user_data: false,
        ..sword(&[], 900, 0)
    };
    let outcome = evaluate_grindstone(Some(&plain), Some(&worn)).unwrap();
    assert_eq!(outcome.damage, Some(1000 + 900 - 1561 - 78));
    assert_eq!(outcome.source, 1, "the copy with user data is the base");
    assert!(outcome.enchantments.is_empty());
    let fresh = evaluate_grindstone(Some(&sword(&[], 10, 0)), Some(&sword(&[], 20, 0))).unwrap();
    assert_eq!(fresh.damage, Some(0));
}

/// An identical curse on both copies counts once, so doubly cursed pairs still make a result.
#[test]
fn identical_curses_on_both_copies_count_once() {
    let outcome = evaluate_grindstone(
        Some(&sword(&[VANISHING], 10, 0)),
        Some(&sword(&[VANISHING], 10, 0)),
    )
    .unwrap();
    assert_eq!(outcome.enchantments, [VANISHING]);
    assert_eq!(outcome.repair_cost, 1);
    let both = [VANISHING, BINDING];
    let outcome =
        evaluate_grindstone(Some(&sword(&both, 10, 0)), Some(&sword(&both, 10, 0))).unwrap();
    assert_eq!(outcome.enchantments, both);
    assert_eq!(outcome.repair_cost, 3);
}

/// Different items, stacks and non-damageable pairs make nothing.
#[test]
fn mismatched_pairs_make_nothing() {
    let other = GrindstoneItem {
        kind: kind("minecraft:iron_sword"),
        ..sword(&[], 10, 0)
    };
    assert_eq!(
        evaluate_grindstone(Some(&sword(&[], 10, 0)), Some(&other)),
        None
    );
    let stack = GrindstoneItem {
        count: 2,
        ..sword(&[SHARPNESS], 0, 0)
    };
    assert_eq!(evaluate_grindstone(Some(&stack), None), None);
    let book = GrindstoneItem {
        max_damage: None,
        ..sword(&[SHARPNESS], 0, 0)
    };
    assert_eq!(evaluate_grindstone(Some(&book), Some(&book)), None);
}

/// A disenchanted book becomes a plain book unless a curse remains.
#[test]
fn enchanted_books_lose_their_kind_without_curses() {
    for (enchantments, plain) in [(&[SHARPNESS][..], true), (&[SHARPNESS, BINDING], false)] {
        let book = GrindstoneItem {
            enchanted_book: true,
            max_damage: None,
            ..sword(enchantments, 0, 0)
        };
        assert_eq!(
            evaluate_grindstone(Some(&book), None).unwrap().plain_book,
            plain
        );
    }
}
