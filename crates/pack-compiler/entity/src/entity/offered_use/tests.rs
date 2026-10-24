use super::*;
use assets::entity::offered_use::{HandItem, HeldItems, OfferedUse};

/// A fixed `player` family whose definition holds no components.
fn interactor() -> Interactor<'static> {
    Interactor {
        families: Some(vec!["player"]),
        components: Some(BTreeSet::new()),
    }
}

/// Holds one unenchanted main-hand item with an empty offhand.
fn main_hand(identifier: &str) -> HeldItems<'_> {
    HeldItems {
        main_hand: HandItem::Item {
            identifier,
            metadata: 0,
        },
        offhand: HandItem::Empty,
        equipment_enchantments: Some(0),
    }
}

/// Evaluates the compiler's original component fixture without a behavior-group heuristic.
fn outcome(entity: Value, held: &str) -> OfferedUse {
    held_outcome(entity, &main_hand(held))
}

/// Evaluates a fixture against both hands and the equipment enchantments.
fn held_outcome(entity: Value, held: &HeldItems<'_>) -> OfferedUse {
    EntityUseAdmission::Components {
        source: 0,
        offerings: offerings(&entity, &interactor()).into(),
    }
    .for_held(held)
}

/// Wraps one interaction filter in a component with no other possible offerings.
fn interaction(filter: Value) -> Value {
    serde_json::json!({"components": {"minecraft:interact": {"interactions": [
        {"on_interact": {"filters": filter}}
    ]}}})
}

