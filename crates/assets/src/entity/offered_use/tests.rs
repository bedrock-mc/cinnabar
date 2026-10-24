use super::*;

/// Holds one main-hand item with an empty offhand.
fn main_hand(identifier: &str, metadata: u32) -> HeldItems<'_> {
    HeldItems {
        main_hand: HandItem::Item {
            identifier,
            metadata,
        },
        offhand: HandItem::Empty,
        equipment_enchantments: Some(0),
    }
}

/// Holds an unreadable main-hand item with an empty offhand.
fn unknown_main_hand() -> HeldItems<'static> {
    HeldItems {
        main_hand: HandItem::Unknown,
        offhand: HandItem::Empty,
        equipment_enchantments: Some(0),
    }
}

/// Creates an original behavior descriptor without assigning component-group activation.
fn descriptor(condition: UseCondition, selector: UseSelector) -> EntityUseAdmission {
    EntityUseAdmission::Components {
        source: 0,
        offerings: Box::from([ComponentUse {
            selector,
            component: "fixture:offering".into(),
            condition,
        }]),
    }
}

#[test]
fn an_excluded_item_rules_out_an_unresolved_group_without_activating_it() {
    let definition = descriptor(
        UseCondition::HeldItem {
            identifier: "fixture:food".into(),
            metadata: None,
        },
        UseSelector::UnresolvedComponentGroup { group: 0 },
    );
    assert_eq!(
        definition.for_held(&main_hand("fixture:projectile", 0)),
        OfferedUse::Absent
    );
    assert_eq!(
        definition.for_held(&main_hand("fixture:food", 0)),
        OfferedUse::Unresolved
    );
    assert_eq!(
        definition.for_held(&unknown_main_hand()),
        OfferedUse::Unresolved
    );
    assert_eq!(
        EntityUseAdmission::Unresolved.for_held(&main_hand("fixture:projectile", 0)),
        OfferedUse::Unresolved
    );
}

#[test]
fn unknown_filters_preserve_possible_offerings_but_cannot_override_a_false_conjunct() {
    let held = UseCondition::HeldItem {
        identifier: "fixture:feed".into(),
        metadata: Some(2),
    };
    let definition = descriptor(
        UseCondition::All(vec![UseCondition::Unresolved, held].into()),
        UseSelector::Base,
    );
    assert_eq!(
        definition.for_held(&main_hand("fixture:feed", 2)),
        OfferedUse::Unresolved
    );
    assert_eq!(
        definition.for_held(&main_hand("fixture:feed", 0)),
        OfferedUse::Absent
    );
    let alternatives = descriptor(
        UseCondition::Any(vec![UseCondition::Never, UseCondition::Unresolved].into()),
        UseSelector::Base,
    );
    assert_eq!(
        alternatives.for_held(&main_hand("fixture:projectile", 0)),
        OfferedUse::Unresolved
    );
}

#[test]
fn either_item_auxiliary_can_be_a_wildcard_without_changing_the_item_identity() {
    let wildcard = u32::from(protocol_ids::ITEM_AUXILIARY_WILDCARD);
    for (expected, held) in [(Some(2), wildcard), (Some(wildcard), 2)] {
        let definition = descriptor(
            UseCondition::HeldItem {
                identifier: "minecraft:snowball".into(),
                metadata: expected,
            },
            UseSelector::Base,
        );
        assert_eq!(
            definition.for_held(&main_hand("minecraft:snowball", held)),
            OfferedUse::Unresolved
        );
        assert_eq!(
            definition.for_held(&main_hand("minecraft:egg", held)),
            OfferedUse::Absent
        );
    }
    let condition = UseCondition::HeldItem {
        identifier: "minecraft:snowball".into(),
        metadata: Some(2),
    };
    assert_eq!(
        condition.permits(&main_hand("minecraft:snowball", u32::MAX)),
        None
    );
    assert_eq!(
        condition.permits(&main_hand("minecraft:egg", u32::MAX)),
        Some(false)
    );
}

