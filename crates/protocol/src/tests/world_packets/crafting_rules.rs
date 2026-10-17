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
