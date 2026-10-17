use super::*;

#[test]
fn crafting_rules_reach_the_world_consumer_without_other_rule_changes() {
    let packet = GameRulesChanged {
        rule_data: GameRulesChangedData {
            rules_list: vec![
                bool_rule("recipesUnlock", true),
                bool_rule("doLimitedCrafting", true),
            ],
        },
    };
    assert!(matches!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
        Some(WorldEvent::GameRules(_))
    ));
}

#[test]
fn crafting_rules_startup_resolves_registered_defaults_and_explicit_values() {
    let defaults = protocol::GameData::default().settings.crafting_rules;
    assert!(defaults.is_complete());
    assert!(defaults.unlocking_enabled());
    assert!(!defaults.limited_crafting_enabled());
    let mut start = StartGame::default();
    start.settings.rule_data.rules_list = vec![
        bool_rule("recipesUnlock", false),
        bool_rule("doLimitedCrafting", true),
    ];
    let data = protocol::GameData::from_wire(&start, &packets::ItemRegistry::default());
    assert!(!data.settings.crafting_rules.unlocking_enabled());
    assert!(data.settings.crafting_rules.limited_crafting_enabled());
}

#[test]
fn crafting_rules_sparse_updates_preserve_absent_and_wrong_typed_fields() {
    let packet = GameRulesChanged {
        rule_data: GameRulesChangedData {
            rules_list: vec![
                bool_rule("ReCiPeSuNlOcK", false),
                GameRule {
                    rule_name: "doLimitedCrafting".into(),
                    rule_can_be_modified: false,
                    rule_value: GameRuleValue::Int32(wire::I32LE(1)),
                },
            ],
        },
    };
    let Some(WorldEvent::GameRules(event)) =
        into_world_event(protocol::Packet::new(packet), 0).unwrap()
    else {
        panic!("the boolean crafting rule must reach its owner");
    };
    let mut rules = protocol::CraftingRules {
        recipes_unlock: Some(true),
        limited_crafting: Some(true),
    };
    rules.apply(event.crafting);
    assert_eq!(rules.recipes_unlock, Some(false));
    assert_eq!(rules.limited_crafting, Some(true));
}
