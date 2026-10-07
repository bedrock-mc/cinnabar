use super::*;

#[test]
fn absorption_sprites_draw_and_clear_through_the_json_ui_heart_renderer() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping absorption sprite test: missing vanilla-v1.mcbeui; make assets");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let mut runtime = UiRuntime::new(1);
    full_stats(&mut player, &mut runtime, 1);
    let baseline = build(&player, &mut presentation, &runtime, 20);
    let passes = presentation.hud_passes();
    runtime.last_health_drop_millis = Some(20);
    for (sequence, current, added_vertices) in [(2, 3.0, 16), (3, 0.0, 0)] {
        runtime
            .apply_local_attributes(
                &mut player,
                crate::ui_runtime::SequencedLocalAttributes {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: 20,
                    server_tick: sequence,
                    attributes: Arc::from([protocol::ActorAttribute {
                        name: Arc::from("minecraft:absorption"),
                        min: 0.0,
                        max: f32::MAX,
                        current,
                        default: Some(0.0),
                        modifiers: Arc::from([]),
                    }]),
                },
            )
            .unwrap();
        let rendered = build(&player, &mut presentation, &runtime, 20);
        assert_eq!(
            rendered.vertices.len(),
            baseline.vertices.len() + added_vertices,
            "two golden sprites and two containers must produce four quads"
        );
        assert_eq!(
            customs(presentation.hud_draw_nodes(), "heart_renderer").len(),
            1
        );
        assert_eq!(
            presentation.hud_passes(),
            passes,
            "native absorption updates do not rebuild the JSON-UI layout"
        );
    }
}

#[test]
fn absorption_above_the_viewport_stays_valid_without_overflowing_ui_nodes() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping absorption overflow test: missing vanilla-v1.mcbeui; make assets");
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let mut runtime = UiRuntime::new(1);
    full_stats(&mut player, &mut runtime, 1);
    let baseline = build(&player, &mut presentation, &runtime, 20);
    runtime
        .apply_local_attributes(
            &mut player,
            crate::ui_runtime::SequencedLocalAttributes {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 20,
                server_tick: 2,
                attributes: Arc::from([protocol::ActorAttribute {
                    name: Arc::from("minecraft:absorption"),
                    min: 0.0,
                    max: f32::MAX,
                    current: u16::MAX as f32,
                    default: Some(0.0),
                    modifiers: Arc::from([]),
                }]),
            },
        )
        .unwrap();
    assert_eq!(runtime.hud().absorption().unwrap().current(), u16::MAX);
    let rendered = build(&player, &mut presentation, &runtime, 20);
    assert!(
        rendered.vertices.len() > baseline.vertices.len(),
        "visible golden hearts still draw"
    );
    assert!(
        rendered.vertices.len() < ui::UiLimits::MAX_NODES,
        "offscreen rows must not exhaust the retained UI node budget"
    );
}