/// Checks a callback in both base components and a possibly active component group.
fn assert_unresolved_callback(component: &str) {
    for entity in [
        serde_json::json!({"components": {component: {}}}),
        serde_json::json!({"components": {}, "component_groups": {
            "fixture:conditional": {component: {}}
        }}),
    ] {
        assert_eq!(
            outcome(entity, "minecraft:snowball"),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn leash_retrieval_cannot_be_excluded_by_the_held_item() {
    assert_unresolved_callback("minecraft:leashable");
}

#[test]
fn leash_attachment_and_retrieval_cannot_be_excluded_by_the_held_item() {
    assert_unresolved_callback("minecraft:leashable_to");
}

#[test]
fn balloon_retrieval_cannot_be_excluded_by_the_held_item() {
    assert_unresolved_callback("minecraft:balloonable");
}

#[test]
fn components_outside_the_interaction_mappings_offer_nothing() {
    for component in ["minecraft:collision_box", "fixture:behavior"] {
        for entity in [
            serde_json::json!({"components": {
                "minecraft:breedable": {"breed_items": ["wheat"]}, component: {}
            }}),
            serde_json::json!({"components": {
                "minecraft:breedable": {"breed_items": ["wheat"]}
            }, "component_groups": {"fixture:conditional": {component: {}}}}),
        ] {
            assert_eq!(outcome(entity, "minecraft:bow"), OfferedUse::Absent);
        }
    }
}

#[test]
fn native_bucket_and_leash_offerings_need_their_tools() {
    let entity = serde_json::json!({"components": {}});
    assert_eq!(outcome(entity.clone(), "minecraft:bow"), OfferedUse::Absent);
    for tool in [
        "minecraft:bucket",
        "minecraft:water_bucket",
        "minecraft:shears",
    ] {
        assert_eq!(outcome(entity.clone(), tool), OfferedUse::Unresolved);
    }
}

#[test]
fn naming_and_spawn_egg_offerings_need_their_item_roles() {
    let entity = serde_json::json!({"components": {
        "minecraft:nameable": {}, "minecraft:spawn_egg_interaction": {}
    }});
    assert_eq!(outcome(entity.clone(), "minecraft:bow"), OfferedUse::Absent);
    for item in ["minecraft:name_tag", "minecraft:cow_spawn_egg"] {
        assert_eq!(outcome(entity.clone(), item), OfferedUse::Unresolved);
    }
}

/// The retail TNT minecart's priming interactions, which only a lighter or fire aspect offers.
fn tnt_minecart() -> Value {
    let game_rule = serde_json::json!({"domain": "tntexplodes", "operator": "==", "test": "is_game_rule", "value": true});
    serde_json::json!({"components": {}, "component_groups": {"minecraft:inactive": {
        "minecraft:interact": {"interactions": [
            {"on_interact": {"filters": {
                "all_of": [{"subject": "other", "test": "is_family", "value": "player"}, game_rule],
                "any_of": [
                    {"domain": "hand", "subject": "other", "test": "has_equipment", "value": "fireball:0"},
                    {"domain": "hand", "subject": "other", "test": "has_equipment", "value": "flint_and_steel"}
                ]
            }}},
            {"on_interact": {"filters": {
                "all_of": [game_rule],
                "any_of": [{"subject": "other", "test": "has_component", "value": "fire_aspect"}]
            }}}
        ]}
    }}})
}

#[test]
fn a_bow_cannot_prime_a_tnt_minecart_while_a_lighter_or_fire_aspect_can() {
    assert_eq!(outcome(tnt_minecart(), "minecraft:bow"), OfferedUse::Absent);
    assert_eq!(
        outcome(tnt_minecart(), "minecraft:flint_and_steel"),
        OfferedUse::Unresolved
    );
    let mut held = main_hand("minecraft:bow");
    held.offhand = HandItem::Item {
        identifier: "minecraft:flint_and_steel",
        metadata: 0,
    };
    assert_eq!(held_outcome(tnt_minecart(), &held), OfferedUse::Unresolved);
    let mut held = main_hand("minecraft:bow");
    held.equipment_enchantments =
        Some(1 << assets::entity::offered_use::enchantment_id("fire_aspect").unwrap());
    assert_eq!(held_outcome(tnt_minecart(), &held), OfferedUse::Unresolved);
    held.equipment_enchantments = None;
    assert_eq!(held_outcome(tnt_minecart(), &held), OfferedUse::Unresolved);
}

#[test]
fn a_single_interaction_object_is_read_like_a_list_entry() {
    let entity = serde_json::json!({"components": {"minecraft:interact": {"interactions": {
        "on_interact": {"filters": {"test": "has_equipment", "subject": "other",
            "domain": "hand", "value": "flint_and_steel"}}
    }}}});
    assert_eq!(outcome(entity.clone(), "minecraft:bow"), OfferedUse::Absent);
    assert_eq!(
        outcome(entity, "minecraft:flint_and_steel"),
        OfferedUse::Unresolved
    );
}

#[test]
fn a_player_component_test_stays_open_when_the_player_definition_may_hold_it() {
    let filter =
        serde_json::json!({"subject": "other", "test": "has_component", "value": "fire_aspect"});
    let player = Interactor {
        families: Some(vec!["player"]),
        components: Some(BTreeSet::from(["minecraft:fire_aspect"])),
    };
    assert_eq!(super::filter(Some(&filter), &player), C::Unresolved);
    let effect =
        serde_json::json!({"subject": "other", "test": "has_component", "value": "fixture:effect"});
    assert_eq!(super::filter(Some(&effect), &interactor()), C::Unresolved);
}

#[test]
fn unsupported_filter_operators_cannot_default_to_equality() {
    for operator in [
        serde_json::json!(1),
        serde_json::json!(false),
        Value::Null,
        serde_json::json!([]),
        serde_json::json!({}),
    ] {
        let entity = interaction(serde_json::json!({
            "test": "is_family", "subject": "other", "value": "cow", "operator": operator
        }));
        assert_eq!(
            outcome(entity, "minecraft:snowball"),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn equipment_item_names_are_case_insensitive() {
    for value in ["MINECRAFT:SNOWBALL", "SNOWBALL", "SNOWBALL:0"] {
        let entity = interaction(serde_json::json!({
            "test": "has_equipment", "subject": "other", "domain": "main_hand", "value": value
        }));
        assert_eq!(
            outcome(entity, "minecraft:snowball"),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn invalid_equipment_auxiliary_values_cannot_exclude_a_matching_item() {
    for value in [
        "minecraft:snowball:256",
        "snowball:32767",
        "minecraft:snowball:4294967295",
    ] {
        let entity = interaction(serde_json::json!({
            "test": "has_equipment", "subject": "other", "domain": "main_hand", "value": value
        }));
        assert_eq!(
            outcome(entity, "minecraft:snowball"),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn mount_rejection_items_remain_possible_interactions() {
    let entity = serde_json::json!({"components": {"minecraft:tamemount": {
        "feed_items": [{"item": "minecraft:wheat"}],
        "auto_reject_items": ["minecraft:snowball"]
    }}});
    assert_eq!(
        outcome(entity, "minecraft:snowball"),
        OfferedUse::Unresolved
    );
}

#[test]
fn giveable_event_filters_do_not_exclude_the_offered_item() {
    let entity = serde_json::json!({"components": {"minecraft:giveable": {"triggers": [{
        "items": ["minecraft:snowball"], "on_give": {"filters": {
            "test": "is_family", "subject": "other", "value": "cow"
        }}
    }]}}});
    assert_eq!(
        outcome(entity.clone(), "minecraft:snowball"),
        OfferedUse::Unresolved
    );
    assert_eq!(outcome(entity, "minecraft:egg"), OfferedUse::Absent);
}

#[test]
fn supported_item_and_family_exclusions_remain_available() {
    for components in [
        serde_json::json!({"minecraft:breedable": {"breed_items": ["wheat"]}}),
        serde_json::json!({"minecraft:tamemount": {
            "feed_items": [{"item": "wheat"}], "auto_reject_items": ["apple"]
        }}),
        serde_json::json!({"minecraft:equippable": {"slots": [{"accepted_items": ["wheat"]}]}}),
        serde_json::json!({"minecraft:nameable": {}, "minecraft:dyeable": {},
            "minecraft:spawn_egg_interaction": {}, "minecraft:boostable": {}}),
    ] {
        assert_eq!(
            outcome(
                serde_json::json!({"components": components}),
                "minecraft:snowball"
            ),
            OfferedUse::Absent
        );
    }
    for operator in [None, Some("equals"), Some("==")] {
        let mut filter =
            serde_json::json!({"test": "is_family", "subject": "other", "value": "cow"});
        if let Some(operator) = operator {
            filter["operator"] = operator.into();
        }
        assert_eq!(
            outcome(interaction(filter), "minecraft:snowball"),
            OfferedUse::Absent
        );
    }
    for operator in ["not", "!=", "not_equals"] {
        let filter = serde_json::json!({"test": "is_family", "subject": "other",
            "value": "player", "operator": operator});
        assert_eq!(
            outcome(interaction(filter), "minecraft:snowball"),
            OfferedUse::Absent
        );
    }
    let condition = item(&serde_json::json!("minecraft:snowball:255"));
    let mut held = main_hand("minecraft:snowball");
    assert_eq!(condition.permits(&held), Some(false));
    held.main_hand = HandItem::Item {
        identifier: "minecraft:snowball",
        metadata: 255,
    };
    assert_eq!(condition.permits(&held), Some(true));
    held.main_hand = HandItem::Unknown;
    assert_eq!(condition.permits(&held), None);
}

#[test]
fn unsupported_item_descriptors_remain_unknown_under_negation() {
    for value in [
        "minecraft:snowball:256",
        "minecraft:snowball:invalid",
        "minecraft:tile.snowball",
    ] {
        let filter = serde_json::json!({"test": "has_equipment", "subject": "other",
            "domain": "main_hand", "value": value, "operator": "not"});
        assert_eq!(
            outcome(interaction(filter), "minecraft:snowball"),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn a_player_cannot_use_a_nonplayer_rider_family_even_in_an_unknown_group() {
    let entity = serde_json::json!({"components": {}, "component_groups": {
        "fixture:conditional": {"minecraft:rideable": {"family_types": ["baby_undead"]}}
    }});
    assert_eq!(outcome(entity, "minecraft:snowball"), OfferedUse::Absent);
}

#[test]
fn ride_trade_feed_and_equipment_candidates_keep_throwable_use_unresolved() {
    for components in [
        serde_json::json!({"minecraft:rideable": {"family_types": ["player"]}}),
        serde_json::json!({"minecraft:economy_trade_table": {}}),
        serde_json::json!({"minecraft:ageable": {"feed_items": ["minecraft:snowball"]}}),
        serde_json::json!({"minecraft:equippable": {"slots": [{"accepted_items": ["snowball"]}]}}),
    ] {
        assert_eq!(
            outcome(
                serde_json::json!({"components": components}),
                "minecraft:snowball"
            ),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn trusting_candidates_keep_throwable_use_unresolved_in_base_and_conditional_components() {
    let trusting = serde_json::json!({
        "minecraft:trusting": {"trust_items": ["minecraft:snowball"]}
    });
    for entity in [
        serde_json::json!({"components": trusting}),
        serde_json::json!({"components": {}, "component_groups": {
            "fixture:conditional": trusting
        }}),
    ] {
        assert_eq!(
            outcome(entity, "minecraft:snowball"),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn interaction_filters_preserve_unknown_state_and_exclude_incompatible_held_items() {
    let entity = serde_json::json!({"components": {"minecraft:interact": {"interactions": [{"on_interact": {
        "filters": {"all_of": [
            {"test": "has_equipment", "subject": "other", "domain": "main_hand", "value": "bucket:0"},
            {"test": "bool_property", "domain": "fixture:unadmitted", "value": true}
        ]}
    }}]}}});
    assert_eq!(
        outcome(entity.clone(), "minecraft:snowball"),
        OfferedUse::Absent
    );
    assert_eq!(outcome(entity, "minecraft:bucket"), OfferedUse::Unresolved);
}

#[test]
fn unknown_equipment_domains_and_expression_items_do_not_prove_absence() {
    let filter = serde_json::json!({"test": "has_equipment", "subject": "other", "domain": "any", "value": "fixture:held"});
    assert_eq!(super::filter(Some(&filter), &interactor()), C::Unresolved);
    assert_eq!(
        item(&serde_json::json!({"tags": "query.any_tag('fixture:feed')"})),
        C::Unresolved
    );
}

#[test]
fn conditional_player_families_cannot_establish_rider_eligibility() {
    let player = serde_json::json!({"components": {"minecraft:type_family": {"family": ["player"]}},
        "component_groups": {"fixture:conditional": {"minecraft:type_family": {"family": ["baby_undead"]}}}});
    assert!(player_families(&player).is_none());
    assert_eq!(
        rideable(&serde_json::json!({"family_types": ["baby_undead"]}), None),
        C::Unresolved
    );
}
