use super::*;

#[test]
fn seasonal_weather_rule_defaults_true_and_admits_only_boolean_start_game_values() {
    let mut game = game_data();
    assert!(WorldEnvironmentBootstrap::from_game_data(&game).weather_cycle_enabled);
    game.start_game.settings.rule_data.rules_list = vec![bool_rule("DoWeatherCycle", false)];
    assert!(!WorldEnvironmentBootstrap::from_game_data(&game).weather_cycle_enabled);
    game.start_game.settings.rule_data.rules_list[0].rule_value = GameRuleRuleValue::Int32(0);
    assert!(WorldEnvironmentBootstrap::from_game_data(&game).weather_cycle_enabled);
}

#[test]
fn seasonal_weather_only_rule_packet_is_retained_without_daylight_or_hud_updates() {
    let packet = GameRulesChangedPacket {
        rule_data: GameRulesChangedPacketData {
            rules_list: vec![bool_rule("DOWEATHERCYCLE", false)],
        },
    };
    assert_eq!(
        into_world_event(packet.into(), 0).unwrap(),
        Some(WorldEvent::GameRules(GameRulesEvent {
            daylight_cycle: None,
            weather_cycle: Some(false),
            hud: HudRules::default(),
        }))
    );
}
