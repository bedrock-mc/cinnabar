use super::*;
use assets::entity::offered_use::OfferedUse;

/// Evaluates the compiler's original component fixture without a behavior-group heuristic.
fn outcome(entity: Value, held: &str) -> OfferedUse {
    EntityUseAdmission::Components {
        source: 0,
        offerings: offerings(&entity, Some(&["player"])).into(),
    }
    .for_throwable(Some(held), 0)
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
    let filter = serde_json::json!({"test": "has_equipment", "subject": "other", "domain": "hand", "value": "fixture:held"});
    assert_eq!(
        super::filter(Some(&filter), Some(&["player"])),
        C::Unresolved
    );
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
