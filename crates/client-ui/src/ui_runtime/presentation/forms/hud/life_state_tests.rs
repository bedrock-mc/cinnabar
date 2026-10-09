use super::*;

#[test]
fn hud_world_text_recovers_with_positive_health_below_display_precision() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply_hud_rules(protocol::HudRules {
        show_coordinates: Some(true),
        show_days_played: Some(true),
    });
    let frame = HudFrame {
        player_block: Some([12, 64, -7]),
        world_time: Some(24_000.0 * 3.0),
        ..Default::default()
    };
    let mut player = player_state::PlayerState::new(1);
    for (sequence, health) in [(1, 0.0), (2, 0.001)] {
        runtime
            .apply_local_attributes(
                &mut player,
                crate::ui_runtime::SequencedLocalAttributes {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: sequence,
                    server_tick: sequence,
                    attributes: vec![protocol::ActorAttribute {
                        name: "minecraft:health".into(),
                        min: 0.0,
                        max: 20.0,
                        current: health,
                        default: None,
                        modifiers: Arc::from([]),
                    }]
                    .into(),
                },
            )
            .unwrap();
        assert_eq!(runtime.hud().health().unwrap().current(), 0);
        let (position, days) = world_text_lines(&runtime, &frame);
        if health == 0.0 {
            assert_eq!((position, days), (None, None));
            runtime.publish_local_actor_health(None);
            assert_eq!(runtime.local_player_alive(), None);
            assert_eq!(world_text_lines(&runtime, &frame), (None, None));
        } else {
            assert_eq!(position.as_deref(), Some("Position: 12, 64, -7"));
            assert_eq!(days.as_deref(), Some("Days played: 3"));
        }
    }
}