#[test]
fn offered_use_carrier_retains_conditions_and_rejects_a_resource_source() {
    let mut compiled =
        crate::carriers::startup::test_carriers::synthetic_interaction_assets("minecraft:zombie");
    let mut sources = compiled.sources.into_vec();
    sources.insert(
        0,
        super::super::EntityAssetSource {
            path: "behavior/entities/fixture.json".into(),
            source_bytes: 1,
            source_sha256: [3; 32],
        },
    );
    compiled.symbols[0].source_index += 1;
    compiled.sources = sources.into();
    compiled
        .interaction_traits
        .iter_mut()
        .find(|entry| entry.identifier.as_ref() == "minecraft:zombie")
        .unwrap()
        .offered_use = EntityUseAdmission::Components {
        source: 0,
        offerings: Box::from([ComponentUse {
            selector: UseSelector::UnresolvedComponentGroup { group: 0 },
            component: "minecraft:rideable".into(),
            condition: UseCondition::Never,
        }]),
    };
    let runtime =
        super::super::RuntimeEntityAssets::decode(&crate::encode_entity_blob(&compiled).unwrap())
            .unwrap();
    assert_eq!(
        runtime
            .offered_use_for("minecraft:zombie")
            .unwrap()
            .for_held(&main_hand("fixture:projectile", 0)),
        OfferedUse::Absent
    );
    let EntityUseAdmission::Components { source, .. } = &mut compiled
        .interaction_traits
        .iter_mut()
        .find(|entry| entry.identifier.as_ref() == "minecraft:zombie")
        .unwrap()
        .offered_use
    else {
        unreachable!()
    };
    *source = 1;
    assert!(crate::encode_entity_blob(&compiled).is_err());
}

#[test]
fn either_hand_conditions_read_the_offhand_and_stay_open_while_it_is_unobserved() {
    let condition = UseCondition::EitherHandItem {
        identifier: "minecraft:flint_and_steel".into(),
        metadata: None,
    };
    let bow = HandItem::Item {
        identifier: "minecraft:bow",
        metadata: 0,
    };
    let lighter = HandItem::Item {
        identifier: "minecraft:flint_and_steel",
        metadata: 0,
    };
    for (offhand, expected) in [
        (HandItem::Empty, Some(false)),
        (lighter, Some(true)),
        (HandItem::Unknown, None),
    ] {
        let held = HeldItems {
            main_hand: bow,
            offhand,
            equipment_enchantments: Some(0),
        };
        assert_eq!(condition.permits(&held), expected, "{offhand:?}");
    }
    let held = HeldItems {
        main_hand: lighter,
        offhand: HandItem::Unknown,
        equipment_enchantments: Some(0),
    };
    assert_eq!(condition.permits(&held), Some(true));
}

#[test]
fn only_spawn_eggs_satisfy_an_actor_placer_condition() {
    let condition = UseCondition::HeldActorPlacer;
    assert_eq!(
        condition.permits(&main_hand("minecraft:cow_spawn_egg", 0)),
        Some(true)
    );
    assert_eq!(
        condition.permits(&main_hand("minecraft:bow", 0)),
        Some(false)
    );
    assert_eq!(condition.permits(&unknown_main_hand()), None);
}

#[test]
fn equipment_enchantment_conditions_read_the_whole_equipment_mask() {
    let fire_aspect = enchantment_id("fire_aspect").unwrap();
    assert_eq!(
        fire_aspect,
        protocol_ids::Enchantment::Fireaspect.wire_value()
    );
    assert_eq!(enchantment_id("minecraft:fire_aspect"), Some(fire_aspect));
    assert_eq!(enchantment_id("fixture:missing"), None);
    let condition = UseCondition::EquipmentEnchantment {
        enchantment: fire_aspect,
    };
    let mut held = main_hand("minecraft:bow", 0);
    assert_eq!(condition.permits(&held), Some(false));
    held.equipment_enchantments = Some(1 << fire_aspect);
    assert_eq!(condition.permits(&held), Some(true));
    held.equipment_enchantments = None;
    assert_eq!(condition.permits(&held), None);
}
